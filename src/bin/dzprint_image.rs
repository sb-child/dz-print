// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印一张 jpg / png 图片。
//!
//! 先查 `0x72` 拿打印头点数，超宽时等比缩放，再抖动成位图交给 [`common`]；
//! 参数见 `--help`。
//!
//! 行数据默认不压缩：DP27P 上 RLE 打真实照片会在纸中间插入空白，`0x29` 甚至会让
//! 固件停止回应，`--rle` 只留给简单线稿和实验。

use clap::Parser;
use dz_print::{frontend::Device, raster::DitherMode};
use image::DynamicImage;
use std::path::PathBuf;
use tracing::info;

mod common;

use common::{
    PrintArgs, PrintSettings,
    image::{fit_to_print_head, load_image, to_bitmap},
    printer::{PrintPage, print_page},
};
use tokio_util::sync::CancellationToken;

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
            common::with_device(&args, async |dev, info| {
                print_image(dev, info.head_dots, args.clone(), img).await
            })
            .await
        }
        .await,
    )
}

async fn print_image(
    dev: &Device,
    dots: u16,
    args: PrintArgs,
    img: DynamicImage,
) -> anyhow::Result<()> {
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

    let page = PrintPage {
        bitmap,
        rle: args.rle.into(),
        options: args.job_options(),
        settings: PrintSettings::from(&args),
    };
    let mut sent = None;
    let cancel = CancellationToken::new();
    print_page(dev, "image", &page, 1, 0, &mut sent, &cancel).await?;
    Ok(())
}
