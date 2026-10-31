// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析软件版本
//!
//! - `SoftwareVersionV2V1V0Tl`: v2,v1,v0机型: `1f 7c`

use super::der;

/// 解析软件版本: v2,v1,v0机型: `1f 7c`
///
/// 载荷是 `x.y yyyymmdd` 格式、`0x00` 结尾的字符串。模板只保留原始字节，
/// 解码交给 [`resp::software_version`](crate::command::resp::software_version)。
#[der(..RespTemplate)]
pub struct SoftwareVersionV2V1V0Tl {
    #[deku(magic = b"\x1f\x7c")]
    #[deku(read_all)]
    pub software_version: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    #[test]
    fn parse_ascii_version() {
        let tl: SoftwareVersionV2V1V0Tl =
            dec_tl(&hexdec("1f 7c 31 2e 30 20 32 30 32 34 30 35 32 30 00"));
        eq(
            &tl.software_version,
            &hexdec("31 2e 30 20 32 30 32 34 30 35 32 30 00"),
        );
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: SoftwareVersionV2V1V0Tl = dec_tl(&hexdec("1f 7d 31 2e 30 00"));
    }
}
