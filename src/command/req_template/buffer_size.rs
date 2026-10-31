// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 查询剩余缓冲容量
//!
//! - `BufferSizeV2V1V0Tl`: v2,v1,v0机型: `1f 77`

use super::der;

/// 查询剩余缓冲容量: v2,v1,v0机型: `1f 77`
///
/// 请求载荷固定为 `00 00`。
#[der(..CmdTemplate)]
pub struct BufferSizeV2V1V0Tl {
    #[deku(magic = b"\x1f\x77")]
    pub _reserved: [u8; 2],
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{enc_tl, eq, hexdec};
    use super::*;

    #[test]
    fn test_buffer_size_v2v1v0_tl_1() {
        let b = BufferSizeV2V1V0Tl::default();
        let enc = enc_tl(&b);
        let exp = hexdec("1f 77 00 00");
        eq(&enc, &exp);
    }
}
