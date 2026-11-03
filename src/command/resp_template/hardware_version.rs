// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析硬件版本
//!
//! - `HardwareVersionV2V1V0Tl`: v2,v1,v0机型: `1f 7a`

use super::der;

/// 解析硬件版本: v2,v1,v0机型: `1f 7a`
///
/// 首字节是 BCD 编码的版本号（`0x26` → `2.6`），其余字节语义未知，
/// 不同固件长度可能不同，统一放 `tail`。
#[der(..RespTemplate)]
pub struct HardwareVersionV2V1V0Tl {
    #[deku(magic = b"\x1f\x7a")]
    pub version: u8,
    #[deku(read_all)]
    pub tail: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    /// DP27P USB 实测: `1f 7a 26 01 04 01 04`
    #[test]
    fn parse_dp27p() {
        let tl: HardwareVersionV2V1V0Tl = dec_tl(&hexdec("1f 7a 26 01 04 01 04"));
        eq(&[tl.version], &[0x26]);
        eq(&tl.tail, &hexdec("01 04 01 04"));
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: HardwareVersionV2V1V0Tl = dec_tl(&hexdec("1f 7b 26 01 04"));
    }
}
