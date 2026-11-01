// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 位图到打印命令的计划。
//!
//! 这里只做一件事：把 [`Bitmap`] 逐行翻译成打印命令的**参数**，不编码、不补长度和校验和、
//! 不管流控。调用方拿到的每个命令自己决定怎么调用命令层：
//!
//! - [`PrintCommand::Line`] → [crate::command::req::print_line::PrintLine]
//! - [`PrintCommand::Feed`] → [crate::command::req::feed_line::FeedLine]
//! - `PrintCommand::Rle*` → [crate::command::req::print_line_rle::PrintLineRle]
//!
//! 页面边界的 `StartPage` / `EndPage` 仍由上层发送；位图高于
//! [`MAX_LINES_PER_PAGE`] 时用 [`print_bitmap_pages`] 先拆页，
//! 发送时的预算和分块见 [`crate::planner::flow`]。
//!
//! 位级 D 模式假定设备把上一条 **D 命令**的解码结果留作下一行的基准；
//! 每页第一行、以及 [`PrintCommand::Feed`] 之后的第一行都按全白处理（只用 X）。
//!
//! DP27P 实测 X / D 不能混用：X 行之后的第一条 D 会基于错误的上一行。因此
//! [`RleMode::Bits4`] / `Bits5` / `Bits6`（自动 X/D）不要用于该机型，
//! 真机请用 [`RleMode::Bits4XOnly`] / `Bits5XOnly` / `Bits6XOnly`。

use crate::bitmap::{Bitmap, BitmapError, BitmapLine};
use crate::command::req::{self, ReqTrait};
use crate::rle::{RleBitStream, RleBits, RleByteStream, RleTransformError};
use thiserror::Error;

/// 单行最大点数。
///
/// 取 `StartPage::data_pixel_width` 的上限 `1521`：它同时满足 `0x2b` 的
/// `skips` / `data_len <= 191` 字节（`191 * 8 = 1528` 点）。
pub const MAX_DOTS_PER_LINE: usize = 1521;

/// 单页最大行数。
///
/// `0x26 TotalLines` 在 v2 上的上限是 `16383`，所以一张高于这个行数的位图
/// 必须拆成多页，每页重新发 `0x20 StartPage` / `0x26 TotalLines` / `0x28 EndPage`。
/// `0x25` 等命令的 2 字节变长字段同样是这个上限。
pub const MAX_LINES_PER_PAGE: usize = 16383;

/// RLE 编码方案。
///
/// 只描述“用哪一族 RLE”；位级模式默认同时算 X 和 D，取字节数更小的那个。
/// `*XOnly` 变体只用 X，用于验证设备是否支持 X / D 混用。
/// 如果 RLE 编码后不比直接发一行小，仍然回退到 [`PrintCommand::Line`]。
///
/// **DP27P 实测 X / D 不能混用**，自动模式在该机型上会出现周期性断层，
/// 真机请用 `Bits4XOnly` / `Bits5XOnly` / `Bits6XOnly`。详见 `rle.md`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RleMode {
    /// 不压缩。
    #[default]
    Off,
    /// 字节级 RLE，`1f 29`。
    Byte,
    /// 4 位符号 RLE，`1f 3a` / `1f 3b`。
    Bits4,
    /// 5 位符号 RLE，`1f 2c` / `1f 2d`。
    Bits5,
    /// 6 位符号 RLE，`1f 3c` / `1f 3d`。
    Bits6,
    /// 4 位符号 RLE，只用 X，`1f 3a`。
    Bits4XOnly,
    /// 5 位符号 RLE，只用 X，`1f 2c`。
    Bits5XOnly,
    /// 6 位符号 RLE，只用 X，`1f 3c`。
    Bits6XOnly,
}

