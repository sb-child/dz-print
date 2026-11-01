// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印一行(RLE 编码)。
//!
//! - `v2` 机型：按 [`RleKind`] 发 `1f 29` / `1f 2c` / `1f 2d` / `1f 3a` ~ `1f 3d`，
//!   `repeats` 用 `1f 2e` 展开（模板本身没有重复字段）。
//! - `v1` / `v0` 机型：没有 RLE 命令，返回 [`PackReqError::UnsupportedUsage`]，
//!   调用方应改用 [`PrintLine`](super::print_line::PrintLine)。
//!
//! # 用法
//!
//! ```
//! use dz_print::command::{
//!     ModelVersion,
//!     req::{self, ReqTrait, print_line_rle::RleKind},
//! };
//!
//! let cmd = req::print_line_rle::PrintLineRle {
//!     repeats: 0,
//!     kind: RleKind::Bits5x,
//!     count: 2,                    // 符号数
//!     data: vec![0x1F, 0xC0],      // MSB-first 位流
//! };
//! let envelopes = cmd.pack(ModelVersion::V2)?;
//! assert_eq!(envelopes.len(), 1);
//!
//! // v1 / v0 没有 RLE 命令
//! assert!(cmd.pack(ModelVersion::V1).is_err());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! 位流由 [`crate::rle`] 生成，选择用哪一族由
//! [`crate::planner::print`] 负责。

use crate::command::{
    ModelVersion, calculate_checksum,
    envelope::{Envelope, EnvelopeKind},
    req::{EncodeSnafu, PackReqError, ReqTrait, print_line::append_repeat_lines},
    req_template::print_line_rle::{
        PrintLineRle4dV2Tl, PrintLineRle4xV2Tl, PrintLineRle5dV2Tl, PrintLineRle5xV2Tl,
        PrintLineRle6dV2Tl, PrintLineRle6xV2Tl, PrintLineRleByteV2Tl,
    },
    varint::{VarAuto, VarintMode},
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// RLE 编码方式，对应 7 条打印命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RleKind {
    /// 字节级 RLE：`1f 29`。
    #[default]
    Byte,
    /// 4 位符号，直接编码当前行：`1f 3a`。
    Bits4x,
    /// 4 位符号，先和上一行 XOR：`1f 3b`。
    Bits4d,
    /// 5 位符号，直接编码当前行：`1f 2c`。
    Bits5x,
    /// 5 位符号，先和上一行 XOR：`1f 2d`。
    Bits5d,
    /// 6 位符号，直接编码当前行：`1f 3c`。
    Bits6x,
    /// 6 位符号，先和上一行 XOR：`1f 3d`。
    Bits6d,
}

/// 打印一行(RLE 编码)。
#[derive(Debug, Clone, Default)]
pub struct PrintLineRle {
    /// 重复次数，`0` 表示这一行只打印一次。
    /// - v2: 用 `1f 2e` 展开，每段最大 `16383` 行。
    /// - v1,v0: 不支持。
    pub repeats: u16,
    /// 编码方式。
    pub kind: RleKind,
    /// 符号数量。
    /// - 字节级 RLE: 忽略，等于 `data.len()`。
    /// - 位级 RLE: RLE 符号数量。
    pub count: u32,
    /// RLE 数据。
    pub data: Vec<u8>,
}

impl ReqTrait for PrintLineRle {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        // RLE 只有 v2 有；v1/v0 请直接用 PrintLine。
        if !matches!(mv, ModelVersion::V2) {
            return Err(PackReqError::UnsupportedUsage);
        }
        // 空数据没有意义，空白行应该用 FeedLine。
        if self.data.is_empty() {
            return Err(PackReqError::UnsupportedUsage);
        }
        // RLE 帧的 `<长度>` / `<符号数>` 是协议字段，模板里已经写好。
        // 这里用 `ManualPack`，避免 `Assembler` 再补一层长度；校验和手动补。
        let bytes = match self.kind {
            RleKind::Byte => {
                let count = var_count(self.data.len() as u32, "rle_byte_count")?;
                PrintLineRleByteV2Tl {
                    rle_byte_count: count,
                    data: self.data.clone(),
                    ..Default::default()
                }
                .to_bytes()
                .context(EncodeSnafu {
                    cmd_name: "PrintLineRleByteV2Tl",
                })?
            }
            RleKind::Bits4x => {
                let cmd = PrintLineRle4xV2Tl {
                    rle_runs_count: var_count(self.count, "rle_runs_count")?,
                    data: self.data.clone(),
                    ..Default::default()
                };
                cmd.to_bytes().context(EncodeSnafu {
                    cmd_name: "PrintLineRle4xV2Tl",
                })?
            }
            RleKind::Bits4d => {
                let cmd = PrintLineRle4dV2Tl {
                    rle_runs_count: var_count(self.count, "rle_runs_count")?,
                    data: self.data.clone(),
                    ..Default::default()
                };
                cmd.to_bytes().context(EncodeSnafu {
                    cmd_name: "PrintLineRle4dV2Tl",
                })?
            }
            RleKind::Bits5x => {
                let cmd = PrintLineRle5xV2Tl {
                    rle_runs_count: var_count(self.count, "rle_runs_count")?,
                    data: self.data.clone(),
                    ..Default::default()
                };
                cmd.to_bytes().context(EncodeSnafu {
                    cmd_name: "PrintLineRle5xV2Tl",
                })?
            }
            RleKind::Bits5d => {
                let cmd = PrintLineRle5dV2Tl {
                    rle_runs_count: var_count(self.count, "rle_runs_count")?,
                    data: self.data.clone(),
                    ..Default::default()
                };
                cmd.to_bytes().context(EncodeSnafu {
                    cmd_name: "PrintLineRle5dV2Tl",
                })?
            }
            RleKind::Bits6x => {
                let cmd = PrintLineRle6xV2Tl {
                    rle_runs_count: var_count(self.count, "rle_runs_count")?,
                    data: self.data.clone(),
                    ..Default::default()
                };
                cmd.to_bytes().context(EncodeSnafu {
                    cmd_name: "PrintLineRle6xV2Tl",
                })?
            }
            RleKind::Bits6d => {
                let cmd = PrintLineRle6dV2Tl {
                    rle_runs_count: var_count(self.count, "rle_runs_count")?,
                    data: self.data.clone(),
                    ..Default::default()
                };
                cmd.to_bytes().context(EncodeSnafu {
                    cmd_name: "PrintLineRle6dV2Tl",
                })?
            }
        };
        let mut frame = bytes;
        let checksum = calculate_checksum(&frame, 1, frame.len());
        frame.push(checksum);

