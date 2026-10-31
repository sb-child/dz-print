// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印机状态码（`0x70` 响应 `payload[0]`）。
//!
//! 值 = 线上字节。来源：
//! - <https://github.com/wx-dev/DzPrinterCsharpSDK/blob/master/DzPrinter.Printer/PrinterCommand.cs#L84>
//! - <https://github.com/Brick-web/SMKT/blob/main/Print%20SDK/lib/LPAUtils.d.ts#L327>

use crate::command::settings::PrintSettingError;

/// 打印机状态码。
///
/// `0x00..=0x0b` 视为可继续打印，`0x0c` 及以上是任务取消、环境或硬件错误。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum PrinterStatus {
    /// 当前可以打印（`0x00`）
    #[default]
    Printable = 0,
    /// 当前正在打印（`0x01`）
    Printing = 1,
    /// 当前正在转动马达（`0x02`）
    Rotating = 2,
    /// 当前没有打印任务（`0x0a`）
    NoJob = 10,
    /// 有打印任务，但页面数据还没有接收完全（`0x0b`）
    PageNotReady = 11,
    /// 当前打印任务被取消（`0x0c`）
    JobCanceled = 12,
    /// 当前不具备打印环境（`0x14`）
    EnvNotReady = 20,
    /// 打印电压太低（`0x1e`）
    VolTooLow = 30,
    /// 打印电压太高（`0x1f`）
    VolTooHigh = 31,
    /// 没有检测到打印头（`0x20`）
    TphNotFound = 32,
    /// 打印头温度太高（`0x21`）
    TphTooHot = 33,
    /// 打印机盖子打开（`0x22`）
    CoverOpened = 34,
    /// 未检测到纸张（`0x23`）
    NoPaper = 35,
    /// 碳带盒未锁紧（`0x24`）
    RibbonCanOpened = 36,
    /// 未检测到碳带（`0x25`）
    NoRibbon = 37,
    /// 不匹配的碳带（`0x26`）
    UnmatchedRibbon = 38,
    /// 环境温度过低（`0x27`）
    TphTooCold = 39,
    /// 用完的碳带（`0x28`）
    UsedupRibbon = 40,
    /// 用完的色带（`0x29`）
    UsedupRibbon2 = 41,
    /// 未检测到标签纸卷（`0x2a`）
    NoLabel = 42,
    /// 不匹配的标签纸卷（`0x2b`）
    UnmatchedLabel = 43,
    /// 用完的标签纸卷（`0x2c`）
    UsedupLabel = 44,
    /// 未检测到色带（`0x2d`）
    NoRibbon2 = 45,
    /// 不匹配的色带（`0x2e`）
    UnmatchedRibbon2 = 46,
    /// 标签盒未锁紧（`0x32`）
    LabelCanOpened = 50,
}

impl PrinterStatus {
    /// 线上值
    pub const fn to_wire(self) -> u8 {
        self as u8
    }

    /// 从线上值解析
    pub fn from_wire(value: u8) -> Result<Self, PrintSettingError> {
        Self::try_from(value)
    }

    /// SDK 注释：`DZIP_PRINTABLE..=DZIP_PAGENOTREADY`（`0..=11`）视为可继续打印
    pub const fn is_printable(self) -> bool {
        (self as u8) <= 11
    }

    /// `0x0c` 及以上是任务取消、环境或硬件错误
    pub const fn is_error(self) -> bool {
        (self as u8) >= 12
    }
}

impl TryFrom<u8> for PrinterStatus {
    type Error = PrintSettingError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Printable),
            1 => Ok(Self::Printing),
            2 => Ok(Self::Rotating),
            10 => Ok(Self::NoJob),
            11 => Ok(Self::PageNotReady),
            12 => Ok(Self::JobCanceled),
            20 => Ok(Self::EnvNotReady),
            30 => Ok(Self::VolTooLow),
            31 => Ok(Self::VolTooHigh),
            32 => Ok(Self::TphNotFound),
            33 => Ok(Self::TphTooHot),
            34 => Ok(Self::CoverOpened),
            35 => Ok(Self::NoPaper),
            36 => Ok(Self::RibbonCanOpened),
            37 => Ok(Self::NoRibbon),
            38 => Ok(Self::UnmatchedRibbon),
            39 => Ok(Self::TphTooCold),
            40 => Ok(Self::UsedupRibbon),
            41 => Ok(Self::UsedupRibbon2),
            42 => Ok(Self::NoLabel),
            43 => Ok(Self::UnmatchedLabel),
            44 => Ok(Self::UsedupLabel),
            45 => Ok(Self::NoRibbon2),
            46 => Ok(Self::UnmatchedRibbon2),
            50 => Ok(Self::LabelCanOpened),
            _ => Err(Self::Error::InvalidU8(value)),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const ALL: [(u8, PrinterStatus); 25] = [
        (0, PrinterStatus::Printable),
        (1, PrinterStatus::Printing),
        (2, PrinterStatus::Rotating),
        (10, PrinterStatus::NoJob),
        (11, PrinterStatus::PageNotReady),
        (12, PrinterStatus::JobCanceled),
        (20, PrinterStatus::EnvNotReady),
        (30, PrinterStatus::VolTooLow),
        (31, PrinterStatus::VolTooHigh),
        (32, PrinterStatus::TphNotFound),
        (33, PrinterStatus::TphTooHot),
        (34, PrinterStatus::CoverOpened),
        (35, PrinterStatus::NoPaper),
        (36, PrinterStatus::RibbonCanOpened),
        (37, PrinterStatus::NoRibbon),
        (38, PrinterStatus::UnmatchedRibbon),
        (39, PrinterStatus::TphTooCold),
        (40, PrinterStatus::UsedupRibbon),
        (41, PrinterStatus::UsedupRibbon2),
        (42, PrinterStatus::NoLabel),
        (43, PrinterStatus::UnmatchedLabel),
        (44, PrinterStatus::UsedupLabel),
        (45, PrinterStatus::NoRibbon2),
        (46, PrinterStatus::UnmatchedRibbon2),
        (50, PrinterStatus::LabelCanOpened),
    ];

    #[test]
    fn wire_values_roundtrip() {
        for (wire, status) in ALL {
            assert_eq!(status as u8, wire);
            assert_eq!(status.to_wire(), wire);
            assert_eq!(PrinterStatus::from_wire(wire).unwrap(), status);
        }
    }

    #[test]
    fn unknown_value_is_invalid() {
        for value in [3, 9, 13, 19, 21, 29, 47, 49, 51, 255] {
            assert!(
                matches!(
                    PrinterStatus::from_wire(value),
                    Err(PrintSettingError::InvalidU8(v)) if v == value
                ),
                "value={value}"
            );
        }
    }

    #[test]
    fn printable_and_error_boundary() {
        for (_, status) in ALL {
            assert_eq!(status.is_printable(), !status.is_error(), "{status:?}");
        }

        assert!(PrinterStatus::Printable.is_printable());
        assert!(PrinterStatus::Printing.is_printable());
        assert!(PrinterStatus::Rotating.is_printable());
        assert!(PrinterStatus::NoJob.is_printable());
        assert!(PrinterStatus::PageNotReady.is_printable());
        assert!(!PrinterStatus::JobCanceled.is_printable());

        assert!(PrinterStatus::JobCanceled.is_error());
        assert!(PrinterStatus::LabelCanOpened.is_error());
    }
}
