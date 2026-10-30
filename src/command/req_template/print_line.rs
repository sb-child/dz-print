// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印一行
//!
//! - `PrintLineV2Tl`: v2机型: `1f 21`
//! - `PrintLineV1Tl`: v1机型: `1f 2a`
//! - `PrintLineV2V1Tl`: v2,v1机型: `1f 2b`
//! - `PrintLineV0Tl`: v0机型: `1d 76`

use super::der;
use super::varint::VarAuto;

/// 打印一行: v2机型: `1f 21`
#[der(..CmdTemplate)]
pub struct PrintLineV2Tl {
    #[deku(magic = b"\x1f\x21")]
    /// repeats = 重复次数。0 代表这一行只打印一次。取值 `0..=16383`。
    pub repeats: VarAuto,
    /// skips = 向右偏移的字节数。取值 `0..=191`。
    pub skips: VarAuto,
    /// data = 位图数据。
    pub data: Vec<u8>,
    pub _tail: (),
}

/// 打印一行: v1机型: `1f 2a`
#[der(..CmdTemplate)]
pub struct PrintLineV1Tl {
    #[deku(magic = b"\x1f\x2a")]
    #[deku(endian = "little")]
    /// dots = 位图数据的点数(比特数)，建议值=`data.len()*8`。取值 `0..=未知`。打印机会打印前dots个比特数据(左对齐)。
    pub dots: u16,
    /// data = 位图数据。
    pub data: Vec<u8>,
    pub _tail: (),
}

/// 打印一行: v2,v1机型: `1f 2b`
#[der(..CmdTemplate)]
pub struct PrintLineV2V1Tl {
    #[deku(magic = b"\x1f\x2b")]
    /// skips = 向右偏移的字节数。取值 `0..=191`。
    pub skips: VarAuto,
    /// data_len = 位图数据字节数。建议值=`data.len()`。取值 `0..=191`。
    pub data_len: VarAuto,
    /// data = 位图数据。
    pub data: Vec<u8>,
    pub _tail: (),
}

/// 打印一行: v0机型: `1d 76`
#[der(..CmdTemplate)]
pub struct PrintLineV0Tl {
    #[deku(magic = b"\x1d\x76\x30\x00")]
    #[deku(endian = "little")]
    /// data_len = 位图数据字节数。建议值=`data.len()`。取值 `0..=65535`。
    pub data_len: u16,
    #[deku(magic = b"\x01\x00")]
    /// data = 位图数据。
    pub data: Vec<u8>,
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

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

    #[test]
    fn test_print_line_v0_tl_1() {
        let b = PrintLineV0Tl {
            data_len: 12,
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1d 76 30 00 0c 00 01 00 01 02 03");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_v0_tl_2() {
        let b = PrintLineV0Tl {
            data_len: 2222,
            data: vec![0x01, 0x02, 0x03, 0x04],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1d 76 30 00 ae 08 01 00 01 02 03 04");
        eq(&enc, &exp);
    }
}
