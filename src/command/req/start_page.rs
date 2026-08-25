// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    envelope::{Envelope, EnvelopeKind},
    models::ModelVersion,
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::{
        gap_length::GapLengthV2Tl,
        gap_type::GapTypeV0V1V2Tl,
        page_offset::PageOffsetV0Tl,
        page_width::PageWidthV2V1Tl,
        start_page::{StartPageV0Tl, StartPageV2Tl},
        start_page_seqs_v0::{StartPageSeq1V0Tl, StartPageSeq2V0Tl, StartPageSeq3V0Tl},
    },
    settings::GapTypeSetting,
    varint::VarAuto,
};
use deku::DekuContainerWrite as _;
use snafu::ResultExt;

/// 开始页
#[derive(Debug, Clone, Default)]
pub struct StartPage {
    /// 页编号。
    /// - v2: 范围`1..=65534`。
    /// - v1: 不支持，此参数不使用。
    /// - v0: 不支持，此参数不使用。
    pub page_idx: u16,
    /// 是否在页尾打印分隔线。
    /// - v2: 支持。
    /// - v1: 不支持，保持`false`。
    /// - v0: 不支持，保持`false`。
    pub print_sep_line: bool,
    /// 打印数据最大像素宽度。
    /// - v2: 范围`0..=1521`。
    /// - v1: 范围`0..=1521`。
    /// - v0: 不支持，此参数不使用。
    pub data_pixel_width: u16,
    /// 设置纸张间隔类型。
    /// - v2,v1,v0: 支持。
    pub printer_gap_type: GapTypeSetting,
    /// 设置纸张间隔长度。
    /// - v2: 支持。范围`50..=4194303`。在`printer_gap_type != GapTypeSetting::Continuous`条件下生效。
    /// - v1,v0: 不支持。
    pub printer_gap_length: u32,
}

impl ReqTrait for StartPage {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let page_key_range = 1..=65534;
        let data_pixel_width_range = 1..=1521;
        let printer_gap_length_range = 50..=4194303;
        let ek = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        let buf = match mv {
            ModelVersion::V0 => {
                if self.print_sep_line {
                    return Err(PackReqError::UnsupportedUsage);
                }
                vec![
                    StartPageV0Tl::default().to_bytes().context(EncodeSnafu {
                        cmd_name: "StartPageV0Tl",
                    })?,
                    StartPageSeq1V0Tl::default()
                        .to_bytes()
                        .context(EncodeSnafu {
                            cmd_name: "StartPageSeq1V0Tl",
                        })?,
                    StartPageSeq2V0Tl::default()
                        .to_bytes()
                        .context(EncodeSnafu {
                            cmd_name: "StartPageSeq2V0Tl",
                        })?,
                    StartPageSeq3V0Tl::default()
                        .to_bytes()
                        .context(EncodeSnafu {
                            cmd_name: "StartPageSeq3V0Tl",
                        })?,
                    PageOffsetV0Tl {
                        offset: 150,
                        ..Default::default()
                    }
                    .to_bytes()
                    .context(EncodeSnafu {
                        cmd_name: "PageOffsetV0Tl",
                    })?,
                    GapTypeV0V1V2Tl {
                        gap_type: self.printer_gap_type as u8,
                        ..Default::default()
                    }
                    .to_bytes()
                    .context(EncodeSnafu {
                        cmd_name: "GapTypeV0V1V2Tl",
                    })?,
                ]
            }
            ModelVersion::V1 => {
                if self.print_sep_line {
                    return Err(PackReqError::UnsupportedUsage);
                }
                data_pixel_width_range
                    .contains(&self.data_pixel_width)
                    .ok_or(PackReqError::ParamOutOfRange {
                        param: "data_pixel_width".into(),
                        value: self.data_pixel_width.into(),
                        range: (Some(0), Some(1521)),
                    })?;
                let data_byte_count = (self.data_pixel_width + 7) / 8;
                vec![
                    PageWidthV2V1Tl {
                        bytes: VarAuto::new(data_byte_count as i32),
                        ..Default::default()
                    }
                    .to_bytes()
                    .context(EncodeSnafu {
                        cmd_name: "PageWidthV2V1Tl",
                    })?,
                    GapTypeV0V1V2Tl {
                        gap_type: self.printer_gap_type as u8,
                        ..Default::default()
                    }
                    .to_bytes()
                    .context(EncodeSnafu {
                        cmd_name: "GapTypeV0V1V2Tl",
                    })?,
                ]
            }
            ModelVersion::V2 => {
                page_key_range
                    .contains(&self.page_idx)
                    .ok_or(PackReqError::ParamOutOfRange {
                        param: "page_idx".into(),
                        value: self.page_idx.into(),
                        range: (Some(1), Some(65534)),
                    })?;
                data_pixel_width_range
                    .contains(&self.data_pixel_width)
                    .ok_or(PackReqError::ParamOutOfRange {
                        param: "data_pixel_width".into(),
                        value: self.data_pixel_width.into(),
                        range: (Some(0), Some(1521)),
                    })?;
                let data_byte_count = (self.data_pixel_width + 7) / 8;
                let mut r = vec![
                    StartPageV2Tl {
                        page_key: self.page_idx,
                        print_separate_line: self.print_sep_line,
                        ..Default::default()
                    }
                    .to_bytes()
                    .context(EncodeSnafu {
                        cmd_name: "StartPageV2Tl",
                    })?,
                    PageWidthV2V1Tl {
                        bytes: VarAuto::new(data_byte_count as i32),
                        ..Default::default()
                    }
                    .to_bytes()
                    .context(EncodeSnafu {
                        cmd_name: "PageWidthV2V1Tl",
                    })?,
                    GapTypeV0V1V2Tl {
                        gap_type: self.printer_gap_type as u8,
                        ..Default::default()
                    }
                    .to_bytes()
                    .context(EncodeSnafu {
                        cmd_name: "GapTypeV0V1V2Tl",
                    })?,
                ];
                if !matches!(self.printer_gap_type, GapTypeSetting::Continuous) {
                    printer_gap_length_range
                        .contains(&self.printer_gap_length)
                        .ok_or(PackReqError::ParamOutOfRange {
                            param: "printer_gap_length".into(),
                            value: self.printer_gap_length.into(),
                            range: (Some(50), Some(4194303)),
                        })?;
                    r.push(
                        GapLengthV2Tl {
                            gap: VarAuto::new(self.printer_gap_length as i32),
                            ..Default::default()
                        }
                        .to_bytes()
                        .context(EncodeSnafu {
                            cmd_name: "GapLengthV2Tl",
                        })?,
                    );
                }
                r
            }
        };
        Ok(buf.into_iter().map(|p| Envelope::new(p, ek)).collect())
    }
}
