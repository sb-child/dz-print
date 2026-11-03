// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印 Typst 文档：逐页渲染成位图，每页单独发一个打印任务。
//!
//! 参数见 `--help`。
//!
//! # 每页覆盖设置
//!
//! 在文档里放 `metadata` 并打上 `<print-settings>` 标签，就可以覆盖命令行参数：
//!
//! ```typst
//! #metadata((gap-type: "adhesive", gap: 2.5, darkness: 12, speed: "max")) <print-settings>
//! ```
//!
//! 字段都可以省略；支持的字段和取值：
//!
//! | 字段                  | 取值                                                              |
//! | --------------------- | ----------------------------------------------------------------- |
//! | `gap-type` / `paper`  | `continuous` / `hole` / `gap` / `black-mark` / `transparent`，或 0..=4 |
//! | `gap` / `gap-length`  | 数值按 mm 解释（`2.5`），或字符串 `"2.5mm"`                       |
//! | `darkness`            | `min` / `normal` / `max`，或 1..=15                                |
//! | `speed`               | `min` / `normal` / `max`，或 1..=5                                 |
//! | `raster`              | `floyd-steinberg` / `threshold`                                   |
//! | `rle`                 | `off` / `byte` / `bits4` / `bits5` / `bits6` / `bits4x` / `bits5x` / `bits6x` |
//!
//! 覆盖从它所在页开始生效，后续页沿用，直到下一次覆盖；相当于 Typst 的
//! `#set` 规则。需要恢复就在后面的页里再写一次。

mod common;

use anyhow::Context as _;
use clap::Parser;
use common::{
    PrintArgs, PrintSettings,
    printer::{PrintPage, print_page},
    typst::{self, PageSettings, TypstWorld, compile, render_bitmap},
};
use dz_print::frontend::Device;
use std::{collections::HashMap, path::PathBuf};
use tokio_util::sync::CancellationToken;
use tracing::info;
use typst_layout::PagedDocument;

#[derive(Parser, Debug)]
#[command(version, about = "打印一页或多页 Typst 文档")]
struct Cli {
    /// 要打印的 .typ 文件
    #[arg(value_name = "FILE")]
    document: PathBuf,

    /// 额外加载的字体文件（ttf / otf，可重复）；默认只用内置 Unifont
    #[arg(long = "font", value_name = "FILE")]
    font: Vec<PathBuf>,

    #[command(flatten)]
    print: PrintArgs,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    common::init_tracing();
    let cli = Cli::parse();
    common::exit_code(run(cli).await)
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    // 编译在连接打印机之前完成，语法 / 布局错误不会白白占着设备。
    let fonts = common::typst::load_fonts(&cli.font)?;
    let package_cache = tempfile::tempdir().context("创建 Typst 包缓存目录失败")?;
    let root = cli
        .document
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let world = TypstWorld::new(
        root,
        &cli.document,
        fonts.fonts,
        fonts.book,
        package_cache.path(),
    )?;
    let doc = compile(&world)?;

    let overrides = typst::page_settings(&doc)?;
    if !overrides.is_empty() {
        info!(pages = overrides.len(), "found <print-settings> overrides");
    }

    let args = cli.print.clone();
    common::with_device(&args, async |dev, info| {
        print_document(dev, info.head_dots, &cli.print, &doc, &overrides).await
    })
    .await
}

