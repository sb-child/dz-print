// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 渲染队列的任务类型，以及 image / Typst 的阻塞渲染。
//!
//! 所有 CPU 和文件 IO 都在 `mytm::TaskManager::spawn_blocking_typed` 里做，
//! 结果通过 oneshot 回给异步 worker，避免把大位图再复制一份。

use crate::{
    common::{PrintArgs, PrintSettings, typst},
    job::JobId,
    printer::PrinterProfile,
};
use anyhow::{Context as _, anyhow, ensure};
use dz_print::{bitmap::Bitmap, frontend::PrintJobOptions, planner::print::RleMode};
use mytm::{TaskHandle, TaskManager};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use tracing::info;

/// 一页已经渲染好的位图，以及这一页需要的打印参数。
#[derive(Debug)]
pub struct RenderedPage {
    pub bitmap: Bitmap,
    pub rle: RleMode,
    pub options: PrintJobOptions,
    /// 这一页生效的 gap / darkness / speed；和上一页不同时打印 worker 重发。
    pub settings: PrintSettings,
}

#[derive(Debug)]
pub struct RenderedJob {
    pub id: JobId,
    pub pages: Vec<RenderedPage>,
    pub repeats: u32,
    pub cancel: CancellationToken,
}

pub enum JobSpec {
    Image {
        id: JobId,
        temp: TempDir,
        input: PathBuf,
        repeats: u32,
        cancel: CancellationToken,
    },
    Typst {
        id: JobId,
        temp: TempDir,
        tar: PathBuf,
        repeats: u32,
        cancel: CancellationToken,
    },
}

impl JobSpec {
    pub fn id(&self) -> JobId {
        match self {
            Self::Image { id, .. } | Self::Typst { id, .. } => *id,
        }
    }

    pub fn cancel(&self) -> &CancellationToken {
        match self {
            Self::Image { cancel, .. } | Self::Typst { cancel, .. } => cancel,
        }
    }

    /// 在 blocking task 里调用。
    pub fn render(
        self,
        profile: PrinterProfile,
        args: &PrintArgs,
        fonts: &typst::FontSet,
        package_cache: &Path,
        cancel: &CancellationToken,
    ) -> anyhow::Result<RenderedJob> {
        ensure!(!cancel.is_cancelled(), "job cancelled");

        match self {
            Self::Image {
                id,
                temp,
                input,
                repeats,
                cancel,
            } => {
                let pages = render_image(&input, profile.head_dots, args, cancel.clone())?;
                drop(temp);
                Ok(RenderedJob {
                    id,
                    pages,
                    repeats,
                    cancel,
                })
            }
            Self::Typst {
                id,
                temp,
                tar,
                repeats,
                cancel,
            } => {
                let pages = render_typst(
                    temp.path(),
                    &tar,
                    profile.head_dots,
                    args,
                    fonts,
                    package_cache,
                    cancel.clone(),
                )?;
                drop(temp);
                Ok(RenderedJob {
                    id,
                    pages,
                    repeats,
                    cancel,
                })
            }
        }
    }
}

fn render_image(
    input: &Path,
    head_dots: u16,
    args: &PrintArgs,
    cancel: CancellationToken,
) -> anyhow::Result<Vec<RenderedPage>> {
    ensure!(!cancel.is_cancelled(), "job cancelled");

    let img = crate::common::image::load_image(input)?;
    let (width, height) = (img.width(), img.height());
    let img = crate::common::image::fit_to_print_head(img, head_dots);
    info!(
        width,
        height,
        out_width = img.width(),
        out_height = img.height(),
        "image scaled"
    );

    let bitmap = crate::common::image::to_bitmap(&img, args.raster.into())?;
    Ok(vec![RenderedPage {
        bitmap,
        rle: args.rle.into(),
        options: args.job_options(),
        settings: PrintSettings::from(args),
    }])
}

