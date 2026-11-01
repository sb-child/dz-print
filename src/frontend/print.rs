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
        flow::{ChunkDecision, FlowPlanner, FlowReport},
        print::{MAX_LINES_PER_PAGE, RleMode, into_reqs, print_bitmap_page},
    },
};
use std::time::Duration;
use tracing::{debug, info, warn};

use super::{Device, FrontendError};

impl Device {
    /// 用 [`FlowPlanner`] 打印一整张位图。
    ///
    /// 位图高于 [`MAX_LINES_PER_PAGE`] 时自动拆成多页：每页发
    /// `StartPage` + `TotalLines`，再按预算分块发送行命令，最后 `EndPage`。
    /// 页与页之间沿用 `Continuous`，不额外走纸。预算不足时发 `0x70` + `0x77`，
    /// `0x77` 超时退化到固定 8 KiB。
    ///
    /// 最后一页的 `EndPage` 之后会调用 [`Device::wait_for_print_done`]，
    /// 等打印头真正走完再返回。
    pub async fn print_bitmap_job(
        &self,
        bm: &Bitmap,
        rle: RleMode,
    ) -> Result<FlowReport, FrontendError> {
        let width = u16::try_from(bm.width())
            .map_err(|_| FrontendError::WidthOverflow { width: bm.width() })?;
        let page_count = bm.height().div_ceil(MAX_LINES_PER_PAGE);

        let mut total = FlowReport::default();
        let mut start = 0usize;
        let mut page_no = 0usize;
        while start < bm.height() {
            // 逐页生成命令，避免长图把所有页的命令同时留在内存里。
            let page = print_bitmap_page(bm, start, rle, MAX_LINES_PER_PAGE)?;
            start += page.lines;
            page_no += 1;

            let page_idx = u16::try_from(page_no)
                .map_err(|_| FrontendError::TooManyPages { pages: page_no })?;
            if page_idx > 65534 {
                return Err(FrontendError::TooManyPages { pages: page_no });
            }
            let lines = u16::try_from(page.lines)
                .map_err(|_| FrontendError::PageTooTall { lines: page.lines })?;

            self.try_send_batch(&[
                &req::start_page::StartPage {
                    page_idx,
                    print_sep_line: false,
                    data_pixel_width: width,
                    printer_gap_type: GapTypeSetting::Continuous,
                    printer_gap_length: 0,
                },
                &req::total_lines::TotalLines { lines },
            ])
            .await?;

            let reqs = into_reqs(&page.commands);
            let refs: Vec<&dyn ReqTrait> = reqs.iter().map(|c| c.as_ref()).collect();
            let mut flow = FlowPlanner::default();
            flow.push_commands(&self.assembler, &refs)?;

            loop {
                match flow.step() {
                    ChunkDecision::Send(chunk) => {
                        self.send_cmd_area(chunk).await?;
                        if flow.needs_gap() {
                            tokio::time::sleep(flow.config().chunk_gap).await;
                        }
                        flow.check_deadline()?;
                    }
                    ChunkDecision::QueryBudget => {
                        tokio::time::sleep(flow.config().chunk_gap).await;
                        self.try_send_batch(&[
                            &req::status::Status {},
                            &req::buffer_size::BufferSize {},
                        ])
                        .await?;
                        // 两条查询的应答按命令字节分别取，顺序和中间夹的推送都不影响。
                        let timeout = flow.config().budget_timeout;
                        match self
                            .recv_cmd_timeout::<resp::status::Status>(resp::status::CMD, timeout)
                            .await
                        {
                            Ok(Some(status)) => debug!(
                                status = ?status.status,
                                page_key = status.page_key,
                                "status before budget"
                            ),
                            Ok(None) => debug!("status before budget timed out"),
                            Err(e) => debug!("status before budget failed: {e}"),
                        }
                        match self
                            .recv_cmd_timeout::<resp::buffer_size::BufferSize>(
                                resp::buffer_size::CMD,
                                timeout,
                            )
                            .await
                        {
                            Ok(Some(bs)) => {
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
                                flow.on_budget_timeout();
                            }
                            Err(e) => {
                                warn!("buffer size failed, fall back to 8 KiB: {e}");
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

            self.try_send_batch(&[&req::end_page::EndPage {}]).await?;

            // `EndPage` 只是把数据交给固件；最后一页要等打印头真正走完，
            // 否则前台一退出，USB 句柄被释放，纸可能只走了一半。
            if start >= bm.height() {
                let status = self
                    .wait_for_print_done(self.config.print_done_timeout)
                    .await?;
                info!(
                    status = ?status.status,
                    page_key = status.page_key,
                    "print completed"
                );
            }

            let report = flow.report();
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
}
