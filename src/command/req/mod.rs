// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 主机 → 设备的请求命令。
//!
//! 每条命令实现 [`ReqTrait`]：`pack(mv)` 按机型选字段，返回一到多条
//! [`Envelope`]（重复行、RLE 这类命令可能展开成多条）。
//!
//! # 用法
//!
//! ```
//! use dz_print::command::{ModelVersion, req::{self, ReqTrait}};
//!
//! // 走纸 3 行
//! let envelopes = req::feed_line::FeedLine { lines: 3 }.pack(ModelVersion::V2)?;
//! assert_eq!(envelopes.len(), 1);
//! assert_eq!(envelopes[0].payload(), &[0x1f, 0x22, 0x02]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! 要发出去时把 `ReqTrait` 对象的切片交给
//! [`crate::planner::Assembler::encode`]；实现新命令时参考同目录下的
//! `req_template` 模板和现有命令的 `pack`。

pub mod bluetooth_mac;
pub mod buffer_size;
pub mod darkness;
pub mod device_info;
pub mod device_name;
pub mod dpi;
pub mod end_page;
pub mod feed_line;
pub mod gap_length;
pub mod gap_type;
pub mod high_command;
pub mod manufacturer;
pub mod print_line;
pub mod print_line_rle;
pub mod print_width;
pub mod software_version;
pub mod speed;
pub mod start_page;
pub mod statistics;
pub mod status;
pub mod total_lines;

use crate::command::{ModelVersion, envelope::Envelope};
use deku::DekuError;
use snafu::Snafu;

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
    #[snafu(display("Envelope payload too short: {len} byte(s), need at least 2 (group + type)."))]
    EnvelopeTooShort {
        len: usize,
    },
    #[snafu(display("Envelope data too long: {len} byte(s), max {max}."))]
    PayloadTooLong {
        len: usize,
        max: usize,
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
