// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::bluetooth_mac::BluetoothMacV2V1V0Tl,
};

/// 蓝牙 MAC 地址。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BluetoothMac {
    /// `0x10` 标签后的地址，可能是经典蓝牙地址。
    pub classic: [u8; 6],
    /// `0x14` 标签后的地址，可能是 BLE 地址。
    pub ble: [u8; 6],
}

impl BluetoothMac {
    /// 按小写hex形式格式化蓝牙地址。
    fn format_addr(addr: &[u8; 6]) -> String {
        addr.iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":")
    }

    /// 格式化经典蓝牙地址。
    pub fn format_classic(&self) -> String {
        Self::format_addr(&self.classic)
    }

    /// 格式化 BLE 地址。
    pub fn format_ble(&self) -> String {
        Self::format_addr(&self.ble)
    }
}

impl RespTrait for BluetoothMac {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: BluetoothMacV2V1V0Tl = decode_resp(payload)?;
        let tag_err = |param: &'static str, value: u8| ParseRespError::InvalidValue {
            param,
            value: format!("0x{value:02x}"),
        };
        if raw.tag_classic != 0x10 {
            return Err(tag_err("tag_classic", raw.tag_classic));
        }
        if raw.tag_ble != 0x14 {
            return Err(tag_err("tag_ble", raw.tag_ble));
        }
        let to_addr = |v: &[u8]| -> [u8; 6] { v.try_into().expect("template reads 6 bytes") };
        Ok(Self {
            classic: to_addr(&raw.classic),
            ble: to_addr(&raw.ble),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn hexdec(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .map(|c| u8::from_str_radix(c, 16).unwrap())
            .collect()
    }

    /// 实测: `1f 7d 10 60 6e 41 37 c4 37 14 60 6e 41 37 c4 37`
    #[test]
    fn parse_dp305p() {
        let m = BluetoothMac::parse(
            &hexdec("1f 7d 10 60 6e 41 37 c4 37 14 60 6e 41 37 c4 37"),
            ModelVersion::V2,
        )
        .unwrap();
        assert_eq!(m.classic, [0x60, 0x6e, 0x41, 0x37, 0xc4, 0x37]);
        assert_eq!(m.ble, [0x60, 0x6e, 0x41, 0x37, 0xc4, 0x37]);
        assert_eq!(m.format_classic(), "60:6e:41:37:c4:37");
        assert_eq!(m.format_ble(), "60:6e:41:37:c4:37");
    }

    #[test]
    fn bad_tag_is_invalid_value() {
        let err = BluetoothMac::parse(
            &hexdec("1f 7d 11 60 6e 41 37 c4 37 14 60 6e 41 37 c4 37"),
            ModelVersion::V2,
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                ParseRespError::InvalidValue {
                    param: "tag_classic",
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = BluetoothMac::parse(
            &hexdec("1f 7c 10 60 6e 41 37 c4 37 14 60 6e 41 37 c4 37"),
            ModelVersion::V2,
        )
        .unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
