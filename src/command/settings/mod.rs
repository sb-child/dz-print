// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印设置值：速度、浓度、纸张间隔类型。
//!
//! 三种值要区分：
//!
//! - **用户面值**：`1..=5`（速度）/ `1..=15`（浓度），界面和命令行用；
//! - **线上值**：写进命令的字节，`v0` 比 `v2`/`v1` 大 1；
//! - **枚举**：Rust 里的类型，`as u8` 是 `v2`/`v1` 的线上值。
//!
//! # 用法
//!
//! ```
//! use dz_print::command::{ModelVersion, settings::SpeedSetting};
//!
//! // 界面上选“正常”（用户面值 3）
//! let speed = SpeedSetting::from_user(3)?;
//! assert_eq!(speed, SpeedSetting::Normal);
//!
//! // 写入 v2 设备：线上值 2；v0 设备：线上值 3
//! assert_eq!(speed.to_wire(ModelVersion::V2), 2);
//! assert_eq!(speed.to_wire(ModelVersion::V0), 3);
//!
//! // 把设备回的线上值转回来
//! assert_eq!(SpeedSetting::from_wire(2, ModelVersion::V2)?, SpeedSetting::Normal);
//! assert_eq!(speed.user_value(), 3);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! [`GapTypeSetting`] 全机型线上值一致，只有 `to_wire` / `from_wire`。

use crate::command::ModelVersion;
use snafu::Snafu;
use std::str::FromStr;

#[derive(Snafu, Debug)]
pub enum PrintSettingError {
    #[snafu(display("Invalid value `{value}`"))]
    InvalidU8 { value: u8 },
    #[snafu(display("Invalid value `{value}`"))]
    InvalidU32 { value: u32 },
    #[snafu(display("Invalid string `{value}`"))]
    InvalidString { value: String },
}

/// 打印速度。
///
/// discriminant 是 `v2`/`v1` 线上值；`v0` 和用户面值见 `to_wire` / `user_value`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SpeedSetting {
    /// 最慢（用户面 1）
    Min = 0,
    /// 稍慢（用户面 2）
    Speed1,
    /// 正常（用户面 3）
    #[default]
    Normal,
    /// 稍快（用户面 4）
    Speed3,
    /// 最快（用户面 5）
    Max,
}

impl SpeedSetting {
    /// 用户面值 `1..=5`
    pub const fn user_value(self) -> u8 {
        self as u8 + 1
    }

    /// 从用户面值 `1..=5` 解析
    pub fn from_user(value: u8) -> Result<Self, PrintSettingError> {
        let wire = value
            .checked_sub(1)
            .ok_or(PrintSettingError::InvalidU8 { value })?;
        Self::try_from(wire).map_err(|_| PrintSettingError::InvalidU8 { value })
    }

    /// v2/v1 原样写，v0 加 1。
    pub const fn to_wire(self, mv: ModelVersion) -> u8 {
        match mv {
            ModelVersion::V0 => self as u8 + 1,
            ModelVersion::V1 | ModelVersion::V2 => self as u8,
        }
    }

    /// 从线上值解析
    pub fn from_wire(value: u8, mv: ModelVersion) -> Result<Self, PrintSettingError> {
        let wire = match mv {
            ModelVersion::V1 | ModelVersion::V2 => value,
            ModelVersion::V0 => value
                .checked_sub(1)
                .ok_or(PrintSettingError::InvalidU8 { value })?,
        };
        Self::try_from(wire)
    }
}

/// 注意：这里的 `u8` 是 `v2`/`v1` 的线上值 `0..=4`，不是用户面的 `1..=5`。
impl TryFrom<u8> for SpeedSetting {
    type Error = PrintSettingError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Min),
            1 => Ok(Self::Speed1),
            2 => Ok(Self::Normal),
            3 => Ok(Self::Speed3),
            4 => Ok(Self::Max),
            _ => Err(Self::Error::InvalidU8 { value }),
        }
    }
}

impl FromStr for SpeedSetting {
    type Err = PrintSettingError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "min" => Ok(Self::Min),
            "normal" => Ok(Self::Normal),
            "max" => Ok(Self::Max),
            _ => value
                .parse::<u8>()
                .map_err(|_| Self::Err::InvalidString {
                    value: value.to_string(),
                })
                .and_then(Self::from_user),
        }
    }
}

