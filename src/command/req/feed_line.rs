// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    envelope::{Envelope, EnvelopeKind},
    models::ModelVersion,
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::feed_line::{FeedLineV1V0Tl, FeedLineV2Tl},
    varint::VarAuto,
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// 走纸(打印空行)
#[derive(Debug, Clone, Default)]
pub struct FeedLine {
    /// 走纸行数。
    /// - v2,v1,v0: 范围`1..=u32::MAX`，自动转换。0为no-op。
    pub lines: u32,
}

impl ReqTrait for FeedLine {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let ek = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Seg {
            /// `0..=16384`
            V2Seg(u16),
            /// `0..=255`
            V1Seg(u8),
        }
        if self.lines == 0 {
            return Ok(Default::default());
        }
        let make_segs = |chunk_size: usize, make_seg: fn(usize) -> Seg| -> Vec<Seg> {
            let count = self.lines / chunk_size as u32;
            let remainder = self.lines % chunk_size as u32;
            let mut segs = Vec::with_capacity(count as usize + usize::from(remainder > 0));
            segs.extend((0..count).map(|_| make_seg(chunk_size)));
            if remainder > 0 {
                segs.push(make_seg(remainder as usize));
            }
            segs
        };
        let segs = match mv {
            ModelVersion::V1 | ModelVersion::V0 => make_segs(255, |val| Seg::V1Seg(val as u8)),
            ModelVersion::V2 => make_segs(16384, |val| Seg::V2Seg(val as u16)),
        };
        let buf = segs
            .into_iter()
            .map(|s| match s {
                Seg::V2Seg(n) => FeedLineV2Tl {
                    lines: VarAuto::new((n - 1) as i32),
                    ..Default::default()
                }
                .to_bytes()
                .context(EncodeSnafu {
                    cmd_name: "FeedLineV2Tl",
                }),
                Seg::V1Seg(n) => FeedLineV1V0Tl {
                    lines: n as u8,
                    ..Default::default()
                }
                .to_bytes()
                .context(EncodeSnafu {
                    cmd_name: "FeedLineV1V0Tl",
                }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(buf.into_iter().map(|p| Envelope::new(p, ek)).collect());
    }
}
