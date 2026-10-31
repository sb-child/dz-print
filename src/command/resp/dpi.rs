// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::dpi::DpiV2V1V0Tl,
};

/// 设备 DPI。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dpi {
    /// 打印头 DPI。
    pub dpi: u16,
}

impl RespTrait for Dpi {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: DpiV2V1V0Tl = decode_resp(payload)?;
        Ok(Self { dpi: raw.dpi })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn payload() -> Vec<u8> {
        vec![0x1f, 0x71, 0x01, 0x2c]
    }

    #[test]
    fn parse_all_versions() {
        for mv in [ModelVersion::V0, ModelVersion::V1, ModelVersion::V2] {
            let d = Dpi::parse(&payload(), mv).unwrap();
            assert_eq!(d.dpi, 300);
        }
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = Dpi::parse(&[0x1f, 0x72, 0x01, 0x2c], ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