/// 一条打印命令，只带调用命令层需要的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintCommand {
    /// 打印一行。[crate::command::req::print_line::PrintLine]
    Line {
        skips: u8,
        repeats: u16,
        data: Vec<u8>,
    },
    /// 字节级 RLE。[crate::command::req::print_line_rle::PrintLineRle]
    RleByte {
        repeats: u16,
        rle_byte_count: u32,
        data: Vec<u8>,
    },
    /// 5 位符号 RLE(X)。[crate::command::req::print_line_rle::PrintLineRle]
    Rle5x {
        repeats: u16,
        rle_runs_count: u32,
        data: Vec<u8>,
    },
    /// 5 位符号 RLE(D)。[crate::command::req::print_line_rle::PrintLineRle]
    Rle5d {
        repeats: u16,
        rle_runs_count: u32,
        data: Vec<u8>,
    },
    /// 4 位符号 RLE(X)。[crate::command::req::print_line_rle::PrintLineRle]
    Rle4x {
        repeats: u16,
        rle_runs_count: u32,
        data: Vec<u8>,
    },
    /// 4 位符号 RLE(D)。[crate::command::req::print_line_rle::PrintLineRle]
    Rle4d {
        repeats: u16,
        rle_runs_count: u32,
        data: Vec<u8>,
    },
    /// 6 位符号 RLE(X)。[crate::command::req::print_line_rle::PrintLineRle]
    Rle6x {
        repeats: u16,
        rle_runs_count: u32,
        data: Vec<u8>,
    },
    /// 6 位符号 RLE(D)。[crate::command::req::print_line_rle::PrintLineRle]
    Rle6d {
        repeats: u16,
        rle_runs_count: u32,
        data: Vec<u8>,
    },
    /// 走纸(打印空行)。[crate::command::req::feed_line::FeedLine]
    Feed { lines: u32 },
}

/// 把打印计划的每条命令翻译成命令层请求对象。
///
/// 这是 [`PrintCommand`] 在库内的唯一消费者；调用方拿到 `Vec<Box<dyn ReqTrait>>`
/// 后交给 [`crate::planner::flow::FlowPlanner::push_commands`] 或
/// [`crate::planner::Assembler::encode`]。
pub fn into_reqs(commands: &[PrintCommand]) -> Vec<Box<dyn ReqTrait>> {
    commands
        .iter()
        .map(|cmd| -> Box<dyn ReqTrait> {
            match cmd {
                PrintCommand::Line {
                    repeats,
                    skips,
                    data,
                } => Box::new(req::print_line::PrintLine {
                    repeats: *repeats,
                    skips: *skips,
                    data: data.clone(),
                }),
                PrintCommand::Feed { lines } => {
                    Box::new(req::feed_line::FeedLine { lines: *lines })
                }
                PrintCommand::RleByte {
                    repeats,
                    rle_byte_count,
                    data,
                } => Box::new(req::print_line_rle::PrintLineRle {
                    repeats: *repeats,
                    kind: req::print_line_rle::RleKind::Byte,
                    count: *rle_byte_count,
                    data: data.clone(),
                }),
                PrintCommand::Rle4x {
                    repeats,
                    rle_runs_count,
                    data,
                } => Box::new(req::print_line_rle::PrintLineRle {
                    repeats: *repeats,
                    kind: req::print_line_rle::RleKind::Bits4x,
                    count: *rle_runs_count,
                    data: data.clone(),
                }),
                PrintCommand::Rle4d {
                    repeats,
                    rle_runs_count,
                    data,
                } => Box::new(req::print_line_rle::PrintLineRle {
                    repeats: *repeats,
                    kind: req::print_line_rle::RleKind::Bits4d,
                    count: *rle_runs_count,
                    data: data.clone(),
                }),
                PrintCommand::Rle5x {
                    repeats,
                    rle_runs_count,
                    data,
                } => Box::new(req::print_line_rle::PrintLineRle {
                    repeats: *repeats,
                    kind: req::print_line_rle::RleKind::Bits5x,
                    count: *rle_runs_count,
                    data: data.clone(),
                }),
                PrintCommand::Rle5d {
                    repeats,
                    rle_runs_count,
                    data,
                } => Box::new(req::print_line_rle::PrintLineRle {
                    repeats: *repeats,
                    kind: req::print_line_rle::RleKind::Bits5d,
                    count: *rle_runs_count,
                    data: data.clone(),
                }),
                PrintCommand::Rle6x {
                    repeats,
                    rle_runs_count,
                    data,
                } => Box::new(req::print_line_rle::PrintLineRle {
                    repeats: *repeats,
                    kind: req::print_line_rle::RleKind::Bits6x,
                    count: *rle_runs_count,
                    data: data.clone(),
                }),
                PrintCommand::Rle6d {
                    repeats,
                    rle_runs_count,
                    data,
                } => Box::new(req::print_line_rle::PrintLineRle {
                    repeats: *repeats,
                    kind: req::print_line_rle::RleKind::Bits6d,
                    count: *rle_runs_count,
                    data: data.clone(),
                }),
            }
        })
        .collect()
}

