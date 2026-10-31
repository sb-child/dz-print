// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp, string::decode_gbk_nul},
    resp_template::device_name::DeviceNameV2V1V0Tl,
};

/// 设备名。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceName {
    /// 设备名。
    /// - 格式 `型号-序列号`。
    pub device_name: Box<str>,
}

impl RespTrait for DeviceName {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: DeviceNameV2V1V0Tl = decode_resp(payload)?;
        Ok(Self {
            device_name: decode_gbk_nul(&raw.device_name)?,
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

    /// BLE 实测: `1F 79 11 "DP305P-Y60870279\0"`
    #[test]
    fn parse_dp305p() {
        let d = DeviceName::parse(
            &hexdec("1f 79 44 50 33 30 35 50 2d 59 36 30 38 37 30 32 37 39 00"),
            ModelVersion::V2,
        )
        .unwrap();
        assert_eq!(&*d.device_name, "DP305P-Y60870279");
    }

    #[test]
    fn parse_without_nul() {
        let d = DeviceName::parse(&hexdec("1f 79 44 50 32 37 50"), ModelVersion::V1).unwrap();
        assert_eq!(&*d.device_name, "DP27P");
    }

    #[test]
    fn invalid_gbk_is_invalid_string() {
        let err = DeviceName::parse(&hexdec("1f 79 ff 00"), ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::InvalidString), "{err:?}");
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = DeviceName::parse(&hexdec("1f 78 44 50 32 37 50"), ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
