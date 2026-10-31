// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp, string::decode_gbk_nul},
    resp_template::manufacturer::ManufacturerV2V1V0Tl,
};

/// 制造商名称。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manufacturer {
    /// 制造商名称。
    pub manufacturer: Box<str>,
}

impl RespTrait for Manufacturer {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: ManufacturerV2V1V0Tl = decode_resp(payload)?;
        Ok(Self {
            manufacturer: decode_gbk_nul(&raw.manufacturer)?,
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
    fn parse_gbk() {
        let m = Manufacturer::parse(&hexdec("1f 75 b5 c2 d9 a1 00"), ModelVersion::V2).unwrap();
        assert_eq!(&*m.manufacturer, "德佟");
    }

    #[test]
    fn parse_ascii_without_nul() {
        let m = Manufacturer::parse(&hexdec("1f 75 44 50 32 37 50"), ModelVersion::V2).unwrap();
        assert_eq!(&*m.manufacturer, "DP27P");
    }

    #[test]
    fn invalid_gbk_is_invalid_string() {
        let err = Manufacturer::parse(&hexdec("1f 75 ff 00"), ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::InvalidString), "{err:?}");
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err =
            Manufacturer::parse(&hexdec("1f 74 b5 c2 d9 a1 00"), ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
