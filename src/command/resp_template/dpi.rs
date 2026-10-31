// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析 DPI
//!
//! - `DpiV2V1V0Tl`: v2,v1,v0机型: `1f 71`

use super::der;

/// 解析 DPI: v2,v1,v0机型: `1f 71`
#[der(..RespTemplate)]
pub struct DpiV2V1V0Tl {
    #[deku(magic = b"\x1f\x71")]
    #[deku(endian = "big")]
    pub dpi: u16,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, hexdec};
    use super::*;

    #[test]
    fn parse_dp27p() {
        let tl: DpiV2V1V0Tl = dec_tl(&hexdec("1f 71 01 2c"));
        assert_eq!(tl.dpi, 300);
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: DpiV2V1V0Tl = dec_tl(&hexdec("1f 72 01 2c"));
    }
}
