// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 纸张间隔类型
//!
//! - `GapTypeV0V1V2Tl`: v0,v1,v2机型: `1f 42`

use super::der;

/// 设置纸张间隔类型: v0,v1,v2机型: `1f 42`
#[der(..CmdTemplate)]
pub struct GapTypeV0V1V2Tl {
    #[deku(magic = b"\x1f\x42")]
    /// gap = 纸张间隔类型。取值：
    /// - 0: 连续纸(Continuous)
    /// - 1: 定位孔纸(Hole)
    /// - 2: 间隙纸/不干胶纸(Gap)
    /// - 3: 黑标纸/卡纸(BlackMark)
    /// - 4: 透明贴(Transparent)
    pub gap_type: u8,
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_gap_type_v0v1v2_tl_1() {
        let b = GapTypeV0V1V2Tl {
            gap_type: 2,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 42 02");
        eq(&enc, &exp);
    }
}
