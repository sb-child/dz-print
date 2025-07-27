use std::env;

use chrono::Local;
use dz_print::{
    backend,
    command::{self, HostCommand},
    image_proc::{
        cmd_parser::{BitmapParser, PrintCommand},
        Bitmap,
    },
};
use tiny_skia::Pixmap;
use typst::{
    diag::{FileError, FileResult},
    foundations::{Bytes, Datetime},
    layout::PagedDocument,
    syntax::{FileId, Source, VirtualPath},
    text::{Font, FontBook, FontInfo},
    utils::LazyHash,
    Library, World,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    main_fn().await
}

async fn main_fn() -> anyhow::Result<()> {
    println!("dz-print for typst");
    let file_name = env::args()
        .nth(1)
        .ok_or(anyhow::anyhow!("please specify a filename"))?;
    println!("reading file");
    let file_content = tokio::fs::read_to_string(file_name).await?;
    println!("creating world");
    let world = Minecraft::new(file_content);
    println!("compiling document");
    let doc = typst::compile::<PagedDocument>(&world);
    for w in doc.warnings {
        println!("warning: {w:?}");
    }
    let doc = doc
        .output
        .map_err(|e| anyhow::anyhow!("compile error: {e:?}"))?;
    println!("connecting to printer");
    let b = backend::USBBackend::new(backend::USBSelector::DeviceSerial(
        "DP27P-Y4094C023".to_string(),
    ))
    .await?;
    for p in doc.pages {
        println!("rendering page {}", p.number);
        // 576px = 48mm
        let r = typst_render::render(&p, 576.0 / (2.8346456693 * 48.0));
        print_page(&b, r).await?;
    }
    Ok(())
}

async fn print_page(b: &backend::USBBackend, pm: Pixmap) -> anyhow::Result<()> {
    assert_eq!(
        pm.width(),
        576,
        "please ensure your page width is 576px or 48mm"
    );
    println!("converting to bitmap");
    let bitmap = Bitmap::from_pixmap(&pm);
    let parser = BitmapParser::new(bitmap, 120);
    println!("set paper type");
    let (cmd, chan) = backend::Command::without_response(
        command::Command::new_host(HostCommand::GetSetPrintPaperType).package(vec![0x00], false),
    );
    b.push(cmd).await?;
    chan.await?;
    println!("set darkness");
    let (cmd, chan) = backend::Command::without_response(
        command::Command::new_host(HostCommand::GetSetPrintDarkness).package(vec![0x05], false),
    );
    b.push(cmd).await?;
    chan.await?;
    println!("set speed");
    let (cmd, chan) = backend::Command::without_response(
        command::Command::new_host(HostCommand::GetSetPrintSpeed).package(vec![0x02], false),
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
                unreachable!()
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
}

impl Minecraft {
    fn new(main_content: String) -> Self {
        let fontbook = LazyHash::new(make_fontbook());
        let library = LazyHash::new(make_library());
        let main_fileid = FileId::new_fake(VirtualPath::new("/main.typ"));
        Self {
            fontbook,
            library,
            main_fileid,
            main_content,
        }
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
            Ok(Source::new(id, self.main_content.clone()))
        } else {
            Err(FileError::AccessDenied)
        }
    }

    fn file(&self, _id: FileId) -> FileResult<Bytes> {
        // todo
        Err(FileError::AccessDenied)
    }

    fn font(&self, index: usize) -> Option<Font> {
        // 需要优化一下?
        let font_unifont_bin = include_bytes!("../asset/unifont-16.0.04.ttf");
        let font_unifont = Font::new(Bytes::new(font_unifont_bin), 0);
        match index {
            0 => font_unifont,
            _ => None,
        }
    }

    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        let _now = Local::now();
        // todo
        None
    }
}

fn make_library() -> Library {
    Library::builder().build()
}

fn make_fontbook() -> FontBook {
    let mut fb = FontBook::new();
    let font_unifont = include_bytes!("../asset/unifont-16.0.04.ttf");
    let font_unifont_info = FontInfo::new(font_unifont, 0).unwrap();
    fb.push(font_unifont_info);
    fb
}
