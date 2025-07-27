// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use super::Bitmap;

pub enum PrintCommand {
    /// 初始化
    ResetPrinter,
    /// 出纸
    FeedLines(u32),
    /// 打印一行, 最大宽度 `.0` 个点
    PrintLine(u32, Vec<bool>),
    /// 跳过 `.1` 个点, 然后打印 `.2` 个点, 最大宽度 `.0` 个点
    SkipPrintLine(u32, u32, Vec<bool>),
    /// 重复上一行
    RepeatLine(u32),
    /// 定位到下一张纸
    NextPaper,
    /// 断点
    Breakpoint,
}

impl PrintCommand {
    pub fn parse(&self) -> Option<Vec<Vec<u8>>> {
        match self {
            PrintCommand::ResetPrinter => Some(vec![vec![0x1b, 0x40]]),
            PrintCommand::FeedLines(ln) => {
                let mut buf = vec![];
                let max_feed = 255;
                let mut r = *ln;
                while r > 0 {
                    let x = if r >= max_feed {
                        r -= max_feed;
                        max_feed
                    } else {
                        let rr = r;
                        r = 0;
                        rr
                    };
                    buf.push(vec![0x1b, 0x4a, x as u8]);
                }
                Some(buf)
            }
            PrintCommand::PrintLine(mw, dots) => {
                if *mw > 65535 {
                    return Self::FeedLines(1).parse();
                }
                let w = (*mw).min(dots.len() as u32);
                let bytes_to_print = (w + 7) / 8;
                let mut b: Vec<u8> = Vec::with_capacity(bytes_to_print as usize);
                b.resize(bytes_to_print as usize, 0);
                for (idx, bit) in dots.into_iter().take(w as usize).enumerate() {
                    let byte = idx / 8;
                    let shift = 7 - idx % 8;
                    b[byte] |= (*bit as u8) << shift;
                }
                let mut c = Vec::with_capacity(b.len() + 4);
                let w16 = w as u16;
                c.extend(&[0x1f, 0x2a]);
                c.extend(w16.to_le_bytes());
                c.extend(b);
                Some(vec![c])
            }
            PrintCommand::SkipPrintLine(mw, skip, dots) => {
                if *mw > 1528 || skip > mw {
                    return Self::FeedLines(1).parse();
                }
                let w = (mw - skip).min(dots.len() as u32);
                let snaped_skip_bytes = skip / 8;
                let snaped_skip_offset = skip % 8;
                let bytes_to_print = 1 + (w + 7) / 8;
                let mut offset_dots = Vec::with_capacity(8 * bytes_to_print as usize);
                for _ in 0..snaped_skip_offset {
                    offset_dots.push(false);
                }
                offset_dots.extend(dots);

                // println!("{:?}", dots);
                // println!("{:?}", offset_dots);
                // println!("w={}", w);

                let mut b: Vec<u8> = Vec::with_capacity(bytes_to_print as usize);
                b.resize(bytes_to_print as usize, 0);
                for (idx, bit) in offset_dots
                    .into_iter()
                    .take((snaped_skip_offset + w) as usize)
                    .enumerate()
                {
                    let byte = idx / 8;
                    let shift = 7 - idx % 8;
                    b[byte] |= (bit as u8) << shift;
                }

                let mut c = Vec::with_capacity(b.len() + 4);
                c.extend(&[0x1f, 0x2b]);
                c.extend([snaped_skip_bytes as u8, bytes_to_print as u8]);
                c.extend(b);
                Some(vec![c])
            }
            PrintCommand::RepeatLine(ln) => {
                let mut buf = vec![];
                let max_feed = 192;
                let mut r = *ln;
                while r > 0 {
                    let x = if r >= max_feed {
                        r -= max_feed;
                        max_feed
                    } else {
                        let rr = r;
                        r = 0;
                        rr
                    };
                    buf.push(vec![0x1f, 0x2e, x as u8 - 1]);
                }
                Some(buf)
            }
            PrintCommand::NextPaper => Some(vec![vec![0x0c]]),
            PrintCommand::Breakpoint => None,
        }
    }
}

pub struct BitmapParser {
    im: Bitmap,
    next_line_cursor: u32,
    breakpoint: u32,
    prev_breakpoint: u32,
    first_breakpoint: bool,
}

impl BitmapParser {
    /// 打印命令转换器
    /// 
    /// - im: 位图
    /// - bp: 每隔多少行插入一个断点命令
    pub fn new(im: Bitmap, bp: u32) -> Self {
        // println!("image= w{} x h{}", im.width(), im.height());
        BitmapParser {
            im,
            next_line_cursor: 0,
            breakpoint: bp,
            prev_breakpoint: 0,
            first_breakpoint: false,
        }
    }
}

