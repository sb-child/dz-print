use std::str::FromStr;

use crate::command::models::ModelVersion;

#[derive(thiserror::Error, Debug)]
pub enum PrintSettingError {
    #[error("Invalid value `{0}`")]
    InvalidU8(u8),
    #[error("Invalid value `{0}`")]
    InvalidU32(u32),
    #[error("Invalid string `{0}`")]
    InvalidString(String),
}

/// 打印速度。
///
/// discriminant = `v2`/`v1` 线上值；用户面值和 `v0` 线上值见各变体注释。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SpeedSetting {
    /// 最慢
    /// - 用户面=1
    /// - 线上 `v0=1` `v2,v1=0`
    Min = 0,
    /// 稍慢
    /// - 用户面=2
    /// - 线上 `v0=2` `v2,v1=1`
    Speed1,
    /// 正常
    /// - 用户面=3
    /// - 线上 `v0=3` `v2,v1=2`
    #[default]
    Normal,
    /// 稍快
    /// - 用户面=4
    /// - 线上 `v0=4` `v2,v1=3`
    Speed3,
    /// 最快
    /// - 用户面=5
    /// - 线上 `v0=5` `v2,v1=4`
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
            .ok_or(PrintSettingError::InvalidU8(value))?;
        Self::try_from(wire).map_err(|_| PrintSettingError::InvalidU8(value))
    }

    /// 线上值
    /// - `v2,v1` 为 `0..=4`
    /// - `v0` 为 `1..=5`
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
                .ok_or(PrintSettingError::InvalidU8(value))?,
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
            _ => Err(Self::Error::InvalidU8(value)),
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
                .map_err(|_| Self::Err::InvalidString(value.to_string()))
                .and_then(Self::from_user),
        }
    }
}

