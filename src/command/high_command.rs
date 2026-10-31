// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! `0x80` 激活命令的参数。
//!
//! 发送 `> 0x80` 的命令前要先发 `1f 80 <mode>` 激活，结束后可发 `1f 80 00`。

use thiserror::Error;

/// `0x80` 的线上参数值。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HighCommandMode {
    /// `0x00` 请求：结束激活。
    Disable = 0x00,
    /// `0x7f` 请求：激活 `> 0x80` 命令。
    #[default]
    Enable = 0x7f,
    /// `0x49` 请求：另一种激活参数，固件 >= `20230605` 使用。
    EnableAlt = 0x49,
    /// `0x80` 响应：设备在结束激活后回的值，主机不应主动发送。
    Disabled = 0x80,
}

impl HighCommandMode {
    /// 线上值。
    pub const fn to_wire(self) -> u8 {
        self as u8
    }

    /// 从线上值解析。
    pub fn from_wire(value: u8) -> Result<Self, HighCommandError> {
        Self::try_from(value)
    }
}

impl TryFrom<u8> for HighCommandMode {
    type Error = HighCommandError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Disable),
            0x49 => Ok(Self::EnableAlt),
            0x7f => Ok(Self::Enable),
            0x80 => Ok(Self::Disabled),
            _ => Err(HighCommandError::InvalidMode(value)),
        }
    }
}

/// `0x80` 参数错误。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HighCommandError {
    #[error("Invalid high command mode: {0:#04x}")]
    InvalidMode(u8),
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn wire_round_trip() {
        for mode in [
            HighCommandMode::Disable,
            HighCommandMode::Enable,
            HighCommandMode::EnableAlt,
            HighCommandMode::Disabled,
        ] {
            assert_eq!(HighCommandMode::from_wire(mode.to_wire()).unwrap(), mode);
        }
    }

    #[test]
    fn default_is_enable() {
        assert_eq!(HighCommandMode::default(), HighCommandMode::Enable);
    }

    #[test]
    fn unknown_value_is_error() {
        let err = HighCommandMode::from_wire(0x01).unwrap_err();
        assert_eq!(err, HighCommandError::InvalidMode(0x01));
    }
}
