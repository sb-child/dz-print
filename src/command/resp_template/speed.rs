// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析打印速度
//!
//! - `SpeedV2V1V0Tl`: v2,v1,v0机型: `1f 44`

use super::der;

/// 解析打印速度: v2,v1,v0机型: `1f 44`
#[der(..RespTemplate)]
pub struct SpeedV2V1V0Tl {
    #[deku(magic = b"\x1f\x44")]
    /// - v2,v1: speed = 打印速度。取值 `0..=4`。
    /// - v0: speed = 打印速度 + 1。取值 `1..=5`。
    pub speed: u8,
    /// 载荷长度随固件变化，这里整段收下。
    #[deku(read_all)]
    pub _tail: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    /// 12 字节载荷：`1f 44` + 10 字节，只取 `[0]`。
    #[test]
    fn parse_with_tail() {
        let tl: SpeedV2V1V0Tl = dec_tl(&hexdec("1f 44 02 00 00 00 00 00 00 00 00 00"));
        assert_eq!(tl.speed, 0x02);
        eq(&tl._tail, &hexdec("00 00 00 00 00 00 00 00 00"));
    }

    /// 固件只回 1 字节数据时也要能解析。
    #[test]
    fn parse_without_tail() {
        let tl: SpeedV2V1V0Tl = dec_tl(&hexdec("1f 44 04"));
        assert_eq!(tl.speed, 0x04);
        assert!(tl._tail.is_empty());
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: SpeedV2V1V0Tl = dec_tl(&hexdec("1f 43 02"));
    }
}
