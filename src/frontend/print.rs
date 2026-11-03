// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 位图打印任务：分页、流控、等待打印完成。
//!
//! # 用法
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use dz_print::{
//!     backend::usb::UsbSelector,
//!     bitmap::Bitmap,
//!     frontend::{Device, DeviceConfig},
//!     planner::print::RleMode,
//! };
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let tm = mytm::TaskManager::new();
//! let device = Device::new(
//!     &tm,
//!     DeviceConfig {
//!         selector: UsbSelector::DeviceName("DP27P-Y4094C023".into()),
//!         ..Default::default()
//!     },
//! );
//! device.ready().await?;
//!
//! // 高位图会自动按 16383 行拆页
//! let mut bitmap = Bitmap::new(384, 2000)?;
//! bitmap.line_mut(0)?.fill(true);
//!
//! let report = device.print_bitmap_job(&bitmap, RleMode::Bits5XOnly).await?;
//! println!("{} chunks, {} bytes", report.chunks, report.bytes);
//!
//! device.shutdown(Duration::from_secs(3)).await?;
//! tm.close();
//! # Ok(())
//! # }
//! ```
//!
//! `print_bitmap_job` 在最后一个 `EndPage` 之后轮询 `0x70`，等打印头真正走完
//! 才返回；中途出错会带着 [`FrontendError`](super::FrontendError) 返回。
//! 如果只想要命令、不碰设备，用 [`crate::planner::print`]。

use crate::{
    bitmap::Bitmap,
    command::{
        req::{self, ReqTrait},
        resp,
        settings::GapTypeSetting,
    },
    planner::{
        flow::{ChunkDecision, FlowConfig, FlowError, FlowPlanner, FlowReport},
        print::{MAX_LINES_PER_PAGE, PrintError, PrintPage, RleMode, into_reqs, print_bitmap_page},
    },
};
use std::time::Duration;
use tracing::{debug, error, info, warn};

use super::{Device, FrontendError, RecvError};

/// [`Device::print_bitmap_job_with`] 的页面参数。
///
/// 默认是连续纸，和 [`Device::print_bitmap_job`] 的行为一致。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrintJobOptions {
    /// 每页 `StartPage` 里的纸张间隔类型。
    pub gap_type: GapTypeSetting,
    /// 纸张间隔长度，单位 `0.01mm`。
    ///
    /// 只在 `gap_type != GapTypeSetting::Continuous` 时生效；v2 要求
    /// `50..=4194303`。
    pub gap_length: u32,
}

impl Device {
    /// 用 [`FlowPlanner`] 打印一整张位图，纸张间隔按连续纸处理。
    ///
    /// 等价于 [`Self::print_bitmap_job_with`] + [`PrintJobOptions::default`]。
    pub async fn print_bitmap_job(
        &self,
        bm: &Bitmap,
        rle: RleMode,
    ) -> Result<FlowReport, FrontendError> {
        self.print_bitmap_job_with(bm, rle, PrintJobOptions::default())
            .await
    }

    /// 用 [`FlowPlanner`] 打印一整张位图，并指定纸张间隔。
    ///
    /// 高于 [`MAX_LINES_PER_PAGE`] 时自动拆页；最后一页的 `EndPage` 之后会等到
    /// 打印头真正走完再返回。同一时刻只允许一个打印任务，否则返回
    /// [`FrontendError::PrintBusy`]。
    ///
    /// 打印期间 `0x70` / `0x77` 的响应归这里所有，不要同时用 `try_recv_one` /
    /// `try_pop_raw` 从队头取帧。
    ///
    /// 只要卡住（读不到状态、整页超时、打印没开始、写命令没有确认），当前 job
    /// 一律作废：先重置一次连接并读回真实状态，再返回
    /// [`FrontendError::Stalled`]。读回正常状态也会照实报出来——那说明打印机
    /// 已经可以接下一个任务，只是这一页废了。
    pub async fn print_bitmap_job_with(
        &self,
        bm: &Bitmap,
        rle: RleMode,
        options: PrintJobOptions,
    ) -> Result<FlowReport, FrontendError> {
        let _job = self
            .print_lock
            .try_lock()
            .map_err(|_| FrontendError::PrintBusy)?;

        match self.print_pages(bm, rle, options).await {
            // 打印机卡住（沉默、总超时、打印没开始、worker 不响应）：job 已经
            // 作废，但连接要重置一次并把真实状态读回来。这样调用方既能知道
            // 原因，也能知道打印机现在是不是还能接下一个 job。
            Err(error) if is_stalled(&error) => Err(self.abort_stalled(error).await),
            result => result,
        }
    }

