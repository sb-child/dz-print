// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印一张 jpg / png 图片。
//!
//! ```text
//! dzprint_image --image <FILE> [--device <NAME>] [--raster <MODE>]
//!               [--gap-type <TYPE>] [--gap-length <MM>]
//!               [--darkness <VALUE>] [--speed <VALUE>]
//! ```
//!
//! - 先查 `0x72` 拿到打印头点数；图片宽度大于点数时等比缩放到点数，否则原样打印。
//! - 彩色图走 [`from_pixmap`]，灰度图走 [`from_gray_image`]。
//! - `--gap-type` / `--gap-length` / `--darkness` / `--speed` 只发用户给出的那个，
//!   并且都在打印任务开始前发送。
//!
//! 纸张间隔同时写进每页的 `StartPage`，否则打印任务会用 `Continuous` 覆盖掉
//! `--gap-type`。`--gap-length` 省略时用 0.5mm，和旧版 `GapSetting` 的默认值一致。

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context as _, anyhow, ensure};
use clap::{Parser, ValueEnum};
use image::{DynamicImage, GrayImage, ImageReader, imageops::FilterType};
use mytm::TaskManager;
use tiny_skia::Pixmap;
use tracing::info;

use dz_print::{
    backend::usb::UsbSelector,
    bitmap::Bitmap,
    command::{
        req::{self, ReqTrait},
        resp,
        settings::{DarknessSetting, GapTypeSetting, SpeedSetting},
    },
    frontend::{Device, DeviceConfig, PrintJobOptions},
    planner::print::{MAX_DOTS_PER_LINE, RleMode},
    raster::{DitherMode, from_gray_image, from_pixmap},
};

/// 退出前等 worker 线程真正停下（释放 USB 句柄）的超时。
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// 打印任务使用的 RLE 模式。
///
/// DP27P 实测 X / D 混用会周期性断层，所以默认只用 X；RLE 不比原始行小的时候
/// planner 会自己回退到 `PrintLine`。
const RLE_MODE: RleMode = RleMode::Bits6XOnly;

/// `--gap-length` 省略时用的间隔长度，单位 `0.01mm`（0.5mm）。
const DEFAULT_GAP_LENGTH: u32 = 50;

/// 抖动方式。
#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RasterArg {
    /// Floyd-Steinberg 误差扩散
    #[default]
    #[value(name = "floyd-steinberg")]
    FloydSteinberg,
    /// 亮度截断
    #[value(name = "threshold")]
    Threshold,
}

impl From<RasterArg> for DitherMode {
    fn from(value: RasterArg) -> Self {
        match value {
            RasterArg::FloydSteinberg => DitherMode::FloydSteinberg,
            RasterArg::Threshold => DitherMode::Threshold,
        }
    }
}

/// 命令行参数。
#[derive(Parser, Debug)]
#[command(version, about = "打印一张 jpg / png 图片")]
struct Cli {
    /// 要打印的图片（jpg / png）
    #[arg(long, value_name = "FILE")]
    image: PathBuf,

    /// 按 USB 设备名（`型号-序列号`）选择；省略时取第一台德佟设备
    #[arg(long, value_name = "NAME")]
    device: Option<String>,

    /// 纸张间隔类型：continuous / hole / gap / black-mark / transparent，或 0..=4
    #[arg(long, value_name = "TYPE", value_parser = parse_gap_type)]
    gap_type: Option<GapTypeSetting>,

    /// 纸张间隔长度，单位 mm，可带小数（例如 2 / 2.5 / 2.5mm）
    #[arg(long, value_name = "MM", value_parser = parse_gap_length)]
    gap_length: Option<u32>,

    /// 打印浓度：min / normal / max，或用户面值 1..=15
    #[arg(long, value_name = "VALUE", value_parser = parse_darkness)]
    darkness: Option<DarknessSetting>,

    /// 打印速度：min / normal / max，或用户面值 1..=5
    #[arg(long, value_name = "VALUE", value_parser = parse_speed)]
    speed: Option<SpeedSetting>,

    /// 抖动方式
    #[arg(long, value_enum, default_value = "floyd-steinberg")]
    raster: RasterArg,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let cli = Cli::parse();
    let selector = match cli.device.as_deref() {
        Some(name) => {
            info!("select device by name: {name}");
            UsbSelector::DeviceName(name.to_owned())
        }
        None => UsbSelector::Auto,
    };

    // 图片先读：路径或格式错了就不用去动打印机。
    let img = load_image(&cli.image)?;

    // 整个程序共用一个 TaskManager：worker 挂在它下面，退出前统一 close。
    let tm = TaskManager::new();
    let dev = Device::new(
        &tm,
        DeviceConfig {
            selector,
            ..Default::default()
        },
    );

