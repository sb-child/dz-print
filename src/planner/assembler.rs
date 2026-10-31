// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 命令组装器。
//!
//! [`Assembler`] 把一组请求编码成连续的命令区，也把陆续读到的字节切成完整响应。它只处理命令区（BLE 帧格式），USB 的 `1e <长度>` 外层由 [`usb`](super::usb) 负责。
//!
//! - 编码：[`Assembler::encode`] 调用 [`ReqTrait::pack`]，再按 [`EnvelopeKind`] 补长度字段和校验和。
//! - 解码：[`Assembler::decode_one`] 每次解出缓冲区头部的一条响应，返回响应和已消费字节数。

use snafu::{ResultExt, Snafu};

use crate::command::{
    ModelVersion, calculate_checksum,
    envelope::{Envelope, EnvelopeKind},
    req::{PackReqError, ReqTrait},
    resp::{ParseRespError, RespTrait},
    varint::VarintMode,
};

#[derive(Debug, Clone, Copy)]
pub struct Assembler {
    mv: ModelVersion,
}

impl Assembler {
    pub const fn new(mv: ModelVersion) -> Self {
        Self { mv }
    }

    /// 当前机型版本。
    pub const fn model_version(&self) -> ModelVersion {
        self.mv
    }

    /// 把一组请求编码成连续的命令区。
    pub fn encode(&self, cmds: &[&dyn ReqTrait]) -> Result<Vec<u8>, PackReqError> {
        // 先把所有命令展开成 Envelope。
        let mut envs = Vec::new();
        for cmd in cmds {
            envs.extend(cmd.pack(self.mv)?);
        }

        // 计算总长度。
        let total = envs.iter().try_fold(0usize, |total, env| {
            Ok::<_, PackReqError>(total + encoded_len(env)?)
        })?;

        let mut out = Vec::with_capacity(total);
        for env in &envs {
            append_envelope(env, &mut out);
        }
        Ok(out)
    }

    /// 解出缓冲区头部的第一条响应。
    ///
    /// - 返回 `Some((resp, consumed))`：
    ///   - `consumed` 是这条响应占用的字节数。
    ///   - `buf[consumed..]` 是下一条响应或尚未收全的尾部。
    /// - 返回 `None`：数据还不足一条完整帧，调用方保留 `buf` 等下次数据。
    pub fn decode_one<T: RespTrait>(&self, buf: &[u8]) -> Result<Option<(T, usize)>, DecodeError> {
        let Some((payload, consumed)) = self.decode_frame(buf)? else {
            return Ok(None);
        };
        let resp = T::parse(&payload, self.mv).context(RespSnafu)?;
        Ok(Some((resp, consumed)))
    }

    /// 切出缓冲区头部的一条完整帧，返回 payload 和已消费字节数。
    fn decode_frame(&self, buf: &[u8]) -> Result<Option<(Vec<u8>, usize)>, DecodeError> {
        // 至少需要 命令组 + 命令类型 + 1 字节长度
        if buf.len() < 3 {
            return Ok(None);
        }

        let (data_len, len_bytes) = match VarintMode::Auto.decode(&buf[2..]) {
            Some(v) => v,
            // 长度字段是 2 字节变长，但第二个字节还没到。
            None if buf.len() == 3 && buf[2] & 0xC0 == 0xC0 => return Ok(None),
            None => {
                return InvalidLengthSnafu {
                    bytes: buf[..3].to_vec(),
                }
                .fail();
            }
        };
        let data_len = data_len as usize;
        let total = 2 + len_bytes + data_len + 1;
        if buf.len() < total {
            return Ok(None);
        }

        let frame = &buf[..total];
        // 校验位 = 0x88 表示不校验
        let actual = frame[total - 1];
        if actual != 0x88 {
            let expected = calculate_checksum(frame, 1, total - 1);
            snafu::ensure!(actual == expected, BadChecksumSnafu { expected, actual });
        }

        let mut payload = Vec::with_capacity(2 + data_len);
        payload.extend_from_slice(&frame[..2]);
        payload.extend_from_slice(&frame[2 + len_bytes..total - 1]);
        Ok(Some((payload, total)))
    }
}

/// 响应切帧或解析错误。
#[derive(Debug, Snafu)]
pub enum DecodeError {
    #[snafu(display("Invalid response length field: {bytes:02X?}."))]
    InvalidLength { bytes: Vec<u8> },
    #[snafu(display("Bad checksum: expected {expected:#04X}, got {actual:#04X}."))]
    BadChecksum { expected: u8, actual: u8 },
    #[snafu(display("Could not parse response."))]
    Resp { source: ParseRespError },
}