async fn print_document(
    dev: &Device,
    dots: u16,
    base: &PrintArgs,
    doc: &PagedDocument,
    overrides: &HashMap<usize, PageSettings>,
) -> anyhow::Result<()> {
    info!("pages = {}", doc.pages().len());

    let mut args = base.clone();
    // 上一次真正发给打印机的设置，用来避免每页都重复发一遍。
    let mut sent: Option<PrintSettings> = None;
    // 命令行暂时没有取消入口，保留 token 让打印循环和 server 共用同一套接口。
    let cancel = CancellationToken::new();

    for (index, page) in doc.pages().iter().enumerate() {
        let page_no = index + 1;
        // 覆盖是粘性的：一页的 <print-settings> 一直沿用到下一处覆盖。
        if let Some(settings) = overrides.get(&page_no) {
            info!(page = page_no, ?settings, "apply <print-settings>");
            settings.apply_to(&mut args);
        }

        let bitmap = render_bitmap(page, dots, args.raster.into())?;
        info!(
            page = page_no,
            width = bitmap.width(),
            height = bitmap.height(),
            "rendered page"
        );

        let page = PrintPage {
            bitmap,
            rle: args.rle.into(),
            options: args.job_options(),
            settings: PrintSettings::from(&args),
        };
        info!(page = page_no, rle = ?args.rle, "printing page");
        print_page(dev, "document", &page, page_no, 0, &mut sent, &cancel).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::typst::foundations::{Dict, Value};
    use dz_print::command::settings::{DarknessSetting, GapTypeSetting, SpeedSetting};
    use tempfile::{TempDir, tempdir};

    fn test_world(path: &std::path::Path) -> (TypstWorld, TempDir) {
        let fonts = common::typst::load_fonts(&[]).unwrap();
        let cache = tempdir().unwrap();
        let world = TypstWorld::new(
            path.parent().unwrap(),
            path,
            fonts.fonts,
            fonts.book,
            cache.path(),
        )
        .unwrap();
        (world, cache)
    }

    fn dict(pairs: &[(&str, Value)]) -> Dict {
        let mut dict = Dict::new();
        for (key, value) in pairs {
            dict.insert((*key).into(), value.clone());
        }
        dict
    }

    fn base_args() -> PrintArgs {
        Cli::try_parse_from(["dzprint_typst", "doc.typ"])
            .unwrap()
            .print
    }

    #[test]
    fn metadata_overrides_every_field() {
        let dict = dict(&[
            ("gap-type", Value::Str("adhesive".into())),
            ("gap", Value::Float(2.5)),
            ("darkness", Value::Int(12)),
            ("speed", Value::Str("max".into())),
            ("raster", Value::Str("threshold".into())),
            ("rle", Value::Str("bits6x".into())),
        ]);
        let mut settings = PageSettings::default();
        settings.apply_dict(&dict, 1).unwrap();

        let mut args = base_args();
        settings.apply_to(&mut args);
        assert_eq!(args.gap_type, Some(GapTypeSetting::Gap));
        assert_eq!(args.gap_length, Some(250));
        assert_eq!(args.darkness, Some(DarknessSetting::Darkness11));
        assert_eq!(args.speed, Some(SpeedSetting::Max));
        assert_eq!(args.raster, common::RasterArg::Threshold);
        assert_eq!(args.rle, common::RleArg::Bits6x);
    }

    #[test]
    fn metadata_accepts_mm_strings_and_old_key_names() {
        let dict = dict(&[
            ("paper", Value::Str("continuous".into())),
            ("gap-length", Value::Str("2.5mm".into())),
        ]);
        let mut settings = PageSettings::default();
        settings.apply_dict(&dict, 1).unwrap();
        assert_eq!(settings.gap_type, Some(GapTypeSetting::Continuous));
        assert_eq!(settings.gap_length, Some(250));
    }

    #[test]
    fn metadata_gap_number_means_millimetres() {
        let dict = dict(&[("gap", Value::Int(2))]);
        let mut settings = PageSettings::default();
        settings.apply_dict(&dict, 1).unwrap();
        assert_eq!(settings.gap_length, Some(200));
    }

    #[test]
    fn metadata_rejects_bad_values() {
        for (key, value) in [
            ("gap", Value::Float(0.4)),
            ("darkness", Value::Int(16)),
            ("speed", Value::Int(6)),
            ("raster", Value::Str("bayer".into())),
            ("rle", Value::Str("bits9".into())),
            ("unknown", Value::Int(1)),
        ] {
            let mut settings = PageSettings::default();
            let error = settings.apply_dict(&dict(&[(key, value)]), 2).unwrap_err();
            assert!(error.to_string().contains("第 2 页"), "{key}: {error}");
        }
    }

    #[test]
    fn compiles_document_and_reads_page_settings() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("doc.typ");
        std::fs::write(
            &path,
            "#set page(width: 48mm, height: 30mm)\n\
             Page one\n\
             #pagebreak()\n\
             #metadata((gap: 2)) <print-settings>\n\
             Page two\n",
        )
        .unwrap();

        let (world, _cache) = test_world(&path);
        let doc = compile(&world).unwrap();
        assert_eq!(doc.pages().len(), 2);

        let overrides = typst::page_settings(&doc).unwrap();
        assert_eq!(
            overrides.get(&2).and_then(|settings| settings.gap_length),
            Some(200)
        );
        assert!(!overrides.contains_key(&1));

        let pixmap = common::typst::render_page(&doc.pages()[0], 576).unwrap();
        assert_eq!(pixmap.width(), 576);
    }

    #[test]
    fn render_never_exceeds_the_print_head() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("doc.typ");
        // 7.3mm 这种宽度容易在四舍五入后多出 1 像素。
        std::fs::write(&path, "#set page(width: 7.3mm, height: 5mm)\nx\n").unwrap();
        let (world, _cache) = test_world(&path);
        let doc = compile(&world).unwrap();
        let pixmap = common::typst::render_page(&doc.pages()[0], 12).unwrap();
        assert!(pixmap.width() <= 12, "rendered {} px", pixmap.width());
        assert!(pixmap.width() > 0);
    }

    #[test]
    fn metadata_must_be_complete_string_for_enum_names() {
        let mut settings = PageSettings::default();
        let error = settings
            .apply_dict(&dict(&[("gap-type", Value::Int(1))]), 1)
            .unwrap_err();
        assert!(error.to_string().contains("应该是字符串"), "{error}");
    }
}