    let result = async {
        dev.ready().await.context("设备没有就绪")?;
        print_image(&dev, &cli, img).await
    }
    .await;

    // 无论打印成功与否，都要等 worker 线程退出后再让进程结束。
    let stopped = dev.shutdown(SHUTDOWN_TIMEOUT).await;
    tm.close();
    result?;
    stopped?;
    Ok(())
}

/// 查询点数、转换位图、发送设置，然后打印。
async fn print_image(dev: &Device, cli: &Cli, img: DynamicImage) -> anyhow::Result<()> {
    let dots = print_head_dots(dev).await?;
    info!("print head dots = {dots}");

    let (width, height) = (img.width(), img.height());
    let img = fit_to_print_head(img, dots);
    info!("image {width}x{height} -> {}x{}", img.width(), img.height());

    let bitmap = to_bitmap(&img, cli.raster.into())?;
    info!(
        "bitmap {}x{}, dither = {:?}",
        bitmap.width(),
        bitmap.height(),
        DitherMode::from(cli.raster)
    );

    send_settings(dev, cli).await?;

    let report = dev
        .print_bitmap_job_with(&bitmap, RLE_MODE, job_options(cli))
        .await?;
    info!(
        chunks = report.chunks,
        bytes = report.bytes,
        budget_queries = report.budget_queries,
        degraded = report.degraded,
        "print finished"
    );
    Ok(())
}

