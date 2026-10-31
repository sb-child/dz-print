// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析设备名
//!
//! - `DeviceNameV2V1V0Tl`: v2,v1,v0机型: `1f 79`

use super::der;

/// 解析设备名: v2,v1,v0机型: `1f 79`
///
/// 载荷是 `型号-序列号` 格式、`0x00` 结尾的字符串。模板只保留原始字节，
/// 解码交给 [`resp::device_name`](crate::command::resp::device_name)。
#[der(..RespTemplate)]
pub struct DeviceNameV2V1V0Tl {
    #[deku(magic = b"\x1f\x79")]
    #[deku(read_all)]
    pub device_name: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    /// BLE 实测: `1F 79 11 "DP305P-Y60870279\0"`
    #[test]
    fn parse_dp305p() {
        let tl: DeviceNameV2V1V0Tl = dec_tl(&hexdec(
            "1f 79 44 50 33 30 35 50 2d 59 36 30 38 37 30 32 37 39 00",
        ));
        eq(
            &tl.device_name,
            &hexdec("44 50 33 30 35 50 2d 59 36 30 38 37 30 32 37 39 00"),
        );
    }

    #[test]
    fn parse_without_nul() {
        let tl: DeviceNameV2V1V0Tl = dec_tl(&hexdec("1f 79 44 50 32 37 50"));
        eq(&tl.device_name, &hexdec("44 50 32 37 50"));
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: DeviceNameV2V1V0Tl = dec_tl(&hexdec("1f 78 44 50 32 37 50"));
    }
}
