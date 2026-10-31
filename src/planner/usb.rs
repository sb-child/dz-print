// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! USB 传输层封包。
//!
//! USB 在命令区外面再套一层：
//!
//! ```text
//! 1e <命令区长度:变长> <命令区...>
//! ```
//!
//! - `0x1e` 是 HID 的 Report ID。
//! - 长度字段只统计命令区，不含 `0x1e` 和长度字段本身。
//! - 一份 HID 报告固定 64 字节：1 字节报告号 + 63 字节数据。数据区以长度字段开头，所以一份报告最多放 62 字节命令区（此时长度字段固定为 1 字节）。
//! - 命令区超过 62 字节时按 62 字节切片，每片各自带 `1e <长度>`；报告尾部补 `0x00` 到 64 字节。接收方按长度字段裁掉补齐的零。
//!
//! 命令区本身的格式见 [`Assembler`](super::Assembler)。BLE 是串口透传，没有这层
//! USB 包头，本模块只用于 USB。

use crate::command::varint::VarintMode;
use snafu::Snafu;

/// HID 报告号。
pub const REPORT_ID: u8 = 0x1e;
/// 一份 HID 报告的总字节数（含报告号）。
pub const REPORT_SIZE: usize = 64;
/// 一份报告里能放的最大命令区字节数：64 - 1（报告号）- 1（长度字段）。
pub const REPORT_PAYLOAD: usize = REPORT_SIZE - 2;

/// USB 封包错误。
#[derive(Debug, Snafu)]
pub enum UsbError {
    #[snafu(display("USB payload too long: {len} byte(s), max {max}."))]
    PayloadTooLong { len: usize, max: usize },
    #[snafu(display("Invalid USB report id: {actual:#04X}, expected {expected:#04X}."))]
    BadReportId { expected: u8, actual: u8 },
    #[snafu(display("Invalid USB length field: {bytes:02X?}."))]
    InvalidLength { bytes: Vec<u8> },
}

/// 打包一条 USB 数据包：`1e <命令区长度:变长> <命令区>`。
///
/// 返回的字节数不补齐到报告长度。数据包适合非 HID 接口或单份报告；
/// 要走 HID 中断端点时用 [`encode_reports`]。
pub fn encode_packet(cmd_area: &[u8]) -> Result<Vec<u8>, UsbError> {
    let max = VarintMode::max_packet_len() as usize;
    if cmd_area.len() > max {
        return Err(UsbError::PayloadTooLong {
            len: cmd_area.len(),
            max,
        });
    }

    let len_bytes = VarintMode::Auto.encoded_len(cmd_area.len() as i32);
    let mut out = Vec::with_capacity(1 + len_bytes + cmd_area.len());
    out.push(REPORT_ID);
    VarintMode::Auto.encode_into(cmd_area.len() as i32, &mut out);
    out.extend_from_slice(cmd_area);
    Ok(out)
}

/// 把命令区切成 HID 报告。
///
/// 每份报告都是 `1e <片段长度> <片段>` 再补 `0x00` 到 64 字节。
/// 空命令区返回空列表，不会产生空报告。
pub fn encode_reports(cmd_area: &[u8]) -> Result<Vec<[u8; REPORT_SIZE]>, UsbError> {
    let mut reports = Vec::with_capacity(cmd_area.len().div_ceil(REPORT_PAYLOAD));
    for chunk in cmd_area.chunks(REPORT_PAYLOAD) {
        let packet = encode_packet(chunk)?;
        debug_assert!(packet.len() <= REPORT_SIZE);
        let mut report = [0u8; REPORT_SIZE];
        report[..packet.len()].copy_from_slice(&packet);
        reports.push(report);
    }
    Ok(reports)
}

/// 解一条 USB 数据包，返回 `(命令区片段, 已消费字节数)`。
///
/// - 数据还不足一条完整的包时返回 `Ok(None)`。
/// - `consumed` 不包含报告尾部补齐的 `0x00`，调用方可以保留剩余字节继续解下一条。
pub fn decode_packet(buf: &[u8]) -> Result<Option<(&[u8], usize)>, UsbError> {
    let Some(&id) = buf.first() else {
        return Ok(None);
    };
    if id != REPORT_ID {
        return Err(UsbError::BadReportId {
            expected: REPORT_ID,
            actual: id,
        });
    }

    // 还需要一字节长度字段。
    if buf.len() < 2 {
        return Ok(None);
    }

    let (cmd_len, len_bytes) = match VarintMode::Auto.decode(&buf[1..]) {
        Some(v) => v,
        // 长度字段是 2 字节变长，但第二个字节还没到。
        None if buf.len() == 2 && buf[1] & 0xC0 == 0xC0 => return Ok(None),
        None => {
            return Err(UsbError::InvalidLength {
                bytes: buf[1..].to_vec(),
            });
        }
    };
    let cmd_len = cmd_len as usize;
    let total = 1 + len_bytes + cmd_len;
    if buf.len() < total {
        return Ok(None);
    }

    Ok(Some((&buf[1 + len_bytes..total], total)))
}

