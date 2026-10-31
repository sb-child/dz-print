// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析打印宽度和出纸宽度
//!
//! - `PrintWidthV2V1V0Tl`: v2,v1,v0机型: `1f 72`

use super::der;

/// 解析打印宽度和出纸宽度: v2,v1,v0机型: `1f 72`
#[der(..RespTemplate)]
pub struct PrintWidthV2V1V0Tl {
    #[deku(magic = b"\x1f\x72")]
    #[deku(endian = "big")]
    /// print_width = 打印头点数
    pub print_width: u16,
    #[deku(endian = "big")]
    /// paper_width = 支持的最大纸宽，单位 0.1mm
    pub paper_width: u16,
    /// 载荷长度随固件变化
    #[deku(read_all)]
    pub _tail: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    #[test]
    fn parse_dp27p() {
        let tl: PrintWidthV2V1V0Tl = dec_tl(&hexdec("1f 72 02 40 02 3a 00 00 00 00 00"));
        assert_eq!(tl.print_width, 576);
        assert_eq!(tl.paper_width, 570);
        eq(&tl._tail, &hexdec("00 00 00 00 00"));
    }

    #[test]
    fn parse_dp305p() {
        let tl: PrintWidthV2V1V0Tl = dec_tl(&hexdec("1f 72 03 c0 03 84 00 00 00 00 00 00 00"));
        assert_eq!(tl.print_width, 960);
        assert_eq!(tl.paper_width, 900);
        eq(&tl._tail, &hexdec("00 00 00 00 00 00 00"));
    }

    #[test]
    fn parse_without_tail() {
        let tl: PrintWidthV2V1V0Tl = dec_tl(&hexdec("1f 72 02 40 02 3a"));
        assert_eq!(tl.print_width, 576);
        assert_eq!(tl.paper_width, 570);
        assert!(tl._tail.is_empty());
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: PrintWidthV2V1V0Tl = dec_tl(&hexdec("1f 71 02 40 02 3a"));
    }
}
