// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 解析剩余缓冲容量
//!
//! - `BufferSizeV2V1V0Tl`: v2,v1,v0机型: `1f 77`

use super::der;
use crate::command::varint::VarAuto;

/// 解析剩余缓冲容量: v2,v1,v0机型: `1f 77`
///
/// 载荷布局：`<size:变长> <status:i32 LE> <syncToken:i16 LE> [其余...]`。
/// SDK 只读前三个字段，其余字节放 tail。
#[der(..RespTemplate)]
pub struct BufferSizeV2V1V0Tl {
    #[deku(magic = b"\x1f\x77")]
    /// size = 可用容量，单位 500 字节；`1` 按 1000 计。
    pub size: VarAuto,
    #[deku(endian = "little")]
    /// status = `0` 正常，`0x0400_0000` 等待数据，其余为错误。
    pub status: i32,
    #[deku(endian = "little")]
    /// sync_token = 疑似固件侧包序号，SDK 只用于日志。
    pub sync_token: i16,
    /// 其余字段
    #[deku(read_all)]
    pub _tail: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::super::test_toolkits::{dec_tl, eq, hexdec};
    use super::*;

    #[test]
    fn parse_size_ok() {
        let tl: BufferSizeV2V1V0Tl = dec_tl(&hexdec("1f 77 32 00 00 00 00 00 00"));
        assert_eq!(tl.size.get(), 50);
        assert_eq!(tl.status, 0);
        assert_eq!(tl.sync_token, 0);
        assert!(tl._tail.is_empty());
    }

    #[test]
    fn parse_status_waiting_data() {
        let tl: BufferSizeV2V1V0Tl = dec_tl(&hexdec("1f 77 32 00 00 00 04 34 12"));
        assert_eq!(tl.size.get(), 50);
        assert_eq!(tl.status, 0x0400_0000);
        assert_eq!(tl.sync_token, 0x1234);
    }

    #[test]
    fn parse_two_byte_size() {
        let tl: BufferSizeV2V1V0Tl = dec_tl(&hexdec("1f 77 c1 2c 00 00 00 00 00 00"));
        assert_eq!(tl.size.get(), 300);
        assert_eq!(tl.status, 0);
        assert_eq!(tl.sync_token, 0);
    }

    #[test]
    fn parse_with_tail() {
        let tl: BufferSizeV2V1V0Tl = dec_tl(&hexdec("1f 77 32 00 00 00 00 00 00 aa bb"));
        assert_eq!(tl.size.get(), 50);
        eq(&tl._tail, &hexdec("aa bb"));
    }

    #[test]
    #[should_panic(expected = "Failed to decode")]
    fn magic_mismatch() {
        let _: BufferSizeV2V1V0Tl = dec_tl(&hexdec("1f 78 32 00 00 00 00 00 00"));
    }
}
