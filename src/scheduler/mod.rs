// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::{FromPrimitive, ToPrimitive};

#[derive(Debug, Clone, Copy)]
pub enum PaperType {
    /// 连续纸
    Ticket,
    /// 定位孔 (间距 0.01mm)
    LocatorHole(u32),
    /// 间隙纸 (间距 0.01mm)
    Adhesive(u32),
    /// 黑标纸 (间距 0.01mm)
    CardPaper(u32),
}

/// 打印速度
#[derive(Debug, Clone, Copy, FromPrimitive, ToPrimitive)]
pub enum PrintSpeed {
    Min,
    Speed1,
    Default,
    Speed3,
    Max,
}

/// 打印颜色深度
#[derive(Debug, Clone, Copy, FromPrimitive, ToPrimitive)]
pub enum PrintDarkness {
    Min,
    Darkness1,
    Darkness2,
    Darkness3,
    Darkness4,
    Default,
    Darkness6,
    Darkness7,
    Darkness8,
    Darkness9,
    Darkness10,
    Darkness11,
    Darkness12,
    Darkness13,
    Max,
}

pub struct Job {}

pub struct Scheduler {}

impl Scheduler {}
