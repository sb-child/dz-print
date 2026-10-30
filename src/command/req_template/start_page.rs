// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 开始页命令
//!
//! - `StartPageV2Tl`: v2机型: `1f 20`
//! - `StartPageV0Tl`: v0机型: `1b 40`
//! - v1机型: 没有这种命令，不需要开始页。

use super::der;

/// 开始页命令: v2机型: `1f 20`
#[der(..CmdTemplate)]
pub struct StartPageV2Tl {
    #[deku(magic = b"\x1f\x20")]
    #[deku(endian = "big")]
    /// page_key = 本页的编号。范围 `1..=65534`，溢出重置到 `1`。
    pub page_key: u16,
    #[deku(magic = b"\x00\x00\x00\x00")]
    /// print_separate_line = 是否打印分隔线，1字节布尔值。
    pub print_separate_line: bool,
    #[deku(magic = b"\x00")]
    pub _tail: (),
}

/// 开始页命令: v0机型: `1b 40`
#[der(..CmdTemplate)]
pub struct StartPageV0Tl {
    #[deku(magic = b"\x1b\x40")]
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_start_page_v2_tl_1() {
        let b = StartPageV2Tl {
            page_key: 30,
            print_separate_line: false,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 20 00 1e 00 00 00 00 00 00");
        eq(&enc, &exp);
    }

    #[test]
    fn test_start_page_v2_tl_2() {
        let b = StartPageV2Tl {
            page_key: 65534,
            print_separate_line: true,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 20 ff fe 00 00 00 00 01 00");
        eq(&enc, &exp);
    }

    #[test]
    fn test_start_page_v0_tl_1() {
        let b = StartPageV0Tl {
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1b 40");
        eq(&enc, &exp);
    }
}
