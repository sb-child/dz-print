// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印机连上之后的公共逻辑：查询设备信息、逐页打印。
//!
//! 只有打印头点数是必需的（渲染需要它）；设备名、版本、DPI、纸宽都是
//! 展示信息，查询失败只 `warn!`，不影响打印。

use super::PrintSettings;
use dz_print::{
    bitmap::Bitmap,
    command::{req, resp, resp::RespTrait},
    frontend::{Device, FrontendError, PrintJobOptions, RecvError},
    planner::print::{MAX_DOTS_PER_LINE, RleMode},
};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// 可选的设备信息查询最多等这么久。
///
/// 这些命令和打印头点数在同一次 batch 里发出，正常情况下回包已经排在
/// 队列里；等这么久还没有，说明固件不支持该命令。
const OPTIONAL_INFO_TIMEOUT: Duration = Duration::from_millis(500);

/// 连上打印机后一次性读到的设备信息。
#[derive(Debug, Clone)]
pub struct PrinterInfo {
    pub device_name: Option<Box<str>>,
    pub hardware_version: Option<resp::hardware_version::HardwareVersion>,
    pub software_version: Option<Box<str>>,
    pub dpi: Option<u16>,
    /// 打印头点数。
    pub head_dots: u16,
    /// 支持的最大纸宽，单位 `0.1mm`。
    pub paper_width: Option<u16>,
}

impl PrinterInfo {
    /// 纸宽，单位 mm。
    pub fn paper_width_mm(&self) -> Option<f32> {
        self.paper_width.map(|width| f32::from(width) / 10.0)
    }

    /// 用一条 `info!` 输出全部信息。
    pub fn log_connected(&self) {
        let device = self.device_name.as_deref().unwrap_or("?");
        let hardware = self
            .hardware_version
            .as_ref()
            .map_or_else(|| "?".to_owned(), ToString::to_string);
        let software = self.software_version.as_deref().unwrap_or("?");
        let paper = self
            .paper_width_mm()
            .map_or_else(|| "?".to_owned(), |width| format!("{width:.1}"));
        let dpi = self
            .dpi
            .map_or_else(|| "?".to_owned(), |dpi| dpi.to_string());

        info!(
            device_name = device,
            hardware_version = hardware,
            software_version = software,
            paper_width_mm = paper,
            dpi = dpi,
            head_dots = self.head_dots,
            "printer connected"
        );
    }
}

/// 查询设备信息；只有打印头点数查询失败会返回错误。
pub async fn query_info(dev: &Device, timeout: Duration) -> anyhow::Result<PrinterInfo> {
    dev.try_send_batch(&[
        &req::device_name::DeviceName {},
        &req::hardware_version::HardwareVersion {},
        &req::software_version::SoftwareVersion {},
        &req::dpi::Dpi {},
        &req::print_width::PrintWidth {},
    ])
    .await?;

    let width =
        match recv_optional::<resp::print_width::PrintWidth>(dev, resp::print_width::CMD, timeout)
            .await
        {
            Some(width) if width.print_width == 0 => {
                anyhow::bail!("设备回报的打印头点数为 0, 无法确定缩放宽度")
            }
            Some(width) if usize::from(width.print_width) > MAX_DOTS_PER_LINE => {
                anyhow::bail!(
                    "打印头点数 {} 超过协议上限 {MAX_DOTS_PER_LINE}",
                    width.print_width
                )
            }
            Some(width) => width,
            None => {
                return Err(FrontendError::Recv {
                    source: RecvError::Timeout { timeout },
                }
                .into());
            }
        };

    let device_name = recv_optional::<resp::device_name::DeviceName>(
        dev,
        resp::device_name::CMD,
        OPTIONAL_INFO_TIMEOUT,
    )
    .await
    .map(|name| name.device_name);

    let hardware_version = recv_optional::<resp::hardware_version::HardwareVersion>(
        dev,
        resp::hardware_version::CMD,
        OPTIONAL_INFO_TIMEOUT,
    )
    .await;

    let software_version = recv_optional::<resp::software_version::SoftwareVersion>(
        dev,
        resp::software_version::CMD,
        OPTIONAL_INFO_TIMEOUT,
    )
    .await
    .map(|version| version.software_version);

    let dpi = recv_optional::<resp::dpi::Dpi>(dev, resp::dpi::CMD, OPTIONAL_INFO_TIMEOUT)
        .await
        .map(|dpi| dpi.dpi);

    Ok(PrinterInfo {
        device_name,
        hardware_version,
        software_version,
        dpi,
        head_dots: width.print_width,
        paper_width: Some(width.paper_width),
    })
}

