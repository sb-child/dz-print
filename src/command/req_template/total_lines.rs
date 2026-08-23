// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 设置本次打印总行数
//!
//! - `TotalLinesV2Tl`: v2机型: `1f 26`
//! - `TotalLinesV1Tl`: v1机型: `1f 26`
//! - v0机型: 没有这种命令。

use super::der;
use super::varint::{VarAuto, VarC0Be16};

/// 设置总行数: v2机型: `1f 26`
#[der(..CmdTemplate)]
pub struct TotalLinesV2Tl {
    #[deku(magic = b"\x1f\x26")]
    /// lines = 本次打印总行数(含空白行)。取值 `0..=16383`。
    lines: VarAuto,
    _tail: (),
}

/// 设置总行数: v1机型: `1f 26`
#[der(..CmdTemplate)]
pub struct TotalLinesV1Tl {
    #[deku(magic = b"\x1f\x26")]
    /// lines = 本次打印总行数(含空白行)。取值 `0..=65535`。
    lines: VarC0Be16,
    _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_total_lines_v2_tl_1() {
        let b = TotalLinesV2Tl {
            lines: VarAuto::new(100),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 26 64");
        eq(&enc, &exp);
    }

    #[test]
    fn test_total_lines_v2_tl_2() {
        let b = TotalLinesV2Tl {
            lines: VarAuto::new(16383),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 26 ff ff");
        eq(&enc, &exp);
    }

    #[test]
    fn test_total_lines_v1_tl_1() {
        let b = TotalLinesV1Tl {
            lines: VarC0Be16::new(100),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 26 c0 00 64");
        eq(&enc, &exp);
    }

    #[test]
    fn test_total_lines_v1_tl_2() {
        let b = TotalLinesV1Tl {
            lines: VarC0Be16::new(0x1234),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 26 c0 12 34");
        eq(&enc, &exp);
    }
}
