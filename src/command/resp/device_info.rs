// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::device_info::DeviceInfoV2V1V0Tl,
};

/// 设备信息（`0x7f`）。
///
/// 字段语义还没确认，先原样保留字节。DP27P 实测是固定 16 字节全 `0x00`：
/// 激活前后、带 1/2 字节子索引、带 16 字节载荷，回包都不变，说明固件忽略参数。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    /// 原始载荷，不含命令头和帧层长度/校验。
    pub data: Vec<u8>,
}

impl RespTrait for DeviceInfo {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: DeviceInfoV2V1V0Tl = decode_resp(payload)?;
        Ok(Self { data: raw.data })
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

    #[test]
    fn parse_dp27p() {
        let info = DeviceInfo::parse(
            &hexdec("1f 7f 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00"),
            ModelVersion::V2,
        )
        .unwrap();
        assert_eq!(info.data, vec![0; 16]);
    }

    #[test]
    fn parse_empty() {
        let info = DeviceInfo::parse(&hexdec("1f 7f"), ModelVersion::V0).unwrap();
        assert!(info.data.is_empty());
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = DeviceInfo::parse(&hexdec("1f 7e 00 00"), ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
