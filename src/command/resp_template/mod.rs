// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 响应命令模板

pub use derive_aliases::derive as der;
pub mod darkness;
pub mod gap_length;
pub mod gap_type;
pub mod speed;
pub mod status;

#[cfg(test)]
mod test_toolkits {
    use pretty_hex::PrettyHex as _;

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
            rest.hex_dump()
        );
        assert_eq!(bits, 0, "DekuRead: trailing {} bit(s)", bits);
        tl
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
