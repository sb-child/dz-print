// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    envelope::{Envelope, EnvelopeKind},
    high_command::HighCommandMode,
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::high_command::HighCommandV2V1V0Tl,
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// 激活 / 结束 `> 0x80` 命令。
///
/// 默认 `mode = Enable`，也就是 `1f 80 7f`。
#[derive(Debug, Clone, Copy, Default)]
pub struct HighCommand {
    /// 激活参数。
    pub mode: HighCommandMode,
}

impl ReqTrait for HighCommand {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let ek = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        let buf = HighCommandV2V1V0Tl {
            mode: self.mode.to_wire(),
            ..Default::default()
        }
        .to_bytes()
        .context(EncodeSnafu {
            cmd_name: "HighCommandV2V1V0Tl",
        })?;
        Ok(Envelope::new(buf, ek).into())
    }
}
