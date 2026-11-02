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

use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context as _, anyhow, bail};
use clap::Parser;
use dz_print::{
    command::settings::{DarknessSetting, GapTypeSetting, SpeedSetting},
    frontend::Device,
    planner::print::RleMode,
    raster::DitherMode,
};
use tracing::info;
use typst::{
    foundations::{Dict, Label, Selector, Value},
    introspection::Introspector as _,
    utils::PicoStr,
};
use typst_layout::PagedDocument;

use common::{
    PrintArgs,
    typst::{TypstWorld, compile, render_bitmap},
};

const SETTINGS_LABEL: &str = "print-settings";

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

    let overrides = page_settings(&doc)?;
    if !overrides.is_empty() {
        info!(pages = overrides.len(), "found <print-settings> overrides");
    }

    let args = cli.print.clone();
    common::with_device(&args, async |dev| {
        print_document(dev, &cli.print, &doc, &overrides).await
    })
    .await
}

async fn print_document(
    dev: &Device,
    base: &PrintArgs,
    doc: &PagedDocument,
    overrides: &HashMap<usize, PageSettings>,
) -> anyhow::Result<()> {
    let dots = common::print_head_dots(dev).await?;
    info!("print head dots = {dots}, pages = {}", doc.pages().len());

    let mut args = base.clone();
    // 上一次真正发给打印机的设置，用来避免每页都重复发一遍。
    let mut sent: Option<SendableSettings> = None;

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

        let current = SendableSettings::from(&args);
        if sent.as_ref() != Some(&current) {
            args.send_settings(dev).await?;
            sent = Some(current);
        }

        info!(
            page = page_no,
            rle = ?args.rle,
            "printing page"
        );
        args.print_bitmap(dev, &bitmap).await?;
    }
    Ok(())
}

/// 真正需要发给打印机的设置；用来判断这一页要不要重发。
#[derive(Debug, Clone, PartialEq, Eq)]
struct SendableSettings {
    gap_type: Option<GapTypeSetting>,
    gap_length: Option<u32>,
    darkness: Option<DarknessSetting>,
    speed: Option<SpeedSetting>,
}

