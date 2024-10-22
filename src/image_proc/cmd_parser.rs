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
}

impl PrintCommand {
    pub fn parse(&self) -> Vec<Vec<u8>> {
        match self {
            PrintCommand::ResetPrinter => {
                vec![vec![0x1b, 0x40]]
            }
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
                buf
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
                vec![c]
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
                vec![c]
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
                buf
            }
            PrintCommand::NextPaper => {
                vec![vec![0x0c]]
            }
        }
    }
}

pub struct BitmapParser {
    im: Bitmap,
    line_cursor: u32,
}

impl BitmapParser {
    pub fn new(im: Bitmap) -> Self {
        BitmapParser { im, line_cursor: 0 }
    }
}

impl Iterator for BitmapParser {
    type Item = PrintCommand;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

#[cfg(test)]
mod test {
    use super::PrintCommand;

    #[test]
    fn test_cmd_parse() {
        let x = PrintCommand::FeedLines(2233).parse();
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

        let x = PrintCommand::NextPaper.parse();
        assert_eq!(x, vec![vec![0x0c]], "unexcepted result: {:02x?}", x);

        let x = PrintCommand::PrintLine(10, vec![false, false, true, true, true]).parse();
        assert_eq!(
            x,
            vec![vec![0x1f, 0x2a, 0x05, 0x00, 0x38]],
            "unexcepted result: {:02x?}",
            x
        );

        let x = PrintCommand::RepeatLine(8964).parse();
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

        let x = PrintCommand::ResetPrinter.parse();
        assert_eq!(x, vec![vec![0x1b, 0x40]], "unexcepted result: {:02x?}", x);

        let x = PrintCommand::SkipPrintLine(10, 5, vec![false, true]).parse();
        assert_eq!(
            x,
            vec![vec![0x1f, 0x2b, 0x00, 0x02, 0x02, 0x00]],
            "unexcepted result: {:02x?}",
            x
        );
        // println!("{:02x?}", x);
    }
}
