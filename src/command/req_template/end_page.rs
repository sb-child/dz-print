// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 结束页命令
//!
//! - `EndPageV2Tl`: v2机型: `1f 28`
//! - `EndPageV1Tl`: v1机型: `0c`
//! - `EndPageV0Tl`: v0机型: `1d 56`

use super::der;

/// 结束页命令: v2机型: `1f 28`
#[der(..CmdTemplate)]
pub struct EndPageV2Tl {
    #[deku(magic = b"\x1f\x28")]
    _tail: (),
}

/// 结束页命令: v1机型: `0c`
#[der(..CmdTemplate)]
pub struct EndPageV1Tl {
    #[deku(magic = b"\x0c")]
    _tail: (),
}

/// 结束页命令: v0机型: `1d 56`
#[der(..CmdTemplate)]
pub struct EndPageV0Tl {
    #[deku(magic = b"\x1d\x56\x42\x00")]
    _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_end_page_v2_tl_1() {
        let b = EndPageV2Tl {
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 28");
        eq(&enc, &exp);
    }

    #[test]
    fn test_end_page_v1_tl_1() {
        let b = EndPageV1Tl {
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("0c");
        eq(&enc, &exp);
    }

    #[test]
    fn test_end_page_v0_tl_1() {
        let b = EndPageV0Tl {
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1d 56 42 00");
        eq(&enc, &exp);
    }
}
