// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::varint::VarAuto;
use derive_aliases::derive as der;

/// 开始页命令: v2机型: `1f 20`
#[der(..CmdTemplate)]
pub struct StartPageV2Tl {
    #[deku(magic = b"\x1f\x20")]
    #[deku(endian = "big")]
    /// page_key = 本页的编号，2字节大端(u16BE)。范围 `1~65534`，溢出重置到 `1`。
    pub page_key: u16,
    #[deku(magic = b"\x00\x00\x00\x00")]
    /// print_separate_line = 是否打印分隔线，1字节布尔值。
    pub print_separate_line: bool,
    #[deku(magic = b"\x00")]
    _tail: (),
}

/// 开始页命令: v0机型: `1b 40`
#[der(..CmdTemplate)]
pub struct StartPageV0Tl {
    #[deku(magic = b"\x1b\x40")]
    _tail: (),
}

/// 打印一行: v2机型: `1f 21`
#[der(..CmdTemplate)]
pub struct PrintLineV2Tl {
    #[deku(magic = b"\x1f\x21")]
    /// repeats = (重复次数 - 1)。0 代表这一行只打印一次。取值 `0~16383`。
    repeats: VarAuto,
    /// skips = 向右偏移的字节数。取值 `0~191`。
    skips: VarAuto,
    /// data = 位图数据。
    data: Vec<u8>,
    _tail: (),
}

/// 打印一行: v1机型: `1f 2a`
#[der(..CmdTemplate)]
pub struct PrintLineV1Tl {
    #[deku(magic = b"\x1f\x2a")]
    #[deku(endian = "little")]
    /// dots = 位图数据的点数(位数)。取值 `0~未知`。
    dots: u16,
    /// data = 位图数据。
    data: Vec<u8>,
    _tail: (),
}

/// 打印一行: v2,v1机型: `1f 2b`
#[der(..CmdTemplate)]
pub struct PrintLineV2V1Tl {
    #[deku(magic = b"\x1f\x2b")]
    /// skips = 向右偏移的字节数。取值 `0~191`。
    skips: VarAuto,
    /// skips = 位图数据长度。取值 `0~191`。
    data_len: VarAuto,
    /// data = 位图数据。
    data: Vec<u8>,
    _tail: (),
}

#[cfg(test)]
mod tests {
    use pretty_hex::PrettyHex;

    use super::*;

    fn hexdec(s: &str) -> Vec<u8> {
        s.split(&[' ', ',', '\t'])
            .filter(|c| c.len() == 2)
            .filter(|c| c.is_ascii())
            .map(|c| u8::from_str_radix(c, 16))
            .flatten()
            .collect()
    }

    fn enc_tl<D>(tl: &D) -> Vec<u8>
    where
        D: deku::DekuWriter + deku::DekuContainerWrite,
    {
        tl.to_bytes().expect("DekuWriter: Failed to encode.")
    }

    fn eq(enc: &[u8], exp: &[u8]) {
        assert_eq!(
            enc,
            exp,
            "Test Failed:\nenc={}\nexp={}",
            enc.hex_dump(),
            exp.hex_dump()
        );
    }

    #[test]
    fn test_start_page_v2_tl_1() {
        let b = StartPageV2Tl {
            page_key: 30,
            print_separate_line: false,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 20 00 1e 00 00 00 00 00 00");
        eq(&enc, &exp);
    }

    #[test]
    fn test_start_page_v2_tl_2() {
        let b = StartPageV2Tl {
            page_key: 65534,
            print_separate_line: true,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 20 ff fe 00 00 00 00 01 00");
        eq(&enc, &exp);
    }

    #[test]
    fn test_start_page_v0_tl_1() {
        let b = StartPageV0Tl {
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1b 40");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_v2_tl_1() {
        let b = PrintLineV2Tl {
            repeats: VarAuto::new(2),
            skips: VarAuto::new(0),
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 21 02 00 01 02 03");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_v2_tl_2() {
        let b = PrintLineV2Tl {
            repeats: VarAuto::new(16383),
            skips: VarAuto::new(191),
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 21 ff ff bf 01 02 03");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_v2v1_tl_1() {
        let b = PrintLineV2V1Tl {
            skips: VarAuto::new(3),
            data_len: VarAuto::new(30),
            data: vec![0x01, 0x02, 0x03, 0x03, 0x03, 0x03, 0x05],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 2b 03 1e 01 02 03 03 03 03 05");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_v2v1_tl_2() {
        let b = PrintLineV2V1Tl {
            skips: VarAuto::new(191),
            data_len: VarAuto::new(191),
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 2b bf bf 01 02 03");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_v1_tl_1() {
        let b = PrintLineV1Tl {
            dots: 3032,
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 2a d8 0b 01 02 03");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_v1_tl_2() {
        let b = PrintLineV1Tl {
            dots: 20,
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 2a 14 00 01 02 03");
        eq(&enc, &exp);
    }
}
