// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 读取 DPI
//!
//! - `DpiV2V1V0Tl`: v2,v1,v0机型: `1f 71`

use super::der;

/// 读取 DPI: v2,v1,v0机型: `1f 71`
#[der(..CmdTemplate)]
pub struct DpiV2V1V0Tl {
    #[deku(magic = b"\x1f\x71")]
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_dpi_v2v1v0_tl_1() {
        let b = DpiV2V1V0Tl::default();
        let enc = enc_tl(&b);
        let exp = hexdec("1f 71");
        eq(&enc, &exp);
    }
}
