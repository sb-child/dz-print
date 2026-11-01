// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 为一行位图选择并生成 RLE 命令。
//!
//! 只有编码后比普通行（`raw_len` 字节）更小才返回 `Some`，否则返回 `None`
//! 让调用方发 [`PrintCommand::Line`]。位级模式默认同时算 X 和 D，取较小的那个；
//! `*XOnly` 只算 X；`prev` 为 `None`（第一行、或刚 `Feed` 过）时也只算 X。

use super::{PrintCommand, PrintError, RleMode};
use crate::{
    bitmap::BitmapLine,
    rle::{RleBitStream, RleBits, RleByteStream},
};

/// 选择并生成 RLE 行命令。
///
/// 只有编码后比普通行（`raw_len` 字节）更小才返回 `Some`，否则返回 `None`
/// 让调用方发 [`PrintCommand::Line`]。位级模式默认同时算 X 和 D，取较小的那个；
/// `*XOnly` 只算 X；`prev` 为 `None`（第一行）时也只算 X。
pub(super) fn select_rle(
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
