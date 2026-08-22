// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use deku::DekuContainerWrite as _;

use crate::command::{
    envelope::{Envelope, EnvelopeKind},
    models::CommandVersion,
    template::StartPageV0Tl,
};

pub trait Command {
    fn pack(&self, cv: CommandVersion) -> Vec<Envelope>; // 需要再设计一下
}

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

impl Command for PageStart {
    fn pack(&self, cv: CommandVersion) -> Vec<Envelope> {
        todo!();
        StartPageV0Tl::default().to_bytes();
        match cv {
            CommandVersion::V0 => vec![Envelope::new(vec![], EnvelopeKind::Raw)],
            CommandVersion::V1 => vec![],
            CommandVersion::V2 => todo!(),
        }
    }
}
