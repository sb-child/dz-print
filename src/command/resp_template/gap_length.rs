// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析纸张间隔长度(仅v2机型)
//!
//! - `GapLengthV2Tl`: v2机型: `1f 45`

use super::der;

/// 解析纸张间隔长度: v2机型: `1f 45`
///
/// 数据是 1/2/3 字节变长编码，`deku` 无法只凭首字节区分 2/3 字节，因此这里
/// 只把原始字节收下，长度和取值交给 `resp::gap_length` 按 `payload.len()` 解码。
#[der(..RespTemplate)]
pub struct GapLengthV2Tl {
    #[deku(magic = b"\x1f\x45")]
    #[deku(read_all)]
    pub gap: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    #[test]
    fn parse_1_byte() {
        let tl: GapLengthV2Tl = dec_tl(&hexdec("1f 45 32"));
        eq(&tl.gap, &hexdec("32"));
    }

    #[test]
    fn parse_2_byte() {
        let tl: GapLengthV2Tl = dec_tl(&hexdec("1f 45 c0 4e"));
        eq(&tl.gap, &hexdec("c0 4e"));
    }

    #[test]
    fn parse_3_byte() {
        let tl: GapLengthV2Tl = dec_tl(&hexdec("1f 45 c0 4e 20"));
        eq(&tl.gap, &hexdec("c0 4e 20"));
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: GapLengthV2Tl = dec_tl(&hexdec("1f 44 32"));
    }
}
