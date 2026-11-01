// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 位图数据。
//!
//! [`Bitmap`] 用一整块 [`BitVec`] 按行优先存储，行通过 [`BitmapLine`] /
//! [`BitmapLineMut`] 借用访问，不复制数据。
//!
//! 位序是 MSB first：一行最左边的点是第一个字节的最高位。
//!
//! # 用法
//!
//! ```
//! use dz_print::bitmap::Bitmap;
//!
//! let mut bitmap = Bitmap::new(16, 2)?;       // 16 x 2，全白
//! assert_eq!((bitmap.width(), bitmap.height()), (16, 2));
//!
//! // 写第 0 行：最左边的点
//! bitmap.line_mut(0)?.set(0, true);
//! // 整行涂黑
//! bitmap.line_mut(1)?.fill(true);
//!
//! // 读一行：first_one / last_one / to_bytes 都不复制
//! let line = bitmap.line(1)?;
//! assert_eq!(line.first_one(), Some(0));
//! assert_eq!(line.last_one(), Some(15));
//! assert_eq!(line.to_bytes(), vec![0xFF, 0xFF]);
//!
//! // 按行遍历
//! assert_eq!(bitmap.lines().count(), 2);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! 整块位缓冲可以用 [`Bitmap::bits`] 借用，或用 [`Bitmap::from_bits`] /
//! [`Bitmap::into_bits`] 和 `deku::bitvec::BitVec<u8, Msb0>` 互相转换。

use deku::bitvec::{BitField, BitSlice, BitVec, BitView, Msb0};
use snafu::Snafu;
use std::ops::Range;

/// 位图操作错误。
#[derive(Debug, Snafu, PartialEq, Eq)]
pub enum BitmapError {
    #[snafu(display("bitmap size must be non-zero, got {width} x {height}"))]
    EmptySize { width: usize, height: usize },

    #[snafu(display("bitmap size {width} x {height} overflows usize"))]
    SizeOverflow { width: usize, height: usize },

    #[snafu(display("line {index} out of range, height = {height}"))]
    LineOutOfRange { index: usize, height: usize },

    #[snafu(display("pixel {x} out of range, width = {width}"))]
    PixelOutOfRange { x: usize, width: usize },

    #[snafu(display("length mismatch: expected {expected}, got {actual}"))]
    LengthMismatch { expected: usize, actual: usize },
}

/// 一张位图。
///
/// 不变量：`bits.len() == width * height`，`width` / `height` 都不为 0。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    width: usize,
    height: usize,
    /// 行优先，`Msb0`。
    bits: BitVec<u8, Msb0>,
}

impl Bitmap {
    /// 新建一张全 0 位图。
    pub fn new(width: usize, height: usize) -> Result<Self, BitmapError> {
        let len = checked_len(width, height)?;
        Ok(Self {
            width,
            height,
            bits: BitVec::repeat(false, len),
        })
    }

    /// 从已有的位缓冲接管，长度必须等于 `width * height`。
    pub fn from_bits(
        width: usize,
        height: usize,
        bits: BitVec<u8, Msb0>,
    ) -> Result<Self, BitmapError> {
        let expected = checked_len(width, height)?;
        if bits.len() != expected {
            return Err(BitmapError::LengthMismatch {
                expected,
                actual: bits.len(),
            });
        }
        Ok(Self {
            width,
            height,
            bits,
        })
    }

    /// 宽度（像素 / 位）。
    pub fn width(&self) -> usize {
        self.width
    }

    /// 高度（行数）。
    pub fn height(&self) -> usize {
        self.height
    }

    /// 只读的整块位缓冲。
    pub fn bits(&self) -> &BitSlice<u8, Msb0> {
        &self.bits
    }

    /// 可写的整块位缓冲。
    ///
    /// 调用方仍然必须保持每行等宽，一般优先用 [`Self::line_mut`]。
    pub fn bits_mut(&mut self) -> &mut BitSlice<u8, Msb0> {
        &mut self.bits
    }

    /// 取出整块位缓冲。
    pub fn into_bits(self) -> BitVec<u8, Msb0> {
        self.bits
    }

    /// 把所有位设为同一个值。
    pub fn fill(&mut self, value: bool) {
        self.bits.fill(value);
    }

    /// 只读访问第 `y` 行。
    pub fn line(&self, y: usize) -> Result<BitmapLine<'_>, BitmapError> {
        let range = self.line_range(y)?;
        Ok(BitmapLine::new(&self.bits[range]))
    }

    /// 可写访问第 `y` 行。
    pub fn line_mut(&mut self, y: usize) -> Result<BitmapLineMut<'_>, BitmapError> {
        let range = self.line_range(y)?;
        Ok(BitmapLineMut {
            bits: &mut self.bits[range],
        })
    }

    /// 按行遍历，`height` 由构造保证，不会失败。
    pub fn lines(&self) -> impl Iterator<Item = BitmapLine<'_>> + '_ {
        self.bits.chunks_exact(self.width).map(BitmapLine::new)
    }

    fn line_range(&self, index: usize) -> Result<Range<usize>, BitmapError> {
        let start = index
            .checked_mul(self.width)
            .ok_or(BitmapError::SizeOverflow {
                width: self.width,
                height: self.height,
            })?;
        let end = start
            .checked_add(self.width)
            .ok_or(BitmapError::SizeOverflow {
                width: self.width,
                height: self.height,
            })?;
        if end > self.bits.len() {
            return Err(BitmapError::LineOutOfRange {
                index,
                height: self.height,
            });
        }
        Ok(start..end)
    }
}

