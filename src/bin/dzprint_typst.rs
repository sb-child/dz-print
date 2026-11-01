// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印 Typst 文档。
//!
//! ```text
//! dzprint_typst <FILE.typ> [--font <FILE>] [--device <NAME>] [--raster <MODE>]
//!               [--rle <MODE>] [--gap-type <TYPE>] [--gap-length <MM>]
//!               [--darkness <VALUE>] [--speed <VALUE>]
//!               [--print-timeout <SECS>] [--recv-timeout <SECS>]
//! ```
//!
//! - 文档在连接打印机之前编译，编译错误不会碰设备。
//! - 每页按“打印头点数 / 页面宽度”渲染，正好铺满打印头；页高按比例。
//! - 每页单独发一个打印任务（`StartPage` … `EndPage`），打完再渲染下一页，
//!   避免多页位图同时留在内存里。
//! - 额外字体用 `--font` 加载，默认只内置两份 Unifont。
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

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, anyhow, bail};
use chrono::{Datelike as _, Local};
use clap::Parser;
use dz_print::{
    command::settings::{DarknessSetting, GapTypeSetting, SpeedSetting},
    frontend::Device,
    planner::print::RleMode,
    raster::{DitherMode, from_pixmap_over_white},
};
use tempfile::{TempDir, tempdir};
use tiny_skia::Pixmap;
use tracing::{info, warn};
use typst::{
    Library, LibraryExt as _, World,
    diag::{FileError, FileResult},
    foundations::{Bytes, Datetime, Dict, Label, Selector, Value},
    introspection::Introspector as _,
    syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot, package::PackageSpec},
    text::{Font, FontBook},
    utils::{LazyHash, PicoStr},
};
use typst_layout::PagedDocument;
use typst_render::RenderOptions;

use common::PrintArgs;

/// 每页覆盖设置用的标签名。
const SETTINGS_LABEL: &str = "print-settings";

/// 命令行参数。
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
    let world = TypstWorld::new(&cli)?;
    let doc = compile(&world)?;

    // 每页的 <print-settings> 覆盖项；按页码索引。
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

/// 编译文档，把编译警告 / 错误都打到日志里。
fn compile(world: &TypstWorld) -> anyhow::Result<PagedDocument> {
    let warned = typst::compile::<PagedDocument>(world);
    for warning in &warned.warnings {
        warn!("Typst: {}", warning.message);
    }
    warned.output.map_err(|errors| {
        let details: Vec<String> = errors
            .iter()
            .map(|error| {
                let hints: Vec<String> = error
                    .hints
                    .iter()
                    .map(|hint| format!("  hint: {}", hint.v))
                    .collect();
                if hints.is_empty() {
                    format!("- {}", error.message)
                } else {
                    format!("- {}\n{}", error.message, hints.join("\n"))
                }
            })
            .collect();
        anyhow!("Typst 编译失败：\n{}", details.join("\n"))
    })
}

/// 渲染并打印所有页。
///
/// 覆盖设置是“粘性”的：一页的 `<print-settings>` 会一直沿用到下一处覆盖。
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
        if let Some(settings) = overrides.get(&page_no) {
            info!(page = page_no, ?settings, "apply <print-settings>");
            settings.apply_to(&mut args);
        }

        let pixmap = render_page(page, dots)?;
        info!(
            page = page_no,
            width = pixmap.width(),
            height = pixmap.height(),
            "rendered page"
        );
        let bitmap = from_pixmap_over_white(&pixmap, args.raster.into())?;

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

