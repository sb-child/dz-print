// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 设置打印浓度
//!
//! - `DarknessV2V1V0Tl`: v2,v1,v0机型: `1f 43`

use super::der;

/// 设置打印浓度: v2,v1,v0机型: `1f 43`
#[der(..CmdTemplate)]
pub struct DarknessV2V1V0Tl {
    #[deku(magic = b"\x1f\x43")]
    /// - v2,v1: darkness = 打印浓度。取值 `0..=14`。
    /// - v0: darkness = 打印浓度 + 1。取值 `1..=15`。
    darkness: u8,
    _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;
    #[test]
    fn test_darkness_v2v1v0_tl_1() {
        let b = DarknessV2V1V0Tl {
            darkness: 5,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 43 05");
        eq(&enc, &exp);
    }
}