/// 打印计划错误。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PrintError {
    #[error("bitmap width {width} exceeds the {max}-dot limit of a single print line")]
    LineTooWide { width: usize, max: usize },

    #[error("page line limit {lines} must be in 1..={max}")]
    InvalidPageLines { lines: usize, max: usize },

    #[error(transparent)]
    Bitmap(#[from] BitmapError),

    #[error(transparent)]
    Rle(#[from] RleTransformError),
}

/// 一页的打印计划。
///
/// `lines` 是这一页的总行数，直接对应 `0x26 TotalLines`；`commands` 是页内
/// 的行命令和走纸命令。上层按 [`PrintPage`] 依次发 `StartPage` /
/// `TotalLines` / `commands` / `EndPage`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintPage {
    /// 这一页的行数，范围 `1..=`[`MAX_LINES_PER_PAGE`]。
    pub lines: usize,
    /// 页内的打印命令。
    pub commands: Vec<PrintCommand>,
}

/// 把位图逐行翻译成打印命令。
///
/// - 连续空白行合并成一条 [`PrintCommand::Feed`]。
/// - 连续相同的行合并成一条命令，用 `repeats` 表达。
/// - 其余行去掉左侧整字节空白放进 `skips`，去掉右侧空白后作为 `data`。
/// - `rle` 不为 [`RleMode::Off`] 时尝试 RLE 编码，只有比普通行更小才采用。
///   RLE 只有 `v2` 机型支持，`v1` / `v0` 请传 [`RleMode::Off`]。
///
/// 返回的命令属于**一页**；位图高于 [`MAX_LINES_PER_PAGE`] 时请改用
/// [`print_bitmap_pages`]，否则 `0x26 TotalLines` 会超出 v2 的字段范围。
pub fn print_bitmap(bm: &Bitmap, rle: RleMode) -> Result<Vec<PrintCommand>, PrintError> {
    print_bitmap_range(bm, 0, bm.height(), rle)
}

/// 把位图切成多页，每页最多 `max_lines` 行。
///
/// v2 的 `0x26 TotalLines` 上限是 [`MAX_LINES_PER_PAGE`]。超过这个行数的位图
/// 必须拆页，否则 `print_bitmap` 的结果无法用一个 `TotalLines` 描述。
///
/// 每页的第一行都按全白基准处理，D 模式不会跨页引用上一页的解码结果。
///
/// 这个函数一次返回所有页；如果位图非常大、不想同时保留所有页的命令，
/// 可以用 [`print_bitmap_page`] 按 `start` 逐页生成。
pub fn print_bitmap_pages(
    bm: &Bitmap,
    rle: RleMode,
    max_lines: usize,
) -> Result<Vec<PrintPage>, PrintError> {
    validate_page_lines(max_lines)?;

    let mut pages = Vec::with_capacity(bm.height().div_ceil(max_lines));
    let mut start = 0usize;
    while start < bm.height() {
        let page = print_bitmap_page(bm, start, rle, max_lines)?;
        start += page.lines;
        pages.push(page);
    }
    Ok(pages)
}

