// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
};

use chrono::Local;
use dz_print::{
    backend,
    command::{self, HostCommand},
    image_proc::{
        cmd_parser::{BitmapParser, PrintCommand},
        Bitmap, DitherMode,
    },
};
use tiny_skia::Pixmap;
use typst::{
    diag::{FileError, FileResult},
    foundations::{Bytes, Datetime, NativeFunc, NativeFuncData},
    layout::PagedDocument,
    syntax::{FileId, Source, VirtualPath},
    text::{Font, FontBook, FontInfo},
    utils::{LazyHash, PicoStr},
    Library, LibraryExt, World,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    main_fn().await
}

async fn main_fn() -> anyhow::Result<()> {
    println!("dz-print for typst");
    let file_name = env::args()
        .nth(1)
        .ok_or(anyhow::anyhow!("please specify a filename"))?;
    let file_path = std::path::PathBuf::from(&file_name);
    println!("reading file");
    let file_content = tokio::fs::read_to_string(file_name).await?;
    println!("creating world");
    let world = Minecraft::new(&file_path, file_content);
    println!("compiling document");
    let doc = typst::compile::<PagedDocument>(&world);
    for w in doc.warnings {
        println!("warning: {w:?}");
    }
    let doc = doc
        .output
        .map_err(|e| anyhow::anyhow!("compile error: {e:?}"))?;

    let mut page_settings_map: HashMap<usize, PrintSettings> = HashMap::new();
    let page_settings_selector = typst::foundations::Selector::Label(
        typst::foundations::Label::new(PicoStr::intern("print-settings")).unwrap(),
    );

    println!("load page settings");
    for content in doc.introspector.query(&page_settings_selector) {
        if let Some(location) = content.location() {
            let page_num = doc.introspector.page(location).get();
            println!("parsing page setting {page_num}");
            let values = content
                .get_by_name("value")
                .map_err(|e| anyhow::anyhow!("parse page setting: {e:?}"))?;
            let typst::foundations::Value::Dict(v) = values else {
                continue;
            };
            let paper = match v.get("paper").ok() {
                Some(typst::foundations::Value::Str(x)) => PaperSetting::try_from(x.as_str())
                    .map_err(|e| anyhow::anyhow!("parse paper setting: {e:?}"))?,
                Some(e) => return Err(anyhow::anyhow!("parse paper setting: Invalid Type {e:?}")),
                None => PaperSetting::default(),
            };
            macro_rules! parse_numeric_setting {
                ($field_name:expr, $setting_type:ty, $cast_type:ty) => {
                    match v.get($field_name).ok() {
                        Some(typst::foundations::Value::Int(x)) => {
                            <$setting_type>::try_from(*x as $cast_type).map_err(|e| {
                                anyhow::anyhow!("parse {} setting: {e:?}", $field_name)
                            })?
                        }
                        Some(typst::foundations::Value::Str(x)) => {
                            <$setting_type>::try_from(x.as_str()).map_err(|e| {
                                anyhow::anyhow!("parse {} setting: {e:?}", $field_name)
                            })?
                        }
                        Some(_) => {
                            return Err(anyhow::anyhow!(
                                "parse {} setting: InvalidType",
                                $field_name
                            ))
                        }
                        None => <$setting_type>::default(),
                    }
                };
            }
            let darkness = parse_numeric_setting!("darkness", DarknessSetting, u8);
            let speed = parse_numeric_setting!("speed", SpeedSetting, u8);
            let gap = parse_numeric_setting!("gap", GapSetting, u16);
            let ps = PrintSettings {
                paper,
                darkness,
                speed,
                gap,
            };
            page_settings_map.insert(page_num, ps);
        }
    }

    println!("connecting to printer");
    let b = backend::USBBackend::new(backend::USBSelector::DeviceSerial(
        "DP27P-Y4094C023".to_string(),
    ))
    .await?;
    for p in doc.pages {
        println!("rendering page {}", p.number);
        // 576px = 48mm
        let r = typst_render::render(&p, 576.0 / (2.834_645_7 * 48.0));
        let ps = page_settings_map
            .get(&(p.number as usize))
            .cloned()
            .unwrap_or_default();
        println!("printing page {}: {:?}, bp {}", p.number, ps, ps.bp());
        print_page(&b, r, ps).await?;
    }
    Ok(())
}

