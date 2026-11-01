// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印机状态码（`0x70` 响应 `payload[0]`）。
//!
//! 值 = 线上字节。来源：
//! - <https://github.com/wx-dev/DzPrinterCsharpSDK/blob/master/DzPrinter.Printer/PrinterCommand.cs#L84>
//! - <https://github.com/Brick-web/SMKT/blob/main/Print%20SDK/lib/LPAUtils.d.ts#L327>
//!
//! # 用法
//!
//! ```
//! use dz_print::command::status::PrinterStatus;
//!
//! let status = PrinterStatus::from_wire(0x23);
//! assert_eq!(status, PrinterStatus::NoPaper);
//! assert!(status.is_error());
//! assert_eq!(status.to_wire(), 0x23);
//!
//! // 未定义的值保留下来；0x0c 及以上仍然按错误处理
//! let unknown = PrinterStatus::from_wire(0x09);
//! assert_eq!(unknown, PrinterStatus::Unknown(0x09));
//! assert!(!unknown.is_error());
//! ```

/// 打印机状态码。
///
/// `0x00..=0x0b` 视为可继续打印，`0x0c` 及以上是任务取消、环境或硬件错误。
/// 未列出的值保留在 [`PrinterStatus::Unknown`] 里，避免新固件的错误码把整个
/// 响应解析打回去（那样会卡住响应队列）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrinterStatus {
    /// 当前可以打印（`0x00`）
    #[default]
    Printable,
    /// 当前正在打印（`0x01`）
    Printing,
    /// 当前正在转动马达（`0x02`）
    Rotating,
    /// 当前没有打印任务（`0x0a`）
    NoJob,
    /// 有打印任务，但页面数据还没有接收完全（`0x0b`）
    PageNotReady,
    /// 当前打印任务被取消（`0x0c`）
    JobCanceled,
    /// 当前不具备打印环境（`0x14`）
    EnvNotReady,
    /// 打印电压太低（`0x1e`）
    VolTooLow,
    /// 打印电压太高（`0x1f`）
    VolTooHigh,
    /// 没有检测到打印头（`0x20`）
    TphNotFound,
    /// 打印头温度太高（`0x21`）
    TphTooHot,
    /// 打印机盖子打开（`0x22`）
    CoverOpened,
    /// 未检测到纸张（`0x23`）
    NoPaper,
    /// 碳带盒未锁紧（`0x24`）
    RibbonCanOpened,
    /// 未检测到碳带（`0x25`）
    NoRibbon,
    /// 不匹配的碳带（`0x26`）
    UnmatchedRibbon,
    /// 环境温度过低（`0x27`）
    TphTooCold,
    /// 用完的碳带（`0x28`）
    UsedupRibbon,
    /// 用完的色带（`0x29`）
    UsedupRibbon2,
    /// 未检测到标签纸卷（`0x2a`）
    NoLabel,
    /// 不匹配的标签纸卷（`0x2b`）
    UnmatchedLabel,
    /// 用完的标签纸卷（`0x2c`）
    UsedupLabel,
    /// 未检测到色带（`0x2d`）
    NoRibbon2,
    /// 不匹配的色带（`0x2e`）
    UnmatchedRibbon2,
    /// 标签盒未锁紧（`0x32`）
    LabelCanOpened,
    /// 未定义的状态码，保留线上值。
    ///
    /// 判定规则仍然是「`0x0c` 及以上算错误」，与 SDK 一致。
    Unknown(u8),
}

impl PrinterStatus {
    /// 线上值。
    pub const fn to_wire(self) -> u8 {
        match self {
            Self::Printable => 0,
            Self::Printing => 1,
            Self::Rotating => 2,
            Self::NoJob => 10,
            Self::PageNotReady => 11,
            Self::JobCanceled => 12,
            Self::EnvNotReady => 20,
            Self::VolTooLow => 30,
            Self::VolTooHigh => 31,
            Self::TphNotFound => 32,
            Self::TphTooHot => 33,
            Self::CoverOpened => 34,
            Self::NoPaper => 35,
            Self::RibbonCanOpened => 36,
            Self::NoRibbon => 37,
            Self::UnmatchedRibbon => 38,
            Self::TphTooCold => 39,
            Self::UsedupRibbon => 40,
            Self::UsedupRibbon2 => 41,
            Self::NoLabel => 42,
            Self::UnmatchedLabel => 43,
            Self::UsedupLabel => 44,
            Self::NoRibbon2 => 45,
            Self::UnmatchedRibbon2 => 46,
            Self::LabelCanOpened => 50,
            Self::Unknown(value) => value,
        }
    }