/// 打印浓度。
///
/// discriminant 是 `v2`/`v1` 线上值；`v0` 和用户面值见 `to_wire` / `user_value`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DarknessSetting {
    /// 最浅（用户面 1）
    Min = 0,
    /// 第 2 档（用户面 2）
    Darkness1,
    /// 第 3 档（用户面 3）
    Darkness2,
    /// 第 4 档（用户面 4）
    Darkness3,
    /// 第 5 档（用户面 5）
    Darkness4,
    /// 正常（用户面 6）
    #[default]
    Normal,
    /// 第 7 档（用户面 7）
    Darkness6,
    /// 第 8 档（用户面 8）
    Darkness7,
    /// 第 9 档（用户面 9）
    Darkness8,
    /// 第 10 档（用户面 10）
    Darkness9,
    /// 第 11 档（用户面 11）
    Darkness10,
    /// 第 12 档（用户面 12）
    Darkness11,
    /// 第 13 档（用户面 13）
    Darkness12,
    /// 第 14 档（用户面 14）
    Darkness13,
    /// 最深（用户面 15）
    Max,
}

impl DarknessSetting {
    /// 用户面值 `1..=15`
    pub const fn user_value(self) -> u8 {
        self as u8 + 1
    }

    /// 从用户面值 `1..=15` 解析
    pub fn from_user(value: u8) -> Result<Self, PrintSettingError> {
        let wire = value
            .checked_sub(1)
            .ok_or(PrintSettingError::InvalidU8 { value })?;
        Self::try_from(wire).map_err(|_| PrintSettingError::InvalidU8 { value })
    }

    /// v2/v1 原样写，v0 加 1。
    pub const fn to_wire(self, mv: ModelVersion) -> u8 {
        match mv {
            ModelVersion::V0 => self as u8 + 1,
            ModelVersion::V1 | ModelVersion::V2 => self as u8,
        }
    }

    /// 从线上值解析
    pub fn from_wire(value: u8, mv: ModelVersion) -> Result<Self, PrintSettingError> {
        let wire = match mv {
            ModelVersion::V1 | ModelVersion::V2 => value,
            ModelVersion::V0 => value
                .checked_sub(1)
                .ok_or(PrintSettingError::InvalidU8 { value })?,
        };
        Self::try_from(wire)
    }
}

/// 注意：这里的 `u8` 是 `v2`/`v1` 的线上值 `0..=14`，不是用户面的 `1..=15`。
impl TryFrom<u8> for DarknessSetting {
    type Error = PrintSettingError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Min),
            1 => Ok(Self::Darkness1),
            2 => Ok(Self::Darkness2),
            3 => Ok(Self::Darkness3),
            4 => Ok(Self::Darkness4),
            5 => Ok(Self::Normal),
            6 => Ok(Self::Darkness6),
            7 => Ok(Self::Darkness7),
            8 => Ok(Self::Darkness8),
            9 => Ok(Self::Darkness9),
            10 => Ok(Self::Darkness10),
            11 => Ok(Self::Darkness11),
            12 => Ok(Self::Darkness12),
            13 => Ok(Self::Darkness13),
            14 => Ok(Self::Max),
            _ => Err(Self::Error::InvalidU8 { value }),
        }
    }
}

impl FromStr for DarknessSetting {
    type Err = PrintSettingError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "min" => Ok(Self::Min),
            "normal" => Ok(Self::Normal),
            "max" => Ok(Self::Max),
            _ => value
                .parse::<u8>()
                .map_err(|_| Self::Err::InvalidString {
                    value: value.to_string(),
                })
                .and_then(Self::from_user),
        }
    }
}

/// 纸张间隔类型
///
/// discriminant = 线上值；`0x42` 命令在各机型上编码一致。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GapTypeSetting {
    /// 小票纸/连续纸
    #[default]
    Continuous = 0,
    /// 定位孔纸
    Hole = 1,
    /// 间隙纸/不干胶纸
    Gap = 2,
    /// 黑标纸/卡纸
    BlackMark = 3,
    /// 透明贴
    Transparent = 4,
}

impl GapTypeSetting {
    /// 线上值（全机型一致）
    pub const fn to_wire(self) -> u8 {
        self as u8
    }

    /// 从线上值解析（全机型一致）
    pub fn from_wire(value: u8) -> Result<Self, PrintSettingError> {
        Self::try_from(value)
    }
}

impl TryFrom<u8> for GapTypeSetting {
    type Error = PrintSettingError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Continuous),
            1 => Ok(Self::Hole),
            2 => Ok(Self::Gap),
            3 => Ok(Self::BlackMark),
            4 => Ok(Self::Transparent),
            _ => Err(Self::Error::InvalidU8 { value }),
        }
    }
}

// (先放着)
// impl TryFrom<&str> for PaperSetting {
//     type Error = PrintSettingError;
//     fn try_from(value: &str) -> Result<Self, Self::Error> {
//         match value {
//             "ticket" => Ok(Self::Continuous),
//             "adhesive" => Ok(Self::Hole),
//             "cardpaper" => Ok(Self::BlackMark),
//             "transparent" => Ok(Self::Transparent),
//             _ => Err(Self::Error::InvalidString(value.to_string())),
//         }
//     }
// }

#[cfg(test)]
mod tests;