    /// 逐页打印，是 [`Device::print_bitmap_job_with`] 的主体。
    ///
    /// 这里只负责发命令和判断进度，不做任何卡住后的收尾。
    async fn print_pages(
        &self,
        bm: &Bitmap,
        rle: RleMode,
        options: PrintJobOptions,
    ) -> Result<FlowReport, FrontendError> {
        let page_count = bm.height().div_ceil(MAX_LINES_PER_PAGE);

        let mut total = FlowReport::default();
        let mut start = 0usize;
        let mut page_no = 0usize;
        while start < bm.height() {
            // 逐页生成命令，避免长图把所有页的命令同时留在内存里。
            let page_start = start;
            let page = print_bitmap_page(bm, start, rle, MAX_LINES_PER_PAGE)?;
            start += page.lines;
            page_no += 1;

            let page_idx = u16::try_from(page_no)
                .map_err(|_| FrontendError::TooManyPages { pages: page_no })?;
            if page_idx > 65534 {
                return Err(FrontendError::TooManyPages { pages: page_no });
            }
            // 失败时不要尝试补 `EndPage` 或续打：固件已经不在处理命令，补发的
            // 命令只会排在队尾。job 作废，收尾（重置连接、读回真实状态）由
            // `abort_stalled` 统一处理。
            let report = self
                .send_page_commands(bm, &page, page_start, page_idx, options)
                .await?;

            // 最后一页在 `EndPage` 之前抓一次 `0x73` 快照。之后如果一直看不到
            // `Printing` / `Rotating`，就用这个快照判断页面到底有没有打印。
            let baseline = if start >= bm.height() && self.config.print_verify {
                self.read_statistics(self.config.recv_timeout).await
            } else {
                None
            };

            self.try_send_batch(&[&req::end_page::EndPage {}]).await?;
            // `EndPage` 只是把数据交给固件；最后一页要等打印头真正走完，
            // 否则前台一退出，USB 句柄被释放，纸可能只走了一半。
            if start >= bm.height() {
                let status = self
                    .wait_for_print_done_with(baseline, self.config.print_done_timeout)
                    .await?;
                info!(
                    status = ?status.status,
                    page_key = status.page_key,
                    "print completed"
                );
            }

            total.chunks += report.chunks;
            total.bytes += report.bytes;
            total.budget_queries += report.budget_queries;
            total.degraded |= report.degraded;
            info!(
                page = page_no,
                pages = page_count,
                lines = page.lines,
                chunks = report.chunks,
                bytes = report.bytes,
                "page finished"
            );
        }
        Ok(total)
    }

    /// 在 `0x77` 已经超时的情况下确认一次 `0x70`。
    ///
    /// 如果状态也不回，说明打印机已经停止处理命令（开盖、缺纸或固件卡住）：
    /// 返回沉默，由 [`Device::print_bitmap_job_with`] 重置连接并读回状态。
    async fn check_after_budget_timeout(&self) -> Result<(), FrontendError> {
        match self.query_status(self.config.recv_timeout).await? {
            Some(status) if status.is_error() => {
                error!(
                    status = ?status,
                    reason = %status,
                    "printer reported an error during the print job"
                );
                Err(FrontendError::PrinterStatus { status })
            }
            Some(_) => Ok(()),
            None => {
                warn!("status also timed out after buffer timeout");
                Err(FrontendError::PrinterSilent {
                    polls: 1,
                    timeout: self.config.recv_timeout,
                })
            }
        }
    }

