// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 请求命令模板

pub use crate::command::varint;
pub use derive_aliases::derive as der;
pub mod darkness;
pub mod end_page;
pub mod feed_line;
pub mod gap_length;
pub mod heat_control;
pub mod print_line;
pub mod repeat_line;
pub mod speed;
pub mod start_page;
pub mod start_page_seqs_v0;
pub mod total_lines;

#[cfg(test)]
mod test_toolkits {
    use pretty_hex::PrettyHex as _;
    pub fn hexdec(s: &str) -> Vec<u8> {
        s.split(&[' ', ',', '\t'])
            .filter(|c| c.len() == 2)
            .filter(|c| c.is_ascii())
            .map(|c| u8::from_str_radix(c, 16))
            .flatten()
            .collect()
    }

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
            enc.hex_dump(),
            exp.hex_dump()
        );
    }
}
