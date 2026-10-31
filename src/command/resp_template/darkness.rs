// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析打印浓度
//!
//! - `DarknessV2V1V0Tl`: v2,v1,v0机型: `1f 43`

use super::der;

/// 解析打印浓度: v2,v1,v0机型: `1f 43`
#[der(..RespTemplate)]
pub struct DarknessV2V1V0Tl {
    #[deku(magic = b"\x1f\x43")]
    /// - v2,v1: darkness = 打印浓度。取值 `0..=14`。
    /// - v0: darkness = 打印浓度 + 1。取值 `1..=15`。
    pub darkness: u8,
    /// 载荷长度随固件变化。
    #[deku(read_all)]
    pub _tail: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    #[test]
    fn parse_dp305p() {
        let tl: DarknessV2V1V0Tl = dec_tl(&hexdec("1f 43 05 0f 00 5f"));
        assert_eq!(tl.darkness, 0x05);
        eq(&tl._tail, &hexdec("0f 00 5f"));
    }

    /// 没有额外尾部时也要能解析
    #[test]
    fn parse_without_tail() {
        let tl: DarknessV2V1V0Tl = dec_tl(&hexdec("1f 43 00"));
        assert_eq!(tl.darkness, 0x00);
        assert!(tl._tail.is_empty());
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: DarknessV2V1V0Tl = dec_tl(&hexdec("1f 44 05"));
    }
}
