// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    envelope::{Envelope, EnvelopeKind},
    models::ModelVersion,
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::darkness::DarknessV2V1V0Tl,
    settings::DarknessSetting,
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// 设置打印浓度。
#[derive(Debug, Clone, Default)]
pub struct Darkness {
    /// 打印浓度。
    pub darkness: DarknessSetting,
}

impl ReqTrait for Darkness {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let ek = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        let buf = DarknessV2V1V0Tl {
            darkness: self.darkness.to_wire(mv),
            ..Default::default()
        }
        .to_bytes()
        .context(EncodeSnafu {
            cmd_name: "DarknessV2V1V0Tl",
        })?;
        Ok(Envelope::new(buf, ek).into())
    }
}
