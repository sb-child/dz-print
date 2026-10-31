// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析蓝牙 MAC 地址
//!
//! - `BluetoothMacV2V1V0Tl`: v2,v1,v0机型: `1f 7d`

use super::der;

/// 解析蓝牙 MAC 地址: v2,v1,v0机型: `1f 7d`
///
/// 载荷布局：`<0x10> <经典蓝牙地址:6> <0x14> <BLE 地址:6>`。
#[der(..RespTemplate)]
pub struct BluetoothMacV2V1V0Tl {
    #[deku(magic = b"\x1f\x7d")]
    pub tag_classic: u8,
    #[deku(count = "6")]
    pub classic: Vec<u8>,
    pub tag_ble: u8,
    #[deku(count = "6")]
    pub ble: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    /// 实测: `1f 7d 10 60 6e 41 37 c4 37 14 60 6e 41 37 c4 37`
    #[test]
    fn parse_dp305p() {
        let tl: BluetoothMacV2V1V0Tl =
            dec_tl(&hexdec("1f 7d 10 60 6e 41 37 c4 37 14 60 6e 41 37 c4 37"));
        assert_eq!(tl.tag_classic, 0x10);
        eq(&tl.classic, &hexdec("60 6e 41 37 c4 37"));
        assert_eq!(tl.tag_ble, 0x14);
        eq(&tl.ble, &hexdec("60 6e 41 37 c4 37"));
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: BluetoothMacV2V1V0Tl =
            dec_tl(&hexdec("1f 7c 10 60 6e 41 37 c4 37 14 60 6e 41 37 c4 37"));
    }
}
