// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    envelope::{Envelope, EnvelopeKind},
    models::ModelVersion,
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::total_lines::{TotalLinesV1Tl, TotalLinesV2Tl},
    varint::{VarAuto, VarC0Be16},
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// 设置本次打印总行数
#[derive(Debug, Clone, Default)]
pub struct TotalLines {
    /// 本次打印总行数(含空白行)。
    /// - v2: 范围`0..=16383`。
    /// - v1: 范围`0..=65535`。
    /// - v0: 不支持，此参数不使用。
    pub lines: u16,
}

impl ReqTrait for TotalLines {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let lines_range_v2 = 0..=16383;
        let ek = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        let buf = match mv {
            ModelVersion::V0 => None,
            ModelVersion::V1 => {
                let r = TotalLinesV1Tl {
                    lines: VarC0Be16::new(self.lines.into()),
                    ..Default::default()
                }
                .to_bytes()
                .context(EncodeSnafu {
                    cmd_name: "TotalLinesV1Tl",
                })?;
                Some(r)
            }
            ModelVersion::V2 => {
                lines_range_v2
                    .contains(&self.lines)
                    .ok_or(PackReqError::ParamOutOfRange {
                        param: "lines".into(),
                        value: self.lines.into(),
                        range: (Some(0), Some(16383)),
                    })?;
                let r = TotalLinesV2Tl {
                    lines: VarAuto::new(self.lines.into()),
                    ..Default::default()
                }
                .to_bytes()
                .context(EncodeSnafu {
                    cmd_name: "TotalLinesV2Tl",
                })?;
                Some(r)
            }
        };
        if let Some(buf) = buf {
            return Ok(Envelope::new(buf, ek).into());
        }
        Ok(Default::default())
    }
}
