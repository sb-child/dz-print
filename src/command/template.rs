// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use deku::{DekuRead, DekuWrite};

use crate::command::varint::VarAuto;

/// 开始页命令: v2机型: `1f 20`
#[derive(Debug, PartialEq, DekuWrite, Default)]
pub struct StartPageV2Tl {
    #[deku(magic = b"\x1f\x20")]
    #[deku(endian = "big")]
    pub page_key: u16,
    #[deku(magic = b"\x00\x00\x00\x00\x00")]
    pub print_separate_line: bool,
    #[deku(magic = b"\x00")]
    _tail: (),
}

fn test() {
    let a = StartPageV2Tl {
        page_key: todo!(),
        print_separate_line: todo!(),
        ..Default::default()
    };
}

/// 开始页命令: v0机型: `1b 40`
#[derive(Debug, PartialEq, DekuWrite, Default)]
pub struct StartPageV0Tl {
    #[deku(magic = b"\x1b\x40")]
    _tail: (),
}

/// 打印一行: v2机型: `1f 21`
#[derive(Debug, PartialEq, DekuWrite, Default)]
pub struct PrintLineV2Tl {
    #[deku(magic = b"\x1f\x21")]
    /// repeats = (重复次数 - 1)。0 代表这一行只打印一次。取值 `0~16383`。
    repeats: VarAuto,
    /// skips = 向右偏移的字节数。取值 `0~191`。
    skips: VarAuto,
    /// data = 位图数据。
    data: Vec<u8>,
    _tail: (),
}