/// 把一页按打印头宽度渲染。
///
/// `pixel_per_pt` 由页面实际宽度算出，所以不同纸张 / 页面宽度都能铺满打印头。
/// 四舍五入偶尔会多出 1 个像素，超出打印头时退一点再渲染一次。
fn render_page(page: &typst_layout::Page, dots: u16) -> anyhow::Result<Pixmap> {
    let width_pt = page.frame.size().x.to_pt();
    if !(width_pt.is_finite() && width_pt > 0.0) {
        bail!("页面宽度 {width_pt}pt 无效，无法渲染");
    }

    let dots_f = f64::from(dots);
    let options = |scale: f64| RenderOptions {
        pixel_per_pt: scale.into(),
        ..Default::default()
    };

    let mut pixmap = typst_render::render(page, &options(dots_f / width_pt));
    if pixmap.width() > u32::from(dots) {
        pixmap = typst_render::render(page, &options((dots_f - 1.0) / width_pt));
    }
    Ok(pixmap)
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

/// 一页的 `<print-settings>` 覆盖项。
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
            args.raster = match raster {
                DitherMode::FloydSteinberg => common::RasterArg::FloydSteinberg,
                DitherMode::Threshold => common::RasterArg::Threshold,
            };
        }
        if let Some(rle) = self.rle {
            args.rle = match rle {
                RleMode::Off => common::RleArg::Off,
                RleMode::Byte => common::RleArg::Byte,
                RleMode::Bits4 => common::RleArg::Bits4,
                RleMode::Bits5 => common::RleArg::Bits5,
                RleMode::Bits6 => common::RleArg::Bits6,
                RleMode::Bits4XOnly => common::RleArg::Bits4x,
                RleMode::Bits5XOnly => common::RleArg::Bits5x,
                RleMode::Bits6XOnly => common::RleArg::Bits6x,
            };
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
struct TypstWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    main: FileId,
    main_source: String,
    root: PathBuf,
    fonts: Vec<Font>,
    package_cache: TempDir,
}

impl TypstWorld {
    fn new(cli: &Cli) -> anyhow::Result<Self> {
        let path = cli
            .document
            .canonicalize()
            .with_context(|| format!("找不到 Typst 文档 {}", cli.document.display()))?;
        let main_source = std::fs::read_to_string(&path)
            .with_context(|| format!("读取 Typst 文档 {} 失败", path.display()))?;
        let root = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let vpath = VirtualPath::new(format!("/{filename}"))
            .map_err(|error| anyhow!("Typst 路径 `/{filename}` 无效: {error:?}"))?;
        let main = FileId::unique(RootedPath::new(VirtualRoot::Project, vpath));

        let (fonts, book) = load_fonts(&cli.font)?;
        Ok(Self {
            library: LazyHash::new(Library::builder().build()),
            book: LazyHash::new(book),
            main,
            main_source,
            root,
            fonts,
            package_cache: tempdir().context("创建 Typst 包缓存目录失败")?,
        })
    }

    fn resolve_id(&self, id: FileId) -> FileResult<PathBuf> {
        if id == self.main {
            return Err(FileError::Other(Some("main file is virtual".into())));
        }
        match id.root() {
            VirtualRoot::Project => {
                let path = id.vpath().realize(&self.root).map_err(FileError::Realize)?;
                if !path.starts_with(&self.root) {
                    return Err(FileError::AccessDenied);
                }
                Ok(path)
            }
            VirtualRoot::Package(package) => {
                let dir = prepare_package(package, self.package_cache.path())?;
                id.vpath().realize(&dir).map_err(FileError::Realize)
            }
        }
    }
}

impl World for TypstWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            return Ok(Source::new(id, self.main_source.clone()));
        }
        let path = self.resolve_id(id)?;
        let content = std::fs::read_to_string(&path).map_err(|e| FileError::from_io(e, &path))?;
        Ok(Source::new(id, content))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main {
            return Ok(Bytes::new(self.main_source.clone()));
        }
        let path = self.resolve_id(id)?;
        let content = std::fs::read(&path).map_err(|e| FileError::from_io(e, &path))?;
        Ok(Bytes::new(content))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        let now = Local::now();
        Datetime::from_ymd(now.year(), now.month() as u8, now.day() as u8)
    }
}

