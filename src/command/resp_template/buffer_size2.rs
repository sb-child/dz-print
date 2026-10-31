// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析固件主动上报的剩余缓冲容量
//!
//! - `BufferSize2V2V1V0Tl`: v2,v1,v0机型: `1f 7b`

use super::der;
use crate::command::varint::VarAuto;

/// 解析固件主动上报的剩余缓冲容量: v2,v1,v0机型: `1f 7b`
///
/// 载荷只含一个变长 `size`，没有 `status` / `syncToken`。这是固件主动推送的
/// 精简通道，主机不应主动发送 `1f 7b`。
#[der(..RespTemplate)]
pub struct BufferSize2V2V1V0Tl {
    #[deku(magic = b"\x1f\x7b")]
    /// size = 可用容量，单位 500 字节；`1` 按 1000 计。
    pub size: VarAuto,
    /// 其余字段
    #[deku(read_all)]
    pub _tail: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    #[test]
    fn parse_sample() {
        let tl: BufferSize2V2V1V0Tl = dec_tl(&hexdec("1f 7b 32"));
        assert_eq!(tl.size.get(), 50);
        assert!(tl._tail.is_empty());
    }

    #[test]
    fn parse_two_byte_size() {
        let tl: BufferSize2V2V1V0Tl = dec_tl(&hexdec("1f 7b c1 2c"));
        assert_eq!(tl.size.get(), 300);
    }

    #[test]
    fn parse_with_tail() {
        let tl: BufferSize2V2V1V0Tl = dec_tl(&hexdec("1f 7b 32 aa"));
        assert_eq!(tl.size.get(), 50);
        eq(&tl._tail, &hexdec("aa"));
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: BufferSize2V2V1V0Tl = dec_tl(&hexdec("1f 77 32"));
    }
}
