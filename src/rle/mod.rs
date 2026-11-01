// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! RLE 编码：字节级 [`RleByteStream`] 和位级 [`RleBitStream`]。
//!
//! 位级 RLE 的符号是 `E i...`：`E` 是元素位（黑 `1` / 白 `0`），后面是码本索引。
//! 码本是 [`RLE4_INDEX_LUT`] / [`RLE5_INDEX_LUT`] / [`RLE6_INDEX_LUT`]，
//! 游程超过码本最大值时拆成多个符号（尽可能少）。
//!
//! 两种编码都会丢掉尾部的白色游程，解码端按行宽补白。
//!
//! # 用法
//!
//! ```
//! use dz_print::{
//!     bitmap::Bitmap,
//!     rle::{RleBitStream, RleBits, RleByteStream},
//! };
//!
//! let mut bitmap = Bitmap::new(32, 1)?;
//! bitmap.line_mut(0)?.fill(true);
//! let line = bitmap.line(0)?;
//!
//! // 字节级：32 个黑点 = 4 个 0xFF，编码成 0xC4 0xFF
//! let byte_rle = RleByteStream::encode(line)?;
//! assert_eq!(byte_rle.data, vec![0xC4, 0xFF]);
//!
//! // 位级 X 模式：直接编码当前行；D 模式先和 prev XOR
//! let x = RleBitStream::encode(line, None, RleBits::Bits4)?;
//! assert_eq!(x.data, vec![0xAE]);
//! let d = RleBitStream::encode(line, Some(line), RleBits::Bits4)?;
//! assert!(d.data.is_empty()); // 和上一行完全一样，不需要符号
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! 选择哪种模式、是否值得 RLE，由
//! [`crate::planner::print`] 里的 `select_rle` 决定；正常使用不需要直接调用这里。
//!
//! 行视图的游程拆分用 [`BitmapLine::runs_bits`](crate::bitmap::BitmapLine::runs_bits)
//! / [`BitmapLine::runs_bytes`](crate::bitmap::BitmapLine::runs_bytes)，它们是
//! [`Run`] 的唯一来源。实现见 `rle/bit.rs` 和 `rle/byte.rs`。

use snafu::Snafu;

mod bit;
mod byte;

#[cfg(test)]
mod tests;

pub use bit::{RLE4_INDEX_LUT, RLE5_INDEX_LUT, RLE6_INDEX_LUT, RleBitStream, RleBits};
pub use byte::RleByteStream;

#[derive(Snafu, Debug, PartialEq, Eq)]
pub enum RleTransformError {
    #[snafu(display("Data is empty."))]
    EmptyData,
    #[snafu(display("Line width mismatch: expected {expected}, got {actual}."))]
    LineWidthMismatch { expected: usize, actual: usize },
}

#[derive(Debug, PartialEq, Clone)]
pub struct Run<V> {
    pub value: V,
    pub count: usize,
}
