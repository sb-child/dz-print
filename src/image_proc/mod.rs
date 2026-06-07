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
                                gray[i] = (gray[i] as i16 + err * 1 / 16).clamp(0, 255) as u8;
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
        let end = self.pixel_loc_unchecked(self.w - 1, h);
        (start, end)
    }
}