/// 内置 Unifont + `--font` 指定的字体。
fn load_fonts(extra: &[PathBuf]) -> anyhow::Result<(Vec<Font>, FontBook)> {
    let mut fonts = Vec::new();
    let mut book = FontBook::new();

    // 内置字体，保证离线也能排中文。
    for bytes in [
        &include_bytes!("../asset/unifont-16.0.04.ttf")[..],
        &include_bytes!("../asset/UnifontExMono.ttf")[..],
    ] {
        push_font(Bytes::new(bytes), &mut fonts, &mut book)?;
    }

    for path in extra {
        let data =
            std::fs::read(path).with_context(|| format!("读取字体 {} 失败", path.display()))?;
        push_font(Bytes::new(data), &mut fonts, &mut book)
            .with_context(|| format!("加载字体 {} 失败", path.display()))?;
    }
    Ok((fonts, book))
}

fn push_font(bytes: Bytes, fonts: &mut Vec<Font>, book: &mut FontBook) -> anyhow::Result<()> {
    let font = Font::new(bytes, 0).ok_or_else(|| anyhow!("不是可用的字体文件"))?;
    book.push(font.info().clone());
    fonts.push(font);
    Ok(())
}

/// 把 `@preview/...` 包下载到临时目录。已存在就直接复用。
fn prepare_package(pkg: &PackageSpec, cache_dir: &Path) -> FileResult<PathBuf> {
    let package_dir = cache_dir.join(format!(
        "{}__{}__{}",
        pkg.namespace.as_str(),
        pkg.name.as_str(),
        pkg.version,
    ));
    if package_dir.exists() {
        return Ok(package_dir);
    }
    if pkg.namespace != "preview" {
        return Err(FileError::NotFound(PathBuf::from(format!(
            "@{}/{} version {}",
            pkg.namespace, pkg.name, pkg.version
        ))));
    }

    let url = format!(
        "https://packages.typst.org/preview/{}-{}.tar.gz",
        pkg.name, pkg.version
    );
    info!("downloading Typst package: {url}");
    let response = ureq::get(&url)
        .call()
        .map_err(|e| FileError::Other(Some(format!("下载 {url} 失败: {e}").into())))?;
    let mut body = response.into_body();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(body.as_reader()));

    let downloading = cache_dir.join(format!(
        "{}__{}__{}__downloading",
        pkg.namespace.as_str(),
        pkg.name.as_str(),
        pkg.version,
    ));
    if downloading.exists() {
        let _ = std::fs::remove_dir_all(&downloading);
    }
    std::fs::create_dir_all(&downloading).map_err(|e| FileError::from_io(e, &downloading))?;
    archive
        .unpack(&downloading)
        .map_err(|e| FileError::Other(Some(format!("解压 {url} 失败: {e}").into())))?;
    std::fs::rename(&downloading, &package_dir).map_err(|e| FileError::from_io(e, &package_dir))?;
    Ok(package_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let cli = Cli::try_parse_from(["dzprint_typst", path.to_str().unwrap()]).unwrap();
        let world = TypstWorld::new(&cli).unwrap();
        let doc = compile(&world).unwrap();
        assert_eq!(doc.pages().len(), 2);

        let overrides = page_settings(&doc).unwrap();
        assert_eq!(
            overrides.get(&2).and_then(|settings| settings.gap_length),
            Some(200)
        );
        assert!(!overrides.contains_key(&1));

        let pixmap = render_page(&doc.pages()[0], 576).unwrap();
        assert_eq!(pixmap.width(), 576);
    }

    #[test]
    fn render_never_exceeds_the_print_head() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("doc.typ");
        // 7.3mm 这种宽度容易在四舍五入后多出 1 像素。
        std::fs::write(&path, "#set page(width: 7.3mm, height: 5mm)\nx\n").unwrap();
        let cli = Cli::try_parse_from(["dzprint_typst", path.to_str().unwrap()]).unwrap();
        let world = TypstWorld::new(&cli).unwrap();
        let doc = compile(&world).unwrap();
        let pixmap = render_page(&doc.pages()[0], 12).unwrap();
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
