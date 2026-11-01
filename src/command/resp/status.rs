// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::status::StatusV2V1V0Tl,
    status::PrinterStatus,
};

/// 命令字节（`0x70`）。
pub const CMD: u8 = 0x70;

/// 打印机状态。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// 状态码。
    pub status: PrinterStatus,
    /// 当前页编号。
    pub page_key: u16,
    /// 其余字段，含义随固件变化，暂不解析。
    pub tail: Vec<u8>,
}

impl RespTrait for Status {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: StatusV2V1V0Tl = decode_resp(payload)?;
        Ok(Self {
            // 未知状态码保留为 `PrinterStatus::Unknown`，不在这里报解析错误。
            status: PrinterStatus::from_wire(raw.status),
            page_key: raw.page_key,
            tail: raw._tail,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn payload(data: &[u8]) -> Vec<u8> {
        let mut b = vec![0x1f, 0x70];
        b.extend_from_slice(data);
        b
    }

    #[test]
    fn parse_known_statuses() {
        for (raw, exp) in [
            (0x00, PrinterStatus::Printable),
            (0x01, PrinterStatus::Printing),
            (0x02, PrinterStatus::Rotating),
            (0x22, PrinterStatus::CoverOpened),
            (0x23, PrinterStatus::NoPaper),
            (0x24, PrinterStatus::RibbonCanOpened),
            (0x2a, PrinterStatus::NoLabel),
            (0x32, PrinterStatus::LabelCanOpened),
        ] {
            let s = Status::parse(&payload(&[raw, 0x01, 0x02, 0x00]), ModelVersion::V2).unwrap();
            assert_eq!(s.status, exp);
            assert_eq!(s.page_key, 0x0102);
            assert_eq!(s.tail, vec![0x00]);
        }
    }

    #[test]
    fn parse_dp27p_full_payload() {
        // 1f 70 | 00 01 01 00 00 35 05 10
        let s = Status::parse(
            &payload(&hexdec("00 01 01 00 00 35 05 10")),
            ModelVersion::V2,
        )
        .unwrap();
        assert_eq!(s.status, PrinterStatus::Printable);
        assert_eq!(s.page_key, 0x0101);
        assert_eq!(s.tail, hexdec("00 00 35 05 10"));
    }

    #[test]
    fn unknown_status_is_kept() {
        let s = Status::parse(&payload(&[0x09, 0x00, 0x01]), ModelVersion::V2).unwrap();
        assert_eq!(s.status, PrinterStatus::Unknown(0x09));
        assert_eq!(s.status.to_wire(), 0x09);
        assert!(!s.status.is_error());
        assert_eq!(s.page_key, 0x0001);

        let s = Status::parse(&payload(&[0x77, 0x00, 0x01]), ModelVersion::V2).unwrap();
        assert_eq!(s.status, PrinterStatus::Unknown(0x77));
        assert!(s.status.is_error());
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = Status::parse(&[0x1f, 0x71, 0x00, 0x00, 0x01], ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }

    fn hexdec(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .map(|c| u8::from_str_radix(c, 16).unwrap())
            .collect()
    }
}
