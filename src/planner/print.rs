// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 位图到打印命令的计划。
//!
//! 这里只做一件事：把 [`Bitmap`] 逐行翻译成打印命令的**参数**，不编码、不补长度和校验和、
//! 不管页面、不管流控。调用方拿到的每个命令自己决定怎么调用命令层：
//!
//! - [`PrintCommand::Line`] → [crate::command::req::print_line::PrintLine]
//! - [`PrintCommand::Feed`] → [crate::command::req::feed_line::FeedLine]
//! - `PrintCommand::Rle*` → [crate::command::req::print_line_rle::PrintLineRle]
//!
//! 页面边界（`StartPage` / `EndPage`）、分页、重试由上层负责；发送时的预算和
//! 分块见 [`crate::planner::flow`]。
//!
//! 位级 D 模式假定设备保留上一行（包括普通 [`PrintCommand::Line`]）的解码结果，
//! 第一行按全白处理。

use crate::bitmap::{Bitmap, BitmapError, BitmapLine};
use crate::rle::{RleBitStream, RleBits, RleByteStream, RleTransformError};
use thiserror::Error;

/// 单行最大点数。
///
/// 取 `StartPage::data_pixel_width` 的上限 `1521`：它同时满足 `0x2b` 的
/// `skips` / `data_len <= 191` 字节（`191 * 8 = 1528` 点）。
pub const MAX_DOTS_PER_LINE: usize = 1521;

/// RLE 编码方案。
///
/// 只描述“用哪一族 RLE”；位级模式默认同时算 X 和 D，取字节数更小的那个。
/// `*XOnly` 变体只用 X，用于验证设备是否支持 X / D 混用。
/// 如果 RLE 编码后不比直接发一行小，仍然回退到 [`PrintCommand::Line`]。
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

/// 打印计划错误。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PrintError {
    #[error("bitmap width {width} exceeds the {max}-dot limit of a single print line")]
    LineTooWide { width: usize, max: usize },

    #[error(transparent)]
    Bitmap(#[from] BitmapError),

    #[error(transparent)]
    Rle(#[from] RleTransformError),
}

/// 把位图逐行翻译成打印命令。
///
/// - 连续空白行合并成一条 [`PrintCommand::Feed`]。
/// - 连续相同的行合并成一条命令，用 `repeats` 表达。
/// - 其余行去掉左侧整字节空白放进 `skips`，去掉右侧空白后作为 `data`。
/// - `rle` 不为 [`RleMode::Off`] 时尝试 RLE 编码，只有比普通行更小才采用。
///   RLE 只有 `v2` 机型支持，`v1` / `v0` 请传 [`RleMode::Off`]。
pub fn print_bitmap(bm: &Bitmap, rle: RleMode) -> Result<Vec<PrintCommand>, PrintError> {
    if bm.width() > MAX_DOTS_PER_LINE {
        return Err(PrintError::LineTooWide {
            width: bm.width(),
            max: MAX_DOTS_PER_LINE,
        });
    }

    let mut out = Vec::new();
    let mut y = 0usize;
    while y < bm.height() {
        let line = bm.line(y)?;
        let Some(first) = line.first_one() else {
            // 连续空白行：走纸一次就够。
            let mut count = 1usize;
            while y + count < bm.height() && bm.line(y + count)?.is_blank() {
                count += 1;
            }
            push_feed(&mut out, count);
            y += count;
            continue;
        };
        let last = line.last_one().unwrap_or(first);
        let raw_len = last / 8 - first / 8 + 1;

        // 连续相同的行合并成 `repeats`；超过 `u16::MAX` 的部分留给下一轮，
        // 会再发一条同样的命令。
        let mut repeats = 0usize;
        while repeats < u16::MAX as usize
            && y + repeats + 1 < bm.height()
            && bm.line(y + repeats + 1)?.bits() == line.bits()
        {
            repeats += 1;
        }
        let repeats = repeats as u16;

        // D 模式需要上一行的解码结果；第一行按全白处理。
        let prev = if y == 0 { None } else { Some(bm.line(y - 1)?) };
        if let Some(cmd) = select_rle(&line, prev, raw_len, repeats, rle)? {
            out.push(cmd);
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
}
