// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 纸张间隔长度(仅v2机型)
//!
//! - `GapLengthV2Tl`: v2机型: `1f 45`

use super::der;
use super::varint::VarAuto;

/// 设置纸张间隔长度: v2机型: `1f 45`
#[der(..CmdTemplate)]
pub struct GapLengthV2Tl {
    #[deku(magic = b"\x1f\x45")]
    /// gap = 纸张间隔(0.01mm)。取值 `50..=4194303`。
    gap: VarAuto,
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_gap_length_v2_tl_1() {
        let b = GapLengthV2Tl {
            gap: VarAuto::new(50),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 45 32");
        eq(&enc, &exp);
    }

    #[test]
    fn test_gap_length_v2_tl_2() {
        let b = GapLengthV2Tl {
            gap: VarAuto::new(20000),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 45 c0 4e 20");
        eq(&enc, &exp);
    }
}
