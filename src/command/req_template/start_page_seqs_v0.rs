// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 开始页序列(仅v0机型)
//!
//! - `StartPageParamV0Seq1Tl`: `1a 38` 参数1
//! - `StartPageParamV0Seq2Tl`: `1a 39` 参数2
//! - `StartPageParamV0Seq3Tl`: `1a 3a` 参数3

use super::der;

/// 开始页序列1: v0机型: `1a 38`
#[der(..CmdTemplate)]
pub struct StartPageSeq1V0Tl {
    #[deku(magic = b"\x1a\x38\x01")]
    pub _tail: (),
}

/// 开始页序列2: v0机型: `1a 39`
#[der(..CmdTemplate)]
pub struct StartPageSeq2V0Tl {
    #[deku(magic = b"\x1a\x39\x01")]
    pub _tail: (),
}

/// 开始页序列3: v0机型: `1a 3a`
#[der(..CmdTemplate)]
pub struct StartPageSeq3V0Tl {
    #[deku(magic = b"\x1a\x3a\x02")]
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_start_page_seq1_v0_tl_1() {
        let b = StartPageSeq1V0Tl {
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1a 38 01");
        eq(&enc, &exp);
    }

    #[test]
    fn test_start_page_seq2_v0_tl_1() {
        let b = StartPageSeq2V0Tl {
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1a 39 01");
        eq(&enc, &exp);
    }

    #[test]
    fn test_start_page_seq3_v0_tl_1() {
        let b = StartPageSeq3V0Tl {
            ..Default::default()
        };
        let enc = enc_tl(&b);
        let exp = hexdec("1a 3a 02");
        eq(&enc, &exp);
    }
}