/// 读一条可选响应；超时或解析失败都返回 `None`，不阻塞连接。
async fn recv_optional<T: RespTrait>(dev: &Device, cmd: u8, timeout: Duration) -> Option<T> {
    match dev.recv_cmd_timeout::<T>(cmd, timeout).await {
        Ok(value) => value,
        Err(error) => {
            warn!(
                command = format_args!("{cmd:#04x}"),
                %error,
                "query device info failed"
            );
            None
        }
    }
}

/// 一页已经渲染好的位图，以及这一页需要的打印参数。
#[derive(Debug)]
pub struct PrintPage {
    pub bitmap: Bitmap,
    pub rle: RleMode,
    pub options: PrintJobOptions,
    /// 这一页生效的 gap / darkness / speed；和上一页不同时重发。
    pub settings: PrintSettings,
}

/// 逐页打印：按需重发设置、支持重复、可以被 `cancel` 打断。
///
/// `label` 只用于日志，通常是 job id 或文档名。`sent_settings` 由调用方在
/// 多次任务之间保留，避免每页都重发同一组设置。
// dzserver 的多页队列用这个入口；dzprint_image / dzprint_typst 只打印一页，
// 直接调用 `print_page`，所以在它们的构建里 `print_pages` 是 dead_code。
#[allow(dead_code)]
pub async fn print_pages(
    dev: &Device,
    label: &str,
    pages: &[PrintPage],
    repeats: u32,
    sent_settings: &mut Option<PrintSettings>,
    cancel: &CancellationToken,
) -> Result<(), PagePrintError> {
    for repeat in 0..=repeats {
        for (index, page) in pages.iter().enumerate() {
            print_page(dev, label, page, index + 1, repeat, sent_settings, cancel).await?;
        }
    }

    Ok(())
}

/// 打印一页：按需重发设置，然后等这一页打完。
///
/// 单页渲染（例如逐页编译的 Typst）可以直接调用它，不必先把所有页都渲染
/// 到内存里。
#[allow(clippy::too_many_arguments)]
pub async fn print_page(
    dev: &Device,
    label: &str,
    page: &PrintPage,
    page_no: usize,
    repeat: u32,
    sent_settings: &mut Option<PrintSettings>,
    cancel: &CancellationToken,
) -> Result<(), PagePrintError> {
    if cancel.is_cancelled() {
        return Err(PagePrintError::Cancelled);
    }

    if sent_settings.as_ref() != Some(&page.settings) {
        let send = page.settings.send(dev);
        tokio::select! {
            _ = cancel.cancelled() => return Err(PagePrintError::Cancelled),
            result = send => result.map_err(PagePrintError::Device)?,
        }
        *sent_settings = Some(page.settings.clone());
    }

    let print = dev.print_bitmap_job_with(&page.bitmap, page.rle, page.options);
    let result = tokio::select! {
        _ = cancel.cancelled() => return Err(PagePrintError::Cancelled),
        result = print => result,
    };
    let report = result.map_err(PagePrintError::Device)?;

    info!(
        job = label,
        repeat = repeat + 1,
        page = page_no,
        chunks = report.chunks,
        bytes = report.bytes,
        "page printed"
    );
    Ok(())
}

/// [`print_pages`] 的失败原因。
#[derive(Debug)]
pub enum PagePrintError {
    Cancelled,
    Device(FrontendError),
}

impl std::fmt::Display for PagePrintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "print cancelled"),
            Self::Device(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for PagePrintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cancelled => None,
            Self::Device(error) => Some(error),
        }
    }
}
