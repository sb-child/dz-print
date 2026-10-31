// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, buffer_size::budget_from_size, decode_resp},
    resp_template::buffer_size2::BufferSize2V2V1V0Tl,
};

/// 固件主动上报的剩余缓冲容量（`0x7b`）。
///
/// 载荷比 [`BufferSize`](super::buffer_size::BufferSize) 精简，只有
/// `size`；固件用它在打印过程中补充预算，主机不应主动发送。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferSize2 {
    /// 可用容量单位数。
    pub size: u16,
    /// 其余字段，长度随固件变化。
    pub tail: Vec<u8>,
}

impl BufferSize2 {
    /// 换算成字节数的发送预算。
    pub fn budget(&self) -> usize {
        budget_from_size(self.size)
    }
}

impl RespTrait for BufferSize2 {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: BufferSize2V2V1V0Tl = decode_resp(payload)?;
        Ok(Self {
            size: raw.size.get() as u16,
            tail: raw._tail,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn hexdec(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .map(|c| u8::from_str_radix(c, 16).unwrap())
            .collect()
    }

    #[test]
    fn parse_sample() {
        let b = BufferSize2::parse(&hexdec("1f 7b 32"), ModelVersion::V2).unwrap();
        assert_eq!(b.size, 50);
        assert_eq!(b.budget(), 25_000);
        assert!(b.tail.is_empty());
    }

    #[test]
    fn parse_two_byte_size() {
        let b = BufferSize2::parse(&hexdec("1f 7b c1 2c"), ModelVersion::V1).unwrap();
        assert_eq!(b.size, 300);
        assert_eq!(b.budget(), 150_000);
    }

    #[test]
    fn parse_with_tail() {
        let b = BufferSize2::parse(&hexdec("1f 7b 32 aa"), ModelVersion::V2).unwrap();
        assert_eq!(b.tail, hexdec("aa"));
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err = BufferSize2::parse(&hexdec("1f 77 32"), ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