/// 打印浓度。
///
/// discriminant = `v2`/`v1` 线上值；用户面值和 `v0` 线上值见各变体注释。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DarknessSetting {
    /// 最浅
    /// - 用户面=1
    /// - 线上 `v0=1` `v2,v1=0`
    Min = 0,
    /// 第 2 档
    /// - 用户面=2
    /// - 线上 `v0=2` `v2,v1=1`
    Darkness1,
    /// 第 3 档
    /// - 用户面=3
    /// - 线上 `v0=3` `v2,v1=2`
    Darkness2,
    /// 第 4 档
    /// - 用户面=4
    /// - 线上 `v0=4` `v2,v1=3`
    Darkness3,
    /// 第 5 档
    /// - 用户面=5
    /// - 线上 `v0=5` `v2,v1=4`
    Darkness4,
    /// 正常
    /// - 用户面=6
    /// - 线上 `v0=6` `v2,v1=5`
    #[default]
    Normal,
    /// 第 7 档
    /// - 用户面=7
    /// - 线上 `v0=7` `v2,v1=6`
    Darkness6,
    /// 第 8 档
    /// - 用户面=8
    /// - 线上 `v0=8` `v2,v1=7`
    Darkness7,
    /// 第 9 档
    /// - 用户面=9
    /// - 线上 `v0=9` `v2,v1=8`
    Darkness8,
    /// 第 10 档
    /// - 用户面=10
    /// - 线上 `v0=10` `v2,v1=9`
    Darkness9,
    /// 第 11 档
    /// - 用户面=11
    /// - 线上 `v0=11` `v2,v1=10`
    Darkness10,
    /// 第 12 档
    /// - 用户面=12
    /// - 线上 `v0=12` `v2,v1=11`
    Darkness11,
    /// 第 13 档
    /// - 用户面=13
    /// - 线上 `v0=13` `v2,v1=12`
    Darkness12,
    /// 第 14 档
    /// - 用户面=14
    /// - 线上 `v0=14` `v2,v1=13`
    Darkness13,
    /// 最深
    /// - 用户面=15
    /// - 线上 `v0=15` `v2,v1=14`
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
            .ok_or(PrintSettingError::InvalidU8(value))?;
        Self::try_from(wire).map_err(|_| PrintSettingError::InvalidU8(value))
    }

    /// 线上值
    /// - `v2,v1` 为 `0..=14`
    /// - `v0` 为 `1..=15`
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
                .ok_or(PrintSettingError::InvalidU8(value))?,
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
            _ => Err(Self::Error::InvalidU8(value)),
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
                .map_err(|_| Self::Err::InvalidString(value.to_string()))
                .and_then(Self::from_user),
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn speed_user_and_wire() {
        assert_eq!(SpeedSetting::Min as u8, 0);
        assert_eq!(SpeedSetting::Max as u8, 4);

        for (user, setting) in [
            (1, SpeedSetting::Min),
            (2, SpeedSetting::Speed1),
            (3, SpeedSetting::Normal),
            (4, SpeedSetting::Speed3),
            (5, SpeedSetting::Max),
        ] {
            assert_eq!(setting.user_value(), user);
            assert_eq!(SpeedSetting::from_user(user).unwrap(), setting);
            assert_eq!(SpeedSetting::try_from(user - 1).unwrap(), setting);
        }

        assert!(matches!(
            SpeedSetting::from_user(0),
            Err(PrintSettingError::InvalidU8(0))
        ));
        assert!(matches!(
            SpeedSetting::from_user(6),
            Err(PrintSettingError::InvalidU8(6))
        ));
        assert!(SpeedSetting::try_from(5).is_err());
    }

    #[test]
    fn speed_wire_by_model() {
        let speed = SpeedSetting::Normal; // 用户面 3

        assert_eq!(speed.to_wire(ModelVersion::V2), 2);
        assert_eq!(speed.to_wire(ModelVersion::V1), 2);
        assert_eq!(speed.to_wire(ModelVersion::V0), 3);

        for mv in [ModelVersion::V0, ModelVersion::V1, ModelVersion::V2] {
            assert_eq!(
                SpeedSetting::from_wire(speed.to_wire(mv), mv).unwrap(),
                speed
            );
        }

        assert!(SpeedSetting::from_wire(0, ModelVersion::V0).is_err());
        assert!(SpeedSetting::from_wire(5, ModelVersion::V2).is_err());
    }

    #[test]
    fn darkness_user_and_wire() {
        assert_eq!(DarknessSetting::Min as u8, 0);
        assert_eq!(DarknessSetting::Normal as u8, 5);
        assert_eq!(DarknessSetting::Max as u8, 14);

        for user in 1..=15u8 {
            let setting = DarknessSetting::from_user(user).unwrap();
            assert_eq!(setting.user_value(), user);
            assert_eq!(setting as u8, user - 1);
        }

        assert!(matches!(
            DarknessSetting::from_user(0),
            Err(PrintSettingError::InvalidU8(0))
        ));
        assert!(matches!(
            DarknessSetting::from_user(16),
            Err(PrintSettingError::InvalidU8(16))
        ));
    }

    #[test]
    fn darkness_wire_by_model() {
        let darkness = DarknessSetting::Normal; // 用户面 6

        assert_eq!(darkness.to_wire(ModelVersion::V2), 5);
        assert_eq!(darkness.to_wire(ModelVersion::V1), 5);
        assert_eq!(darkness.to_wire(ModelVersion::V0), 6);

        for mv in [ModelVersion::V0, ModelVersion::V1, ModelVersion::V2] {
            assert_eq!(
                DarknessSetting::from_wire(darkness.to_wire(mv), mv).unwrap(),
                darkness
            );
        }

        assert!(DarknessSetting::from_wire(0, ModelVersion::V0).is_err());
        assert!(DarknessSetting::from_wire(15, ModelVersion::V2).is_err());
    }

    #[test]
    fn parse_from_str() {
        assert_eq!("min".parse::<SpeedSetting>().unwrap(), SpeedSetting::Min);
        assert_eq!("3".parse::<SpeedSetting>().unwrap(), SpeedSetting::Normal);
        assert_eq!("max".parse::<SpeedSetting>().unwrap(), SpeedSetting::Max);
        assert!("0".parse::<SpeedSetting>().is_err());

        assert_eq!(
            "6".parse::<DarknessSetting>().unwrap(),
            DarknessSetting::Normal
        );
        assert_eq!(
            "15".parse::<DarknessSetting>().unwrap(),
            DarknessSetting::Max
        );
        assert!("0".parse::<DarknessSetting>().is_err());
        assert!("16".parse::<DarknessSetting>().is_err());
        assert!("fast".parse::<SpeedSetting>().is_err());
    }
}
