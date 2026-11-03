// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 图片解码、缩放和抖动，`dzprint_image` 和 `dzserver` 共用。
//!
//! `dzprint_typst` 不需要这个模块，`common::mod` 里已经按 feature 排除。

use anyhow::{Context as _, anyhow};
use dz_print::{
    bitmap::Bitmap,
    raster::{DitherMode, from_gray_image, from_pixmap},
};
use image::{DynamicImage, GrayImage, ImageReader, imageops::FilterType};
use std::path::Path;
use tiny_skia::Pixmap;

pub fn load_image(path: &Path) -> anyhow::Result<DynamicImage> {
    let reader = ImageReader::open(path)
        .with_context(|| format!("打开图片 {} 失败", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("识别图片格式 {} 失败", path.display()))?;
    reader
        .decode()
        .with_context(|| format!("解码图片 {} 失败", path.display()))
}

/// 图片宽度大于打印头点数时等比缩放到点数，否则原样返回。
pub fn fit_to_print_head(img: DynamicImage, dots: u16) -> DynamicImage {
    let dots = u32::from(dots);
    let (width, height) = (img.width(), img.height());
    if width <= dots {
        return img;
    }
    // 用 u64 算，避免超大图在 u32 乘法里溢出。
    let new_height = (u64::from(height) * u64::from(dots) / u64::from(width)).max(1) as u32;
    img.resize_exact(dots, new_height, FilterType::Lanczos3)
}

/// 按图像是否带彩色选择抖动入口；带 alpha 的图先铺白底。
pub fn to_bitmap(img: &DynamicImage, mode: DitherMode) -> anyhow::Result<Bitmap> {
    if img.color().has_color() {
        // 彩色：先合成到白底、填成不透明 RGBA，再交给 from_pixmap。
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        let mut pixmap = Pixmap::new(width, height)
            .ok_or_else(|| anyhow!("图片尺寸 {width}x{height} 无法创建画布"))?;
        let (dst_pixels, _) = pixmap.data_mut().as_chunks_mut::<4>();
        for (src, dst) in rgba.pixels().zip(dst_pixels) {
            let [r, g, b, a] = src.0;
            dst[0] = over_white(r, a);
            dst[1] = over_white(g, a);
            dst[2] = over_white(b, a);
            dst[3] = 255;
        }
        Ok(from_pixmap(&pixmap, mode)?)
    } else {
        // 灰度：有 alpha 时同样先铺白底，否则直接用原灰度值。
        let gray = if img.color().has_alpha() {
            let rgba = img.to_rgba8();
            GrayImage::from_fn(rgba.width(), rgba.height(), |x, y| {
                let px = rgba.get_pixel(x, y);
                image::Luma([over_white(px[0], px[3])])
            })
        } else {
            img.to_luma8()
        };
        Ok(from_gray_image(&gray, mode)?)
    }
}

fn over_white(value: u8, alpha: u8) -> u8 {
    let alpha = u32::from(alpha);
    ((u32::from(value) * alpha + 255 * (255 - alpha)) / 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;

    #[test]
    fn fit_scales_only_when_wider_than_head() {
        let wide = DynamicImage::ImageRgb8(RgbImage::from_pixel(1000, 500, image::Rgb([0, 0, 0])));
        let fitted = fit_to_print_head(wide, 576);
        assert_eq!((fitted.width(), fitted.height()), (576, 288));

        let narrow = DynamicImage::ImageRgb8(RgbImage::from_pixel(384, 100, image::Rgb([0, 0, 0])));
        let same = fit_to_print_head(narrow, 576);
        assert_eq!((same.width(), same.height()), (384, 100));
    }

    #[test]
    fn color_goes_through_pixmap() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(4, 2, image::Rgb([255, 255, 255])));
        let bm = to_bitmap(&img, DitherMode::Threshold).unwrap();
        assert_eq!((bm.width(), bm.height()), (4, 2));
        assert!(!bm.line(0).unwrap().get(0).unwrap());
    }

    #[test]
    fn gray_stays_gray_and_honors_alpha() {
        let mut gray = GrayImage::from_pixel(2, 1, image::Luma([0]));
        gray.put_pixel(1, 0, image::Luma([255]));
        let bm = to_bitmap(&DynamicImage::ImageLuma8(gray), DitherMode::Threshold).unwrap();
        assert!(bm.line(0).unwrap().get(0).unwrap());
        assert!(!bm.line(0).unwrap().get(1).unwrap());

        let rgba = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0]));
        let bm = to_bitmap(&DynamicImage::ImageRgba8(rgba), DitherMode::Threshold).unwrap();
        assert!(!bm.line(0).unwrap().get(0).unwrap(), "透明像素应铺成白色");
    }
}
