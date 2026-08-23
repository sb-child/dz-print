// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 重复打印上一行
//!
//! - `RepeatLineV2V1Tl`: v2,v1机型: `1f 2e`
//! - v0机型: 没有这种命令。

use super::der;
use super::varint::VarAuto;

/// 重复打印上一行: v2机型: `1f 2e`
#[der(..CmdTemplate)]
pub struct RepeatLineV2V1Tl {
    #[deku(magic = b"\x1f\x2e")]
    /// lines = (重复打印行数 - 1)。0 代表只重复打印一行。
    /// - v2机型取值 `0..=16383`。
    /// - v1机型取值 `0..=191`。
    lines: VarAuto,
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_repeat_line_v2v1_tl_1() {
        let b = RepeatLineV2V1Tl {
            lines: VarAuto::new(40),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 2e 28");
        eq(&enc, &exp);
    }

    #[test]
    fn test_repeat_line_v2v1_tl_2() {
        let b = RepeatLineV2V1Tl {
            lines: VarAuto::new(191),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 2e bf");
        eq(&enc, &exp);
    }

    #[test]
    fn test_repeat_line_v2v1_tl_3() {
        let b = RepeatLineV2V1Tl {
            lines: VarAuto::new(16383),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 2e ff ff");
        eq(&enc, &exp);
    }
}