/// 一条 [Envelope] 打包后的精确字节数。
///
/// 同时校验 payload 能否打包。`encode` 先逐条调用它，既得到总长度，也保证后面的 [`append_envelope`] 不会中途失败。
fn encoded_len(env: &Envelope) -> Result<usize, PackReqError> {
    let payload = env.payload();
    match env.kind() {
        EnvelopeKind::Raw | EnvelopeKind::ManualPack => Ok(payload.len()),
        EnvelopeKind::AutoPackWithChecksum | EnvelopeKind::AutoPackWithFixedChecksum => {
            // payload 至少要有命令组和命令类型，否则 `payload[2..]` 会越界。
            if payload.len() < 2 {
                return Err(PackReqError::EnvelopeTooShort { len: payload.len() });
            }

            // 长度字段只统计 data，不包含前两个命令头字节。
            // 例：`1f 43 05` -> data = `05` -> 长度字段写 `01`。
            let data_len = payload.len() - 2;

            // 帧长度走 1/2 字节的 Auto 编码，解包侧也只认这两种，
            // 所以上限是 `16383`，不是 Auto 编码本身的 `4194303`。
            let max = VarintMode::max_packet_len() as usize;
            if data_len > max {
                return Err(PackReqError::PayloadTooLong { len: data_len, max });
            }

            // 命令头 2 + 变长长度字段 + data + 校验和 1
            let len_bytes = VarintMode::Auto.encoded_len(data_len as i32);
            Ok(2 + len_bytes + data_len + 1)
        }
    }
}

/// 把一条 [Envelope] 追加到命令区。
///
/// [Envelope::payload] 的布局是 `命令组 + 命令类型 + data`，不含长度字段和校验和。
/// 这里按 `kind` 补上长度和校验和，最终线上布局：
///
/// ```text
/// <命令组:1> <命令类型:1> <data_len:变长> <data...> <校验和:1>
/// ```
///
/// 前置条件：[Envelope] 已经用 [encoded_len] 校验过，预留足够的空间获得最佳性能。
fn append_envelope(env: &Envelope, out: &mut Vec<u8>) {
    let payload = env.payload();
    match env.kind() {
        // Raw：`v0` 机型的 payload 本身就是完整命令，例如 `1b 40`、`1d 56 42 00`。
        // ManualPack：调用方已自行补好长度字段和校验和，组装器不再改动。
        EnvelopeKind::Raw | EnvelopeKind::ManualPack => {
            // 两者都原样追加，不计算长度和校验和。
            out.extend_from_slice(payload);
        }
        // 这一段把 `payload`（组 + 类型 + data）改写成线上帧：
        // 组 + 类型 + 变长长度 + data + 校验和。
        // `kind @ (...)` 把实际命中的枚举值绑到 `kind`，后面用它决定校验和。
        kind @ (EnvelopeKind::AutoPackWithChecksum | EnvelopeKind::AutoPackWithFixedChecksum) => {
            // encoded_len 已保证 payload 至少有 2 字节。直接跳过命令头读取 data。
            let data = &payload[2..];
            // payload = [命令组] [命令类型] [data...]
            // data =                        [data...]

            // 记住本帧在输出缓冲区里的起点。
            // 校验和要按帧内偏移计算。
            let start = out.len();
            // out = ... [上一帧] [本帧]
            // start =            ^ 本帧第一个字节位置

            // 命令组 + 命令类型原样保留。
            out.extend_from_slice(&payload[..2]);
            // out = ... [上一帧] [命令组] [命令类型]
            // 新增 2 字节        -------- ----------

            // 在命令头和 data 之间插入变长长度字段：
            // - `0..=191`：1 字节
            // - `192..=16383`：2 字节，`0xC0 | 高 6 位` + 低 8 位
            //
            // `encoded_len` 已检查过 `data.len() <= 16383`，这里不会越界。
            VarintMode::Auto.encode_into(data.len() as i32, out);
            // out = ... [上一帧] [命令组] [命令类型] [长度]
            // 新增 1 或 2 字节                       ------

            // data 紧跟在长度字段后面。
            out.extend_from_slice(data);
            // out = ... [上一帧] [命令组] [命令类型] [长度] [data...]
            // 新增 data.len() 字节                          ---------

            // 校验和覆盖帧内除命令组以外的所有字节。
            let checksum = match kind {
                // v1 固定写 `0x88`，表示“不校验”。
                EnvelopeKind::AutoPackWithFixedChecksum => 0x88,
                // 校验位本身按 0 参与计算，所以直接算到 out.len()，结果一样。
                // 区间是 `[start + 1, out.len())`。
                _ => calculate_checksum(out, start + 1, out.len()),
            };
            // checksum = 0x88 / checksum([命令类型] [长度] [data...])
            out.push(checksum);
            // out = ... [上一帧] [命令组] [命令类型] [长度] [data...] [checksum]
            // 新增 1 字节                                             ----------
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{
        req::{
            darkness::Darkness as ReqDarkness, end_page::EndPage, print_line::PrintLine,
            status::Status as ReqStatus,
        },
        resp::{darkness::Darkness as RespDarkness, status::Status as RespStatus},
        status::PrinterStatus,
    };

    fn hexdec(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .map(|c| u8::from_str_radix(c, 16).unwrap())
            .collect()
    }

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
}
