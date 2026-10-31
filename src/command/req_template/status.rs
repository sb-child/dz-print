// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 读取打印机状态
//!
//! - `StatusV2V1V0Tl`: v2,v1,v0机型: `1f 70`

use super::der;

/// 读取打印机状态: v2,v1,v0机型: `1f 70`
///
/// 空载荷。打包后线上是 `1f 70 00 <校验和>`。
#[der(..CmdTemplate)]
pub struct StatusV2V1V0Tl {
    #[deku(magic = b"\x1f\x70")]
    pub _tail: (),
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_status_v2v1v0_tl_1() {
        let b = StatusV2V1V0Tl::default();
        let enc = enc_tl(&b);
        let exp = hexdec("1f 70");
        eq(&enc, &exp);
    }
}
