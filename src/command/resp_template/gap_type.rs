// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析纸张间隔类型
//!
//! - `GapTypeV0V1V2Tl`: v0,v1,v2机型: `1f 42`

use super::der;

/// 解析纸张间隔类型: v0,v1,v2机型: `1f 42`
#[der(..RespTemplate)]
pub struct GapTypeV0V1V2Tl {
    #[deku(magic = b"\x1f\x42")]
    /// gap_type = 纸张间隔类型。取值：
    /// - 0: 连续纸(Continuous)
    /// - 1: 定位孔纸(Hole)
    /// - 2: 间隙纸/不干胶纸(Gap)
    /// - 3: 黑标纸/卡纸(BlackMark)
    /// - 4: 透明贴(Transparent)
    pub gap_type: u8,
    /// 载荷长度随固件变化。
    #[deku(read_all)]
    pub _tail: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    #[test]
    fn parse_without_tail() {
        let tl: GapTypeV0V1V2Tl = dec_tl(&hexdec("1f 42 02"));
        assert_eq!(tl.gap_type, 0x02);
        assert!(tl._tail.is_empty());
    }

    #[test]
    fn parse_with_tail() {
        let tl: GapTypeV0V1V2Tl = dec_tl(&hexdec("1f 42 03 aa bb"));
        assert_eq!(tl.gap_type, 0x03);
        eq(&tl._tail, &hexdec("aa bb"));
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: GapTypeV0V1V2Tl = dec_tl(&hexdec("1f 43 02"));
    }
}
