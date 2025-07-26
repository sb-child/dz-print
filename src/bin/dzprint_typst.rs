use std::env;

use chrono::Local;
use dz_print::image_proc::Bitmap;
use tiny_skia::Pixmap;
use typst::{
    diag::{FileError, FileResult},
    foundations::{Bytes, Datetime},
    layout::PagedDocument,
    syntax::{FileId, Source, VirtualPath},
    text::{Font, FontBook, FontInfo},
    utils::LazyHash,
    Document, Library, World,
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
    let doc = doc.output.map_err(|e| anyhow::anyhow!("error: {e:?}"))?;
    for p in doc.pages {
        println!("rendering page {}", p.number);
        // 576px = 48mm
        let r = typst_render::render(&p, 576.0 / (2.8346456693 * 48.0));
        print_page(r).await;
    }
    Ok(())
}

async fn print_page(pm: Pixmap) {
    assert_eq!(
        pm.width(),
        576,
        "please ensure your page width is 576px or 48mm"
    );

    let bitmap = Bitmap::from_pixmap(&pm);

    // todo
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

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Err(FileError::AccessDenied)
    }

    fn font(&self, index: usize) -> Option<Font> {
        let font_unifont_bin = include_bytes!("../asset/unifont-16.0.04.ttf");
        let font_unifont = Font::new(Bytes::new(font_unifont_bin), 0);
        match index {
            0 => font_unifont,
            _ => None,
        }
    }

    fn today(&self, offset: Option<i64>) -> Option<Datetime> {
        let now = Local::now();
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
