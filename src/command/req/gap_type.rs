// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    envelope::{Envelope, EnvelopeKind},
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::gap_type::GapTypeV0V1V2Tl,
    settings::GapTypeSetting,
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// 设置纸张间隔类型。
#[derive(Debug, Clone, Default)]
pub struct GapType {
    /// 纸张间隔类型。
    pub gap_type: GapTypeSetting,
}

impl ReqTrait for GapType {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let ek = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        let buf = GapTypeV0V1V2Tl {
            gap_type: self.gap_type.to_wire(),
            ..Default::default()
        }
        .to_bytes()
        .context(EncodeSnafu {
            cmd_name: "GapTypeV0V1V2Tl",
        })?;
        Ok(Envelope::new(buf, ek).into())
    }
}
