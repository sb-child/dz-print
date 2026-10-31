// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 读取制造商名称
//!
//! - `ManufacturerV2V1V0Tl`: v2,v1,v0机型: `1f 75`

use super::der;

/// 读取制造商名称: v2,v1,v0机型: `1f 75`
#[der(..CmdTemplate)]
pub struct ManufacturerV2V1V0Tl {
    #[deku(magic = b"\x1f\x75")]
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_manufacturer_v2v1v0_tl_1() {
        let b = ManufacturerV2V1V0Tl::default();
        let enc = enc_tl(&b);
        let exp = hexdec("1f 75");
        eq(&enc, &exp);
    }
}
