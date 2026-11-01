// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 加热控制（`1f 25`）。
//!
//! `1f 25 <单行最大黑点数> <平均黑点数>`，两个字段都是强制 2 字节的变长编码。
//!
//! 厂商 SDK 在每个 band 的行命令发完后、结束页之前发一条（`MaxSfDots` /
//! `MaxLEDots`），把这一批行里最黑的一行和平均值告诉固件，固件据此决定
//! 打印头的加热脉冲。
//!
//! DP27P 上还没有观察到它改变打印行为：加不加这条，页耗时和纸面效果都一样
//! （RLE 的空白问题也不是它引起的）。这里按厂商流程保留。
//!
//! # 用法
//!
//! ```
//! use dz_print::command::{ModelVersion, req::{self, ReqTrait}};
//!
//! let cmd = req::heat_control::HeatControl {
//!     max_dots: 20,
//!     avg_dots: 30,
//! };
//! let envelopes = cmd.pack(ModelVersion::V2)?;
//! assert_eq!(envelopes[0].payload(), &[0x1f, 0x25, 0xC0, 0x14, 0xC0, 0x1E]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! 取值范围 `0..=16383`，超出返回 [`PackReqError::ParamOutOfRange`]。

use crate::command::{
    ModelVersion,
    envelope::{Envelope, EnvelopeKind},
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::heat_control::HeatControlV2V1V0Tl,
    varint::VarFixed2,
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// 一个 band 的加热统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeatControl {
    /// 单行最大黑点数（`0..=16383`）。
    pub max_dots: u16,
    /// 平均黑点数（`0..=16383`），按 `0.01` 点截断即可，固件只用它调功率。
    pub avg_dots: u16,
}

impl ReqTrait for HeatControl {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let ek = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        for (param, value) in [("max_dots", self.max_dots), ("avg_dots", self.avg_dots)] {
            if value > 16383 {
                return Err(PackReqError::ParamOutOfRange {
                    param: param.into(),
                    value: i64::from(value),
                    range: (Some(0), Some(16383)),
                });
            }
        }
        let buf = HeatControlV2V1V0Tl {
            start_dots: VarFixed2::new(i32::from(self.max_dots)),
            end_dots: VarFixed2::new(i32::from(self.avg_dots)),
            ..Default::default()
        }
        .to_bytes()
        .context(EncodeSnafu {
            cmd_name: "HeatControlV2V1V0Tl",
        })?;
        Ok(vec![Envelope::new(buf, ek)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::hexdec;

    fn payload(cmd: &HeatControl, mv: ModelVersion) -> Vec<u8> {
        cmd.pack(mv).unwrap()[0].payload().to_vec()
    }

    #[test]
    fn v2_payload_uses_fixed_two_byte_fields() {
        let cmd = HeatControl {
            max_dots: 526,
            avg_dots: 263,
        };
        assert_eq!(payload(&cmd, ModelVersion::V2), hexdec("1f 25 c2 0e c1 07"));
    }

    #[test]
    fn every_model_version_uses_the_same_payload() {
        let cmd = HeatControl {
            max_dots: 10,
            avg_dots: 20,
        };
        for mv in [ModelVersion::V0, ModelVersion::V1, ModelVersion::V2] {
            assert_eq!(payload(&cmd, mv), hexdec("1f 25 c0 0a c0 14"), "{mv:?}");
        }
    }

    #[test]
    fn too_many_dots_is_rejected() {
        let cmd = HeatControl {
            max_dots: 16_384,
            avg_dots: 0,
        };
        assert!(matches!(
            cmd.pack(ModelVersion::V2),
            Err(PackReqError::ParamOutOfRange { .. })
        ));
    }
}
