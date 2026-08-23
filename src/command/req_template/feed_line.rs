// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 走纸(打印空行)
//!
//! - `FeedLineV2Tl`: v2机型: `1f 22`
//! - `FeedLineV1V0Tl`: v1,v0机型: `1b 4a`

use super::der;
use super::varint::VarAuto;

/// 走纸(打印空行): v2机型: `1f 22`
#[der(..CmdTemplate)]
pub struct FeedLineV2Tl {
    #[deku(magic = b"\x1f\x22")]
    /// lines = (走纸行数 - 1)。0 代表只走一行。取值 `0..=16383`。
    lines: VarAuto,
    pub _tail: (),
}

/// 走纸(打印空行): v1,v0机型: `1b 4a`
#[der(..CmdTemplate)]
pub struct FeedLineV1V0Tl {
    #[deku(magic = b"\x1b\x4a")]
    /// lines = 走纸行数。取值 `1..=255`。
    lines: u8,
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_feed_v2_tl_1() {
        let b = FeedLineV2Tl {
            lines: VarAuto::new(20),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 22 14");
        eq(&enc, &exp);
    }

    #[test]
    fn test_feed_v2_tl_2() {
        let b = FeedLineV2Tl {
            lines: VarAuto::new(16383),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 22 ff ff");
        eq(&enc, &exp);
    }

    #[test]
    fn test_feed_v1v0_tl_1() {
        let b = FeedLineV1V0Tl {
            lines: 20,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1b 4a 14");
        eq(&enc, &exp);
    }

    #[test]
    fn test_feed_v1v0_tl_2() {
        let b = FeedLineV1V0Tl {
            lines: 255,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1b 4a ff");
        eq(&enc, &exp);
    }
}
