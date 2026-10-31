// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析打印统计信息
//!
//! - `StatisticsV2V1V0Tl`: v2,v1,v0机型: `1f 73`

use super::der;

/// 解析打印统计信息: v2,v1,v0机型: `1f 73`
///
/// 载荷是 4 个大端 `u32`。长度是否随固件变化尚未确认，多出来的字节放 tail。
#[der(..RespTemplate)]
pub struct StatisticsV2V1V0Tl {
    #[deku(magic = b"\x1f\x73")]
    #[deku(endian = "big")]
    /// work_lines = 电机一共走过的行数。
    pub work_lines: u32,
    #[deku(endian = "big")]
    /// print_lines = 其中打印过的行数。
    pub print_lines: u32,
    #[deku(endian = "big")]
    /// null_lines = 其中走过的空行数。
    pub null_lines: u32,
    #[deku(endian = "big")]
    /// print_pages = 打印过的总页数。
    pub print_pages: u32,
    /// 其余字段
    #[deku(read_all)]
    pub _tail: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    #[test]
    fn parse_sample() {
        let tl: StatisticsV2V1V0Tl = dec_tl(&hexdec(
            "1f 73 00 01 b6 4b 00 01 76 ce 00 00 71 58 00 00 00 de",
        ));
        assert_eq!(tl.work_lines, 112_203);
        assert_eq!(tl.print_lines, 95_950);
        assert_eq!(tl.null_lines, 29_016);
        assert_eq!(tl.print_pages, 222);
        assert!(tl._tail.is_empty());
    }

    #[test]
    fn parse_with_tail() {
        let tl: StatisticsV2V1V0Tl = dec_tl(&hexdec(
            "1f 73 00 00 00 01 00 00 00 02 00 00 00 03 00 00 00 04 aa bb",
        ));
        assert_eq!(tl.work_lines, 1);
        assert_eq!(tl.print_lines, 2);
        assert_eq!(tl.null_lines, 3);
        assert_eq!(tl.print_pages, 4);
        eq(&tl._tail, &hexdec("aa bb"));
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: StatisticsV2V1V0Tl = dec_tl(&hexdec(
            "1f 72 00 00 00 01 00 00 00 02 00 00 00 03 00 00 00 04",
        ));
    }
}