async fn print_page(b: &backend::USBBackend, pm: Pixmap, ps: PrintSettings) -> anyhow::Result<()> {
    assert_eq!(
        pm.width(),
        576,
        "please ensure your page width is 576px or 48mm"
    );
    println!("converting to bitmap");
    let bitmap = Bitmap::from_pixmap(&pm, DitherMode::FloydSteinberg);
    // 这个 bp 参数其实是 magic number，以下是建议值
    // 最慢 50 | 较慢 75 | 正常 100 | 较快 110 | 最快 120
    // 可能受打印浓度影响
    let parser = BitmapParser::new(bitmap, ps.bp());
    println!("set paper type");
    let (cmd, chan) = backend::Command::without_response(
        command::Command::new_host(HostCommand::GetSetPrintPaperType)
            .package(ps.paper_vec(), false),
    );
    b.push(cmd).await?;
    chan.await?;
    println!("set darkness");
    let (cmd, chan) = backend::Command::without_response(
        command::Command::new_host(HostCommand::GetSetPrintDarkness)
            .package(ps.darkness_vec(), false),
    );
    b.push(cmd).await?;
    chan.await?;
    println!("set speed");
    let (cmd, chan) = backend::Command::without_response(
        command::Command::new_host(HostCommand::GetSetPrintSpeed).package(ps.speed_vec(), false),
    );
    b.push(cmd).await?;
    chan.await?;
    println!("set gap");
    let (cmd, chan) = backend::Command::without_response(
        command::Command::new_host(HostCommand::GetSetPrintPaperGap).package(ps.gap_vec(), false),
    );
    b.push(cmd).await?;
    chan.await?;
    println!("get status");
    let (cmd, chan) = backend::Command::with_response(
        command::Command::new_host(HostCommand::GetPrinterStatus).package(vec![], false),
    );
    b.push(cmd).await.ok();
    let resp = chan.await?.ok_or(anyhow::anyhow!("get status error"))?;
    println!("status: {:?}", resp.await?.get_command());
    println!("enable high command");
    let (cmd, chan) = backend::Command::with_response(
        command::Command::new_host(HostCommand::EnableHighCommand).package(vec![0x7f], false),
    );
    b.push(cmd).await?;
    let resp = chan
        .await?
        .ok_or(anyhow::anyhow!("enable high command error"))?;
    println!("enable high command: {:?}", resp.await?.get_command());
    println!("reset printer");
    let (cmd, chan) = backend::Command::without_response(
        PrintCommand::ResetPrinter
            .parse()
            .unwrap()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
    );
    b.push(cmd).await?;
    chan.await?;
    println!("printing");
    let mut errored = false;
    for c in parser {
        if let Some(c) = c.parse() {
            for c in c {
                let (cmd, _ch) = backend::Command::without_response(c);
                b.push(cmd).await?;
                // 这是相当保守且恐怖的，我下次应该等待整个buffer变空? 我不知道
                // ch.await?;
            }
        } else {
            let (cmd1, chan1) = backend::Command::with_response(
                command::Command::new_host(HostCommand::GetPrinterStatus).package(vec![], false),
            );
            b.push(cmd1).await.ok();
            let chan = chan1.await?;
            if let Some(chan) = chan {
                let resp = chan.await;
                let resp = if let Ok(resp) = resp {
                    resp
                } else {
                    // 如果收不到东西，那一定是打印机 buffer 炸了
                    // 这是打印机固件写的烂，我没什么好办法
                    println!("receive error");
                    errored = true;
                    break;
                };
                let stat = resp.get_payload()[0];
                println!("status: {:?}", stat);
                if stat == 35 {
                    println!("no paper");
                    errored = true;
                    break;
                }
            } else {
                println!("print failed");
                errored = true;
                break;
            }
        }
    }
    if errored {
        println!("reset device...");
        let (cmd, chan) = backend::Command::reset();
        b.push(cmd).await?;
        chan.await?;
        let (cmd1, chan1) = backend::Command::with_response(
            command::Command::new_host(HostCommand::GetPrinterStatus).package(vec![], false),
        );
        b.push(cmd1).await?;
        let chan = chan1.await?;
        if let Some(chan) = chan {
            let resp = chan.await;
            let resp = if let Ok(resp) = resp {
                resp
            } else {
                unreachable!("炸了炸了，但我不知道怎么修")
            };
            let stat = resp.get_payload()[0];
            println!("status: {:?}", stat);
            if stat == 35 {
                println!("no paper");
            }
        } else {
            println!("print failed");
        }
        panic!("print errored");
    }
    // println!("feed 2 lines");
    // let (cmd, _) = backend::Command::without_response(
    //     PrintCommand::FeedLines(2)
    //         .parse()
    //         .unwrap()
    //         .into_iter()
    //         .flatten()
    //         .collect::<Vec<_>>(),
    // );
    // b.push(cmd).await?;
    println!("next paper");
    let (cmd, _) = backend::Command::without_response(
        PrintCommand::NextPaper
            .parse()
            .unwrap()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
    );
    b.push(cmd).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    Ok(())
}

