// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    envelope::{Envelope, EnvelopeKind},
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::buffer_size::BufferSizeV2V1V0Tl,
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// 查询剩余缓冲容量
#[derive(Debug, Clone, Default)]
pub struct BufferSize {}

impl ReqTrait for BufferSize {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let ek = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        let buf = BufferSizeV2V1V0Tl::default()
            .to_bytes()
            .context(EncodeSnafu {
                cmd_name: "BufferSizeV2V1V0Tl",
            })?;
        Ok(Envelope::new(buf, ek).into())
    }
}