/// 读 `0x72` 拿打印头点数。
async fn print_head_dots(dev: &Device) -> anyhow::Result<u16> {
    dev.try_send_batch(&[&req::print_width::PrintWidth {}])
        .await?;
    let width: resp::print_width::PrintWidth = dev.try_recv_one().await?;

    ensure!(
        width.print_width > 0,
        "设备回报的打印头点数为 0，无法确定缩放宽度"
    );
    ensure!(
        usize::from(width.print_width) <= MAX_DOTS_PER_LINE,
        "打印头点数 {} 超过协议上限 {MAX_DOTS_PER_LINE}",
        width.print_width
    );
    Ok(width.print_width)
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

/// 只发送用户给出的设置，顺序固定为纸张类型 / 间隔 / 浓度 / 速度。
async fn send_settings(dev: &Device, cli: &Cli) -> anyhow::Result<()> {
    let mut reqs: Vec<Box<dyn ReqTrait>> = Vec::new();
    if let Some(gap_type) = cli.gap_type {
        reqs.push(Box::new(req::gap_type::GapType { gap_type }));
    }
    if let Some(gap) = cli.gap_length {
        reqs.push(Box::new(req::gap_length::GapLength { gap }));
    }
    if let Some(darkness) = cli.darkness {
        reqs.push(Box::new(req::darkness::Darkness { darkness }));
    }
    if let Some(speed) = cli.speed {
        reqs.push(Box::new(req::speed::Speed { speed }));
    }
    if reqs.is_empty() {
        return Ok(());
    }

    info!(count = reqs.len(), "send print settings");
    let refs: Vec<&dyn ReqTrait> = reqs.iter().map(|req| req.as_ref()).collect();
    dev.try_send_batch(&refs).await?;
    Ok(())
}

/// 把命令行里的纸张设置带进 `StartPage`。
///
/// 只给了 `--gap-type` 时用 0.5mm，否则 v2 的 `printer_gap_length` 会校验失败。
fn job_options(cli: &Cli) -> PrintJobOptions {
    let gap_type = cli.gap_type.unwrap_or_default();
    let gap_length = match cli.gap_length {
        Some(gap) => gap,
        None if gap_type != GapTypeSetting::Continuous => DEFAULT_GAP_LENGTH,
        None => 0,
    };
    PrintJobOptions {
        gap_type,
        gap_length,
    }
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

fn parse_gap_type(value: &str) -> Result<GapTypeSetting, String> {
    match value.to_ascii_lowercase().as_str() {
        "continuous" | "ticket" => Ok(GapTypeSetting::Continuous),
        "hole" => Ok(GapTypeSetting::Hole),
        "gap" | "adhesive" => Ok(GapTypeSetting::Gap),
        "black-mark" | "black_mark" | "cardpaper" => Ok(GapTypeSetting::BlackMark),
        "transparent" => Ok(GapTypeSetting::Transparent),
        other => other.parse::<u8>().map_or_else(
            |_| {
                Err(format!(
                    "未知纸张间隔类型 `{value}`，可选 continuous / hole / gap / \
                     black-mark / transparent 或 0..=4"
                ))
            },
            |raw| GapTypeSetting::try_from(raw).map_err(|e| e.to_string()),
        ),
    }
}

/// 把 `2` / `2.5` / `2.5mm` 转成命令层用的 `0.01mm`。
fn parse_gap_length(value: &str) -> Result<u32, String> {
    let trimmed = value.trim();
    let number = trimmed.strip_suffix("mm").map_or(trimmed, str::trim).trim();
    let mm: f64 = number
        .parse()
        .map_err(|_| format!("无法解析间隔长度 `{value}`，请用 mm 数值，例如 2.5"))?;
    if !mm.is_finite() {
        return Err(format!("间隔长度 `{value}` 不是有限数值"));
    }

    let centi_mm = (mm * 100.0).round();
    if !(50.0..=4_194_303.0).contains(&centi_mm) {
        return Err(format!(
            "间隔长度 {mm}mm 超出范围，应在 0.5mm..=41943.03mm 之间"
        ));
    }
    Ok(centi_mm as u32)
}

fn parse_darkness(value: &str) -> Result<DarknessSetting, String> {
    value
        .parse()
        .map_err(|e: dz_print::command::settings::PrintSettingError| {
            format!("{e}（浓度可选 min / normal / max 或 1..=15）")
        })
}

fn parse_speed(value: &str) -> Result<SpeedSetting, String> {
    value
        .parse()
        .map_err(|e: dz_print::command::settings::PrintSettingError| {
            format!("{e}（速度可选 min / normal / max 或 1..=5）")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;

    #[test]
    fn cli_defaults() {
        let cli = Cli::try_parse_from(["dzprint_image", "--image", "a.png"]).unwrap();
        assert_eq!(cli.image, PathBuf::from("a.png"));
        assert!(cli.device.is_none());
        assert!(cli.gap_type.is_none());
        assert!(cli.gap_length.is_none());
        assert!(cli.darkness.is_none());
        assert!(cli.speed.is_none());
        assert_eq!(cli.raster, RasterArg::FloydSteinberg);
    }

    #[test]
    fn cli_parses_all_options() {
        let cli = Cli::try_parse_from([
            "dzprint_image",
            "--image",
            "a.jpg",
            "--device",
            "DP27P-Y4094C023",
            "--gap-type",
            "gap",
            "--gap-length",
            "2.5mm",
            "--darkness",
            "12",
            "--speed",
            "max",
            "--raster",
            "threshold",
        ])
        .unwrap();

        assert_eq!(cli.gap_type, Some(GapTypeSetting::Gap));
        assert_eq!(cli.gap_length, Some(250));
        assert_eq!(cli.darkness, Some(DarknessSetting::Darkness11));
        assert_eq!(cli.speed, Some(SpeedSetting::Max));
        assert_eq!(cli.raster, RasterArg::Threshold);
    }

    #[test]
    fn gap_length_accepts_mm_and_decimals() {
        assert_eq!(parse_gap_length("2").unwrap(), 200);
        assert_eq!(parse_gap_length("2.5").unwrap(), 250);
        assert_eq!(parse_gap_length("2.5mm").unwrap(), 250);
        assert_eq!(parse_gap_length(" 0.5 mm ").unwrap(), 50);
        assert!(parse_gap_length("0.4").is_err());
        assert!(parse_gap_length("abc").is_err());
        assert!(parse_gap_length("inf").is_err());
    }

    #[test]
    fn gap_type_accepts_names_and_numbers() {
        assert_eq!(
            parse_gap_type("ticket").unwrap(),
            GapTypeSetting::Continuous
        );
        assert_eq!(parse_gap_type("adhesive").unwrap(), GapTypeSetting::Gap);
        assert_eq!(
            parse_gap_type("black-mark").unwrap(),
            GapTypeSetting::BlackMark
        );
        assert_eq!(parse_gap_type("4").unwrap(), GapTypeSetting::Transparent);
        assert!(parse_gap_type("5").is_err());
        assert!(parse_gap_type("unknown").is_err());
    }

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

    #[test]
    fn job_options_default_to_continuous() {
        let cli = Cli::try_parse_from(["dzprint_image", "--image", "a.png"]).unwrap();
        let options = job_options(&cli);
        assert_eq!(options.gap_type, GapTypeSetting::Continuous);
        assert_eq!(options.gap_length, 0);
    }

    #[test]
    fn gap_type_without_length_uses_default() {
        let cli = Cli::try_parse_from(["dzprint_image", "--image", "a.png", "--gap-type", "gap"])
            .unwrap();
        let options = job_options(&cli);
        assert_eq!(options.gap_type, GapTypeSetting::Gap);
        assert_eq!(options.gap_length, DEFAULT_GAP_LENGTH);
    }
}
