// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::statistics::StatisticsV2V1V0Tl,
};

/// 打印统计信息。
///
/// 四个计数器是累计值，会跨页累加；对账时要取前后两次的差值。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Statistics {
    /// 电机一共走过的行数。
    pub work_lines: u32,
    /// 其中打印过的行数。
    pub print_lines: u32,
    /// 其中走过的空行数。
    pub null_lines: u32,
    /// 打印过的总页数。
    pub print_pages: u32,
    /// 其余字段，长度随固件变化。
    pub tail: Vec<u8>,
}

impl RespTrait for Statistics {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: StatisticsV2V1V0Tl = decode_resp(payload)?;
        Ok(Self {
            work_lines: raw.work_lines,
            print_lines: raw.print_lines,
            null_lines: raw.null_lines,
            print_pages: raw.print_pages,
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
    fn parse_sample() {
        let s = Statistics::parse(
            &hexdec("1f 73 00 01 b6 4b 00 01 76 ce 00 00 71 58 00 00 00 de"),
            ModelVersion::V2,
        )
        .unwrap();
        assert_eq!(s.work_lines, 112_203);
        assert_eq!(s.print_lines, 95_950);
        assert_eq!(s.null_lines, 29_016);
        assert_eq!(s.print_pages, 222);
        assert!(s.tail.is_empty());
    }

    #[test]
    fn parse_with_tail() {
        let s = Statistics::parse(
            &hexdec("1f 73 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 aa"),
            ModelVersion::V1,
        )
        .unwrap();
        assert_eq!(s.print_pages, 0);
        assert_eq!(s.tail, hexdec("aa"));
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = Statistics::parse(
            &hexdec("1f 72 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00"),
            ModelVersion::V2,
        )
        .unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
