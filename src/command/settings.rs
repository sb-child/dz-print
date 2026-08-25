#[derive(thiserror::Error, Debug)]
pub enum PrintSettingError {
    #[error("Invalid value `{0}`")]
    InvalidU8(u8),
    #[error("Invalid value `{0}`")]
    InvalidU32(u32),
    #[error("Invalid string `{0}`")]
    InvalidString(String),
}

#[derive(Debug, Clone, Copy, Default)]
pub enum SpeedSetting {
    /// 最慢(1)
    Min,
    /// 稍慢(2)
    Speed1,
    /// 正常(3)
    #[default]
    Normal,
    /// 稍快(4)
    Speed3,
    /// 最快(5)
    Max,
}

impl TryFrom<u8> for SpeedSetting {
    type Error = PrintSettingError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Min),
            2 => Ok(Self::Speed1),
            3 => Ok(Self::Normal),
            4 => Ok(Self::Speed3),
            5 => Ok(Self::Max),
            _ => Err(Self::Error::InvalidU8(value)),
        }
    }
}

impl TryFrom<&str> for SpeedSetting {
    type Error = PrintSettingError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "min" | "1" => Ok(Self::Min),
            "2" => Ok(Self::Speed1),
            "normal" | "3" => Ok(Self::Normal),
            "4" => Ok(Self::Speed3),
            "max" | "5" => Ok(Self::Max),
            _ => Err(Self::Error::InvalidString(value.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum DarknessSetting {
    /// 最浅(1)
    Min,
    Darkness1,
    Darkness2,
    Darkness3,
    Darkness4,
    /// 正常(6)
    #[default]
    Normal,
    Darkness6,
    Darkness7,
    Darkness8,
    Darkness9,
    Darkness10,
    Darkness11,
    Darkness12,
    Darkness13,
    /// 最深(15)
    Max,
}

impl TryFrom<u8> for DarknessSetting {
    type Error = PrintSettingError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Min),
            2 => Ok(Self::Darkness1),
            3 => Ok(Self::Darkness2),
            4 => Ok(Self::Darkness3),
            5 => Ok(Self::Darkness4),
            6 => Ok(Self::Normal),
            7 => Ok(Self::Darkness6),
            8 => Ok(Self::Darkness7),
            9 => Ok(Self::Darkness8),
            10 => Ok(Self::Darkness9),
            11 => Ok(Self::Darkness10),
            12 => Ok(Self::Darkness11),
            13 => Ok(Self::Darkness12),
            14 => Ok(Self::Darkness13),
            15 => Ok(Self::Max),
            _ => Err(Self::Error::InvalidU8(value)),
        }
    }
}

impl TryFrom<&str> for DarknessSetting {
    type Error = PrintSettingError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "min" | "1" => Ok(Self::Min),
            "2" => Ok(Self::Darkness1),
            "3" => Ok(Self::Darkness2),
            "4" => Ok(Self::Darkness3),
            "5" => Ok(Self::Darkness4),
            "normal" | "6" => Ok(Self::Normal),
            "7" => Ok(Self::Darkness6),
            "8" => Ok(Self::Darkness7),
            "9" => Ok(Self::Darkness8),
            "10" => Ok(Self::Darkness9),
            "11" => Ok(Self::Darkness10),
            "12" => Ok(Self::Darkness11),
            "13" => Ok(Self::Darkness12),
            "14" => Ok(Self::Darkness13),
            "max" | "15" => Ok(Self::Max),
            _ => Err(Self::Error::InvalidString(value.to_string())),
        }
    }
}

/// 纸张间隔类型
#[derive(Debug, Clone, Copy, Default)]
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
