// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印一张 jpg / png 图片。
//!
//! ```text
//! dzprint_image --image <FILE> [--device <NAME>] [--raster <MODE>]
//!               [--rle <MODE>] [--gap-type <TYPE>] [--gap-length <MM>]
//!               [--darkness <VALUE>] [--speed <VALUE>]
//!               [--print-timeout <SECS>] [--recv-timeout <SECS>]
//! ```
//!
//! - 先查 `0x72` 拿到打印头点数；图片宽度大于点数时等比缩放到点数，否则原样打印。
//! - 彩色图走 [`from_pixmap`]，灰度图走 [`from_gray_image`]。
//! - `--gap-type` / `--gap-length` / `--darkness` / `--speed` 只发用户给出的那个，
//!   并且都在打印任务开始前发送。
//!
//! 行数据默认不压缩（`--rle off`）：DP27P 上 `0x2c` / `0x3c` 打真实照片时固件会在
//! 打印中途停走、在纸上插入一段空白；`0x29` 更糟，整页发完后固件连 `0x70` 都不回。
//! `--rle` 仍保留给简单线稿和实验用。
//!
//! 卡住时会自己失败退出：连续多次状态查询没有回应返回 `PrinterSilent`，
//! 整个任务超过 `--print-timeout` 返回 `PrintTimeout`。
//!
//! 纸张间隔同时写进每页的 `StartPage`，否则打印任务会用 `Continuous` 覆盖掉
//! `--gap-type`。`--gap-length` 省略时用 0.5mm，和旧版 `GapSetting` 的默认值一致。
//!
//! 设备连接、设置、超时这些和 `dzprint_typst` 共用的部分在 [`common`]。

use anyhow::{Context as _, anyhow};
use clap::Parser;
use dz_print::{
    bitmap::Bitmap,
    frontend::Device,
    raster::{DitherMode, from_gray_image, from_pixmap},
};
use image::{DynamicImage, GrayImage, ImageReader, imageops::FilterType};
use std::path::{Path, PathBuf};
use tiny_skia::Pixmap;
use tracing::info;

mod common;

use common::PrintArgs;

/// 命令行参数。
#[derive(Parser, Debug)]
#[command(version, about = "打印一张 jpg / png 图片")]
struct Cli {
    /// 要打印的图片（jpg / png）
    #[arg(long, value_name = "FILE")]
    image: PathBuf,

    #[command(flatten)]
    print: PrintArgs,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    common::init_tracing();
    let cli = Cli::parse();

    common::exit_code(
        async {
            // 图片路径 / 格式错误要在连接打印机之前报出来。
            let img = load_image(&cli.image)?;
            let args = cli.print.clone();
            common::with_device(&args, async |dev| print_image(dev, args.clone(), img).await).await
        }
        .await,
    )
}

async fn print_image(dev: &Device, args: PrintArgs, img: DynamicImage) -> anyhow::Result<()> {
    // 查询点数
    let dots = common::print_head_dots(dev).await?;
    info!("print head dots = {dots}");

    // 转换位图
    let (width, height) = (img.width(), img.height());
    let img = fit_to_print_head(img, dots);
    info!("image {width}x{height} -> {}x{}", img.width(), img.height());

    let bitmap = to_bitmap(&img, args.raster.into())?;
    info!(
        "bitmap {}x{}, dither = {:?}",
        bitmap.width(),
        bitmap.height(),
        DitherMode::from(args.raster)
    );

    // 发送设置
    args.send_settings(dev).await?;

    // 打印
    args.print_bitmap(dev, &bitmap).await?;
    Ok(())
}

/// 图片宽度大于打印头点数时等比缩放到点数，否则原样返回。
fn fit_to_print_head(img: DynamicImage, dots: u16) -> DynamicImage {
    let dots = u32::from(dots);
    let (width, height) = (img.width(), img.height());
    if width <= dots {
        return img;
    }
    // 用 u64 算，避免超大图在 u32 乘法里溢出。
    let new_height = (u64::from(height) * u64::from(dots) / u64::from(width)).max(1) as u32;
    img.resize_exact(dots, new_height, FilterType::Lanczos3)
}

/// 按图像是否带彩色选择抖动入口。
///
/// 带 alpha 的图先铺白底，避免透明区域被当成黑色。
fn to_bitmap(img: &DynamicImage, mode: DitherMode) -> anyhow::Result<Bitmap> {
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

/// 把带 alpha 的通道值合成到白底上。
fn over_white(value: u8, alpha: u8) -> u8 {
    let alpha = u32::from(alpha);
    ((u32::from(value) * alpha + 255 * (255 - alpha)) / 255) as u8
}

fn load_image(path: &Path) -> anyhow::Result<DynamicImage> {
    let reader = ImageReader::open(path)
        .with_context(|| format!("打开图片 {} 失败", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("识别图片格式 {} 失败", path.display()))?;
    reader
        .decode()
        .with_context(|| format!("解码图片 {} 失败", path.display()))
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
