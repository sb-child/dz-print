// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    envelope::{Envelope, EnvelopeKind},
    models::ModelVersion,
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::speed::SpeedV2V1V0Tl,
    settings::SpeedSetting,
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// 设置打印速度。
#[derive(Debug, Clone, Default)]
pub struct Speed {
    /// 打印速度。
    pub speed: SpeedSetting,
}

impl ReqTrait for Speed {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let ek = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        let buf = SpeedV2V1V0Tl {
            speed: self.speed.to_wire(mv),
            ..Default::default()
        }
        .to_bytes()
        .context(EncodeSnafu {
            cmd_name: "SpeedV2V1V0Tl",
        })?;
        Ok(Envelope::new(buf, ek).into())
    }
}
