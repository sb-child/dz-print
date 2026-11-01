// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 请求命令模板
//!
//! 这些结构体只描述**线上字节**（deku magic + 字段），不处理机型差异；
//! 机型归一化在 [`crate::command::req`]。
//!
//! # 用法
//!
//! ```
//! use deku::DekuContainerWrite;
//! use dz_print::command::req_template::feed_line::FeedLineV2Tl;
//! use dz_print::command::varint::VarAuto;
//!
//! let cmd = FeedLineV2Tl {
//!     lines: VarAuto::new(2), // 线上存的是“行数 - 1”
//!     ..Default::default()
//! };
//! assert_eq!(cmd.to_bytes()?, vec![0x1f, 0x22, 0x02]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! 业务代码请用 [`crate::command::req`] 里的同名命令，不要直接拼模板。

pub use crate::command::varint;
pub use derive_aliases::derive as der;
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
pub mod heat_control;
pub mod high_command;
pub mod manufacturer;
pub mod page_offset;
pub mod page_width;
pub mod print_line;
pub mod print_line_rle;
pub mod print_width;
pub mod repeat_line;
pub mod software_version;
pub mod speed;
pub mod start_page;
pub mod start_page_seqs_v0;
pub mod statistics;
pub mod status;
pub mod total_lines;

#[cfg(test)]
mod test_toolkits {
    pub(crate) use crate::test_util::hexdec;
    use pretty_hex::simple_hex;

    pub fn enc_tl<D>(tl: &D) -> Vec<u8>
    where
        D: deku::DekuWriter + deku::DekuContainerWrite,
    {
        tl.to_bytes().expect("DekuWriter: Failed to encode.")
    }

    pub fn eq(enc: &[u8], exp: &[u8]) {
        assert_eq!(
            enc,
            exp,
            "Test Failed:\nenc={}\nexp={}",
            simple_hex(&enc),
            simple_hex(&exp)
        );
    }
}
