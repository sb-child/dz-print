// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 读取设备名
//!
//! - `DeviceNameV2V1V0Tl`: v2,v1,v0机型: `1f 79`

use super::der;

/// 读取设备名: v2,v1,v0机型: `1f 79`
#[der(..CmdTemplate)]
pub struct DeviceNameV2V1V0Tl {
    #[deku(magic = b"\x1f\x79")]
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_device_name_v2v1v0_tl_1() {
        let b = DeviceNameV2V1V0Tl::default();
        let enc = enc_tl(&b);
        let exp = hexdec("1f 79");
        eq(&enc, &exp);
    }
}