/// 一行的只读视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitmapLine<'a> {
    bits: &'a BitSlice<u8, Msb0>,
}

impl<'a> BitmapLine<'a> {
    /// 用一段位切片构造行视图。
    pub fn new(bits: &'a BitSlice<u8, Msb0>) -> Self {
        Self { bits }
    }

    /// 这一行的位宽。
    pub fn len(&self) -> usize {
        self.bits.len()
    }

    /// 这一行是否没有点。
    pub fn is_empty(&self) -> bool {
        self.bits.is_empty()
    }

    /// 这一行是否没有任何黑点。
    pub fn is_blank(&self) -> bool {
        self.first_one().is_none()
    }

    /// 拿到背后的位切片。
    pub fn bits(&self) -> &'a BitSlice<u8, Msb0> {
        self.bits
    }

    /// 读取第 `x` 个点。
    pub fn get(&self, x: usize) -> Option<bool> {
        self.bits.get(x).map(|bit| *bit)
    }

    /// 第一个黑点（`true`）的位置。
    pub fn first_one(&self) -> Option<usize> {
        self.bits.first_one()
    }

    /// 最后一个黑点（`true`）的位置。
    pub fn last_one(&self) -> Option<usize> {
        // 不用 `BitSlice::last_one` / `iter_ones().last()`：bitvec 1.1.1 在
        // 整行全 0 的短切片上会触发 `attempt to subtract with overflow`。
        self.bits.iter().rposition(|bit| *bit)
    }

    /// 裁掉右侧空白，返回一个更短的行视图。
    pub fn trim_end(&self) -> Self {
        match self.last_one() {
            Some(last) => Self::new(&self.bits[..last + 1]),
            None => Self::new(&self.bits[..0]),
        }
    }

    /// 按 MSB-first 打包成字节，最后一段不足 8 位时左对齐补 `0`。
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.bits.len().div_ceil(8)];
        for (byte, chunk) in out.iter_mut().zip(self.bits.chunks(8)) {
            let val = chunk.load_be::<u8>();
            *byte = if chunk.len() >= 8 {
                val
            } else {
                val << (8 - chunk.len())
            };
        }
        out
    }
}

/// 一行的可写视图。
#[derive(Debug)]
pub struct BitmapLineMut<'a> {
    bits: &'a mut BitSlice<u8, Msb0>,
}

impl<'a> BitmapLineMut<'a> {
    /// 这一行的位宽。
    pub fn len(&self) -> usize {
        self.bits.len()
    }

    /// 这一行是否没有点。
    pub fn is_empty(&self) -> bool {
        self.bits.is_empty()
    }

    /// 只读视图。
    pub fn as_line(&self) -> BitmapLine<'_> {
        BitmapLine::new(self.bits)
    }

    /// 拿到背后的位切片。
    pub fn bits(&self) -> &BitSlice<u8, Msb0> {
        self.bits
    }

    /// 拿到背后的可变位切片。
    pub fn bits_mut(&mut self) -> &mut BitSlice<u8, Msb0> {
        self.bits
    }

    /// 设置第 `x` 个点。
    ///
    /// # Panics
    ///
    /// `x >= self.len()` 时 panic，和切片索引一致。
    pub fn set(&mut self, x: usize, value: bool) {
        self.bits.set(x, value);
    }

    /// 设置第 `x` 个点，越界返回错误。
    pub fn try_set(&mut self, x: usize, value: bool) -> Result<(), BitmapError> {
        let width = self.bits.len();
        match self.bits.get_mut(x) {
            Some(mut bit) => {
                *bit = value;
                Ok(())
            }
            None => Err(BitmapError::PixelOutOfRange { x, width }),
        }
    }

    /// 把整行设为同一个值。
    pub fn fill(&mut self, value: bool) {
        self.bits.fill(value);
    }

    /// 用另一段等长的位切片覆盖整行。
    pub fn copy_from_bits(&mut self, src: &BitSlice<u8, Msb0>) -> Result<(), BitmapError> {
        if src.len() != self.bits.len() {
            return Err(BitmapError::LengthMismatch {
                expected: self.bits.len(),
                actual: src.len(),
            });
        }
        self.bits.copy_from_bitslice(src);
        Ok(())
    }

    /// 用字节覆盖整行，MSB-first。
    ///
    /// 字节数必须等于 `len().div_ceil(8)`；最后一个字节里超出位宽的低位会被忽略。
    pub fn copy_from_bytes(&mut self, src: &[u8]) -> Result<(), BitmapError> {
        let expected = self.bits.len().div_ceil(8);
        if src.len() != expected {
            return Err(BitmapError::LengthMismatch {
                expected,
                actual: src.len(),
            });
        }
        let src = src.view_bits::<Msb0>();
        self.bits.copy_from_bitslice(&src[..self.bits.len()]);
        Ok(())
    }
}

fn checked_len(width: usize, height: usize) -> Result<usize, BitmapError> {
    if width == 0 || height == 0 {
        return Err(BitmapError::EmptySize { width, height });
    }
    width
        .checked_mul(height)
        .ok_or(BitmapError::SizeOverflow { width, height })
}

#[cfg(test)]
mod tests;
