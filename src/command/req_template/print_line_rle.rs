// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印一行(RLE编码)
//!
//! - `PrintLineRleByteV2Tl`: v2机型: `1f 29`
//! - `PrintLineRle5xV2Tl`: v2机型: `1f 2c`
//! - `PrintLineRle5dV2Tl`: v2机型: `1f 2d`
//! - `PrintLineRle4xV2Tl`: v2机型: `1f 3a`
//! - `PrintLineRle4dV2Tl`: v2机型: `1f 3b`
//! - `PrintLineRle6xV2Tl`: v2机型: `1f 3c`
//! - `PrintLineRle6dV2Tl`: v2机型: `1f 3d`
//! - v1,v0机型: 替代使用 `print_line` 命令。

use super::der;
use super::varint::VarAuto;

/// 打印一行(RLE字节级编码): v2机型: `1f 29`
#[der(..CmdTemplate)]
pub struct PrintLineRleByteV2Tl {
    #[deku(magic = b"\x1f\x29")]
    /// rle_byte_count = RLE数据字节数。建议值=`data.len()`。取值 `0..=未知`。
    rle_byte_count: VarAuto,
    /// data = RLE数据。
    data: Vec<u8>,
    _tail: (),
}

/// 打印一行(RLE5X编码): v2机型: `1f 2c`
#[der(..CmdTemplate)]
pub struct PrintLineRle5xV2Tl {
    #[deku(magic = b"\x1f\x2c")]
    /// rle_runs_count = data中的RLE符号数量。取值 `0..=未知`。
    rle_runs_count: VarAuto,
    /// data = RLE数据。
    data: Vec<u8>,
    _tail: (),
}

/// 打印一行(RLE5D编码): v2机型: `1f 2d`
#[der(..CmdTemplate)]
pub struct PrintLineRle5dV2Tl {
    #[deku(magic = b"\x1f\x2d")]
    /// rle_runs_count = data中的RLE符号数量。取值 `0..=未知`。
    rle_runs_count: VarAuto,
    /// data = RLE 5位位流数据。
    data: Vec<u8>,
    _tail: (),
}

/// 打印一行(RLE4X编码): v2机型: `1f 3a`
#[der(..CmdTemplate)]
pub struct PrintLineRle4xV2Tl {
    #[deku(magic = b"\x1f\x3a")]
    /// rle_runs_count = data中的RLE符号数量。取值 `0..=未知`。
    rle_runs_count: VarAuto,
    /// data = RLE 4位位流数据。
    data: Vec<u8>,
    _tail: (),
}

/// 打印一行(RLE4D编码): v2机型: `1f 3b`
#[der(..CmdTemplate)]
pub struct PrintLineRle4dV2Tl {
    #[deku(magic = b"\x1f\x3b")]
    /// rle_runs_count = data中的RLE符号数量。取值 `0..=未知`。
    rle_runs_count: VarAuto,
    /// data = RLE 4位位流数据。
    data: Vec<u8>,
    _tail: (),
}

/// 打印一行(RLE6X编码): v2机型: `1f 3c`
#[der(..CmdTemplate)]
pub struct PrintLineRle6xV2Tl {
    #[deku(magic = b"\x1f\x3c")]
    /// rle_runs_count = data中的RLE符号数量。取值 `0..=未知`。
    rle_runs_count: VarAuto,
    /// data = RLE 6位位流数据。
    data: Vec<u8>,
    _tail: (),
}

/// 打印一行(RLE6D编码): v2机型: `1f 3d`
#[der(..CmdTemplate)]
pub struct PrintLineRle6dV2Tl {
    #[deku(magic = b"\x1f\x3d")]
    /// rle_runs_count = data中的RLE符号数量。取值 `0..=未知`。
    rle_runs_count: VarAuto,
    /// data = RLE 6位位流数据。
    data: Vec<u8>,
    _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_print_line_rle_byte_v2_tl_1() {
        let b = PrintLineRleByteV2Tl {
            rle_byte_count: VarAuto::new(23),
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 29 17 01 02 03");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_byte_v2_tl_2() {
        let b = PrintLineRleByteV2Tl {
            rle_byte_count: VarAuto::new(1000),
            data: vec![0x01, 0x02, 0x03, 0x04],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 29 c3 e8 01 02 03 04");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_5x_v2_tl_1() {
        let b = PrintLineRle5xV2Tl {
            rle_runs_count: VarAuto::new(23),
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 2c 17 01 02 03");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_5x_v2_tl_2() {
        let b = PrintLineRle5xV2Tl {
            rle_runs_count: VarAuto::new(1000),
            data: vec![0x01, 0x02, 0x03, 0x04],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 2c c3 e8 01 02 03 04");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_5d_v2_tl_1() {
        let b = PrintLineRle5dV2Tl {
            rle_runs_count: VarAuto::new(23),
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 2d 17 01 02 03");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_5d_v2_tl_2() {
        let b = PrintLineRle5dV2Tl {
            rle_runs_count: VarAuto::new(1000),
            data: vec![0x01, 0x02, 0x03, 0x04],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 2d c3 e8 01 02 03 04");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_4x_v2_tl_1() {
        let b = PrintLineRle4xV2Tl {
            rle_runs_count: VarAuto::new(23),
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 3a 17 01 02 03");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_4x_v2_tl_2() {
        let b = PrintLineRle4xV2Tl {
            rle_runs_count: VarAuto::new(1000),
            data: vec![0x01, 0x02, 0x03, 0x04],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 3a c3 e8 01 02 03 04");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_4d_v2_tl_1() {
        let b = PrintLineRle4dV2Tl {
            rle_runs_count: VarAuto::new(23),
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 3b 17 01 02 03");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_4d_v2_tl_2() {
        let b = PrintLineRle4dV2Tl {
            rle_runs_count: VarAuto::new(1000),
            data: vec![0x01, 0x02, 0x03, 0x04],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 3b c3 e8 01 02 03 04");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_6x_v2_tl_1() {
        let b = PrintLineRle6xV2Tl {
            rle_runs_count: VarAuto::new(23),
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 3c 17 01 02 03");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_6x_v2_tl_2() {
        let b = PrintLineRle6xV2Tl {
            rle_runs_count: VarAuto::new(1000),
            data: vec![0x01, 0x02, 0x03, 0x04],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 3c c3 e8 01 02 03 04");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_6d_v2_tl_1() {
        let b = PrintLineRle6dV2Tl {
            rle_runs_count: VarAuto::new(23),
            data: vec![0x01, 0x02, 0x03],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 3d 17 01 02 03");
        eq(&enc, &exp);
    }

    #[test]
    fn test_print_line_rle_6d_v2_tl_2() {
        let b = PrintLineRle6dV2Tl {
            rle_runs_count: VarAuto::new(1000),
            data: vec![0x01, 0x02, 0x03, 0x04],
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 3d c3 e8 01 02 03 04");
        eq(&enc, &exp);
    }
}
