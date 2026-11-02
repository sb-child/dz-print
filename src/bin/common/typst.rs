// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Typst world、编译和渲染，`dzprint_typst` 和 `dzserver` 共用。
//!
//! 这里把文档的文件访问限制在 `root` 内：
//!
//! - Typst 的 `VirtualPath` 本身会把 `/` 当成 project root，`..` 逃逸直接报错；
//! - 解包只接受普通文件和目录，拒绝符号链接 / 硬链接；
//! - 读取前对解析出的真实路径做 `canonicalize`，再检查是否仍在 `root` 内。

// `dzprint_image` 也会编译 `common`，但只用到 image 部分。
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use anyhow::{Context as _, anyhow, bail, ensure};
use chrono::{Datelike as _, Local};
use dz_print::{bitmap::Bitmap, raster::DitherMode, raster::from_pixmap_over_white};
use tiny_skia::Pixmap;
use tracing::{info, warn};
use typst::{
    Library, LibraryExt as _, World,
    diag::{FileError, FileResult},
    foundations::{Bytes, Datetime},
    syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot, package::PackageSpec},
    text::{Font, FontBook},
    utils::LazyHash,
};
use typst_layout::PagedDocument;
use typst_render::RenderOptions;

/// Typst 包缓存和单 job 解包共用的解包上限。
pub const MAX_EXTRACT_BYTES: u64 = 64 * 1024 * 1024;

/// 内置字体 + 额外字体。
#[derive(Clone)]
pub struct FontSet {
    pub fonts: Vec<Font>,
    pub book: FontBook,
}

/// 一个文档的 Typst world。
pub struct TypstWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    main: FileId,
    main_source: String,
    root: PathBuf,
    fonts: Vec<Font>,
    package_cache: PathBuf,
}

impl TypstWorld {
    /// `root` 是文档目录，`main` 必须是 root 内的普通文件。
    pub fn new(
        root: impl AsRef<Path>,
        main: impl AsRef<Path>,
        fonts: Vec<Font>,
        book: FontBook,
        package_cache: impl AsRef<Path>,
    ) -> anyhow::Result<Self> {
        let root = root
            .as_ref()
            .canonicalize()
            .with_context(|| format!("找不到文档目录 {}", root.as_ref().display()))?;

        let main_path = main.as_ref();
        let meta = std::fs::symlink_metadata(main_path)
            .with_context(|| format!("找不到主文档 {}", main_path.display()))?;
        ensure!(
            meta.file_type().is_file(),
            "主文档 {} 必须是普通文件，不能是符号链接",
            main_path.display()
        );

        let main_path = main_path
            .canonicalize()
            .with_context(|| format!("解析主文档路径 {} 失败", main_path.display()))?;
        ensure!(
            main_path.starts_with(&root),
            "主文档 {} 不在根目录 {} 内",
            main_path.display(),
            root.display()
        );

        let main_source = std::fs::read_to_string(&main_path)
            .with_context(|| format!("读取主文档 {} 失败", main_path.display()))?;
        let filename = main_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let vpath = VirtualPath::new(format!("/{filename}"))
            .map_err(|error| anyhow!("Typst 路径 `/{filename}` 无效: {error:?}"))?;
        let main = FileId::unique(RootedPath::new(VirtualRoot::Project, vpath));

        let package_cache = package_cache.as_ref().to_path_buf();
        std::fs::create_dir_all(&package_cache)
            .with_context(|| format!("创建 Typst 包缓存目录 {} 失败", package_cache.display()))?;
        let package_cache = package_cache
            .canonicalize()
            .with_context(|| format!("解析包缓存目录 {} 失败", package_cache.display()))?;

        Ok(Self {
            library: LazyHash::new(Library::builder().build()),
            book: LazyHash::new(book),
            main,
            main_source,
            root,
            fonts,
            package_cache,
        })
    }

    fn resolve_id(&self, id: FileId) -> FileResult<PathBuf> {
        if id == self.main {
            return Err(FileError::Other(Some("main file is virtual".into())));
        }

        match id.root() {
            VirtualRoot::Project => resolve_under(&self.root, id.vpath()),
            VirtualRoot::Package(package) => {
                let dir = prepare_package(package, &self.package_cache)?;
                let dir = dir
                    .canonicalize()
                    .map_err(|error| FileError::from_io(error, &dir))?;
                if !dir.starts_with(&self.package_cache) {
                    return Err(FileError::AccessDenied);
                }
                resolve_under(&dir, id.vpath())
            }
        }
    }
}

