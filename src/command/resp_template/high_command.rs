// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析激活 / 结束 `> 0x80` 命令的响应
//!
//! - `HighCommandV2V1V0Tl`: v2,v1,v0机型: `1f 80`

use super::der;

/// 解析激活 / 结束 `> 0x80` 命令的响应: v2,v1,v0机型: `1f 80`
#[der(..RespTemplate)]
pub struct HighCommandV2V1V0Tl {
    #[deku(magic = b"\x1f\x80")]
    /// mode = 设备回显的激活参数。
    pub mode: u8,
    /// 其余字段，长度随固件变化。
    #[deku(read_all)]
    pub _tail: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    #[test]
    fn parse_enable() {
        let tl: HighCommandV2V1V0Tl = dec_tl(&hexdec("1f 80 7f"));
        assert_eq!(tl.mode, 0x7f);
        assert!(tl._tail.is_empty());
    }

    #[test]
    fn parse_disable() {
        let tl: HighCommandV2V1V0Tl = dec_tl(&hexdec("1f 80 00"));
        assert_eq!(tl.mode, 0x00);
    }

    #[test]
    fn parse_with_tail() {
        let tl: HighCommandV2V1V0Tl = dec_tl(&hexdec("1f 80 7f aa bb"));
        assert_eq!(tl.mode, 0x7f);
        eq(&tl._tail, &hexdec("aa bb"));
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: HighCommandV2V1V0Tl = dec_tl(&hexdec("1f 81 7f"));
    }
}
