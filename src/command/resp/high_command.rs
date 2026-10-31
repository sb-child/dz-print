// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    high_command::HighCommandMode,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::high_command::HighCommandV2V1V0Tl,
};

/// 激活 / 结束 `> 0x80` 命令的响应。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HighCommand {
    /// 设备回显的激活参数。
    pub mode: HighCommandMode,
    /// 其余字段，长度随固件变化。
    pub tail: Vec<u8>,
}

impl RespTrait for HighCommand {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: HighCommandV2V1V0Tl = decode_resp(payload)?;
        let mode =
            HighCommandMode::from_wire(raw.mode).map_err(|e| ParseRespError::InvalidValue {
                param: "mode",
                value: e.to_string(),
            })?;
        Ok(Self {
            mode,
            tail: raw._tail,
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
    fn parse_enable() {
        let h = HighCommand::parse(&hexdec("1f 80 7f"), ModelVersion::V2).unwrap();
        assert_eq!(h.mode, HighCommandMode::Enable);
        assert!(h.tail.is_empty());
    }

    #[test]
    fn parse_disable() {
        let h = HighCommand::parse(&hexdec("1f 80 00"), ModelVersion::V2).unwrap();
        assert_eq!(h.mode, HighCommandMode::Disable);
    }

    #[test]
    fn parse_enable_alt() {
        let h = HighCommand::parse(&hexdec("1f 80 49"), ModelVersion::V2).unwrap();
        assert_eq!(h.mode, HighCommandMode::EnableAlt);
    }

    #[test]
    fn parse_disabled_ack() {
        // DP27P 实测：发送 `1f 80 00` 后回 `1f 80 80`。
        let h = HighCommand::parse(&hexdec("1f 80 80"), ModelVersion::V2).unwrap();
        assert_eq!(h.mode, HighCommandMode::Disabled);
    }

    #[test]
    fn parse_with_tail() {
        let h = HighCommand::parse(&hexdec("1f 80 7f aa"), ModelVersion::V2).unwrap();
        assert_eq!(h.mode, HighCommandMode::Enable);
        assert_eq!(h.tail, hexdec("aa"));
    }

    #[test]
    fn unknown_mode_is_invalid_value() {
        let err = HighCommand::parse(&hexdec("1f 80 01"), ModelVersion::V2).unwrap_err();
        assert!(
            matches!(err, ParseRespError::InvalidValue { param: "mode", .. }),
            "{err:?}"
        );
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = HighCommand::parse(&hexdec("1f 81 7f"), ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
