// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub mod end_page;
pub mod feed_line;
pub mod print_line;
pub mod start_page;
pub mod total_lines;

use deku::DekuError;
use snafu::Snafu;

use crate::command::{envelope::Envelope, models::ModelVersion};

pub trait ReqTrait {
    fn pack(&self, cv: ModelVersion) -> Result<Vec<Envelope>, PackReqError>;
}

#[derive(Debug, Snafu)]
pub enum PackReqError {
    UnsupportedUsage,
    #[snafu(display("Could not encode '{cmd_name}' pack."))]
    Encode {
        source: DekuError,
        cmd_name: String,
    },
    #[snafu(display(
        "Param out of range: '{param}'={value}, but range is '{}'.",
        format_range(range)
    ))]
    ParamOutOfRange {
        param: String,
        value: i64,
        /// a..=b
        range: (Option<i64>, Option<i64>),
    },
}

fn format_range(r: &(Option<i64>, Option<i64>)) -> String {
    let mut buf = itoa::Buffer::new();
    match r {
        (None, None) => "..".to_owned(),
        (None, Some(b)) => {
            let b_s = buf.format(*b);
            "..=".to_owned() + b_s
        }
        (Some(a), None) => {
            let a_s = buf.format(*a);
            a_s.to_owned() + ".."
        }
        (Some(a), Some(b)) => {
            let a_s = buf.format(*a);
            let mut buf = itoa::Buffer::new();
            let b_s = buf.format(*b);
            a_s.to_owned() + "..=" + b_s
        }
    }
}