#[allow(clippy::too_many_arguments)]
fn render_typst(
    temp_root: &Path,
    tar: &Path,
    head_dots: u16,
    args: &PrintArgs,
    fonts: &typst::FontSet,
    package_cache: &Path,
    cancel: CancellationToken,
) -> anyhow::Result<Vec<RenderedPage>> {
    ensure!(!cancel.is_cancelled(), "job cancelled");

    let doc_dir = temp_root.join("doc");
    std::fs::create_dir_all(&doc_dir)
        .with_context(|| format!("创建 {} 失败", doc_dir.display()))?;

    let file = File::open(tar).with_context(|| format!("打开 {} 失败", tar.display()))?;
    let mut archive = tar::Archive::new(file);
    typst::extract_archive(&mut archive, &doc_dir, typst::MAX_EXTRACT_BYTES)?;

    let main = doc_dir.join("main.typ");
    let world = typst::TypstWorld::new(
        &doc_dir,
        &main,
        fonts.fonts.clone(),
        fonts.book.clone(),
        package_cache,
    )?;
    let doc = typst::compile(&world)?;
    info!(pages = doc.pages().len(), "typst compiled");

    let overrides = typst::page_settings(&doc)?;
    let mut page_args = args.clone();
    let mut pages = Vec::with_capacity(doc.pages().len());
    for (index, page) in doc.pages().iter().enumerate() {
        ensure!(!cancel.is_cancelled(), "job cancelled");
        let page_no = index + 1;
        // 覆盖是粘性的：一直沿用到下一处覆盖。
        if let Some(settings) = overrides.get(&page_no) {
            info!(page = page_no, ?settings, "apply <print-settings>");
            settings.apply_to(&mut page_args);
        }

        let bitmap = typst::render_bitmap(page, head_dots, page_args.raster.into())?;
        info!(
            page = page_no,
            width = bitmap.width(),
            height = bitmap.height(),
            "typst page rendered"
        );
        pages.push(RenderedPage {
            bitmap,
            rle: page_args.rle.into(),
            options: page_args.job_options(),
            settings: PrintSettings::from(&page_args),
        });
    }

    Ok(pages)
}

/// 把一个阻塞闭包挂到 mytm 下，取消时立即返回，不等待阻塞线程。
pub async fn run_blocking<T, F>(
    tm: &TaskManager,
    token: &CancellationToken,
    f: F,
) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    ensure!(!token.is_cancelled(), "job cancelled");

    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle: TaskHandle<(), (), anyhow::Error> =
        tm.spawn_blocking_typed(move |_tm, _progress, _task_token| {
            let result = f();
            let _ = tx.send(result);
            Ok::<(), anyhow::Error>(())
        });

    tokio::select! {
        _ = token.cancelled() => {
            handle.cancel();
            Err(anyhow!("job cancelled"))
        }
        result = rx => result.map_err(|_| anyhow!("渲染任务没有返回结果"))?,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cli;
    use clap::Parser;
    use dz_print::command::settings::DarknessSetting;
    use dz_print::planner::print::RleMode;

    fn print_args() -> PrintArgs {
        Cli::try_parse_from(["dzserver", "/tmp", "127.0.0.1:0"])
            .unwrap()
            .print
    }

    fn fonts() -> typst::FontSet {
        typst::load_fonts(&[]).unwrap()
    }

    #[test]
    fn renders_an_image_job() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input.png");
        image::RgbImage::from_pixel(2, 2, image::Rgb([0, 0, 0]))
            .save(&input)
            .unwrap();

        let job = JobSpec::Image {
            id: [1; 16],
            temp,
            input,
            repeats: 0,
            cancel: CancellationToken::new(),
        };
        let cache = tempfile::tempdir().unwrap();
        let rendered = job
            .render(
                PrinterProfile { head_dots: 576 },
                &print_args(),
                &fonts(),
                cache.path(),
                &CancellationToken::new(),
            )
            .unwrap();

        assert_eq!(rendered.pages.len(), 1);
        assert_eq!(rendered.pages[0].bitmap.width(), 2);
    }

    #[test]
    fn renders_a_typst_job_from_tar() {
        let temp = tempfile::tempdir().unwrap();
        let tar_path = temp.path().join("input.tar");

        let mut builder = tar::Builder::new(Vec::new());
        let source = b"#set page(width: 20mm, height: 50mm)\nPage one\n                       #pagebreak()\n                       #metadata((darkness: 12, rle: \"bits6x\")) <print-settings>\n                       Page two\n                       #pagebreak()\n                       Page three";
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(source.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "main.typ", &source[..])
            .unwrap();
        std::fs::write(&tar_path, builder.into_inner().unwrap()).unwrap();

        let job = JobSpec::Typst {
            id: [2; 16],
            temp,
            tar: tar_path,
            repeats: 0,
            cancel: CancellationToken::new(),
        };
        let cache = tempfile::tempdir().unwrap();
        let rendered = job
            .render(
                PrinterProfile { head_dots: 576 },
                &print_args(),
                &fonts(),
                cache.path(),
                &CancellationToken::new(),
            )
            .unwrap();

        assert_eq!(rendered.pages.len(), 3);
        assert!(rendered.pages[0].bitmap.width() <= 576);
        assert!(rendered.pages[0].bitmap.width() > 0);

        // 第 2 页覆盖，第 3 页沿用第 2 页的设置。
        assert_eq!(rendered.pages[0].settings.darkness, None);
        assert_eq!(
            rendered.pages[1].settings.darkness,
            Some(DarknessSetting::Darkness11)
        );
        assert_eq!(rendered.pages[1].rle, RleMode::Bits6XOnly);
        assert_eq!(
            rendered.pages[2].settings.darkness,
            Some(DarknessSetting::Darkness11)
        );
        assert_eq!(rendered.pages[2].rle, RleMode::Bits6XOnly);
    }
}
