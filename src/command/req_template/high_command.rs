// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 激活 / 结束 `> 0x80` 命令
//!
//! - `HighCommandV2V1V0Tl`: v2,v1,v0机型: `1f 80`

use super::der;

/// 激活 / 结束 `> 0x80` 命令: v2,v1,v0机型: `1f 80`
#[der(..CmdTemplate)]
pub struct HighCommandV2V1V0Tl {
    #[deku(magic = b"\x1f\x80")]
    /// mode = `0x00` 结束激活；`0x7f` 激活；`0x49` 另一种激活参数。
    pub mode: u8,
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_high_command_enable() {
        let b = HighCommandV2V1V0Tl {
            mode: 0x7f,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 80 7f");
        eq(&enc, &exp);
    }

    #[test]
    fn test_high_command_disable() {
        let b = HighCommandV2V1V0Tl {
            mode: 0x00,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 80 00");
        eq(&enc, &exp);
    }

    #[test]
    fn test_high_command_enable_alt() {
        let b = HighCommandV2V1V0Tl {
            mode: 0x49,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 80 49");
        eq(&enc, &exp);
    }
}
