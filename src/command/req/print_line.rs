// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印一行（普通位图数据，`1f 21` / `1f 2a` / `1f 2b` / `1d 76`）。
//!
//! 数据是 MSB first 的位图字节，`skips` 是左侧整字节空白。
//! `repeats` 表示这一行重复打印几次，超过机型上限时自动展开成 `1f 2e`。
//!
//! # 用法
//!
//! ```
//! use dz_print::command::{ModelVersion, req::{self, ReqTrait}};
//!
//! // 左侧空 1 字节，然后打印 0x80（一个黑点），不重复
//! let line = req::print_line::PrintLine {
//!     repeats: 0,
//!     skips: 1,
//!     data: vec![0x80],
//! };
//!
//! let envelopes = line.pack(ModelVersion::V2)?;
//! assert_eq!(envelopes.len(), 1);
//! assert_eq!(envelopes[0].payload(), &[0x1f, 0x21, 0x00, 0x01, 0x80]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! 从整张位图生成行命令时不要手动拼，用
//! [`crate::planner::print::print_bitmap`]。

use crate::command::{
    ModelVersion,
    envelope::{Envelope, EnvelopeKind},
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::{
        print_line::{PrintLineV0Tl, PrintLineV1Tl, PrintLineV2Tl, PrintLineV2V1Tl},
        repeat_line::RepeatLineV2V1Tl,
    },
    varint::VarAuto,
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// 打印一行
#[derive(Debug, Clone, Default)]
pub struct PrintLine {
    /// 重复次数。
    /// - v2,v1,v0: 范围`0..=u16::MAX`，自动转换。
    pub repeats: u16,
    /// 向右偏移的字节数。
    /// - v2,v1,v0: 范围`0..=191`，自动转换。
    pub skips: u8,
    /// 位图数据。必须有数据，否则考虑使用`feed_line`命令。
    /// - v2,v1,v0: 支持。
    pub data: Vec<u8>,
}

impl ReqTrait for PrintLine {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let skips_range = 0..=191;
        let ek = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        // 位图必须有数据
        if self.data.is_empty() {
            return Err(PackReqError::UnsupportedUsage);
        }
        // skips_range 范围 0..=191
        skips_range
            .contains(&self.skips)
            .ok_or(PackReqError::ParamOutOfRange {
                param: "skips".into(),
                value: self.skips.into(),
                range: (Some(0), Some(191)),
            })?;
        let mut res = Vec::new();
        match mv {
            ModelVersion::V0 => {
                // 要打印的数据
                let full_len = self.skips as usize + self.data.len();
                let data_len =
                    u16::try_from(full_len).map_err(|_| PackReqError::ParamOutOfRange {
                        param: "data_len".into(),
                        value: i64::try_from(full_len).unwrap_or(i64::MAX),
                        range: (Some(0), Some(u16::MAX as i64)),
                    })?;
                let mut full_data = Vec::with_capacity(full_len);
                // 向右偏移填充
                full_data.resize(self.skips as usize, 0x00);
                // 加上传入的数据
                full_data.extend_from_slice(&self.data);
                let cmd = PrintLineV0Tl {
                    data_len,
                    data: full_data,
                    ..Default::default()
                }
                .to_bytes()
                .context(EncodeSnafu {
                    cmd_name: "PrintLineV0Tl",
                })?;
                let env = Envelope::new(cmd, ek);
                // 重复次数 + 1 = 打印命令数量
                let total_times = (self.repeats as usize).saturating_add(1);
                res.resize(total_times, env);
            }
            ModelVersion::V1 => {
                if self.skips > 0 {
                    let first_cmd = PrintLineV2V1Tl {
                        skips: VarAuto::new(self.skips as i32),
                        data_len: VarAuto::new(self.data.len() as i32),
                        data: self.data.clone(),
                        ..Default::default()
                    };
                    res.push(Envelope::new(
                        first_cmd.to_bytes().context(EncodeSnafu {
                            cmd_name: "PrintLineV2V1Tl",
                        })?,
                        ek,
                    ));
                } else {
                    let dots = self
                        .data
                        .len()
                        .checked_mul(8)
                        .and_then(|dots| u16::try_from(dots).ok())
                        .ok_or_else(|| PackReqError::ParamOutOfRange {
                            param: "dots".into(),
                            value: i64::try_from(self.data.len().saturating_mul(8))
                                .unwrap_or(i64::MAX),
                            range: (Some(0), Some(u16::MAX as i64)),
                        })?;
                    let first_cmd = PrintLineV1Tl {
                        dots,
                        data: self.data.clone(),
                        ..Default::default()
                    };
                    res.push(Envelope::new(
                        first_cmd.to_bytes().context(EncodeSnafu {
                            cmd_name: "PrintLineV1Tl",
                        })?,
                        ek,
                    ));
                }
                // 插入 RepeatLineV2V1Tl
                // 每个命令最大走 191 行纸。命令上限 192 行。
                append_repeat_lines(&mut res, ek, self.repeats as usize, 191)?;
            }
            ModelVersion::V2 => {
                // PrintLineV2Tl 最大重复次数
                let max_v2_repeats = 16383;
                // 不够再用 RepeatLineV2V1Tl
                let (first_repeats, remain_repeats) = if (self.repeats as usize) > max_v2_repeats {
                    (max_v2_repeats, (self.repeats as usize) - max_v2_repeats)
                } else {
                    (self.repeats as usize, 0)
                };
                let first_cmd = PrintLineV2Tl {
                    repeats: VarAuto::new(first_repeats as i32),
                    skips: VarAuto::new(self.skips as i32),
                    data: self.data.clone(),
                    ..Default::default()
                };
                res.push(Envelope::new(
                    first_cmd.to_bytes().context(EncodeSnafu {
                        cmd_name: "PrintLineV2Tl",
                    })?,
                    ek,
                ));
                // 插入 RepeatLineV2V1Tl
                // 每个命令最大走 16383 行纸。命令上限 16384 行。
                append_repeat_lines(&mut res, ek, remain_repeats, 16383)?;
            }
        }
        Ok(res)
    }
}

