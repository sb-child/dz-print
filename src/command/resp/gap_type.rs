// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    models::ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::gap_type::GapTypeV0V1V2Tl,
    settings::GapTypeSetting,
};

/// 纸张间隔类型。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GapType {
    /// 纸张间隔类型。
    pub gap_type: GapTypeSetting,
}

impl RespTrait for GapType {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: GapTypeV0V1V2Tl = decode_resp(payload)?;
        let gap_type =
            GapTypeSetting::from_wire(raw.gap_type).map_err(|e| ParseRespError::InvalidValue {
                param: "gap_type",
                value: e.to_string(),
            })?;
        Ok(Self { gap_type })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn payload(raw: u8) -> Vec<u8> {
        vec![0x1f, 0x42, raw]
    }

    #[test]
    fn all_known_values() {
        for (raw, exp) in [
            (0, GapTypeSetting::Continuous),
            (1, GapTypeSetting::Hole),
            (2, GapTypeSetting::Gap),
            (3, GapTypeSetting::BlackMark),
            (4, GapTypeSetting::Transparent),
        ] {
            for mv in [ModelVersion::V0, ModelVersion::V1, ModelVersion::V2] {
                let t = GapType::parse(&payload(raw), mv).unwrap();
                assert_eq!(t.gap_type, exp);
            }
        }
    }

    #[test]
    fn out_of_range_is_invalid_value() {
        let err = GapType::parse(&payload(5), ModelVersion::V2).unwrap_err();
        assert!(
            matches!(
                err,
                ParseRespError::InvalidValue {
                    param: "gap_type",
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = GapType::parse(&[0x1f, 0x43, 0x02], ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
