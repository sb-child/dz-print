// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 响应命令模板
//!
//! 这些结构体只描述**线上字节**，`_tail` 之类的字段会接收固件差异；
//! 机型归一化在 [`crate::command::resp`]。
//!
//! # 用法
//!
//! ```
//! use deku::DekuContainerRead;
//! use dz_print::command::resp_template::status::StatusV2V1V0Tl;
//!
//! let payload = vec![0x1f, 0x70, 0x00, 0x01, 0x01, 0x00];
//! let ((rest, _), status) = StatusV2V1V0Tl::from_bytes((payload.as_slice(), 0))?;
//! assert!(rest.is_empty());
//! assert_eq!(status.status, 0x00);
//! assert_eq!(status.page_key, 0x0101);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! 业务代码请用 [`crate::command::resp`] 里的同名类型。

pub use derive_aliases::derive as der;
pub mod bluetooth_mac;
pub mod buffer_size;
pub mod buffer_size2;
pub mod darkness;
pub mod device_info;
pub mod device_name;
pub mod dpi;
pub mod gap_length;
pub mod gap_type;
pub mod high_command;
pub mod manufacturer;
pub mod print_width;
pub mod software_version;
pub mod speed;
pub mod statistics;
pub mod status;

#[cfg(test)]
mod test_toolkits {
    use pretty_hex::simple_hex;

    pub fn hexdec(s: &str) -> Vec<u8> {
        s.split(&[' ', ',', '\t'])
            .filter(|c| c.len() == 2)
            .filter(|c| c.is_ascii())
            .flat_map(|c| u8::from_str_radix(c, 16))
            .collect()
    }

    pub fn dec_tl<'a, D>(buf: &'a [u8]) -> D
    where
        D: deku::DekuContainerRead<'a>,
    {
        let ((rest, bits), tl) = D::from_bytes((buf, 0)).expect("DekuRead: Failed to decode.");
        assert!(
            rest.is_empty(),
            "DekuRead: {} trailing byte(s): {}",
            rest.len(),
            simple_hex(&rest)
        );
        assert_eq!(bits, 0, "DekuRead: trailing {} bit(s)", bits);
        tl
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
