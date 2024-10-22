// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use super::Bitmap;

pub enum PrintCommands {
    /// 初始化
    ResetPrinter,
    /// 出纸
    FeedLines(u32),
    /// 打印一行
    PrintLine(Vec<bool>),
    /// 跳过 `.0` 个点, 然后打印 `.1` 个点
    SkipPrintLine(u32, Vec<bool>),
    /// 重复上一行
    RepeatLine(u32),
    /// 定位到下一纸张
    NextPaper,
}

pub struct PrintCommandParser {
    im: Bitmap,
    line_cursor: u32,
}

impl PrintCommandParser {
    pub fn new(im: Bitmap) -> Self {
        PrintCommandParser { im, line_cursor: 0 }
    }
}

impl Iterator for PrintCommandParser {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}
