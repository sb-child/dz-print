// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::speed::SpeedV2V1V0Tl,
    settings::SpeedSetting,
};

/// 打印速度。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Speed {
    /// 打印速度。
    pub speed: SpeedSetting,
}

impl RespTrait for Speed {
    fn parse(payload: &[u8], mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: SpeedV2V1V0Tl = decode_resp(payload)?;
        let speed =
            SpeedSetting::from_wire(raw.speed, mv).map_err(|e| ParseRespError::InvalidValue {
                param: "speed",
                value: e.to_string(),
            })?;
        Ok(Self { speed })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// `1f 44 <speed> <tail...>`
    fn payload(raw: u8) -> Vec<u8> {
        let mut b = vec![0x1f, 0x44, raw];
        b.extend_from_slice(&[0x00; 9]);
        b
    }

    #[test]
    fn v2_v1_raw_is_user_value_minus_one() {
        for (raw, exp) in [
            (0, SpeedSetting::Min),
            (2, SpeedSetting::Normal),
            (4, SpeedSetting::Max),
        ] {
            for mv in [ModelVersion::V1, ModelVersion::V2] {
                let s = Speed::parse(&payload(raw), mv).unwrap();
                assert_eq!(s.speed, exp);
            }
        }
    }

    #[test]
    fn v0_raw_is_user_value() {
        for (raw, exp) in [
            (1, SpeedSetting::Min),
            (3, SpeedSetting::Normal),
            (5, SpeedSetting::Max),
        ] {
            let s = Speed::parse(&payload(raw), ModelVersion::V0).unwrap();
            assert_eq!(s.speed, exp);
        }
    }

    #[test]
    fn out_of_range_is_invalid_value() {
        let err = Speed::parse(&payload(5), ModelVersion::V2).unwrap_err();
        assert!(
            matches!(err, ParseRespError::InvalidValue { param: "speed", .. }),
            "{err:?}"
        );

        let err = Speed::parse(&payload(0), ModelVersion::V0).unwrap_err();
        assert!(
            matches!(err, ParseRespError::InvalidValue { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = Speed::parse(&[0x1f, 0x43, 0x02], ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