        let mut res = vec![Envelope::new(frame, EnvelopeKind::ManualPack)];
        // 每段 0x2e 最多 16383 行；`0x2e` 是普通命令，仍走自动长度 + 校验。
        append_repeat_lines(
            &mut res,
            EnvelopeKind::AutoPackWithChecksum,
            self.repeats as usize,
            16383,
        )?;
        Ok(res)
    }
}

fn var_count(count: u32, param: &str) -> Result<VarAuto, PackReqError> {
    let max = VarintMode::Auto.max_value();
    if i64::from(count) > i64::from(max) {
        return Err(PackReqError::ParamOutOfRange {
            param: param.into(),
            value: i64::from(count),
            range: (Some(0), Some(i64::from(max))),
        });
    }
    Ok(VarAuto::new(count as i32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::hexdec;

    fn payloads(cmd: &PrintLineRle, mv: ModelVersion) -> Vec<Vec<u8>> {
        cmd.pack(mv)
            .unwrap()
            .iter()
            .map(|env| env.payload().to_vec())
            .collect()
    }

    #[test]
    fn v2_uses_the_matching_template() {
        let cases = [
            // RLE 帧是 ManualPack：协议字段 + 校验和，不再有 Assembler 的额外长度。
            (
                RleKind::Byte,
                vec![0xC2, 0xAA, 0xC1],
                "1f 29 03 c2 aa c1 a6",
            ),
            (RleKind::Bits4x, vec![0xAE], "1f 3a 02 ae 15"),
            (RleKind::Bits4d, vec![0xAE], "1f 3b 02 ae 14"),
            (RleKind::Bits5x, vec![0x1F, 0xC0], "1f 2c 02 1f c0 f2"),
            (RleKind::Bits5d, vec![0x1F, 0xC0], "1f 2d 02 1f c0 f1"),
            (RleKind::Bits6x, vec![0x3F, 0xF0], "1f 3c 02 3f f0 92"),
            (RleKind::Bits6d, vec![0x3F, 0xF0], "1f 3d 02 3f f0 91"),
        ];
        for (kind, data, expected) in cases {
            let cmd = PrintLineRle {
                repeats: 0,
                kind,
                count: 2,
                data,
            };
            assert_eq!(
                payloads(&cmd, ModelVersion::V2),
                vec![hexdec(expected)],
                "{kind:?}"
            );
        }
    }

    #[test]
    fn v2_repeats_expand_to_0x2e() {
        let cmd = PrintLineRle {
            repeats: 3,
            kind: RleKind::Bits4x,
            count: 2,
            data: vec![0xAE],
        };
        assert_eq!(
            payloads(&cmd, ModelVersion::V2),
            vec![hexdec("1f 3a 02 ae 15"), hexdec("1f 2e 02")]
        );
    }

    #[test]
    fn v1_and_v0_are_unsupported() {
        let cmd = PrintLineRle {
            kind: RleKind::Bits4x,
            count: 2,
            data: vec![0xAE],
            ..Default::default()
        };
        for mv in [ModelVersion::V1, ModelVersion::V0] {
            assert!(
                matches!(cmd.pack(mv), Err(PackReqError::UnsupportedUsage)),
                "{mv:?}"
            );
        }
    }

    #[test]
    fn empty_data_is_unsupported() {
        let cmd = PrintLineRle {
            kind: RleKind::Bits4x,
            count: 0,
            data: Vec::new(),
            ..Default::default()
        };
        assert!(matches!(
            cmd.pack(ModelVersion::V2),
            Err(PackReqError::UnsupportedUsage)
        ));
    }

    #[test]
    fn count_above_varint_max_is_rejected() {
        let cmd = PrintLineRle {
            kind: RleKind::Bits6x,
            count: 4_194_304,
            data: vec![0xFF],
            ..Default::default()
        };
        assert!(matches!(
            cmd.pack(ModelVersion::V2),
            Err(PackReqError::ParamOutOfRange { .. })
        ));
    }
}
