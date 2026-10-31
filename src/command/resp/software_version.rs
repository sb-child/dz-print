// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp, string::decode_gbk_nul},
    resp_template::software_version::SoftwareVersionV2V1V0Tl,
};

/// 软件版本。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoftwareVersion {
    /// 软件版本。
    /// - 格式 `x.y yyyymmdd` 或 `x.y.yyyymmdd`。
    pub software_version: Box<str>,
}

impl RespTrait for SoftwareVersion {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: SoftwareVersionV2V1V0Tl = decode_resp(payload)?;
        Ok(Self {
            software_version: decode_gbk_nul(&raw.software_version)?,
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

    #[test]
    fn parse_version() {
        let v = SoftwareVersion::parse(
            &hexdec("1f 7c 31 2e 30 20 32 30 32 34 30 35 32 30 00"),
            ModelVersion::V2,
        )
        .unwrap();
        assert_eq!(&*v.software_version, "1.0 20240520");
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err =
            SoftwareVersion::parse(&hexdec("1f 7d 31 2e 30 00"), ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