/// 计算从第 `start` 行开始的一页，最多 `max_lines` 行。
///
/// `start` 必须小于位图高度；返回的 [`PrintPage::lines`] 是实际行数
/// （最后一页可能不足 `max_lines`）。
pub fn print_bitmap_page(
    bm: &Bitmap,
    start: usize,
    rle: RleMode,
    max_lines: usize,
) -> Result<PrintPage, PrintError> {
    validate_page_lines(max_lines)?;
    if start >= bm.height() {
        return Err(BitmapError::LineOutOfRange {
            index: start,
            height: bm.height(),
        }
        .into());
    }

    let end = (start + max_lines).min(bm.height());
    Ok(PrintPage {
        lines: end - start,
        commands: print_bitmap_range(bm, start, end, rle)?,
    })
}

fn validate_page_lines(max_lines: usize) -> Result<(), PrintError> {
    if max_lines == 0 || max_lines > MAX_LINES_PER_PAGE {
        return Err(PrintError::InvalidPageLines {
            lines: max_lines,
            max: MAX_LINES_PER_PAGE,
        });
    }
    Ok(())
}

/// [`print_bitmap`] 的行区间版本。
fn print_bitmap_range(
    bm: &Bitmap,
    start: usize,
    end: usize,
    rle: RleMode,
) -> Result<Vec<PrintCommand>, PrintError> {
    if bm.width() > MAX_DOTS_PER_LINE {
        return Err(PrintError::LineTooWide {
            width: bm.width(),
            max: MAX_DOTS_PER_LINE,
        });
    }

    let mut out = Vec::new();
    let mut y = start;
    // 上一行是否真的作为行数据发出去了。空白行被合并成 `Feed`，
    // 之后的第一行不能拿“位图里的上一行”当 D 的基准。
    let mut prev_is_line = false;
    while y < end {
        let line = bm.line(y)?;
        let Some(first) = line.first_one() else {
            // 连续空白行：走纸一次就够。
            let mut count = 1usize;
            while y + count < end && bm.line(y + count)?.is_blank() {
                count += 1;
            }
            push_feed(&mut out, count);
            prev_is_line = false;
            y += count;
            continue;
        };
        let last = line.last_one().unwrap_or(first);
        let raw_len = last / 8 - first / 8 + 1;

        // 连续相同的行合并成 `repeats`；超过 `u16::MAX` 的部分留给下一轮，
        // 会再发一条同样的命令。
        let mut repeats = 0usize;
        while repeats < u16::MAX as usize
            && y + repeats + 1 < end
            && bm.line(y + repeats + 1)?.bits() == line.bits()
        {
            repeats += 1;
        }
        let repeats = repeats as u16;

        // D 模式需要上一行的解码结果；每页第一行和 Feed 之后按全白处理。
        let prev = if y == start || !prev_is_line {
            None
        } else {
            Some(bm.line(y - 1)?)
        };
        if let Some(cmd) = select_rle(&line, prev, raw_len, repeats, rle)? {
            out.push(cmd);
            prev_is_line = true;
            y += 1 + repeats as usize;
            continue;
        }

        let data = line.to_bytes();
        let skips = (first / 8) as u8;
        let data = data[skips as usize..=last / 8].to_vec();

        out.push(PrintCommand::Line {
            skips,
            repeats,
            data,
        });
        prev_is_line = true;
        y += 1 + repeats as usize;
    }
    Ok(out)
}

