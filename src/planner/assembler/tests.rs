// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use super::*;
use crate::command::{
    req::{
        darkness::Darkness as ReqDarkness, end_page::EndPage, print_line::PrintLine,
        status::Status as ReqStatus,
    },
    resp::{darkness::Darkness as RespDarkness, status::Status as RespStatus},
    status::PrinterStatus,
};
use crate::test_util::hexdec;

#[test]
fn encode_v2_adds_length_and_checksum() {
    let bytes = Assembler::new(ModelVersion::V2)
        .encode(&[&ReqStatus {}])
        .unwrap();
    assert_eq!(bytes, hexdec("1f 70 00 8f"));
}

#[test]
fn encode_v1_uses_fixed_checksum() {
    let bytes = Assembler::new(ModelVersion::V1)
        .encode(&[&ReqDarkness::default()])
        .unwrap();
    assert_eq!(bytes, hexdec("1f 43 01 05 88"));
}

#[test]
fn encode_v0_is_raw() {
    let bytes = Assembler::new(ModelVersion::V0)
        .encode(&[&EndPage {}])
        .unwrap();
    assert_eq!(bytes, hexdec("1d 56 42 00"));
}

#[test]
fn encode_concatenates_mixed_commands() {
    let bytes = Assembler::new(ModelVersion::V2)
        .encode(&[&ReqStatus {}, &ReqDarkness::default()])
        .unwrap();
    assert_eq!(bytes, hexdec("1f 70 00 8f 1f 43 01 05 b6"));
}

/// 测试用响应：原样收下 payload，用来检查切帧结果。
#[derive(Debug, PartialEq)]
struct RawPayload(Vec<u8>);

impl RespTrait for RawPayload {
    fn parse(
        payload: &[u8],
        _mv: ModelVersion,
    ) -> Result<Self, crate::command::resp::ParseRespError> {
        Ok(Self(payload.to_vec()))
    }
}

#[test]
fn encode_uses_two_byte_length_above_191() {
    let line = PrintLine {
        data: vec![0xAA; 200],
        ..Default::default()
    };
    let bytes = Assembler::new(ModelVersion::V2).encode(&[&line]).unwrap();
    // payload = 1f 21 | repeats=00 skips=00 data=200B
    // data_len = 202 = 0xCA -> C0 CA
    assert_eq!(&bytes[..4], &[0x1F, 0x21, 0xC0, 0xCA]);

    let (payload, consumed) = Assembler::new(ModelVersion::V2)
        .decode_one::<RawPayload>(&bytes)
        .unwrap()
        .unwrap();
    assert_eq!(consumed, bytes.len());
    assert_eq!(payload.0.len(), 204);
    assert_eq!(&payload.0[..4], &[0x1F, 0x21, 0x00, 0x00]);
    assert_eq!(&payload.0[4..], &[0xAA; 200]);
}

#[test]
fn encode_rejects_too_long_data() {
    let line = PrintLine {
        data: vec![0; 16_382],
        ..Default::default()
    };
    let err = Assembler::new(ModelVersion::V2)
        .encode(&[&line])
        .unwrap_err();
    assert!(
        matches!(err, PackReqError::PayloadTooLong { len: 16_384, .. }),
        "{err:?}"
    );
}

#[test]
fn decode_typed_status() {
    let frame = hexdec("1f 70 08 00 01 01 00 00 35 05 10 3b");
    let (resp, consumed) = Assembler::new(ModelVersion::V2)
        .decode_one::<RespStatus>(&frame)
        .unwrap()
        .unwrap();
    assert_eq!(consumed, frame.len());
    assert_eq!(resp.status, PrinterStatus::Printable);
    assert_eq!(resp.page_key, 0x0101);
    assert_eq!(resp.tail, hexdec("00 00 35 05 10"));
}

#[test]
fn decode_one_at_a_time() {
    let mut buf = hexdec("1f 70 08 00 01 01 00 00 35 05 10 3b");
    let first_len = buf.len();
    buf.extend_from_slice(&hexdec("1f 70 08 22 01 01 00 00 35 05 10 19"));
    let second_len = buf.len() - first_len;
    // 半个下一条
    buf.extend_from_slice(&hexdec("1f 70 08 23"));

    let asm = Assembler::new(ModelVersion::V2);
    let (first, c1) = asm.decode_one::<RespStatus>(&buf).unwrap().unwrap();
    assert_eq!(c1, first_len);
    assert_eq!(first.status, PrinterStatus::Printable);

    let (second, c2) = asm.decode_one::<RespStatus>(&buf[c1..]).unwrap().unwrap();
    assert_eq!(c2, second_len);
    assert_eq!(second.status, PrinterStatus::CoverOpened);

    assert!(
        asm.decode_one::<RespStatus>(&buf[c1 + c2..])
            .unwrap()
            .is_none()
    );
}

#[test]
fn decode_incomplete_returns_none() {
    let resp = Assembler::new(ModelVersion::V2)
        .decode_one::<RespStatus>(&hexdec("1f 70 08 00 01"))
        .unwrap();
    assert!(resp.is_none());
}

#[test]
fn decode_accepts_fixed_checksum_0x88() {
    let (resp, consumed) = Assembler::new(ModelVersion::V1)
        .decode_one::<RespStatus>(&hexdec("1f 70 03 00 01 01 88"))
        .unwrap()
        .unwrap();
    assert_eq!(consumed, 7);
    assert_eq!(resp.page_key, 0x0101);
    assert!(resp.tail.is_empty());
}

#[test]
fn decode_bad_checksum() {
    let err = Assembler::new(ModelVersion::V2)
        .decode_one::<RespStatus>(&hexdec("1f 70 08 00 01 01 00 00 35 05 10 00"))
        .unwrap_err();
    assert!(matches!(err, DecodeError::BadChecksum { .. }), "{err:?}");
}

#[test]
fn decode_wrong_response_type() {
    let err = Assembler::new(ModelVersion::V2)
        .decode_one::<RespDarkness>(&hexdec("1f 70 08 00 01 01 00 00 35 05 10 3b"))
        .unwrap_err();
    assert!(matches!(err, DecodeError::Resp { .. }), "{err:?}");
}