/// 解一份定长 HID 报告，返回命令区片段。
///
/// 报告尾部补齐的 `0x00` 由长度字段裁掉。报告不完整或长度字段非法时报错。
pub fn decode_report(report: &[u8]) -> Result<&[u8], UsbError> {
    match decode_packet(report)? {
        Some((payload, _)) => Ok(payload),
        None => Err(UsbError::InvalidLength {
            bytes: report.get(1..).unwrap_or_default().to_vec(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hexdec(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .map(|c| u8::from_str_radix(c, 16).unwrap())
            .collect()
    }

    #[test]
    fn encode_packet_matches_flow_control_sample() {
        let cmd_area = hexdec("1f 77 02 00 00 86");
        let packet = encode_packet(&cmd_area).unwrap();
        assert_eq!(packet, hexdec("1e 06 1f 77 02 00 00 86"));
    }

    #[test]
    fn encode_packet_uses_two_byte_length_above_191() {
        let cmd_area = vec![0xAA; 192];
        let packet = encode_packet(&cmd_area).unwrap();
        assert_eq!(&packet[..3], &[REPORT_ID, 0xC0, 0xC0]);
        assert_eq!(packet.len(), 1 + 2 + 192);
    }

    #[test]
    fn encode_packet_rejects_too_long_payload() {
        let cmd_area = vec![0; VarintMode::max_packet_len() as usize + 1];
        let err = encode_packet(&cmd_area).unwrap_err();
        assert!(matches!(err, UsbError::PayloadTooLong { .. }), "{err:?}");
    }

    #[test]
    fn encode_reports_fits_exactly_one_report() {
        let cmd_area = vec![0xAA; REPORT_PAYLOAD];
        let reports = encode_reports(&cmd_area).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0][0], REPORT_ID);
        assert_eq!(reports[0][1], REPORT_PAYLOAD as u8);
        assert_eq!(&reports[0][2..], &cmd_area[..]);
    }

    #[test]
    fn encode_reports_splits_and_pads() {
        let cmd_area: Vec<u8> = (0..REPORT_PAYLOAD as u8 + 1).collect();
        let reports = encode_reports(&cmd_area).unwrap();
        assert_eq!(reports.len(), 2);

        // 第一份正好 64 字节，没有补齐。
        assert_eq!(reports[0][0], REPORT_ID);
        assert_eq!(reports[0][1], REPORT_PAYLOAD as u8);
        assert_eq!(&reports[0][2..], &cmd_area[..REPORT_PAYLOAD]);

        // 第二份只有 1 字节数据，其余补 0。
        assert_eq!(reports[1][0], REPORT_ID);
        assert_eq!(reports[1][1], 1);
        assert_eq!(reports[1][2], cmd_area[REPORT_PAYLOAD]);
        assert!(reports[1][3..].iter().all(|&b| b == 0));
    }

    #[test]
    fn encode_reports_empty_is_empty() {
        assert!(encode_reports(&[]).unwrap().is_empty());
    }

    #[test]
    fn decode_packet_round_trip() {
        let cmd_area = hexdec("1f 70 00 8f");
        let packet = encode_packet(&cmd_area).unwrap();
        let (decoded, consumed) = decode_packet(&packet).unwrap().unwrap();
        assert_eq!(decoded, cmd_area);
        assert_eq!(consumed, packet.len());
    }

    #[test]
    fn decode_packet_incomplete_returns_none() {
        assert!(decode_packet(&[]).unwrap().is_none());
        assert!(decode_packet(&[REPORT_ID]).unwrap().is_none());
        assert!(decode_packet(&hexdec("1e 06 1f 77")).unwrap().is_none());
        // 2 字节长度字段只到了第一个字节。
        assert!(decode_packet(&hexdec("1e c0")).unwrap().is_none());
    }

    #[test]
    fn decode_packet_bad_report_id() {
        let err = decode_packet(&hexdec("1f 06 1f 77 02 00 00 86")).unwrap_err();
        assert!(
            matches!(err, UsbError::BadReportId { actual: 0x1F, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn decode_report_ignores_padding() {
        let cmd_area = hexdec("1f 70 00 8f");
        let mut report = [0u8; REPORT_SIZE];
        let packet = encode_packet(&cmd_area).unwrap();
        report[..packet.len()].copy_from_slice(&packet);

        let decoded = decode_report(&report).unwrap();
        assert_eq!(decoded, cmd_area);
    }
}
