// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 命令组装器。
//!
//! [`Assembler`] 把一组请求编码成连续的命令区，也把陆续读到的字节切成完整响应。它只处理命令区（BLE 帧格式），USB 的 `1e <长度>` 外层由 [`usb_frame`](crate::backend::usb_frame) 负责。
//!
//! - 编码：[`Assembler::encode`] 调用 [`ReqTrait::pack`]，再按 [`EnvelopeKind`] 补长度字段和校验和。
//! - 解码：[`Assembler::decode_one`] 每次解出缓冲区头部的一条响应，返回响应和已消费字节数。
//!
//! # 用法
//!
//! ```
//! use dz_print::command::{ModelVersion, req, resp, status::PrinterStatus};
//! use dz_print::planner::Assembler;
//!
//! let assembler = Assembler::new(ModelVersion::V2);
//!
//! // 请求 → 命令区
//! let command_area = assembler.encode(&[&req::status::Status {}])?;
//! assert_eq!(command_area, vec![0x1f, 0x70, 0x00, 0x8f]);
//!
//! // 响应帧 → 解析结果 + 已消费字节数
//! let frame = [0x1f, 0x70, 0x08, 0x00, 0x01, 0x01, 0x00, 0x00, 0x35, 0x05, 0x10, 0x3b];
//! let (status, used) = assembler
//!     .decode_one::<resp::status::Status>(&frame)?
//!     .expect("complete frame");
//! assert_eq!(status.status, PrinterStatus::Printable);
//! assert_eq!(used, frame.len());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! `decode_one` 在数据不完整时返回 `Ok(None)`，不会消费缓冲区；需要原始
//! payload（先按命令字节分发再解析）时用 [`Assembler::decode_frame`]。

use crate::command::{
    ModelVersion, calculate_checksum,
    envelope::{Envelope, EnvelopeKind},
    req::{PackReqError, ReqTrait},
    resp::{ParseRespError, RespTrait},
    varint::VarintMode,
};
use pretty_hex::simple_hex;
use snafu::{ResultExt, Snafu};

#[derive(Debug, Clone, Copy)]
pub struct Assembler {
    mv: ModelVersion,
}

impl Assembler {
    pub const fn new(mv: ModelVersion) -> Self {
        Self { mv }
    }

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
    ///
    /// - `payload` 是 `命令组 + 命令类型 + data`，不含长度字段和校验和。
    /// - `consumed` 是这条帧占用的字节数，调用方可以 `buf[consumed..]` 继续解下一条。
    /// - 返回 `None` 表示数据还不足一条完整帧。
    ///
    /// 传输层只负责切帧，不认识具体响应类型；需要类型化结果时用 [`Self::decode_one`]。
    pub fn decode_frame(&self, buf: &[u8]) -> Result<Option<(Vec<u8>, usize)>, DecodeError> {
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
        // 校验位 = 0x88 表示「不校验」。SDK 也按这个规则处理。
        // 注意歧义：v2 的合法校验和也可能正好是 0x88，此时会被当成不校验而跳过验证
        // （概率约 1/256）。这里保持 SDK 行为，不额外区分。
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
    #[snafu(display("Invalid response length field: {}.", simple_hex(bytes)))]
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

/// 把一条 [Envelope] 追加到命令区，按 `kind` 补上长度字段和校验和。
///
/// 前置条件：[Envelope] 已经用 [`encoded_len`] 校验过。
fn append_envelope(env: &Envelope, out: &mut Vec<u8>) {
    let payload = env.payload();
    let kind @ (EnvelopeKind::AutoPackWithChecksum | EnvelopeKind::AutoPackWithFixedChecksum) =
        env.kind()
    else {
        // Raw / ManualPack 已经是完整命令，不再改动。
        out.extend_from_slice(payload);
        return;
    };

    // encoded_len 已保证 payload 至少有命令组和命令类型。
    let data = &payload[2..];
    let start = out.len();
    out.extend_from_slice(&payload[..2]);
    VarintMode::Auto.encode_into(data.len() as i32, out);
    out.extend_from_slice(data);

    let checksum = match kind {
        EnvelopeKind::AutoPackWithFixedChecksum => 0x88,
        // 校验位本身按 0 参与计算，所以直接算到当前末尾。
        _ => calculate_checksum(out, start + 1, out.len()),
    };
    out.push(checksum);
}

#[cfg(test)]
mod tests;