impl Iterator for BitmapParser {
    type Item = PrintCommand;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_line_cursor >= self.im.height() {
            // println!("next_line_cursor={} None", self.next_line_cursor);
            return None;
        }
        if self.breakpoint > 0
            && ((self.next_line_cursor % self.breakpoint == 0
                && self.prev_breakpoint != self.next_line_cursor)
                || !self.first_breakpoint)
        {
            self.prev_breakpoint = self.next_line_cursor;
            self.first_breakpoint = true;
            return Some(PrintCommand::Breakpoint);
        }
        // 跳过空行
        let mut empty_line_counter = 0;
        for i in self.next_line_cursor..self.im.height() {
            if self.breakpoint > 0
                && empty_line_counter > 0
                && (self.next_line_cursor + empty_line_counter) % self.breakpoint == 0
            {
                break;
            }
            if self.im.is_line_empty(i) {
                empty_line_counter += 1;
            } else {
                break;
            }
        }
        if empty_line_counter > 0 {
            // println!(
            //     "next_line_cursor={} FeedLines({})",
            //     self.next_line_cursor, empty_line_counter
            // );
            self.next_line_cursor += empty_line_counter;
            return Some(PrintCommand::FeedLines(empty_line_counter));
        }
        // 否则这一行是有东西的
        // 比较上一行和后续行, 得到重复次数
        let mut repeat_line_counter = 0;
        for i in self.next_line_cursor..self.im.height() {
            // 第 0 行前面是万万不能看的
            if self.next_line_cursor == 0 {
                break;
            }
            if self.breakpoint > 0
                && repeat_line_counter > 0
                && (self.next_line_cursor + repeat_line_counter) % self.breakpoint == 0
            {
                break;
            }
            if self.im.same_lines(self.next_line_cursor - 1, i) {
                repeat_line_counter += 1;
            } else {
                break;
            }
        }
        if repeat_line_counter > 0 {
            // println!(
            //     "next_line_cursor={} RepeatLine({})",
            //     self.next_line_cursor, repeat_line_counter
            // );
            self.next_line_cursor += repeat_line_counter;
            return Some(PrintCommand::RepeatLine(repeat_line_counter));
        }
        // 否则这一行和上一行不一样
        // 看看前缀多少空白
        // 这一行肯定有黑色的像素
        let first_black = self
            .im
            .first_black_pixel_in_line(self.next_line_cursor)
            .unwrap()
            - self.im.line_loc_unchecked(self.next_line_cursor).0;
        let last_black = self
            .im
            .last_black_pixel_in_line(self.next_line_cursor)
            .unwrap()
            - self.im.line_loc_unchecked(self.next_line_cursor).0;
        if first_black > 0 {
            let skipped: Vec<bool> = self
                .im
                .get_line(self.next_line_cursor)
                .into_iter()
                .take(last_black + 1)
                .skip(first_black)
                .collect();
            // println!(
            //     "next_line_cursor={} SkipPrintLine({}, {})",
            //     self.next_line_cursor,
            //     first_black,
            //     skipped.len()
            // );
            self.next_line_cursor += 1;
            return Some(PrintCommand::SkipPrintLine(
                self.im.width(),
                first_black as u32,
                skipped,
            ));
        }
        // 否则第 0 个像素就是黑色的
        let line: Vec<bool> = self
            .im
            .get_line(self.next_line_cursor)
            .into_iter()
            .take(last_black + 1)
            .collect();
        // println!(
        //     "next_line_cursor={} PrintLine({})",
        //     self.next_line_cursor,
        //     line.len()
        // );
        self.next_line_cursor += 1;
        return Some(PrintCommand::PrintLine(self.im.width(), line));
    }
}

#[cfg(test)]
mod test {
    use super::PrintCommand;

    #[test]
    fn test_cmd_parse() {
        let x = PrintCommand::FeedLines(2233).parse().unwrap();
        assert_eq!(
            x,
            vec![
                vec![0x1b, 0x4a, 0xff],
                vec![0x1b, 0x4a, 0xff],
                vec![0x1b, 0x4a, 0xff],
                vec![0x1b, 0x4a, 0xff],
                vec![0x1b, 0x4a, 0xff],
                vec![0x1b, 0x4a, 0xff],
                vec![0x1b, 0x4a, 0xff],
                vec![0x1b, 0x4a, 0xff],
                vec![0x1b, 0x4a, 0xc1]
            ],
            "unexcepted result: {:02x?}",
            x
        );

        let x = PrintCommand::NextPaper.parse().unwrap();
        assert_eq!(x, vec![vec![0x0c]], "unexcepted result: {:02x?}", x);

        let x = PrintCommand::PrintLine(10, vec![false, false, true, true, true])
            .parse()
            .unwrap();
        assert_eq!(
            x,
            vec![vec![0x1f, 0x2a, 0x05, 0x00, 0x38]],
            "unexcepted result: {:02x?}",
            x
        );

        let x = PrintCommand::RepeatLine(8964).parse().unwrap();
        assert_eq!(
            x,
            vec![
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0xbf],
                vec![0x1f, 0x2e, 0x83],
            ],
            "unexcepted result: {:02x?}",
            x
        );

        let x = PrintCommand::ResetPrinter.parse().unwrap();
        assert_eq!(x, vec![vec![0x1b, 0x40]], "unexcepted result: {:02x?}", x);

        let x = PrintCommand::SkipPrintLine(10, 5, vec![false, true])
            .parse()
            .unwrap();
        assert_eq!(
            x,
            vec![vec![0x1f, 0x2b, 0x00, 0x02, 0x02, 0x00]],
            "unexcepted result: {:02x?}",
            x
        );

        let x = PrintCommand::RepeatLine(0).parse().unwrap();
        assert_eq!(x, Vec::<Vec<u8>>::new(), "unexcepted result: {:02x?}", x);

        // println!("{:02x?}", x);
    }
}
