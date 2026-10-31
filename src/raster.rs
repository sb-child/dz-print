// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 图像 → 位图的抖动转换。
//!
//! 这里只做纯像素运算，不涉及命令编码。打印链路是：
//!
//! [`from_gray_image`] / [`from_pixmap`] → [`Bitmap`] →
//! [`crate::planner::print::print_bitmap`] → 命令层。

use crate::bitmap::{Bitmap, BitmapError};
use deku::bitvec::{BitVec, Msb0};
use image::GrayImage;
use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};
use tiny_skia::Pixmap;

/// 灰度图转黑白位图时的抖动方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DitherMode {
    /// 亮度截断。
    Threshold,
    /// Floyd-Steinberg 误差扩散。
    FloydSteinberg,
}

/// 把灰度图转成位图。
///
/// 灰度 `< 128` 的像素为黑，对应位 `true`。
pub fn from_gray_image(im: &GrayImage, mode: DitherMode) -> Result<Bitmap, BitmapError> {
    let width = im.width() as usize;
    let height = im.height() as usize;
    let mut gray: Vec<u8> = im.pixels().map(|p| p[0]).collect();
    process_dither(&mut gray, width, height, mode);
    build_bitmap(gray, width, height)
}

/// 把 `tiny-skia` 画布转成位图。
///
/// 先按 `0.299 R + 0.587 G + 0.114 B` 转灰度，再抖动。
pub fn from_pixmap(im: &Pixmap, mode: DitherMode) -> Result<Bitmap, BitmapError> {
    let width = im.width() as usize;
    let height = im.height() as usize;
    let mut gray: Vec<u8> = im
        .pixels()
        .iter()
        .map(|px| {
            let px = px.demultiply();
            (0.299 * px.red() as f32 + 0.587 * px.green() as f32 + 0.114 * px.blue() as f32) as u8
        })
        .collect();
    process_dither(&mut gray, width, height, mode);
    build_bitmap(gray, width, height)
}

/// 抖动后的灰度缓冲 → 位图。`gray` 长度必须等于 `width * height`。
fn build_bitmap(gray: Vec<u8>, width: usize, height: usize) -> Result<Bitmap, BitmapError> {
    let bits: BitVec<u8, Msb0> = gray.into_iter().map(|px| px < 128).collect();
    Bitmap::from_bits(width, height, bits)
}

fn process_dither(gray: &mut [u8], width: usize, height: usize, mode: DitherMode) {
    match mode {
        DitherMode::Threshold => {
            gray.par_iter_mut().for_each(|px| {
                *px = if *px > 127 { 255 } else { 0 };
            });
        }
        DitherMode::FloydSteinberg => {
            for y in 0..height {
                for x in 0..width {
                    let idx = y * width + x;
                    let old_pixel = gray[idx];
                    let new_pixel = if old_pixel > 127 { 255 } else { 0 };
                    gray[idx] = new_pixel;

                    let err = old_pixel as i16 - new_pixel as i16;
                    if err == 0 {
                        continue;
                    }
                    if x + 1 < width {
                        let i = idx + 1;
                        gray[i] = (gray[i] as i16 + err * 7 / 16).clamp(0, 255) as u8;
                    }
                    if y + 1 < height {
                        if x > 0 {
                            let i = (y + 1) * width + x - 1;
                            gray[i] = (gray[i] as i16 + err * 3 / 16).clamp(0, 255) as u8;
                        }
                        let i = (y + 1) * width + x;
                        gray[i] = (gray[i] as i16 + err * 5 / 16).clamp(0, 255) as u8;
                        if x + 1 < width {
                            let i = (y + 1) * width + x + 1;
                            gray[i] = (gray[i] as i16 + err / 16).clamp(0, 255) as u8;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;
    use tiny_skia::PremultipliedColorU8;

    #[test]
    fn gray_line_covers_whole_width() {
        // 8x3，每行只有最后一个像素是黑的。
        let img = GrayImage::from_fn(
            8,
            3,
            |x, _| {
                if x == 7 { Luma([0u8]) } else { Luma([255u8]) }
            },
        );
        let bm = from_gray_image(&img, DitherMode::Threshold).unwrap();
        assert_eq!(bm.width(), 8);
        assert_eq!(bm.height(), 3);
        for y in 0..bm.height() {
            let line = bm.line(y).unwrap();
            assert_eq!(line.len(), 8, "row {y} must be 8 pixels wide");
            assert!(
                line.bits()[7],
                "row {y} lost its right-most pixel to line_loc_unchecked"
            );
        }
    }

    #[test]
    fn threshold_uses_127_and_128_as_the_boundary() {
        let img = GrayImage::from_fn(3, 1, |x, _| {
            Luma([match x {
                0 => 127,
                1 => 128,
                _ => 0,
            }])
        });
        let bm = from_gray_image(&img, DitherMode::Threshold).unwrap();
        let line = bm.line(0).unwrap();
        assert!(line.bits()[0], "127 should be black");
        assert!(!line.bits()[1], "128 should be white");
        assert!(line.bits()[2], "0 should be black");
    }

    #[test]
    fn floyd_steinberg_spreads_error_to_the_right() {
        // 200 -> 白，误差 -55 的 7/16 扩散到右侧：100 - 24 = 76 -> 黑。
        let img = GrayImage::from_fn(2, 1, |x, _| Luma([if x == 0 { 200u8 } else { 100u8 }]));
        let bm = from_gray_image(&img, DitherMode::FloydSteinberg).unwrap();
        let line = bm.line(0).unwrap();
        assert!(!line.bits()[0]);
        assert!(line.bits()[1]);
    }

    #[test]
    fn pixmap_is_converted_through_luma() {
        let mut pm = Pixmap::new(2, 1).unwrap();
        pm.pixels_mut()[0] = PremultipliedColorU8::from_rgba(0, 0, 0, 255).unwrap();
        pm.pixels_mut()[1] = PremultipliedColorU8::from_rgba(255, 255, 255, 255).unwrap();
        let bm = from_pixmap(&pm, DitherMode::Threshold).unwrap();
        let line = bm.line(0).unwrap();
        assert!(line.bits()[0]);
        assert!(!line.bits()[1]);
    }

    #[test]
    fn empty_image_is_rejected() {
        let img = GrayImage::new(0, 0);
        assert!(matches!(
            from_gray_image(&img, DitherMode::Threshold),
            Err(BitmapError::EmptySize { .. })
        ));
    }
}
