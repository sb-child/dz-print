// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub mod cmd_parser;
use image::GrayImage;
use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};
use tiny_skia::Pixmap;

#[derive(Clone, Copy)]
pub enum DitherMode {
    /// 亮度截断
    Threshold,
    /// Floyd-Steinberg 误差扩散
    FloydSteinberg,
}

#[derive(Clone)]
pub struct Bitmap {
    w: u32,
    h: u32,
    pix: Vec<bool>,
}

impl Bitmap {
    /// black (0) pixel will convert to `true`, otherwise to `false`
    pub fn from_gray_image(im: &GrayImage, mode: DitherMode) -> Bitmap {
        let w = im.width();
        let h = im.height();
        let mut gray: Vec<u8> = im.pixels().map(|p| p[0]).collect();

        Self::process_dither(&mut gray, w as usize, h as usize, mode);

        let pix: Vec<bool> = gray.into_iter().map(|px| px < 128).collect();
        Bitmap { w, h, pix }
    }

    /// black (0) pixel will convert to `true`, otherwise to `false`
    pub fn from_pixmap(im: &Pixmap, mode: DitherMode) -> Bitmap {
        let w = im.width();
        let h = im.height();

        // 转换为灰度缓冲 (Luma)
        let mut gray: Vec<u8> = im
            .pixels()
            .iter()
            .map(|px| {
                let px = px.demultiply();
                // 常规的灰度化公式
                (0.299 * px.red() as f32 + 0.587 * px.green() as f32 + 0.114 * px.blue() as f32)
                    as u8
            })
            .collect();

        Self::process_dither(&mut gray, w as usize, h as usize, mode);

        // 黑色为 true，白色为 false
        let pix: Vec<bool> = gray.into_iter().map(|px| px < 128).collect();
        Bitmap { w, h, pix }
    }

    fn process_dither(gray: &mut Vec<u8>, w: usize, h: usize, mode: DitherMode) {
        match mode {
            DitherMode::Threshold => {
                gray.par_iter_mut().for_each(|px| {
                    *px = if *px > 127 { 255 } else { 0 };
                });
            }
            DitherMode::FloydSteinberg => {
                for y in 0..h {
                    for x in 0..w {
                        let idx = y * w + x;
                        let old_pixel = gray[idx];
                        let new_pixel = if old_pixel > 127 { 255 } else { 0 };
                        gray[idx] = new_pixel;

                        let err = old_pixel as i16 - new_pixel as i16;
                        if err == 0 {
                            continue;
                        }
                        if x + 1 < w {
                            let i = idx + 1;
                            gray[i] = (gray[i] as i16 + err * 7 / 16).clamp(0, 255) as u8;
                        }
                        if y + 1 < h {
                            if x > 0 {
                                let i = (y + 1) * w + x - 1;
                                gray[i] = (gray[i] as i16 + err * 3 / 16).clamp(0, 255) as u8;
                            }
                            let i = (y + 1) * w + x;
                            gray[i] = (gray[i] as i16 + err * 5 / 16).clamp(0, 255) as u8;
                            if x + 1 < w {
                                let i = (y + 1) * w + x + 1;
                                gray[i] = (gray[i] as i16 + err / 16).clamp(0, 255) as u8;
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    pub fn get_pixel(&self, w: u32, h: u32) -> bool {
        self.pix[self.pixel_loc_unchecked(w, h)]
    }

    pub fn get_line(&self, h: u32) -> Vec<bool> {
        let (start, end) = self.line_loc_unchecked(h);
        self.pix[start..end].to_vec()
    }

    pub fn is_line_empty(&self, h: u32) -> bool {
        let (start, end) = self.line_loc_unchecked(h);
        !self.pix[start..end].iter().any(|x| *x)
    }

    pub fn same_lines(&self, h1: u32, h2: u32) -> bool {
        let (start1, end1) = self.line_loc_unchecked(h1);
        let (start2, end2) = self.line_loc_unchecked(h2);
        self.pix[start1..end1] == self.pix[start2..end2]
    }

    pub fn first_black_pixel_in_line(&self, h: u32) -> Option<usize> {
        let (start, end) = self.line_loc_unchecked(h);
        self.pix[start..end]
            .iter()
            .position(|&x| x)
            .map(|i| start + i)
    }

    pub fn last_black_pixel_in_line(&self, h: u32) -> Option<usize> {
        let (start, end) = self.line_loc_unchecked(h);
        self.pix[start..end]
            .iter()
            .rposition(|&x| x)
            .map(|i| start + i)
    }

    pub fn pixel_loc_unchecked(&self, w: u32, h: u32) -> usize {
        h as usize * self.w as usize + w as usize
    }

    /// returns [start..end] of a line
    pub fn line_loc_unchecked(&self, h: u32) -> (usize, usize) {
        let start = self.pixel_loc_unchecked(0, h);
        // 返回的是排他上界, 必须包含整行 w 个像素。原来用 `w - 1` 当上界,
        // 会把每一行最后一个像素静默丢掉 (右边缘 1 个点永远打印不出来)。
        (start, start + self.w as usize)
    }
}

#[cfg(test)]
mod test {
    use super::{Bitmap, DitherMode};
    use image::{GrayImage, Luma};

    /// 每一行都必须覆盖完整的 w 个像素
    #[test]
    fn test_bitmap_line_covers_whole_width() {
        // 8x3, 每行只有最后一个像素是黑的
        let img: GrayImage = GrayImage::from_fn(8, 3, |x, _| {
            if x == 7 {
                Luma([0u8])
            } else {
                Luma([255u8])
            }
        });
        let bitmap = Bitmap::from_gray_image(&img, DitherMode::Threshold);
        assert_eq!(bitmap.width(), 8);
        for y in 0..bitmap.height() {
            let line = bitmap.get_line(y);
            assert_eq!(line.len(), 8, "row {y} must be 8 pixels wide");
            assert!(
                *line.last().unwrap(),
                "row {y} lost its right-most pixel to line_loc_unchecked"
            );
        }
    }
}
