// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 设置水平偏移
//!
//! - `PageOffsetV0Tl`: v0机型: `1a 37`
//! - v1,v2机型: 也许你需要 `1f 27` (PageWidthV2V1Tl)。

use super::der;

/// 设置水平偏移: v0机型: `1a 37`
#[der(..CmdTemplate)]
pub struct PageOffsetV0Tl {
    #[deku(magic = b"\x1a\x37")]
    /// offset = (纸宽px-内容宽px)/2/(dpi/25.4)+150。设置打印向右偏移毫米数，150=不偏移。取值 `150..=255`。
    /// - v0机型: DPI写死`[300/600/305/180/203]`
    /// - v0机型: 纸宽写死384px
    pub offset: u8,
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_page_offset_v0_tl_1() {
        let b = PageOffsetV0Tl {
            offset: 155,
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1a 37 9b");
        eq(&enc, &exp);
    }
}
