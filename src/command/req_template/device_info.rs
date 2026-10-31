// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 读取设备信息（`0x7f`，语义未确认）
//!
//! - `DeviceInfoV2V1V0Tl`: v2,v1,v0机型: `1f 7f`

use super::der;

/// 读取设备信息: v2,v1,v0机型: `1f 7f`
#[der(..CmdTemplate)]
pub struct DeviceInfoV2V1V0Tl {
    #[deku(magic = b"\x1f\x7f")]
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_device_info_v2v1v0_tl_1() {
        let b = DeviceInfoV2V1V0Tl::default();
        let enc = enc_tl(&b);
        let exp = hexdec("1f 7f");
        eq(&enc, &exp);
    }
}
