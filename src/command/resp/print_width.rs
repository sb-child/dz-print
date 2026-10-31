// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::print_width::PrintWidthV2V1V0Tl,
};

/// 打印宽度和出纸宽度。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrintWidth {
    /// 打印头点数。
    pub print_width: u16,
    /// 支持的最大纸宽，单位 0.1mm。
    pub paper_width: u16,
    /// 其余字段，长度随固件变化。
    pub tail: Vec<u8>,
}

impl RespTrait for PrintWidth {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: PrintWidthV2V1V0Tl = decode_resp(payload)?;
        Ok(Self {
            print_width: raw.print_width,
            paper_width: raw.paper_width,
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
    fn parse_dp27p() {
        let w = PrintWidth::parse(
            &hexdec("1f 72 02 40 02 3a 00 00 00 00 00"),
            ModelVersion::V2,
        )
        .unwrap();
        assert_eq!(w.print_width, 576);
        assert_eq!(w.paper_width, 570);
        assert_eq!(w.tail, hexdec("00 00 00 00 00"));
    }

    #[test]
    fn parse_dp305p() {
        let w = PrintWidth::parse(
            &hexdec("1f 72 03 c0 03 84 00 00 00 00 00 00 00"),
            ModelVersion::V2,
        )
        .unwrap();
        assert_eq!(w.print_width, 960);
        assert_eq!(w.paper_width, 900);
        assert_eq!(w.tail, hexdec("00 00 00 00 00 00 00"));
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err =
            PrintWidth::parse(&[0x1f, 0x71, 0x02, 0x40, 0x02, 0x3a], ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
