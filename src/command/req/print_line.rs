// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    envelope::{Envelope, EnvelopeKind},
    models::ModelVersion,
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
                let mut full_data = Vec::with_capacity(self.skips as usize + self.data.len());
                // 向右偏移填充
                full_data.resize(self.skips as usize, 0x00);
                // 加上传入的数据
                full_data.extend_from_slice(&self.data);
                let cmd = PrintLineV0Tl {
                    data_len: full_data.len() as u16,
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
                    let first_cmd = PrintLineV1Tl {
                        dots: (self.data.len() * 8) as u16,
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

fn append_repeat_lines(
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