/// 选择并生成 RLE 行命令。
///
/// 只有编码后比普通行（`raw_len` 字节）更小才返回 `Some`，否则返回 `None`
/// 让调用方发 [`PrintCommand::Line`]。位级模式默认同时算 X 和 D，取较小的那个；
/// `*XOnly` 只算 X；`prev` 为 `None`（第一行）时也只算 X。
fn select_rle(
    line: &BitmapLine<'_>,
    prev: Option<BitmapLine<'_>>,
    raw_len: usize,
    repeats: u16,
    rle: RleMode,
) -> Result<Option<PrintCommand>, PrintError> {
    let (bits, allow_d) = match rle {
        RleMode::Off => return Ok(None),
        RleMode::Byte => {
            let stream = RleByteStream::encode(*line)?;
            if stream.data.len() >= raw_len {
                return Ok(None);
            }
            return Ok(Some(PrintCommand::RleByte {
                repeats,
                rle_byte_count: stream.byte_count as u32,
                data: stream.data,
            }));
        }
        RleMode::Bits4 => (RleBits::Bits4, true),
        RleMode::Bits5 => (RleBits::Bits5, true),
        RleMode::Bits6 => (RleBits::Bits6, true),
        RleMode::Bits4XOnly => (RleBits::Bits4, false),
        RleMode::Bits5XOnly => (RleBits::Bits5, false),
        RleMode::Bits6XOnly => (RleBits::Bits6, false),
    };

    let x = RleBitStream::encode(*line, None, bits)?;
    let d = match (allow_d, prev) {
        (true, Some(prev)) => Some(RleBitStream::encode(*line, Some(prev), bits)?),
        _ => None,
    };
    let (is_d, stream) = match d {
        Some(d) if d.data.len() < x.data.len() => (true, d),
        _ => (false, x),
    };
    if stream.data.len() >= raw_len {
        return Ok(None);
    }
    let rle_runs_count = stream.symbol_count as u32;
    let data = stream.data;
    let cmd = match (bits, is_d) {
        (RleBits::Bits4, false) => PrintCommand::Rle4x {
            repeats,
            rle_runs_count,
            data,
        },
        (RleBits::Bits4, true) => PrintCommand::Rle4d {
            repeats,
            rle_runs_count,
            data,
        },
        (RleBits::Bits5, false) => PrintCommand::Rle5x {
            repeats,
            rle_runs_count,
            data,
        },
        (RleBits::Bits5, true) => PrintCommand::Rle5d {
            repeats,
            rle_runs_count,
            data,
        },
        (RleBits::Bits6, false) => PrintCommand::Rle6x {
            repeats,
            rle_runs_count,
            data,
        },
        (RleBits::Bits6, true) => PrintCommand::Rle6d {
            repeats,
            rle_runs_count,
            data,
        },
    };
    Ok(Some(cmd))
}

