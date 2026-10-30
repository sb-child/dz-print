// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 设置打印宽度
//!
//! - `PageWidthV2V1Tl`: v2,v1机型: `1f 27`
//! - v0机型: 也许你需要 `1a 37` (PageOffsetV0Tl)。

use super::der;
use super::varint::VarAuto;

/// 设置打印宽度: v2,v1机型: `1f 27`
#[der(..CmdTemplate)]
pub struct PageWidthV2V1Tl {
    #[deku(magic = b"\x1f\x27")]
    /// bytes = (最终打印宽度dot+7)/8。取值 `0..=191`。
    pub bytes: VarAuto,
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_page_width_v2v1_tl_1() {
        let b = PageWidthV2V1Tl {
            bytes: VarAuto::new(72),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 27 48");
        eq(&enc, &exp);
    }

    #[test]
    fn test_page_width_v2v1_tl_2() {
        let b = PageWidthV2V1Tl {
            bytes: VarAuto::new(191),
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1f 27 bf");
        eq(&enc, &exp);
    }
}