impl From<&PrintArgs> for SendableSettings {
    fn from(args: &PrintArgs) -> Self {
        Self {
            gap_type: args.gap_type,
            gap_length: args.gap_length,
            darkness: args.darkness,
            speed: args.speed,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PageSettings {
    gap_type: Option<GapTypeSetting>,
    gap_length: Option<u32>,
    darkness: Option<DarknessSetting>,
    speed: Option<SpeedSetting>,
    raster: Option<DitherMode>,
    rle: Option<RleMode>,
}

impl PageSettings {
    fn apply_to(&self, args: &mut PrintArgs) {
        if let Some(gap_type) = self.gap_type {
            args.gap_type = Some(gap_type);
        }
        if let Some(gap_length) = self.gap_length {
            args.gap_length = Some(gap_length);
        }
        if let Some(darkness) = self.darkness {
            args.darkness = Some(darkness);
        }
        if let Some(speed) = self.speed {
            args.speed = Some(speed);
        }
        if let Some(raster) = self.raster {
            args.raster = raster.into();
        }
        if let Some(rle) = self.rle {
            args.rle = rle.into();
        }
    }

    fn apply_dict(&mut self, dict: &Dict, page: usize) -> anyhow::Result<()> {
        for (key, value) in dict.iter() {
            let name = key.as_str();
            match name {
                "paper" | "gap-type" => {
                    self.gap_type =
                        Some(common::parse_gap_type(&string(value, page, name)?).map_err(
                            |error| anyhow!("第 {page} 页 <print-settings> 的 `{name}`: {error}"),
                        )?);
                }
                "gap" | "gap-length" => {
                    self.gap_length = Some(gap_length(value, page)?);
                }
                "darkness" => {
                    self.darkness = Some(darkness(value, page)?);
                }
                "speed" => {
                    self.speed = Some(speed(value, page)?);
                }
                "raster" => {
                    let raw = string(value, page, name)?;
                    self.raster = Some(parse_raster(&raw).ok_or_else(|| {
                        anyhow!(
                            "第 {page} 页 <print-settings> 的 `raster`: 未知取值 `{raw}`，\
                             可选 floyd-steinberg / threshold"
                        )
                    })?);
                }
                "rle" => {
                    let raw = string(value, page, name)?;
                    self.rle = Some(parse_rle(&raw).ok_or_else(|| {
                        anyhow!(
                            "第 {page} 页 <print-settings> 的 `rle`: 未知取值 `{raw}`，\
                             可选 off / byte / bits4 / bits5 / bits6 / bits4x / bits5x / bits6x"
                        )
                    })?);
                }
                other => bail!(
                    "第 {page} 页 <print-settings> 有未知字段 `{other}`，\
                     可用 gap-type / gap / darkness / speed / raster / rle"
                ),
            }
        }
        Ok(())
    }
}

fn string(value: &Value, page: usize, key: &str) -> anyhow::Result<String> {
    match value {
        Value::Str(raw) => Ok(raw.as_str().to_owned()),
        other => bail!("第 {page} 页 <print-settings> 的 `{key}` 应该是字符串，实际是 {other:?}"),
    }
}

/// `gap` 数值按 mm 解释；字符串走和命令行一样的解析。
fn gap_length(value: &Value, page: usize) -> anyhow::Result<u32> {
    let mm = match value {
        Value::Int(mm) => *mm as f64,
        Value::Float(mm) => *mm,
        Value::Str(raw) => {
            return common::parse_gap_length(raw.as_str())
                .map_err(|error| anyhow!("第 {page} 页 <print-settings> 的 `gap`: {error}"));
        }
        other => bail!(
            "第 {page} 页 <print-settings> 的 `gap` 应该是数值（mm）或字符串，实际是 {other:?}"
        ),
    };
    common::parse_gap_length(&mm.to_string())
        .map_err(|error| anyhow!("第 {page} 页 <print-settings> 的 `gap`: {error}"))
}

fn darkness(value: &Value, page: usize) -> anyhow::Result<DarknessSetting> {
    let raw = match value {
        Value::Int(raw) => {
            let raw = u8::try_from(*raw)
                .map_err(|_| anyhow!("第 {page} 页 <print-settings> 的 `darkness` 超出 1..=15"))?;
            DarknessSetting::from_user(raw)
                .map_err(|error| anyhow!("第 {page} 页 <print-settings> 的 `darkness`: {error}"))?
        }
        Value::Str(raw) => common::parse_darkness(raw.as_str())
            .map_err(|error| anyhow!("第 {page} 页 <print-settings> 的 `darkness`: {error}"))?,
        other => bail!(
            "第 {page} 页 <print-settings> 的 `darkness` 应该是整数或字符串，实际是 {other:?}"
        ),
    };
    Ok(raw)
}

fn speed(value: &Value, page: usize) -> anyhow::Result<SpeedSetting> {
    let raw = match value {
        Value::Int(raw) => {
            let raw = u8::try_from(*raw)
                .map_err(|_| anyhow!("第 {page} 页 <print-settings> 的 `speed` 超出 1..=5"))?;
            SpeedSetting::from_user(raw)
                .map_err(|error| anyhow!("第 {page} 页 <print-settings> 的 `speed`: {error}"))?
        }
        Value::Str(raw) => common::parse_speed(raw.as_str())
            .map_err(|error| anyhow!("第 {page} 页 <print-settings> 的 `speed`: {error}"))?,
        other => {
            bail!("第 {page} 页 <print-settings> 的 `speed` 应该是整数或字符串，实际是 {other:?}")
        }
    };
    Ok(raw)
}

fn parse_raster(raw: &str) -> Option<DitherMode> {
    match raw.to_ascii_lowercase().as_str() {
        "floyd-steinberg" | "floyd" | "fs" => Some(DitherMode::FloydSteinberg),
        "threshold" | "none" => Some(DitherMode::Threshold),
        _ => None,
    }
}

fn parse_rle(raw: &str) -> Option<RleMode> {
    match raw.to_ascii_lowercase().as_str() {
        "off" | "none" => Some(RleMode::Off),
        "byte" => Some(RleMode::Byte),
        "bits4" => Some(RleMode::Bits4),
        "bits5" => Some(RleMode::Bits5),
        "bits6" => Some(RleMode::Bits6),
        "bits4x" => Some(RleMode::Bits4XOnly),
        "bits5x" => Some(RleMode::Bits5XOnly),
        "bits6x" => Some(RleMode::Bits6XOnly),
        _ => None,
    }
}

/// 从文档里收集每页的 `<print-settings>`。
///
/// 同一页出现多个 `metadata` 时按出现顺序合并，后者覆盖前者。
fn page_settings(doc: &PagedDocument) -> anyhow::Result<HashMap<usize, PageSettings>> {
    let Some(label) = Label::new(PicoStr::intern(SETTINGS_LABEL)) else {
        return Ok(HashMap::new());
    };
    let selector = Selector::Label(label);
    let introspector = doc.introspector();

    let mut overrides: HashMap<usize, PageSettings> = HashMap::new();
    for content in introspector.query(&selector) {
        let Some(location) = content.location() else {
            continue;
        };
        let Some(page) = introspector.page(location) else {
            continue;
        };
        let page = page.get();

        let value = content.get_by_name("value").map_err(|error| {
            anyhow!("第 {page} 页 <{SETTINGS_LABEL}> 无法读取 value: {error:?}")
        })?;
        let Value::Dict(dict) = value else {
            bail!(
                "第 {page} 页 <{SETTINGS_LABEL}> 必须是字典，\
                 例如 #metadata((gap: 2.5)) <{SETTINGS_LABEL}>"
            );
        };
        overrides.entry(page).or_default().apply_dict(&dict, page)?;
    }
    Ok(overrides)
}

/// 文档里 `World` 的简化实现：只挂内置 / `--font` 字体，包从 preview 源下载。
#[cfg(test)]
mod tests {
    use super::*;
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

        let overrides = page_settings(&doc).unwrap();
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
