// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::command::{
    ModelVersion,
    resp::{ParseRespError, RespTrait, decode_resp},
    resp_template::buffer_size::BufferSizeV2V1V0Tl,
};

/// 剩余缓冲容量（`0x77`）。
///
/// 预算是 `size × 500` 字节，`size == 1` 时按 1000 字节计。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferSize {
    /// 可用容量单位数。
    pub size: u16,
    /// 状态：`0` 正常，`0x0400_0000` 等待数据，其余为错误。
    pub status: i32,
    /// 疑似固件侧包序号，SDK 只用于日志。
    pub sync_token: i16,
    /// 其余字段，长度随固件变化。
    pub tail: Vec<u8>,
}

impl BufferSize {
    /// 换算成字节数的发送预算。
    pub fn budget(&self) -> usize {
        budget_from_size(self.size)
    }

    /// 状态正常。
    pub fn is_ok(&self) -> bool {
        self.status == 0
    }

    /// 缓冲还在消化，需要等待数据。
    pub fn is_waiting_data(&self) -> bool {
        self.status == 0x0400_0000
    }

    /// 状态为错误。
    ///
    /// SDK 的判定是「除 bit26 外必须全为 0」，所以除 `0` 和 `0x0400_0000`
    /// 以外的取值都算错误。
    pub fn is_error(&self) -> bool {
        !self.is_ok() && !self.is_waiting_data()
    }
}

impl RespTrait for BufferSize {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        let raw: BufferSizeV2V1V0Tl = decode_resp(payload)?;
        Ok(Self {
            size: raw.size.get() as u16,
            status: raw.status,
            sync_token: raw.sync_token,
            tail: raw._tail,
        })
    }
}

/// 把容量单位数换算成字节数。
pub(crate) fn budget_from_size(size: u16) -> usize {
    match size {
        1 => 1000,
        n => n as usize * 500,
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
    fn parse_ok() {
        let b = BufferSize::parse(&hexdec("1f 77 32 00 00 00 00 00 00"), ModelVersion::V2).unwrap();
        assert_eq!(b.size, 50);
        assert_eq!(b.budget(), 25_000);
        assert_eq!(b.sync_token, 0);
        assert!(b.is_ok());
        assert!(!b.is_waiting_data());
        assert!(!b.is_error());
    }

    #[test]
    fn parse_waiting_data() {
        let b = BufferSize::parse(&hexdec("1f 77 32 00 00 00 04 34 12"), ModelVersion::V2).unwrap();
        assert_eq!(b.status, 0x0400_0000);
        assert_eq!(b.sync_token, 0x1234);
        assert!(!b.is_ok());
        assert!(b.is_waiting_data());
        assert!(!b.is_error());
    }

    #[test]
    fn parse_error_status() {
        let b = BufferSize::parse(&hexdec("1f 77 32 01 00 00 00 00 00"), ModelVersion::V2).unwrap();
        assert!(b.is_error());
    }

    #[test]
    fn budget_size_one_is_1000() {
        assert_eq!(budget_from_size(1), 1000);
    }

    #[test]
    fn budget_uses_500_units() {
        assert_eq!(budget_from_size(0), 0);
        assert_eq!(budget_from_size(2), 1000);
        assert_eq!(budget_from_size(50), 25_000);
        assert_eq!(budget_from_size(300), 150_000);
    }

    #[test]
    fn parse_with_tail() {
        let b =
            BufferSize::parse(&hexdec("1f 77 32 00 00 00 00 00 00 aa"), ModelVersion::V2).unwrap();
        assert_eq!(b.tail, hexdec("aa"));
    }

    #[test]
    fn magic_mismatch_is_decode_error() {
        let err =
            BufferSize::parse(&hexdec("1f 78 32 00 00 00 00 00 00"), ModelVersion::V2).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }
}
