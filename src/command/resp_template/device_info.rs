// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析设备信息（`0x7f`，语义未确认）
//!
//! - `DeviceInfoV2V1V0Tl`: v2,v1,v0机型: `1f 7f`

use super::der;

/// 解析设备信息: v2,v1,v0机型: `1f 7f`
///
/// 语义还没确认。DP27P 实测载荷是 16 字节全 `0x00`，这里先原样保留，
/// 长度是否随固件变化也未知。
#[der(..RespTemplate)]
pub struct DeviceInfoV2V1V0Tl {
    #[deku(magic = b"\x1f\x7f")]
    #[deku(read_all)]
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    #[test]
    fn parse_dp27p() {
        let tl: DeviceInfoV2V1V0Tl = dec_tl(&hexdec(
            "1f 7f 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00",
        ));
        eq(
            &tl.data,
            &hexdec("00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00"),
        );
    }

    #[test]
    fn parse_empty() {
        let tl: DeviceInfoV2V1V0Tl = dec_tl(&hexdec("1f 7f"));
        assert!(tl.data.is_empty());
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: DeviceInfoV2V1V0Tl = dec_tl(&hexdec("1f 7e 00 00"));
    }
}
