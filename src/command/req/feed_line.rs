// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    envelope::{Envelope, EnvelopeKind},
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

/// 单次 `FeedLine` 最多拆成多少段。
///
/// `lines` 是 `u32`，直接按段展开的话 `u32::MAX` 在 v0/v1 上会生成上千万条
/// 命令。这里限制单次调用的段数，正常打印（每页最多 16383 行）远低于上限。
pub const MAX_FEED_SEGMENTS: usize = 4096;

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
        let chunk_size = match mv {
            ModelVersion::V1 | ModelVersion::V0 => 255u32,
            ModelVersion::V2 => 16384,
        };
        let max_lines = chunk_size * MAX_FEED_SEGMENTS as u32;
        if self.lines > max_lines {
            return Err(PackReqError::ParamOutOfRange {
                param: "lines".into(),
                value: self.lines as i64,
                range: (Some(1), Some(max_lines as i64)),
            });
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
                    lines: n,
                    ..Default::default()
                }
                .to_bytes()
                .context(EncodeSnafu {
                    cmd_name: "FeedLineV1V0Tl",
                }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(buf.into_iter().map(|p| Envelope::new(p, ek)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_lines_is_noop() {
        assert!(
            FeedLine { lines: 0 }
                .pack(ModelVersion::V2)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_too_many_lines() {
        let err = FeedLine { lines: u32::MAX }
            .pack(ModelVersion::V0)
            .unwrap_err();
        assert!(
            matches!(&err, PackReqError::ParamOutOfRange { param, .. } if param == "lines"),
            "{err:?}"
        );
    }

    #[test]
    fn accepts_max_segments() {
        let lines = 255 * MAX_FEED_SEGMENTS as u32;
        let cmds = FeedLine { lines }.pack(ModelVersion::V1).unwrap();
        assert_eq!(cmds.len(), MAX_FEED_SEGMENTS);

        let lines = 16_384 * MAX_FEED_SEGMENTS as u32;
        let cmds = FeedLine { lines }.pack(ModelVersion::V2).unwrap();
        assert_eq!(cmds.len(), MAX_FEED_SEGMENTS);
    }

    #[test]
    fn splits_into_protocol_chunks() {
        let cmds = FeedLine { lines: 300 }.pack(ModelVersion::V2).unwrap();
        assert_eq!(cmds.len(), 1);

        let cmds = FeedLine { lines: 16_385 }.pack(ModelVersion::V2).unwrap();
        assert_eq!(cmds.len(), 2);

        let cmds = FeedLine { lines: 300 }.pack(ModelVersion::V1).unwrap();
        assert_eq!(cmds.len(), 2);
    }
}