pub(crate) fn append_repeat_lines(
    res: &mut Vec<Envelope>,
    ek: EnvelopeKind,
    mut remaining_lines: usize,
    chunk_size: usize,
) -> Result<(), PackReqError> {
    while remaining_lines > 0 {
        let count = remaining_lines.min(chunk_size);
        let repeat_cmd = RepeatLineV2V1Tl {
            lines: VarAuto::new((count - 1) as i32),
            ..Default::default()
        };
        res.push(Envelope::new(
            repeat_cmd.to_bytes().context(EncodeSnafu {
                cmd_name: "RepeatLineV2V1Tl",
            })?,
            ek,
        ));
        remaining_lines -= count;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v0_rejects_data_len_overflow() {
        let line = PrintLine {
            data: vec![0; 65_536],
            ..Default::default()
        };
        let err = line.pack(ModelVersion::V0).unwrap_err();
        assert!(
            matches!(&err, PackReqError::ParamOutOfRange { param, .. } if param == "data_len"),
            "{err:?}"
        );
    }

    #[test]
    fn v1_rejects_dots_overflow() {
        let line = PrintLine {
            data: vec![0; 8_192],
            ..Default::default()
        };
        let err = line.pack(ModelVersion::V1).unwrap_err();
        assert!(
            matches!(&err, PackReqError::ParamOutOfRange { param, .. } if param == "dots"),
            "{err:?}"
        );
    }

    #[test]
    fn v1_accepts_max_dots() {
        let line = PrintLine {
            data: vec![0; 8_191],
            ..Default::default()
        };
        assert!(!line.pack(ModelVersion::V1).unwrap().is_empty());
    }
}
