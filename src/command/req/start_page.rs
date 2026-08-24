use crate::command::{
    envelope::{Envelope, EnvelopeKind},
    models::ModelVersion,
    req::{EncodeSnafu, PackReqError, ReqTrait},
    req_template::{
        start_page::{StartPageV0Tl, StartPageV2Tl},
        start_page_seqs_v0::{StartPageSeq1V0Tl, StartPageSeq2V0Tl, StartPageSeq3V0Tl},
    },
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
}

impl ReqTrait for StartPage {
    fn pack(&self, cv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let page_key_range = 1..=65534;
        let ek = match cv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        let buf = match cv {
            ModelVersion::V0 => {
                if self.print_sep_line {
                    return Err(PackReqError::UnsupportedUsage);
                }
                Some(vec![
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
                ])
            }
            ModelVersion::V1 => {
                if self.print_sep_line {
                    return Err(PackReqError::UnsupportedUsage);
                }
                None
            }
            ModelVersion::V2 => {
                page_key_range
                    .contains(&self.page_idx)
                    .ok_or(PackReqError::ParamOutOfRange {
                        param: "page_idx".into(),
                        value: self.page_idx.into(),
                        range: (Some(1), Some(65534)),
                    })?;
                Some(vec![
                    StartPageV2Tl {
                        page_key: self.page_idx,
                        print_separate_line: self.print_sep_line,
                        ..Default::default()
                    }
                    .to_bytes()
                    .context(EncodeSnafu {
                        cmd_name: "StartPageV2Tl",
                    })?,
                ])
            }
        };
        if let Some(buf) = buf {
            return Ok(buf.into_iter().map(|p| Envelope::new(p, ek)).collect());
        }
        Ok(Default::default())
    }
}
