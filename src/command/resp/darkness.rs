// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::darkness::DarknessV2V1V0Tl,
    settings::DarknessSetting,
};

/// 打印浓度。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Darkness {
    /// 打印浓度。
    pub darkness: DarknessSetting,
}

impl RespTrait for Darkness {
    fn parse(payload: &[u8], mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: DarknessV2V1V0Tl = decode_resp(payload)?;
        let darkness = DarknessSetting::from_wire(raw.darkness, mv).map_err(|e| {
            ParseRespError::InvalidValue {
                param: "darkness",
                value: e.to_string(),
            }
        })?;
        Ok(Self { darkness })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn payload(raw: u8) -> Vec<u8> {
        vec![0x1f, 0x43, raw, 0x0f, 0x00, 0x5f]
    }

    #[test]
    fn v2_v1_raw_is_user_value_minus_one() {
        for (raw, exp) in [
            (0, DarknessSetting::Min),
            (5, DarknessSetting::Normal),
            (14, DarknessSetting::Max),
        ] {
            for mv in [ModelVersion::V1, ModelVersion::V2] {
                let d = Darkness::parse(&payload(raw), mv).unwrap();
                assert_eq!(d.darkness, exp);
            }
        }
    }

    #[test]
    fn v0_raw_is_user_value() {
        for (raw, exp) in [
            (1, DarknessSetting::Min),
            (6, DarknessSetting::Normal),
            (15, DarknessSetting::Max),
        ] {
            let d = Darkness::parse(&payload(raw), ModelVersion::V0).unwrap();
            assert_eq!(d.darkness, exp);
        }
    }

    #[test]
    fn out_of_range_is_invalid_value() {
        let err = Darkness::parse(&payload(15), ModelVersion::V2).unwrap_err();
        assert!(
            matches!(
                err,
                ParseRespError::InvalidValue {
                    param: "darkness",
                    ..
                }
            ),
            "{err:?}"
        );

        let err = Darkness::parse(&payload(0), ModelVersion::V0).unwrap_err();
        assert!(
            matches!(err, ParseRespError::InvalidValue { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = Darkness::parse(&[0x1f, 0x44, 0x05], ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
