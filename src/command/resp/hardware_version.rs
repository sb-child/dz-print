// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::hardware_version::HardwareVersionV2V1V0Tl,
};
use std::fmt;

pub const CMD: u8 = 0x7a;

/// 硬件版本。
///
/// 固件用 BCD 编码版本号，例如 `0x26` 表示 `2.6`；其余字节语义未知。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HardwareVersion {
    /// BCD 编码的原始版本字节。
    pub raw: u8,
    /// 其余字节，长度随固件变化。
    pub tail: Vec<u8>,
}

impl HardwareVersion {
    /// 把 BCD 字节拆成 `(major, minor)`；任意半字节大于 9 时返回 `None`。
    pub fn major_minor(&self) -> Option<(u8, u8)> {
        let major = self.raw >> 4;
        let minor = self.raw & 0x0f;
        (major <= 9 && minor <= 9).then_some((major, minor))
    }
}

impl fmt::Display for HardwareVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.major_minor() {
            Some((major, minor)) => write!(f, "{major}.{minor}"),
            None => write!(f, "0x{:02x}", self.raw),
        }
    }
}

impl RespTrait for HardwareVersion {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: HardwareVersionV2V1V0Tl = decode_resp(payload)?;
        Ok(Self {
            raw: raw.version,
            tail: raw.tail,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_util::hexdec;

    #[test]
    fn parse_dp27p() {
        let v = HardwareVersion::parse(&hexdec("1f 7a 26 01 04 01 04"), ModelVersion::V2).unwrap();
        assert_eq!(v.raw, 0x26);
        assert_eq!(v.major_minor(), Some((2, 6)));
        assert_eq!(v.to_string(), "2.6");
        assert_eq!(v.tail, hexdec("01 04 01 04"));
    }

    #[test]
    fn non_bcd_falls_back_to_hex() {
        let v = HardwareVersion::parse(&hexdec("1f 7a fa"), ModelVersion::V2).unwrap();
        assert_eq!(v.major_minor(), None);
        assert_eq!(v.to_string(), "0xfa");
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = HardwareVersion::parse(&hexdec("1f 7b 26"), ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