fn resolve_under(root: &Path, vpath: &VirtualPath) -> FileResult<PathBuf> {
    let path = vpath.realize(root).map_err(FileError::Realize)?;
    let real = path
        .canonicalize()
        .map_err(|error| FileError::from_io(error, &path))?;
    if !real.starts_with(root) {
        return Err(FileError::AccessDenied);
    }
    Ok(real)
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

pub fn compile(world: &TypstWorld) -> anyhow::Result<PagedDocument> {
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

/// 由页面实际宽度算出 `pixel_per_pt`，让页面铺满打印头。
pub fn render_page(page: &typst_layout::Page, dots: u16) -> anyhow::Result<Pixmap> {
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
    // 四舍五入偶尔多出 1 个像素，超出打印头时退一点再渲染一次。
    if pixmap.width() > u32::from(dots) {
        pixmap = typst_render::render(page, &options((dots_f - 1.0) / width_pt));
    }
    Ok(pixmap)
}

/// 把 Typst 页面渲染成位图。
pub fn render_bitmap(
    page: &typst_layout::Page,
    dots: u16,
    dither: DitherMode,
) -> anyhow::Result<Bitmap> {
    let pixmap = render_page(page, dots)?;
    Ok(from_pixmap_over_white(&pixmap, dither)?)
}

/// 加载内置 Unifont 和 `extra` 指定的字体。
pub fn load_fonts(extra: &[PathBuf]) -> anyhow::Result<FontSet> {
    let mut fonts = Vec::new();
    let mut book = FontBook::new();

    for bytes in [
        &include_bytes!("../../asset/unifont-16.0.04.ttf")[..],
        &include_bytes!("../../asset/UnifontExMono.ttf")[..],
    ] {
        push_font(Bytes::new(bytes), &mut fonts, &mut book)?;
    }

    for path in extra {
        let data =
            std::fs::read(path).with_context(|| format!("读取字体 {} 失败", path.display()))?;
        push_font(Bytes::new(data), &mut fonts, &mut book)
            .with_context(|| format!("加载字体 {} 失败", path.display()))?;
    }

    Ok(FontSet { fonts, book })
}

fn push_font(bytes: Bytes, fonts: &mut Vec<Font>, book: &mut FontBook) -> anyhow::Result<()> {
    let font = Font::new(bytes, 0).ok_or_else(|| anyhow!("不是可用的字体文件"))?;
    book.push(font.info().clone());
    fonts.push(font);
    Ok(())
}

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

    // 并发下载同一个包时不能共用一个临时目录；用随机目录再原子 rename。
    let downloading = tempfile::Builder::new()
        .prefix(".downloading-")
        .tempdir_in(cache_dir)
        .map_err(|e| FileError::from_io(e, cache_dir))?;
    extract_archive(&mut archive, downloading.path(), MAX_EXTRACT_BYTES)
        .map_err(|e| FileError::Other(Some(format!("解压 {url} 失败: {e}").into())))?;

    let downloading = downloading.keep();
    match std::fs::rename(&downloading, &package_dir) {
        Ok(()) => {}
        Err(_error) if package_dir.exists() => {
            let _ = std::fs::remove_dir_all(&downloading);
        }
        Err(error) => return Err(FileError::from_io(error, &package_dir)),
    }
    Ok(package_dir)
}

/// 安全解包：只接受普通文件和目录，拒绝路径逃逸和符号链接。
pub fn extract_archive<R: std::io::Read>(
    archive: &mut tar::Archive<R>,
    root: &Path,
    max_bytes: u64,
) -> anyhow::Result<()> {
    let mut total = 0u64;

    for entry in archive.entries().context("读取 tar 条目失败")? {
        let mut entry = entry.context("读取 tar 条目失败")?;
        let path = entry.path().context("读取 tar 路径失败")?.into_owned();

        ensure!(
            path.is_relative(),
            "tar 条目 `{}` 是绝对路径",
            path.display()
        );
        ensure!(
            !path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
            "tar 条目 `{}` 包含 `..`",
            path.display()
        );

        let kind = entry.header().entry_type();
        ensure!(
            matches!(kind, tar::EntryType::Regular | tar::EntryType::Directory),
            "tar 条目 `{}` 的类型 {:?} 不受支持",
            path.display(),
            kind
        );

        total = total
            .checked_add(entry.header().size().context("读取 tar 条目大小失败")?)
            .context("tar 解包大小溢出")?;
        ensure!(total <= max_bytes, "tar 解包后超过 {} 字节", max_bytes);

        ensure!(
            entry.unpack_in(root).context("解包 tar 条目失败")?,
            "tar 条目 `{}` 试图逃出解包目录",
            path.display()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use typst::syntax::{RootedPath, VirtualPath, VirtualRoot};

    fn font_set() -> FontSet {
        load_fonts(&[]).unwrap()
    }

    #[test]
    #[cfg(unix)]
    fn symlink_cannot_escape_root() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("main.typ");
        std::fs::write(&main, "hello").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.path().join("evil")).unwrap();

        let cache = tempfile::tempdir().unwrap();
        let fonts = font_set();
        let world =
            TypstWorld::new(root.path(), &main, fonts.fonts, fonts.book, cache.path()).unwrap();

        let vpath = VirtualPath::new("/evil").unwrap();
        let id = FileId::unique(RootedPath::new(VirtualRoot::Project, vpath));
        assert!(matches!(world.file(id), Err(FileError::AccessDenied)));
    }

    #[test]
    fn extract_rejects_symlink_entries() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_cksum();
        builder
            .append_link(&mut header, "evil", "/etc/passwd")
            .unwrap();
        let data = builder.into_inner().unwrap();

        let root = tempfile::tempdir().unwrap();
        let mut archive = tar::Archive::new(std::io::Cursor::new(data));
        let error = extract_archive(&mut archive, root.path(), MAX_EXTRACT_BYTES).unwrap_err();
        assert!(error.to_string().contains("不受支持"), "{error}");
        assert!(!root.path().join("evil").exists());
    }

    #[test]
    fn extract_accepts_regular_files() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(5);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "main.typ", &b"hello"[..])
            .unwrap();
        let data = builder.into_inner().unwrap();

        let root = tempfile::tempdir().unwrap();
        let mut archive = tar::Archive::new(std::io::Cursor::new(data));
        extract_archive(&mut archive, root.path(), MAX_EXTRACT_BYTES).unwrap();
        assert_eq!(
            std::fs::read(root.path().join("main.typ")).unwrap(),
            b"hello"
        );
    }
}