fn push_feed(out: &mut Vec<PrintCommand>, mut lines: usize) {
    while lines > 0 {
        let chunk = lines.min(u32::MAX as usize) as u32;
        out.push(PrintCommand::Feed { lines: chunk });
        lines -= chunk as usize;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// 按“每行黑点的 x 坐标”造位图。
    fn make(width: usize, rows: &[&[usize]]) -> Bitmap {
        let mut bm = Bitmap::new(width, rows.len()).unwrap();
        for (y, xs) in rows.iter().enumerate() {
            let mut line = bm.line_mut(y).unwrap();
            for &x in *xs {
                line.set(x, true);
            }
        }
        bm
    }

    #[test]
    fn blank_rows_become_one_feed() {
        let bm = make(8, &[&[], &[], &[]]);
        assert_eq!(
            print_bitmap(&bm, RleMode::Off).unwrap(),
            vec![PrintCommand::Feed { lines: 3 }]
        );
    }

    #[test]
    fn feed_skips_trailing_blank_rows_in_one_command() {
        let bm = make(8, &[&[0], &[], &[], &[]]);
        assert_eq!(
            print_bitmap(&bm, RleMode::Off).unwrap(),
            vec![
                PrintCommand::Line {
                    skips: 0,
                    repeats: 0,
                    data: vec![0x80],
                },
                PrintCommand::Feed { lines: 3 },
            ]
        );
    }

    #[test]
    fn leading_whole_bytes_go_to_skips() {
        let bm = make(16, &[&[8]]);
        assert_eq!(
            print_bitmap(&bm, RleMode::Off).unwrap(),
            vec![PrintCommand::Line {
                skips: 1,
                repeats: 0,
                data: vec![0x80],
            }]
        );
    }

    #[test]
    fn sub_byte_leading_blank_stays_in_data() {
        let bm = make(16, &[&[4]]);
        assert_eq!(
            print_bitmap(&bm, RleMode::Off).unwrap(),
            vec![PrintCommand::Line {
                skips: 0,
                repeats: 0,
                data: vec![0x08],
            }]
        );
    }

    #[test]
    fn trailing_blank_bytes_are_trimmed() {
        let bm = make(16, &[&[4, 8]]);
        assert_eq!(
            print_bitmap(&bm, RleMode::Off).unwrap(),
            vec![PrintCommand::Line {
                skips: 0,
                repeats: 0,
                data: vec![0x08, 0x80],
            }]
        );
    }

    #[test]
    fn equal_rows_collapse_into_repeats() {
        let bm = make(8, &[&[0], &[0], &[0], &[7]]);
        assert_eq!(
            print_bitmap(&bm, RleMode::Off).unwrap(),
            vec![
                PrintCommand::Line {
                    skips: 0,
                    repeats: 2,
                    data: vec![0x80],
                },
                PrintCommand::Line {
                    skips: 0,
                    repeats: 0,
                    data: vec![0x01],
                },
            ]
        );
    }

    #[test]
    fn blank_and_line_are_interleaved_in_order() {
        let bm = make(8, &[&[], &[0], &[], &[]]);
        assert_eq!(
            print_bitmap(&bm, RleMode::Off).unwrap(),
            vec![
                PrintCommand::Feed { lines: 1 },
                PrintCommand::Line {
                    skips: 0,
                    repeats: 0,
                    data: vec![0x80],
                },
                PrintCommand::Feed { lines: 2 },
            ]
        );
    }

    #[test]
    fn width_above_limit_errors() {
        let bm = Bitmap::new(MAX_DOTS_PER_LINE + 1, 1).unwrap();
        assert_eq!(
            print_bitmap(&bm, RleMode::Off).unwrap_err(),
            PrintError::LineTooWide {
                width: MAX_DOTS_PER_LINE + 1,
                max: MAX_DOTS_PER_LINE,
            }
        );
    }

    #[test]
    fn max_width_line_still_fits_protocol_limits() {
        let last = MAX_DOTS_PER_LINE - 1;
        let bm = make(MAX_DOTS_PER_LINE, &[&[last]]);
        assert_eq!(
            print_bitmap(&bm, RleMode::Off).unwrap(),
            vec![PrintCommand::Line {
                skips: (last / 8) as u8,
                repeats: 0,
                data: vec![0x80],
            }]
        );
    }

    #[test]
    fn rle_is_skipped_when_not_smaller_than_raw_line() {
        // 一整行黑: 原始 1 字节, RLE4 也是 1 字节, 不赚就回退。
        let bm = make(8, &[&[0, 1, 2, 3, 4, 5, 6, 7]]);
        for mode in [
            RleMode::Byte,
            RleMode::Bits4,
            RleMode::Bits5,
            RleMode::Bits6,
        ] {
            assert_eq!(
                print_bitmap(&bm, mode).unwrap(),
                vec![PrintCommand::Line {
                    skips: 0,
                    repeats: 0,
                    data: vec![0xFF],
                }],
                "{mode:?}"
            );
        }
    }

    #[test]
    fn byte_rle_wins_for_a_solid_row() {
        let mut bm = Bitmap::new(40, 1).unwrap();
        bm.line_mut(0).unwrap().fill(true);
        assert_eq!(
            print_bitmap(&bm, RleMode::Byte).unwrap(),
            vec![PrintCommand::RleByte {
                repeats: 0,
                rle_byte_count: 2,
                data: vec![0xC5, 0xFF],
            }]
        );
    }

    #[test]
    fn bits4_rle_wins_for_a_solid_row() {
        let mut bm = Bitmap::new(32, 1).unwrap();
        bm.line_mut(0).unwrap().fill(true);
        // 32 = 3 + 29 -> 黑,索引2 + 黑,索引6 -> 1010 1110
        assert_eq!(
            print_bitmap(&bm, RleMode::Bits4).unwrap(),
            vec![PrintCommand::Rle4x {
                repeats: 0,
                rle_runs_count: 2,
                data: vec![0xAE],
            }]
        );
    }

    #[test]
    fn rle_d_is_chosen_when_xor_is_smaller() {
        let evens: Vec<usize> = (0..32).step_by(2).collect();
        let odds: Vec<usize> = (1..32).step_by(2).collect();
        let bm = make(32, &[&evens, &odds]);
        assert_eq!(
            print_bitmap(&bm, RleMode::Bits4).unwrap(),
            vec![
                // 第一行棋盘格, RLE 比 4 字节还大, 回退普通行
                PrintCommand::Line {
                    skips: 0,
                    repeats: 0,
                    data: vec![0xAA, 0xAA, 0xAA, 0xAA],
                },
                // 第二行与第一行 XOR 后是全黑, 2 个符号 1 字节
                PrintCommand::Rle4d {
                    repeats: 0,
                    rle_runs_count: 2,
                    data: vec![0xAE],
                },
            ]
        );
    }

    #[test]
    fn x_only_mode_never_emits_d() {
        let evens: Vec<usize> = (0..32).step_by(2).collect();
        let odds: Vec<usize> = (1..32).step_by(2).collect();
        let bm = make(32, &[&evens, &odds]);
        for mode in [
            RleMode::Bits4XOnly,
            RleMode::Bits5XOnly,
            RleMode::Bits6XOnly,
        ] {
            let cmds = print_bitmap(&bm, mode).unwrap();
            assert!(
                !cmds.iter().any(|c| matches!(
                    c,
                    PrintCommand::Rle4d { .. }
                        | PrintCommand::Rle5d { .. }
                        | PrintCommand::Rle6d { .. }
                )),
                "{mode:?} emitted a D command"
            );
        }
    }

    #[test]
    fn rle_keeps_repeats_of_equal_rows() {
        let mut bm = Bitmap::new(32, 2).unwrap();
        bm.line_mut(0).unwrap().fill(true);
        bm.line_mut(1).unwrap().fill(true);
        assert_eq!(
            print_bitmap(&bm, RleMode::Bits4).unwrap(),
            vec![PrintCommand::Rle4x {
                repeats: 1,
                rle_runs_count: 2,
                data: vec![0xAE],
            }]
        );
    }

    /// 一组命令实际打印 / 走纸的总行数。
    fn printed_lines(cmds: &[PrintCommand]) -> usize {
        cmds.iter()
            .map(|cmd| match cmd {
                PrintCommand::Feed { lines } => *lines as usize,
                PrintCommand::Line { repeats, .. }
                | PrintCommand::RleByte { repeats, .. }
                | PrintCommand::Rle4x { repeats, .. }
                | PrintCommand::Rle4d { repeats, .. }
                | PrintCommand::Rle5x { repeats, .. }
                | PrintCommand::Rle5d { repeats, .. }
                | PrintCommand::Rle6x { repeats, .. }
                | PrintCommand::Rle6d { repeats, .. } => *repeats as usize + 1,
            })
            .sum()
    }

    #[test]
    fn long_bitmap_splits_at_total_lines_limit() {
        let height = MAX_LINES_PER_PAGE + 10;
        let mut bm = Bitmap::new(8, height).unwrap();
        bm.fill(true);

        let pages = print_bitmap_pages(&bm, RleMode::Off, MAX_LINES_PER_PAGE).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].lines, MAX_LINES_PER_PAGE);
        assert_eq!(pages[1].lines, 10);
        assert_eq!(printed_lines(&pages[0].commands), MAX_LINES_PER_PAGE);
        assert_eq!(printed_lines(&pages[1].commands), 10);
        assert_eq!(
            pages[0].lines + pages[1].lines,
            bm.height(),
            "pages must cover the whole bitmap"
        );
    }

    #[test]
    fn into_reqs_maps_line_and_rle_commands() {
        let commands = vec![
            PrintCommand::Line {
                skips: 0,
                repeats: 0,
                data: vec![0x80],
            },
            PrintCommand::Rle5x {
                repeats: 0,
                rle_runs_count: 2,
                data: vec![0x1F, 0xC0],
            },
        ];
        let reqs = into_reqs(&commands);
        let refs: Vec<&dyn ReqTrait> = reqs.iter().map(|req| req.as_ref()).collect();
        let bytes = crate::planner::Assembler::new(crate::command::ModelVersion::V2)
            .encode(&refs)
            .unwrap();
        assert_eq!(
            bytes,
            vec![
                0x1f, 0x21, 0x03, 0x00, 0x00, 0x80, 0x5b, // PrintLine
                0x1f, 0x2c, 0x02, 0x1f, 0xc0, 0xf2, // RLE5X
            ]
        );
    }

    #[test]
    fn pages_do_not_reuse_previous_page_for_d() {
        let height = MAX_LINES_PER_PAGE + 2;
        let mut bm = Bitmap::new(32, height).unwrap();
        bm.fill(true);

        let pages = print_bitmap_pages(&bm, RleMode::Bits5, MAX_LINES_PER_PAGE).unwrap();
        assert!(
            matches!(pages[1].commands[0], PrintCommand::Rle5x { .. }),
            "the first line of a page must not XOR against the previous page, got {:?}",
            pages[1].commands[0]
        );
    }

    #[test]
    fn d_is_not_used_after_feed() {
        // 第 0 行整行黑；中间两行空白合并成 Feed；第 3 行只差最后一个点。
        // 如果拿位图里的空白行当 D 基准，或跨 Feed 引用第 0 行，都会算错。
        let mut bm = Bitmap::new(64, 4).unwrap();
        bm.line_mut(0).unwrap().fill(true);
        bm.line_mut(3).unwrap().fill(true);
        bm.line_mut(3).unwrap().set(63, false);

        let cmds = print_bitmap(&bm, RleMode::Bits4).unwrap();
        assert!(
            matches!(cmds[1], PrintCommand::Feed { lines: 2 }),
            "{cmds:?}"
        );
        assert!(
            matches!(cmds[2], PrintCommand::Rle4x { .. }),
            "feed must reset the D base, got {:?}",
            cmds[2]
        );
        assert!(
            !cmds.iter().any(|c| matches!(
                c,
                PrintCommand::Rle4d { .. }
                    | PrintCommand::Rle5d { .. }
                    | PrintCommand::Rle6d { .. }
            )),
            "{cmds:?}"
        );
    }

    #[test]
    fn page_line_limit_must_fit_total_lines() {
        let bm = Bitmap::new(8, 1).unwrap();
        for limit in [0, MAX_LINES_PER_PAGE + 1] {
            assert_eq!(
                print_bitmap_pages(&bm, RleMode::Off, limit),
                Err(PrintError::InvalidPageLines {
                    lines: limit,
                    max: MAX_LINES_PER_PAGE,
                })
            );
        }
    }

    #[test]
    fn page_start_must_be_inside_the_bitmap() {
        let bm = Bitmap::new(8, 4).unwrap();
        assert_eq!(
            print_bitmap_page(&bm, 4, RleMode::Off, MAX_LINES_PER_PAGE),
            Err(PrintError::Bitmap(BitmapError::LineOutOfRange {
                index: 4,
                height: 4,
            }))
        );

        let page = print_bitmap_page(&bm, 2, RleMode::Off, MAX_LINES_PER_PAGE).unwrap();
        assert_eq!(page.lines, 2);
    }
}
