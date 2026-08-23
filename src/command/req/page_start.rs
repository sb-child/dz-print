use crate::command::{
    envelope::{Envelope, EnvelopeKind},
    models::ModelVersion,
    req::ReqTrait,
};
// use deku::DekuContainerWrite as _;

/// 开始页
pub struct PageStart {
    page_idx: u16,
    print_sep_line: bool,
}

impl PageStart {
    fn new(page_idx: u16, print_sep_line: bool) -> Self {
        PageStart {
            page_idx,
            print_sep_line,
        }
    }
}

impl ReqTrait for PageStart {
    fn pack(&self, cv: ModelVersion) -> Vec<Envelope> {
        todo!();
        // StartPageV0Tl::default().to_bytes();
        match cv {
            ModelVersion::V0 => vec![Envelope::new(vec![], EnvelopeKind::Raw)],
            ModelVersion::V1 => vec![],
            ModelVersion::V2 => todo!(),
        }
    }
}