    /// 发完一页的 `StartPage` / 数据 / 加热统计，但不发 `EndPage`。
    ///
    /// 失败时直接返回，job 作废；调用方不要再补 `EndPage` 或续打，收尾走
    /// [`Device::abort_stalled`]。
    async fn send_page_commands(
        &self,
        bm: &Bitmap,
        page: &PrintPage,
        page_start: usize,
        page_idx: u16,
        options: PrintJobOptions,
    ) -> Result<FlowReport, FrontendError> {
        let width = u16::try_from(bm.width())
            .map_err(|_| FrontendError::WidthOverflow { width: bm.width() })?;
        let lines = u16::try_from(page.lines)
            .map_err(|_| FrontendError::PageTooTall { lines: page.lines })?;

        self.try_send_batch(&[
            &req::start_page::StartPage {
                page_idx,
                print_sep_line: false,
                data_pixel_width: width,
                printer_gap_type: options.gap_type,
                printer_gap_length: options.gap_length,
            },
            &req::total_lines::TotalLines { lines },
        ])
        .await?;

        let reqs = into_reqs(&page.commands);
        let refs: Vec<&dyn ReqTrait> = reqs.iter().map(|c| c.as_ref()).collect();
        // 预算查询和整页发送都受设备配置约束，不要用 planner 里写死的
        // 5 秒 / 60 秒：用户把 `--recv-timeout` 调到 500ms 时，开盖这类
        // 错误应该尽快报出来，而不是先等两个 5 秒超时。
        let mut flow = FlowPlanner::new(FlowConfig {
            budget_timeout: self.config.recv_timeout.max(Duration::from_millis(1)),
            job_timeout: self.config.print_done_timeout.max(Duration::from_millis(1)),
            ..FlowConfig::default()
        });
        flow.push_commands(&self.assembler, &refs)?;
        // 开盖 / 缺纸时固件会直接停止回应所有查询。上一轮预算查询里 `0x77`
        // 已经超时、这次连 `0x70` 也不回时，说明不能继续灌数据了：退化模式
        // 下每块之后都会调用 check_after_budget_timeout 复位连接。
        let mut budget_timeout_seen = false;

        loop {
            match flow.step() {
                ChunkDecision::Send(chunk) => {
                    self.send_cmd_area(chunk).await?;
                    if flow.needs_gap() {
                        tokio::time::sleep(flow.config().chunk_gap).await;
                    }
                    flow.check_deadline()?;
                    // 退化模式不会再回到 QueryBudget；每块之后补一次状态
                    // 查询，确认 `0x77` 超时不是固件已经被堵住的信号。
                    if budget_timeout_seen {
                        self.check_after_budget_timeout().await?;
                    }
                }
                ChunkDecision::QueryBudget => {
                    // 设备长时间不给预算时也要受整任务超时约束，
                    // 否则 `size = 0` 会让这个分支无限循环。
                    flow.check_deadline()?;
                    tokio::time::sleep(flow.config().chunk_gap).await;
                    self.try_send_batch(&[
                        &req::status::Status {},
                        &req::buffer_size::BufferSize {},
                    ])
                    .await?;
                    // 两条查询的应答按命令字节分别取，顺序和中间夹的推送都不影响。
                    // 两条共用一个 deadline，最坏只等一个 budget_timeout。
                    let deadline = tokio::time::Instant::now() + flow.config().budget_timeout;
                    let status_timeout =
                        deadline.saturating_duration_since(tokio::time::Instant::now());
                    let status = match self
                        .recv_cmd_timeout::<resp::status::Status>(resp::status::CMD, status_timeout)
                        .await
                    {
                        Ok(Some(status)) => {
                            debug!(
                                status = ?status.status,
                                page_key = status.page_key,
                                "status before budget"
                            );
                            Some(status)
                        }
                        Ok(None) => {
                            // 单次漏答不算异常，继续看这个 deadline 里 `0x77`
                            // 有没有回：两条一起超时才说明打印机停止处理命令，
                            // 退化模式会在每块之后调用 check_after_budget_timeout。
                            debug!(
                                timeout = ?self.config.recv_timeout,
                                "status before budget timed out"
                            );
                            None
                        }
                        Err(e @ RecvError::Closed) => return Err(e.into()),
                        Err(e) => {
                            debug!("status before budget failed: {e}");
                            None
                        }
                    };
                    // 无纸 / 开盖等错误状态必须立即中止，不要再多等一次
                    // `0x77` 的响应。
                    if let Some(status) = status
                        && status.status.is_error()
                    {
                        error!(
                            status = ?status.status,
                            reason = %status.status,
                            "printer reported an error during the print job"
                        );
                        return Err(FrontendError::PrinterStatus {
                            status: status.status,
                        });
                    }
                    let buffer_timeout =
                        deadline.saturating_duration_since(tokio::time::Instant::now());
                    match self
                        .recv_cmd_timeout::<resp::buffer_size::BufferSize>(
                            resp::buffer_size::CMD,
                            buffer_timeout,
                        )
                        .await
                    {
                        Ok(Some(bs)) => {
                            budget_timeout_seen = false;
                            debug!(
                                size = bs.size,
                                budget = bs.budget(),
                                status = bs.status,
                                "buffer size"
                            );
                            flow.on_buffer_size(&bs);
                        }
                        Ok(None) => {
                            warn!("buffer size timed out, fall back to 8 KiB");
                            budget_timeout_seen = true;
                            flow.on_budget_timeout();
                        }
                        Err(e @ RecvError::Closed) => return Err(e.into()),
                        Err(e) => {
                            warn!("buffer size failed, fall back to 8 KiB: {e}");
                            budget_timeout_seen = true;
                            flow.on_budget_timeout();
                        }
                    }
                }
                ChunkDecision::Wait => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                ChunkDecision::Done => break,
            }
        }

        // 厂商 SDK 在每个 band 的行命令发完、结束页之前发一次加热统计（`0x25`），
        // 让固件知道这批行有多黑。DP27P 上没观察到行为变化（加不加都一样快、
        // RLE 的空白也没消失），这里按厂商流程保留。
        let heat = page_heat_stats(bm, page_start, page.lines)?;
        debug!(
            max_dots = heat.max_dots,
            avg_dots = heat.avg_dots,
            page = page_idx,
            "heat control"
        );
        self.try_send_batch(&[&heat]).await?;

        Ok(flow.report())
    }
}