    /// 从线上值解析。
    ///
    /// 未知值不会报错，而是变成 [`PrinterStatus::Unknown`]。
    pub const fn from_wire(value: u8) -> Self {
        match value {
            0 => Self::Printable,
            1 => Self::Printing,
            2 => Self::Rotating,
            10 => Self::NoJob,
            11 => Self::PageNotReady,
            12 => Self::JobCanceled,
            20 => Self::EnvNotReady,
            30 => Self::VolTooLow,
            31 => Self::VolTooHigh,
            32 => Self::TphNotFound,
            33 => Self::TphTooHot,
            34 => Self::CoverOpened,
            35 => Self::NoPaper,
            36 => Self::RibbonCanOpened,
            37 => Self::NoRibbon,
            38 => Self::UnmatchedRibbon,
            39 => Self::TphTooCold,
            40 => Self::UsedupRibbon,
            41 => Self::UsedupRibbon2,
            42 => Self::NoLabel,
            43 => Self::UnmatchedLabel,
            44 => Self::UsedupLabel,
            45 => Self::NoRibbon2,
            46 => Self::UnmatchedRibbon2,
            50 => Self::LabelCanOpened,
            other => Self::Unknown(other),
        }
    }

    /// SDK 注释：`DZIP_PRINTABLE..=DZIP_PAGENOTREADY`（`0..=11`）视为可继续打印
    pub const fn is_printable(self) -> bool {
        self.to_wire() <= 11
    }

    /// `0x0c` 及以上是任务取消、环境或硬件错误
    pub const fn is_error(self) -> bool {
        self.to_wire() >= 12
    }

    /// 状态的简短说明，用于日志和错误信息。
    ///
    /// 未定义的状态码返回 `"unknown status"`，具体线上值用 [`Self::to_wire`] 看。
    pub const fn description(self) -> &'static str {
        match self {
            Self::Printable => "ready",
            Self::Printing => "printing",
            Self::Rotating => "motor rotating",
            Self::NoJob => "no print job",
            Self::PageNotReady => "page data not complete",
            Self::JobCanceled => "print job canceled",
            Self::EnvNotReady => "environment not ready",
            Self::VolTooLow => "print voltage too low",
            Self::VolTooHigh => "print voltage too high",
            Self::TphNotFound => "print head not detected",
            Self::TphTooHot => "print head too hot",
            Self::CoverOpened => "cover is open",
            Self::NoPaper => "no paper detected",
            Self::RibbonCanOpened => "ribbon cartridge is not locked",
            Self::NoRibbon => "no ribbon detected",
            Self::UnmatchedRibbon => "ribbon mismatch",
            Self::TphTooCold => "print head too cold",
            Self::UsedupRibbon => "ribbon used up",
            Self::UsedupRibbon2 => "ribbon used up",
            Self::NoLabel => "no label roll detected",
            Self::UnmatchedLabel => "label roll mismatch",
            Self::UsedupLabel => "label roll used up",
            Self::NoRibbon2 => "no thermal ribbon detected",
            Self::UnmatchedRibbon2 => "thermal ribbon mismatch",
            Self::LabelCanOpened => "label cartridge is not locked",
            Self::Unknown(_) => "unknown status",
        }
    }
}

/// 日志和错误信息用：`printing`、`cover is open`、`unknown status 0x09`。
impl std::fmt::Display for PrinterStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(value) => write!(f, "unknown status {value:#04X}"),
            other => f.write_str(other.description()),
        }
    }
}

impl From<u8> for PrinterStatus {
    fn from(value: u8) -> Self {
        Self::from_wire(value)
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
            assert_eq!(status.to_wire(), wire);
            assert_eq!(PrinterStatus::from_wire(wire), status);
            assert_eq!(PrinterStatus::from(wire), status);
        }
    }

    #[test]
    fn unknown_value_is_kept() {
        for value in [3, 9, 13, 19, 21, 29, 47, 49, 51, 255] {
            let status = PrinterStatus::from_wire(value);
            assert_eq!(status, PrinterStatus::Unknown(value), "value={value}");
            assert_eq!(status.to_wire(), value);
            assert_eq!(status.is_error(), value >= 12, "value={value}");
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

    /// 每个状态都要有可读说明，日志和错误信息直接用 `Display`。
    #[test]
    fn every_status_has_a_description() {
        for (_, status) in ALL {
            assert!(!status.description().is_empty(), "{status:?}");
            assert_eq!(status.to_string(), status.description(), "{status:?}");
        }

        assert_eq!(PrinterStatus::CoverOpened.to_string(), "cover is open");
        assert_eq!(PrinterStatus::NoPaper.to_string(), "no paper detected");
        assert_eq!(
            PrinterStatus::Unknown(0x09).to_string(),
            "unknown status 0x09"
        );
    }
}