/// 我的[世界](typst::World)
struct Minecraft {
    fontbook: LazyHash<FontBook>,
    library: LazyHash<Library>,
    main_fileid: FileId,
    main_content: String,
    root_path: PathBuf,
}

impl Minecraft {
    fn new(main_file_path: &Path, main_content: String) -> Self {
        let fontbook = LazyHash::new(make_fontbook());
        let library = LazyHash::new(make_library());
        let root_path = main_file_path
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        let root_path = root_path.canonicalize().unwrap_or(root_path);
        let main_filename = main_file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let vpath = VirtualPath::new(format!("/{}", main_filename));
        let main_fileid = FileId::new_fake(vpath);
        Self {
            fontbook,
            library,
            main_fileid,
            main_content,
            root_path,
        }
    }

    fn resolve_path(&self, vpath: &VirtualPath) -> FileResult<PathBuf> {
        let path = vpath
            .resolve(&self.root_path)
            .ok_or(FileError::AccessDenied)?;
        if !path.starts_with(&self.root_path) {
            return Err(FileError::AccessDenied);
        }
        Ok(path)
    }
}

impl World for Minecraft {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.fontbook
    }

    fn main(&self) -> FileId {
        self.main_fileid
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main_fileid {
            return Ok(Source::new(id, self.main_content.clone()));
        }
        let path = self.resolve_path(id.vpath())?;
        let content = std::fs::read_to_string(&path).map_err(|e| FileError::from_io(e, &path))?;
        Ok(Source::new(id, content))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let path = self.resolve_path(id.vpath())?;
        let content = std::fs::read(&path).map_err(|e| FileError::from_io(e, &path))?;
        Ok(Bytes::new(content))
    }

    fn font(&self, index: usize) -> Option<Font> {
        // 需要优化一下?
        let font_unifont_bin = include_bytes!("../asset/unifont-16.0.04.ttf");
        let font_unifont = Font::new(Bytes::new(font_unifont_bin), 0);
        let font_unifontex_bin = include_bytes!("../asset/UnifontExMono.ttf");
        let font_unifontex = Font::new(Bytes::new(font_unifontex_bin), 0);
        match index {
            0 => font_unifont,
            1 => font_unifontex,
            _ => None,
        }
    }

    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        let _now = Local::now();
        // todo
        None
    }
}

#[derive(thiserror::Error, Debug)]
enum PrintSettingError {
    #[error("Invalid value `{0}`")]
    InvalidU8(u8),
    #[error("Invalid value `{0}`")]
    InvalidU16(u16),
    #[error("Invalid string `{0}`")]
    InvalidString(String),
}

