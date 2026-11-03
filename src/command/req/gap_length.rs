// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    envelope::{Envelope, EnvelopeKind},
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::gap_length::GapLengthV2Tl,
    varint::VarAuto,
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// v2 允许的纸张间隔下限。
///
/// 单位 `0.01mm`，即 0.5mm；`StartPage` 里的 `printer_gap_length` 共用这个范围。
pub const MIN_GAP_LENGTH: u32 = 50;
/// v2 允许的纸张间隔上限，单位 `0.01mm`（约 41.9m）。
pub const MAX_GAP_LENGTH: u32 = 4_194_303;

/// 设置纸张间隔长度
#[derive(Debug, Clone, Default)]
pub struct GapLength {
    /// 纸张间隔。
    /// - 单位 0.01mm。
    /// - v2: 范围 [`MIN_GAP_LENGTH`]`..=`[`MAX_GAP_LENGTH`]。
    /// - v1,v0: 协议里没有确认，当前返回 `UnsupportedUsage`。
    pub gap: u32,
}

impl ReqTrait for GapLength {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let gap_range = MIN_GAP_LENGTH..=MAX_GAP_LENGTH;
        let ek = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        if !matches!(mv, ModelVersion::V2) {
            return Err(PackReqError::UnsupportedUsage);
        }
        gap_range
            .contains(&self.gap)
            .ok_or(PackReqError::ParamOutOfRange {
                param: "gap".into(),
                value: self.gap.into(),
                range: (
                    Some(i64::from(MIN_GAP_LENGTH)),
                    Some(i64::from(MAX_GAP_LENGTH)),
                ),
            })?;
        let buf = GapLengthV2Tl {
            gap: VarAuto::new(self.gap as i32),
            ..Default::default()
        }
        .to_bytes()
        .context(EncodeSnafu {
            cmd_name: "GapLengthV2Tl",
        })?;
        Ok(Envelope::new(buf, ek).into())
    }
}
