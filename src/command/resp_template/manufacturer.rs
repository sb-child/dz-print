// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析制造商名称
//!
//! - `ManufacturerV2V1V0Tl`: v2,v1,v0机型: `1f 75`

use super::der;

/// 解析制造商名称: v2,v1,v0机型: `1f 75`
///
/// 载荷是 GBK/GB2312 编码、`0x00` 结尾的字符串。模板只保留原始字节，
/// 解码交给 [`resp::manufacturer`](crate::command::resp::manufacturer)。
#[der(..RespTemplate)]
pub struct ManufacturerV2V1V0Tl {
    #[deku(magic = b"\x1f\x75")]
    #[deku(read_all)]
    pub manufacturer: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    #[test]
    fn parse_dp27p() {
        let tl: ManufacturerV2V1V0Tl = dec_tl(&hexdec("1f 75 b5 c2 d9 a1 00"));
        eq(&tl.manufacturer, &hexdec("b5 c2 d9 a1 00"));
    }

    #[test]
    fn parse_without_nul() {
        let tl: ManufacturerV2V1V0Tl = dec_tl(&hexdec("1f 75 44 50 32 37 50"));
        eq(&tl.manufacturer, &hexdec("44 50 32 37 50"));
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: ManufacturerV2V1V0Tl = dec_tl(&hexdec("1f 74 b5 c2 d9 a1 00"));
    }
}
