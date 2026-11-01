// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 位级 RLE：4 / 5 / 6 位符号。
//!
//! 游程拆分在 [`BitmapLine::runs_bits`](crate::bitmap::BitmapLine::runs_bits)，
//! 码本和符号布局见 [`RleBits`] 和 `rle.md`。

use super::{RleTransformError, Run};
use crate::bitmap::BitmapLine;
use std::sync::OnceLock;

pub const RLE4_INDEX_LUT: [usize; 8] = [1, 2, 3, 4, 9, 14, 29, 88];
pub const RLE5_INDEX_LUT: [usize; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 24, 36, 48, 120];
pub const RLE6_INDEX_LUT: [usize; 32] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 41, 62, 83, 104, 125,
    146, 167, 188, 209, 230, 461, 923,
];

impl BitmapLine<'_> {
    /// 把这一行拆成黑白游程（MSB first）。
    ///
    /// 空行返回 [`RleTransformError::EmptyData`]。
    pub fn runs_bits(&self) -> Result<Vec<Run<bool>>, RleTransformError> {
        let data = self.bits().to_bitvec();
        let len = data.len();
        let first_bit: bool = *(data.first().ok_or(RleTransformError::EmptyData)?);
        let mut shifted = data.clone();
        shifted.shift_end(1);
        let xored = data.clone() ^ shifted;
        let prepend_zero = if !first_bit { Some(0) } else { None };
        let mut points = prepend_zero
            .into_iter()
            .chain(xored.iter_ones())
            .chain(std::iter::once(len));
        let mut prev = points.next().expect("line is non-empty");
        let res: Vec<Run<bool>> = points
            .enumerate()
            .map(|(idx, point)| {
                let count = point - prev;
                prev = point;
                let bitval = (idx % 2 != 0) ^ first_bit;
                Run {
                    value: bitval,
                    count,
                }
            })
            .collect();
        Ok(res)
    }
}

/// 位级 RLE 的符号宽度。
///
/// 符号是 `E i...`：`E` 是元素位（黑为 `1`，白为 `0`），后面是码本索引。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RleBits {
    /// 4 位符号：`E iii`，码本 [`RLE4_INDEX_LUT`]。
    Bits4,
    /// 5 位符号：`E iiii`，码本 [`RLE5_INDEX_LUT`]。
    Bits5,
    /// 6 位符号：`E iiiii`，码本 [`RLE6_INDEX_LUT`]。
    Bits6,
}

impl RleBits {
    /// 码本：索引 -> 游程长度。
    pub fn lut(self) -> &'static [usize] {
        match self {
            Self::Bits4 => &RLE4_INDEX_LUT,
            Self::Bits5 => &RLE5_INDEX_LUT,
            Self::Bits6 => &RLE6_INDEX_LUT,
        }
    }

    /// 索引占的位数。
    pub const fn index_bits(self) -> u8 {
        match self {
            Self::Bits4 => 3,
            Self::Bits5 => 4,
            Self::Bits6 => 5,
        }
    }

    /// 一个符号的总位数。
    pub const fn symbol_bits(self) -> u8 {
        self.index_bits() + 1
    }

    /// 长度 -> 最优码本索引的查表，表长为 `码本最大值 + 1`。
    ///
    /// 更长的游程在 [`push_run`] 里先反复取最大码本值，再查这张表。
    fn split_table(self) -> &'static [u8] {
        static T4: OnceLock<Vec<u8>> = OnceLock::new();
        static T5: OnceLock<Vec<u8>> = OnceLock::new();
        static T6: OnceLock<Vec<u8>> = OnceLock::new();
        let cell = match self {
            Self::Bits4 => &T4,
            Self::Bits5 => &T5,
            Self::Bits6 => &T6,
        };
        cell.get_or_init(|| build_split_table(self.lut()))
    }
}

/// 用动态规划求每个长度最少需要几个符号，记录最后一次用的码本索引。
fn build_split_table(lut: &[usize]) -> Vec<u8> {
    let max = *lut.last().expect("lut is non-empty");
    let mut best = vec![u32::MAX; max + 1];
    let mut pick = vec![0u8; max + 1];
    best[0] = 0;
    for n in 1..=max {
        for (idx, &value) in lut.iter().enumerate() {
            if value <= n && best[n - value] + 1 < best[n] {
                best[n] = best[n - value] + 1;
                pick[n] = idx as u8;
            }
        }
    }
    pick
}

/// MSB-first 位写入器，末尾不足一字节时补 `0`。
struct BitWriter {
    out: Vec<u8>,
    acc: u8,
    used: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            acc: 0,
            used: 0,
        }
    }

    fn push(&mut self, value: u16, bits: u8) {
        for shift in (0..bits).rev() {
            self.acc = (self.acc << 1) | ((value >> shift) & 1) as u8;
            self.used += 1;
            if self.used == 8 {
                self.out.push(self.acc);
                self.acc = 0;
                self.used = 0;
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.acc <<= 8 - self.used;
            self.out.push(self.acc);
        }
        self.out
    }
}

/// 位级 RLE 编码结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RleBitStream {
    /// 符号数量。
    pub symbol_count: usize,
    /// 符号按 MSB-first 拼接后的位流，末尾不足一字节补 `0`。
    pub data: Vec<u8>,
}

impl RleBitStream {
    /// 编码一行。
    ///
    /// - `prev` 为 `None`：X 模式，直接编码这一行。
    /// - `prev` 为 `Some`：D 模式，先和上一行 XOR 再编码。
    ///
    /// 两种模式都会丢掉尾部的白色游程，解码端按行宽补白。
    pub fn encode(
        line: BitmapLine<'_>,
        prev: Option<BitmapLine<'_>>,
        bits: RleBits,
    ) -> Result<Self, RleTransformError> {
        let mut runs: Vec<Run<bool>> = match prev {
            None => line.runs_bits()?,
            Some(prev) => {
                if prev.len() != line.len() {
                    return Err(RleTransformError::LineWidthMismatch {
                        expected: line.len(),
                        actual: prev.len(),
                    });
                }
                let diff = prev.bits().to_bitvec() ^ line.bits().to_bitvec();
                BitmapLine::new(&diff).runs_bits()?
            }
        };
        // 尾部白色不编码，解码端会补白。
        while matches!(runs.last(), Some(run) if !run.value) {
            runs.pop();
        }

        let mut writer = BitWriter::new();
        let mut symbol_count = 0usize;
        for run in runs {
            symbol_count += push_run(&mut writer, bits, run.value, run.count);
        }
        Ok(Self {
            symbol_count,
            data: writer.finish(),
        })
    }
}

/// 把一个游程拆成若干符号，返回符号数。
fn push_run(writer: &mut BitWriter, bits: RleBits, value: bool, mut count: usize) -> usize {
    let lut = bits.lut();
    let max = *lut.last().expect("lut is non-empty");
    let max_index = (lut.len() - 1) as u8;
    let table = bits.split_table();
    let mut symbols = 0;
    // 最大码本值远大于次大值，超过它的部分反复取最大值一定最优。
    while count > max {
        push_symbol(writer, bits, value, max_index);
        count -= max;
        symbols += 1;
    }
    while count > 0 {
        let index = table[count];
        push_symbol(writer, bits, value, index);
        count -= lut[index as usize];
        symbols += 1;
    }
    symbols
}

fn push_symbol(writer: &mut BitWriter, bits: RleBits, value: bool, index: u8) {
    let symbol = ((value as u16) << bits.index_bits()) | index as u16;
    writer.push(symbol, bits.symbol_bits());
}
