// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    envelope::{Envelope, EnvelopeKind},
    models::ModelVersion,
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::{
        end_page::{EndPageV0Tl, EndPageV1Tl, EndPageV2Tl},
        start_page::{StartPageV0Tl, StartPageV2Tl},
        start_page_seqs_v0::{StartPageSeq1V0Tl, StartPageSeq2V0Tl, StartPageSeq3V0Tl},
    },
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// 结束页
#[derive(Debug, Clone, Default)]
pub struct EndPage {}

impl ReqTrait for EndPage {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let ek = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        let buf = match mv {
            ModelVersion::V0 => EndPageV0Tl::default().to_bytes().context(EncodeSnafu {
                cmd_name: "EndPageV0Tl",
            }),
            ModelVersion::V1 => EndPageV1Tl::default().to_bytes().context(EncodeSnafu {
                cmd_name: "EndPageV1Tl",
            }),
            ModelVersion::V2 => EndPageV2Tl::default().to_bytes().context(EncodeSnafu {
                cmd_name: "EndPageV2Tl",
            }),
        }?;
        return Ok(Envelope::new(buf, ek).into());
    }
}
