// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 字节级 RLE（`1f 29`）。
//!
//! 游程拆分在 [`BitmapLine::runs_bytes`](crate::bitmap::BitmapLine::runs_bytes)，
//! 符号规则见 [`RleByteStream::encode`]。

use deku::bitvec::BitField;

use crate::bitmap::BitmapLine;

use super::{RleTransformError, Run};

impl BitmapLine<'_> {
    /// 按字节拆游程（不足一字节的行在右侧补 0）。
    ///
    /// 空行返回 [`RleTransformError::EmptyData`]。
    pub fn runs_bytes(&self) -> Result<Vec<Run<u8>>, RleTransformError> {
        if self.is_empty() {
            return Err(RleTransformError::EmptyData);
        }
        let bytes: Vec<u8> = self
            .bits()
            .chunks(8)
            .map(|chunk| {
                let val = chunk.load_be::<u8>();
                if chunk.len() >= 8 {
                    return val;
                }
                val << (8 - chunk.len())
            })
            .collect();
        let res: Vec<Run<u8>> = bytes
            .chunk_by(|a, b| a == b)
            .map(|chunk| Run {
                value: chunk[0],
                count: chunk.len(),
            })
            .collect();
        Ok(res)
    }
}

/// 字节级 RLE 编码结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RleByteStream {
    /// RLE 数据字节数。
    pub byte_count: usize,
    /// RLE 数据。
    pub data: Vec<u8>,
}

impl RleByteStream {
    /// 编码一行。
    ///
    /// 符号规则见 `rle.md`：
    ///
    /// - 单字节 `<= 0xC0`：原样输出。
    /// - 单字节 `>= 0xC1`：`C1 <字节>`。
    /// - 连续 `2..=63` 个相同字节：`<0xC0 | 数量> <字节>`。
    /// - 超过 63 个就继续切分。
    ///
    /// 尾部的 `0x00`（白色）游程不编码，解码端会补白。
    pub fn encode(line: BitmapLine<'_>) -> Result<Self, RleTransformError> {
        let mut runs: Vec<Run<u8>> = line.runs_bytes()?;
        while matches!(runs.last(), Some(run) if run.value == 0x00) {
            runs.pop();
        }
        let mut data = Vec::new();
        for run in runs {
            let mut count = run.count;
            while count > 0 {
                let chunk = count.min(63);
                if chunk == 1 {
                    if run.value <= 0xC0 {
                        data.push(run.value);
                    } else {
                        data.push(0xC1);
                        data.push(run.value);
                    }
                } else {
                    data.push(0xC0 | chunk as u8);
                    data.push(run.value);
                }
                count -= chunk;
            }
        }
        Ok(Self {
            byte_count: data.len(),
            data,
        })
    }
}