/// 这些错误意味着打印机卡住了，收尾时应当重置连接并读回状态。
///
/// 共同点都是“打印没法继续，而且打印机可能已经不再按预期工作”：读不到回包、
/// 整页超时、打印没开始、或者 worker 连一条写命令都确认不了。
fn is_stalled(error: &FrontendError) -> bool {
    match error {
        FrontendError::PrinterSilent { .. }
        | FrontendError::PrintTimeout { .. }
        | FrontendError::PrintDidNotStart { .. }
        | FrontendError::WorkerTimeout { .. } => true,
        FrontendError::Flow { source } => matches!(source, FlowError::Timeout { .. }),
        _ => false,
    }
}

/// 一页的加热统计：单行最大黑点数与平均黑点数。
///
/// 厂商 SDK 用这两个值（`0x25` 的 `MaxSfDots` / `MaxLEDots`）告诉固件这一批行
/// 有多黑，固件据此决定加热脉冲。DP27P 上没观察到它改变打印行为。
fn page_heat_stats(
    bm: &Bitmap,
    start: usize,
    lines: usize,
) -> Result<req::heat_control::HeatControl, PrintError> {
    let mut max_dots = 0u16;
    let mut sum = 0usize;
    for y in start..start + lines {
        let ones = bm.line(y)?.bits().count_ones();
        max_dots = max_dots.max(u16::try_from(ones).unwrap_or(u16::MAX));
        sum += ones;
    }
    let avg_dots = (sum / lines.max(1)) as u16;
    Ok(req::heat_control::HeatControl { max_dots, avg_dots })
}
