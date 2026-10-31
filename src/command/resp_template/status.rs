// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析打印机状态
//!
//! - `StatusV2V1V0Tl`: v2,v1,v0机型: `1f 70`

use super::der;

/// 解析打印机状态: v2,v1,v0机型: `1f 70`
///
/// - 载荷布局：`<状态码:1> <PageKey:2 BE> <tail...>`。
/// - DP27P 共 8 字节，DP305P 共 28 字节，尾部字段含义未确认。
#[der(..RespTemplate)]
pub struct StatusV2V1V0Tl {
    #[deku(magic = b"\x1f\x70")]
    /// status = 状态码，见 [`PrinterStatus`](crate::command::status::PrinterStatus)。
    pub status: u8,
    #[deku(endian = "big")]
    /// page_key = 当前页编号。
    pub page_key: u16,
    /// 载荷长度随固件变化。
    #[deku(read_all)]
    pub _tail: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    #[test]
    fn parse_dp27p_normal() {
        let tl: StatusV2V1V0Tl = dec_tl(&hexdec("1f 70 00 01 01 00 00 35 05 10"));
        assert_eq!(tl.status, 0x00);
        assert_eq!(tl.page_key, 0x0101);
        eq(&tl._tail, &hexdec("00 00 35 05 10"));
    }

    #[test]
    fn parse_dp27p_printing() {
        let tl: StatusV2V1V0Tl = dec_tl(&hexdec("1f 70 01 01 02 00 00 0f 05 11"));
        assert_eq!(tl.status, 0x01);
        assert_eq!(tl.page_key, 0x0102);
        eq(&tl._tail, &hexdec("00 00 0f 05 11"));
    }

    #[test]
    fn parse_without_tail() {
        let tl: StatusV2V1V0Tl = dec_tl(&hexdec("1f 70 00 00 01"));
        assert_eq!(tl.status, 0x00);
        assert_eq!(tl.page_key, 1);
        assert!(tl._tail.is_empty());
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: StatusV2V1V0Tl = dec_tl(&hexdec("1f 71 00 00 01"));
    }
}
