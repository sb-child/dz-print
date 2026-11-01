// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

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
        Err(PrintSettingError::InvalidU8 { value: 0 })
    ));
    assert!(matches!(
        SpeedSetting::from_user(6),
        Err(PrintSettingError::InvalidU8 { value: 6 })
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
        Err(PrintSettingError::InvalidU8 { value: 0 })
    ));
    assert!(matches!(
        DarknessSetting::from_user(16),
        Err(PrintSettingError::InvalidU8 { value: 16 })
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
fn gap_type_wire() {
    for (wire, setting) in [
        (0, GapTypeSetting::Continuous),
        (1, GapTypeSetting::Hole),
        (2, GapTypeSetting::Gap),
        (3, GapTypeSetting::BlackMark),
        (4, GapTypeSetting::Transparent),
    ] {
        assert_eq!(setting as u8, wire);
        assert_eq!(setting.to_wire(), wire);
        assert_eq!(GapTypeSetting::from_wire(wire).unwrap(), setting);
        assert_eq!(GapTypeSetting::try_from(wire).unwrap(), setting);
    }

    assert!(GapTypeSetting::from_wire(5).is_err());
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
