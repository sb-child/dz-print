// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 设置打印速度
//!
//! - `SpeedV2V1V0Tl`: v2,v1,v0机型: `1f 44`

use super::der;

/// 设置打印速度: v2,v1,v0机型: `1f 44`
#[der(..CmdTemplate)]
pub struct SpeedV2V1V0Tl {
    #[deku(magic = b"\x1f\x44")]
    /// - v2,v1: speed = 打印速度。取值 `0..=4`。
    /// - v0: speed = 打印速度 + 1。取值 `1..=5`。
    speed: u8,
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;
    #[test]
    fn test_darkness_v2v1v0_tl_1() {
        let b = SpeedV2V1V0Tl {
            speed: 3,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 44 03");
        eq(&enc, &exp);
    }
}