#[derive(Debug, Clone, Copy, Default)]
enum SpeedSetting {
    /// 最慢(1)
    Min,
    /// 稍慢(2)
    Speed1,
    /// 正常(3)
    #[default]
    Normal,
    /// 稍快(4)
    Speed3,
    /// 最快(5)
    Max,
}

impl TryFrom<u8> for SpeedSetting {
    type Error = PrintSettingError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Min),
            2 => Ok(Self::Speed1),
            3 => Ok(Self::Normal),
            4 => Ok(Self::Speed3),
            5 => Ok(Self::Max),
            _ => Err(Self::Error::InvalidU8(value)),
        }
    }
}

impl TryFrom<&str> for SpeedSetting {
    type Error = PrintSettingError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "min" | "1" => Ok(Self::Min),
            "2" => Ok(Self::Speed1),
            "normal" | "3" => Ok(Self::Normal),
            "4" => Ok(Self::Speed3),
            "max" | "5" => Ok(Self::Max),
            _ => Err(Self::Error::InvalidString(value.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
enum DarknessSetting {
    /// 最浅(1)
    Min,
    Darkness1,
    Darkness2,
    Darkness3,
    Darkness4,
    /// 正常(6)
    #[default]
    Normal,
    Darkness6,
    Darkness7,
    Darkness8,
    Darkness9,
    Darkness10,
    Darkness11,
    Darkness12,
    Darkness13,
    /// 最深(15)
    Max,
}

impl TryFrom<u8> for DarknessSetting {
    type Error = PrintSettingError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Min),
            2 => Ok(Self::Darkness1),
            3 => Ok(Self::Darkness2),
            4 => Ok(Self::Darkness3),
            5 => Ok(Self::Darkness4),
            6 => Ok(Self::Normal),
            7 => Ok(Self::Darkness6),
            8 => Ok(Self::Darkness7),
            9 => Ok(Self::Darkness8),
            10 => Ok(Self::Darkness9),
            11 => Ok(Self::Darkness10),
            12 => Ok(Self::Darkness11),
            13 => Ok(Self::Darkness12),
            14 => Ok(Self::Darkness13),
            15 => Ok(Self::Max),
            _ => Err(Self::Error::InvalidU8(value)),
        }
    }
}

