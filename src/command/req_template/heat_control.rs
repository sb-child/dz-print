// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 加热控制
//!
//! - `HeatControlV2V1V0Tl`: v2,v1,v0机型: `1f 25`

use super::der;
use super::varint::VarFixed2;

/// 加热控制: v2,v1,v0机型: `1f 25`
#[der(..CmdTemplate)]
pub struct HeatControlV2V1V0Tl {
    #[deku(magic = b"\x1f\x25")]
    /// - start_dots = 单行最大黑点数。取值 `0..=16383`。
    start_dots: VarFixed2,
    /// - end_dots = 滚动窗口平均黑点数。取值 `0..=16383`。
    end_dots: VarFixed2,
    _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;
    #[test]
    fn test_heat_control_v2v1v0_tl_1() {
        let b = HeatControlV2V1V0Tl {
            start_dots: VarFixed2::new(20),
            end_dots: VarFixed2::new(30),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 25 c0 14 c0 1e");
        eq(&enc, &exp);
    }

    #[test]
    fn test_heat_control_v2v1v0_tl_2() {
        let b = HeatControlV2V1V0Tl {
            start_dots: VarFixed2::new(7324),
            end_dots: VarFixed2::new(4834),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 25 dc 9c d2 e2");
        eq(&enc, &exp);
    }
}
