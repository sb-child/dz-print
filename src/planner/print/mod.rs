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
//!
//! # 用法
//!
//! ```
//! use dz_print::bitmap::Bitmap;
//! use dz_print::command::{ModelVersion, req::ReqTrait};
//! use dz_print::planner::{
//!     Assembler,
//!     print::{MAX_LINES_PER_PAGE, RleMode, into_reqs, print_bitmap_page},
//! };
//!
//! // 384 x 20：第 0 行整行黑，其余留白
//! let mut bitmap = Bitmap::new(384, 20)?;
//! bitmap.line_mut(0)?.fill(true);
//!
//! // 一页的计划；高位图用 print_bitmap_page 逐页生成
//! let page = print_bitmap_page(&bitmap, 0, RleMode::Bits5XOnly, MAX_LINES_PER_PAGE)?;
//! assert_eq!(page.lines, 20);
//!
//! // 命令参数 → 命令层请求 → 命令区
//! let reqs = into_reqs(&page.commands);
//! let refs: Vec<&dyn ReqTrait> = reqs.iter().map(|req| req.as_ref()).collect();
//! let command_area = Assembler::new(ModelVersion::V2).encode(&refs)?;
//! assert!(!command_area.is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! 页面边界的 `StartPage` / `TotalLines` / `EndPage` 不在这里生成；
//! 完整打印流程见 [`crate::frontend::Device::print_bitmap_job`]。
//!
//! # RLE 命名对照
//!
//! 同一行数据在三个类型里各有一层名字，含义不同：
//!
//! | 层 | 类型 | 变体 | 含义 |
//! | --- | --- | --- | --- |
//! | 编码方案 | [`RleMode`] | `Bits4` / `Bits5` / `Bits6` | 自动在 X / D 里取字节数小的 |
//! | 编码方案 | [`RleMode`] | `Bits4XOnly` / `Bits5XOnly` / `Bits6XOnly` | 只用 X |
//! | 单行命令 | [`crate::command::req::print_line_rle::RleKind`] | `Rle4x` / `Rle4d` / `Rle5x` / … | 已经确定 X 或 D |
//! | 打印计划 | [`PrintCommand`] | `Rle4x` / `Rle4d` / `Rle5x` / … | 待翻译成请求 |
//!
//! `select_rle` 从 `RleMode` 选出 [`PrintCommand`]，[`into_reqs`] 再把它映射成
//! `RleKind`；三层名字只差前缀，不要混用。

use crate::{
    bitmap::{Bitmap, BitmapError},
    command::req::{self, ReqTrait},
    rle::RleTransformError,
};
use snafu::Snafu;

mod rle;

#[cfg(test)]
mod tests;

use rle::select_rle;

/// 单行最大点数。
///
/// 直接跟随 [`MAX_DATA_PIXEL_WIDTH`](crate::command::req::start_page::MAX_DATA_PIXEL_WIDTH)：
/// 这个上限同时满足 `0x2b` 的 `skips` / `data_len <= 191` 字节
/// （`191 * 8 = 1528` 点）。
pub const MAX_DOTS_PER_LINE: usize = crate::command::req::start_page::MAX_DATA_PIXEL_WIDTH as usize;

/// 单页最大行数。
///
/// `0x26 TotalLines` 在 v2 上的上限是 `16383`，所以一张高于这个行数的位图
/// 必须拆成多页，每页重新发 `0x20 StartPage` / `0x26 TotalLines` / `0x28 EndPage`。
/// `0x25` 等命令的 2 字节变长字段同样是这个上限。
pub const MAX_LINES_PER_PAGE: usize = 16383;

/// RLE 编码方案。
///
/// 位级模式默认同时算 X 和 D，取字节数更小的那个；`*XOnly` 只用 X。
/// 编码后不比普通行小时仍回退到 [`PrintCommand::Line`]。
///
/// DP27P 实测 X / D 不能混用，真机请用 `Bits4XOnly` / `Bits5XOnly` / `Bits6XOnly`。
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
                    kind: req::print_line_rle::RleKind::Rle4x,
                    count: *rle_runs_count,
                    data: data.clone(),
                }),
                PrintCommand::Rle4d {
                    repeats,
                    rle_runs_count,
                    data,
                } => Box::new(req::print_line_rle::PrintLineRle {
                    repeats: *repeats,
                    kind: req::print_line_rle::RleKind::Rle4d,
                    count: *rle_runs_count,
                    data: data.clone(),
                }),
                PrintCommand::Rle5x {
                    repeats,
                    rle_runs_count,
                    data,
                } => Box::new(req::print_line_rle::PrintLineRle {
                    repeats: *repeats,
                    kind: req::print_line_rle::RleKind::Rle5x,
                    count: *rle_runs_count,
                    data: data.clone(),
                }),
                PrintCommand::Rle5d {
                    repeats,
                    rle_runs_count,
                    data,
                } => Box::new(req::print_line_rle::PrintLineRle {
                    repeats: *repeats,
                    kind: req::print_line_rle::RleKind::Rle5d,
                    count: *rle_runs_count,
                    data: data.clone(),
                }),
                PrintCommand::Rle6x {
                    repeats,
                    rle_runs_count,
                    data,
                } => Box::new(req::print_line_rle::PrintLineRle {
                    repeats: *repeats,
                    kind: req::print_line_rle::RleKind::Rle6x,
                    count: *rle_runs_count,
                    data: data.clone(),
                }),
                PrintCommand::Rle6d {
                    repeats,
                    rle_runs_count,
                    data,
                } => Box::new(req::print_line_rle::PrintLineRle {
                    repeats: *repeats,
                    kind: req::print_line_rle::RleKind::Rle6d,
                    count: *rle_runs_count,
                    data: data.clone(),
                }),
            }
        })
        .collect()
}

/// 打印计划错误。
#[derive(Debug, Snafu, PartialEq, Eq)]
pub enum PrintError {
    #[snafu(display("bitmap width {width} exceeds the {max}-dot limit of a single print line"))]
    LineTooWide { width: usize, max: usize },

    #[snafu(display("page line limit {lines} must be in 1..={max}"))]
    InvalidPageLines { lines: usize, max: usize },

    #[snafu(transparent)]
    Bitmap { source: BitmapError },

    #[snafu(transparent)]
    Rle { source: RleTransformError },
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
/// 一次返回所有页；很大的位图可以用 [`print_bitmap_page`] 按 `start` 逐页生成。
///
/// 每页第一行都按全白基准处理，D 模式不会跨页引用上一页的解码结果。
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

fn push_feed(out: &mut Vec<PrintCommand>, mut lines: usize) {
    while lines > 0 {
        let chunk = lines.min(u32::MAX as usize) as u32;
        out.push(PrintCommand::Feed { lines: chunk });
        lines -= chunk as usize;
    }
}