impl TryFrom<&str> for DarknessSetting {
    type Error = PrintSettingError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "min" | "1" => Ok(Self::Min),
            "2" => Ok(Self::Darkness1),
            "3" => Ok(Self::Darkness2),
            "4" => Ok(Self::Darkness3),
            "5" => Ok(Self::Darkness4),
            "normal" | "6" => Ok(Self::Normal),
            "7" => Ok(Self::Darkness6),
            "8" => Ok(Self::Darkness7),
            "9" => Ok(Self::Darkness8),
            "10" => Ok(Self::Darkness9),
            "11" => Ok(Self::Darkness10),
            "12" => Ok(Self::Darkness11),
            "13" => Ok(Self::Darkness12),
            "14" => Ok(Self::Darkness13),
            "max" | "15" => Ok(Self::Max),
            _ => Err(Self::Error::InvalidString(value.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
enum PaperSetting {
    /// 小票纸
    #[default]
    Ticket = 0,
    /// 不干胶
    Adhesive = 2,
    /// 卡纸
    CardPaper = 3,
    /// 透明贴
    Transparent = 4,
}

impl TryFrom<&str> for PaperSetting {
    type Error = PrintSettingError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "ticket" => Ok(Self::Ticket),
            "adhesive" => Ok(Self::Adhesive),
            "cardpaper" => Ok(Self::CardPaper),
            "transparent" => Ok(Self::Transparent),
            _ => Err(Self::Error::InvalidString(value.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct GapSetting(u16);

impl Default for GapSetting {
    fn default() -> Self {
        Self(50)
    }
}

impl TryFrom<u16> for GapSetting {
    type Error = PrintSettingError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value < 50 {
            Err(Self::Error::InvalidU16(value))
        } else {
            Ok(Self(value))
        }
    }
}

impl TryFrom<&str> for GapSetting {
    type Error = PrintSettingError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let lower = value.trim().to_lowercase();
        let num_str = lower.strip_suffix("mm").unwrap_or(&lower).trim();
        let val_f: f64 = num_str
            .parse()
            .map_err(|_| PrintSettingError::InvalidString(value.to_string()))?;
        let val_rounded = (val_f * 100.0).round();
        if val_rounded < 0.0 || val_rounded > u16::MAX as f64 {
            return Err(PrintSettingError::InvalidU16(val_rounded as u16));
        }
        let val_u16 = val_rounded as u16;
        Self::try_from(val_u16)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct PrintSettings {
    paper: PaperSetting,
    darkness: DarknessSetting,
    speed: SpeedSetting,
    gap: GapSetting,
}

impl PrintSettings {
    fn bp(&self) -> u32 {
        let basic_bp = match self.speed {
            SpeedSetting::Min => 50,
            SpeedSetting::Speed1 => 75,
            SpeedSetting::Normal => 100,
            SpeedSetting::Speed3 => 110,
            SpeedSetting::Max => 120,
        };
        let normal_darkness = 5;
        let step = 3;
        let adjustment = (normal_darkness - self.darkness as i32) * step;
        let final_bp = basic_bp as i32 + adjustment;
        final_bp.max(0) as u32
    }

    fn paper_vec(&self) -> Vec<u8> {
        let v = self.paper as u8;
        v.to_be_bytes().to_vec()
    }

    fn speed_vec(&self) -> Vec<u8> {
        let v = self.speed as u8;
        v.to_be_bytes().to_vec()
    }

    fn darkness_vec(&self) -> Vec<u8> {
        let v = self.darkness as u8;
        v.to_be_bytes().to_vec()
    }

    fn gap_vec(&self) -> Vec<u8> {
        let v = self.gap.0;
        v.to_be_bytes().to_vec()
    }
}

fn make_library() -> Library {
    let mut library = Library::builder().build();
    #[allow(unused_variables)]
    let scope = library.global.scope_mut();
    // scope.define_func::<QrCodeFunc>();
    library
}

pub struct QrCodeFunc {}

impl NativeFunc for QrCodeFunc {
    #[allow(unreachable_code, unused_variables)]
    fn data() -> &'static NativeFuncData {
        let data = NativeFuncData {
            function: typst_library::foundations::NativeFuncPtr(&Self::f),
            name: todo!(),
            title: todo!(),
            docs: todo!(),
            keywords: todo!(),
            contextual: todo!(),
            scope: todo!(),
            params: todo!(),
            returns: todo!(),
        };
        todo!();
        // &data
    }
}

// Fn(&mut Engine, Tracked<Context>, &mut Args) -> SourceResult<Value> + Send + Sync;

impl QrCodeFunc {
    #[allow(unreachable_code, unused_variables)]
    pub fn f(
        engine: &mut typst_library::engine::Engine,
        context: typst::comemo::Tracked<typst_library::foundations::Context>,
        args: &mut typst_library::foundations::Args,
    ) -> typst_library::diag::SourceResult<typst_library::foundations::Value> {
        todo!()
    }
}

fn make_fontbook() -> FontBook {
    let mut fb = FontBook::new();
    let font_unifont_bin = include_bytes!("../asset/unifont-16.0.04.ttf");
    let mut font_unifont_info = FontInfo::new(font_unifont_bin, 0).unwrap();
    font_unifont_info.family = "Unifont".to_string();
    fb.push(font_unifont_info);
    let font_unifontex_bin = include_bytes!("../asset/UnifontExMono.ttf");
    let mut font_unifontex_info = FontInfo::new(font_unifontex_bin, 0).unwrap();
    font_unifontex_info.family = "UnifontExMono".to_string();
    fb.push(font_unifontex_info);
    fb
}
