// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::gap_length::GapLengthV2Tl,
    varint::VarintMode,
};

/// 纸张间隔长度。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GapLength {
    /// 纸张间隔。
    /// - 单位 0.01mm。
    pub gap: u32,
}

impl RespTrait for GapLength {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: GapLengthV2Tl = decode_resp(payload)?;
        let gap = decode_gap(&raw.gap)?;
        Ok(Self { gap })
    }
}

fn decode_gap(raw: &[u8]) -> Result<u32, ParseRespError> {
    let mode = match raw.len() {
        1 => VarintMode::Auto,
        2 => VarintMode::Fixed2,
        3 => VarintMode::Fixed3,
        _ => {
            return Err(ParseRespError::BadLength {
                expected: "1..=3 bytes",
                actual: raw.len(),
            });
        }
    };

    let (value, used) = mode
        .decode(raw)
        .ok_or_else(|| ParseRespError::InvalidValue {
            param: "gap",
            value: format!("invalid {mode:?} varint: {raw:02X?}"),
        })?;
    debug_assert_eq!(used, raw.len());

    u32::try_from(value).map_err(|_| ParseRespError::InvalidValue {
        param: "gap",
        value: value.to_string(),
    })
}

#[cfg(test)]
mod test {
    use super::*;

    fn payload(data: &[u8]) -> Vec<u8> {
        let mut b = vec![0x1f, 0x45];
        b.extend_from_slice(data);
        b
    }

    #[test]
    fn one_byte() {
        let g = GapLength::parse(&payload(&[0x32]), ModelVersion::V2).unwrap();
        assert_eq!(g.gap, 50);
    }

    #[test]
    fn two_byte() {
        let g = GapLength::parse(&payload(&[0xC0, 0x4E]), ModelVersion::V2).unwrap();
        assert_eq!(g.gap, 78);
    }

    #[test]
    fn three_byte() {
        let g = GapLength::parse(&payload(&[0xC0, 0x4E, 0x20]), ModelVersion::V2).unwrap();
        assert_eq!(g.gap, 20_000);
    }

    #[test]
    fn bad_length_is_reported() {
        let err =
            GapLength::parse(&payload(&[0xC0, 0x4E, 0x20, 0x00]), ModelVersion::V2).unwrap_err();
        assert!(
            matches!(
                err,
                ParseRespError::BadLength {
                    expected: "1..=3 bytes",
                    actual: 4,
                }
            ),
            "{err:?}"
        );

        let err = GapLength::parse(&payload(&[]), ModelVersion::V2).unwrap_err();
        assert!(
            matches!(err, ParseRespError::BadLength { actual: 0, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn missing_c0_flag_is_invalid_value() {
        let err = GapLength::parse(&payload(&[0x40, 0x00]), ModelVersion::V2).unwrap_err();
        assert!(
            matches!(err, ParseRespError::InvalidValue { param: "gap", .. }),
            "{err:?}"
        );
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = GapLength::parse(&[0x1f, 0x44, 0x32], ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
