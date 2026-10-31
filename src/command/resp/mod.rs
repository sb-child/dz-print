// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 设备 → 主机（resp）响应层。
//!
//! 分工与 `req` 对称：
//! - [`resp_template`](crate::command::resp_template)：只描述线上字节，保留
//!   `#[deku(magic = ...)]`，不做版本归一化。
//! - `resp`：把模板解析结果按机型（[`ModelVersion`]）归一化成调用方使用的 domain 类型。
//!
//! 一条 `payload` = `命令组 + 命令类型 + 数据`，不含长度字段和校验和（与
//! [`Envelope::payload`](crate::command::envelope::Envelope) 对称）。多条响应叠加时由帧层按长度字段
//! 切分，模板只解析一条。

use crate::command::ModelVersion;
use deku::{DekuContainerRead, DekuError};
use snafu::{ResultExt, Snafu};

pub mod bluetooth_mac;
pub mod buffer_size;
pub mod buffer_size2;
pub mod darkness;
pub mod device_name;
pub mod dpi;
pub mod gap_length;
pub mod gap_type;
pub mod manufacturer;
pub mod print_width;
pub mod software_version;
pub mod speed;
pub mod statistics;
pub mod status;
pub mod string;

/// 单条响应的 domain 类型。
///
/// 实现者内部调用 [`decode_resp`] 解析 `resp_template`，再按 `mv` 归一化。
pub trait RespTrait {
    /// 解析单条响应的 payload
    fn parse(payload: &[u8], mv: ModelVersion) -> Result<Self, ParseRespError>
    where
        Self: Sized;
}

/// 响应解析错误。
#[derive(Debug, Snafu)]
pub enum ParseRespError {
    #[snafu(display("Could not decode response."))]
    Decode { source: DekuError },
    #[snafu(display(
        "Response has trailing data: {trailing_bytes} byte(s) + {trailing_bits} bit(s)."
    ))]
    TrailingBytes {
        trailing_bytes: usize,
        trailing_bits: usize,
    },
    #[snafu(display("Invalid value in response: '{param}'={value}."))]
    InvalidValue { param: &'static str, value: String },
    #[snafu(display("Bad length for response: expected {expected}, got {actual}."))]
    BadLength {
        expected: &'static str,
        actual: usize,
    },
    #[snafu(display("Invalid string in response."))]
    InvalidString,
}

/// 用一个 `resp_template` 解析 payload，并要求模板把 payload 完整消费。
///
/// `T` 需要保留 `#[deku(magic = ...)]`，magic 不匹配时 deku 会报错，最终落到
/// [`ParseRespError::Decode`]。
pub fn decode_resp<'a, T>(payload: &'a [u8]) -> Result<T, ParseRespError>
where
    T: DekuContainerRead<'a>,
{
    let ((rest, bits), value) = T::from_bytes((payload, 0)).context(DecodeSnafu)?;
    snafu::ensure!(
        rest.is_empty() && bits == 0,
        TrailingBytesSnafu {
            trailing_bytes: rest.len(),
            trailing_bits: bits,
        }
    );
    Ok(value)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::command::resp_template::der;

    /// 固定长度响应：`1f 43 <value>`。
    #[der(..RespTemplate)]
    struct FakeHeader {
        #[deku(magic = b"\x1f\x43")]
        value: u8,
    }

    /// 变长尾部响应：`1f 43 <value> <tail...>`。
    #[der(..RespTemplate)]
    struct FakeTail {
        #[deku(magic = b"\x1f\x43")]
        value: u8,
        #[deku(read_all)]
        _tail: Vec<u8>,
    }

    #[test]
    fn decode_ok() {
        let v: FakeHeader = decode_resp(&[0x1f, 0x43, 0x05]).unwrap();
        assert_eq!(v.value, 5);
    }

    #[test]
    fn decode_with_tail_ok() {
        let v: FakeTail = decode_resp(&[0x1f, 0x43, 0x05, 0xaa, 0xbb]).unwrap();
        assert_eq!(v.value, 5);
        assert_eq!(v._tail, vec![0xaa, 0xbb]);
    }

    #[test]
    fn bad_magic_is_decode_error() {
        let err = decode_resp::<FakeHeader>(&[0x1f, 0x44, 0x05]).unwrap_err();
        assert!(matches!(err, ParseRespError::Decode { .. }), "{err:?}");
    }

    #[test]
    fn trailing_bytes() {
        let err = decode_resp::<FakeHeader>(&[0x1f, 0x43, 0x05, 0xaa]).unwrap_err();
        assert!(
            matches!(
                err,
                ParseRespError::TrailingBytes {
                    trailing_bytes: 1,
                    trailing_bits: 0,
                }
            ),
            "{err:?}"
        );
    }
}
