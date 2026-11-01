// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 位图打印任务：分页、流控、等待打印完成。

use std::time::Duration;

use tracing::{debug, info, warn};

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
                        // 先 0x70 后 0x77，按顺序取。
                        let _ = self
                            .try_recv_one_timeout::<resp::status::Status>(
                                flow.config().budget_timeout,
                            )
                            .await;
                        match self
                            .try_recv_one_timeout::<resp::buffer_size::BufferSize>(
                                flow.config().budget_timeout,
                            )
                            .await
                        {
                            Ok(bs) => {
                                debug!(
                                    size = bs.size,
                                    budget = bs.budget(),
                                    status = bs.status,
                                    "buffer size"
                                );
                                flow.on_buffer_size(&bs);
                            }
                            Err(e) => {
                                warn!("buffer size timeout, fall back to 8 KiB: {e}");
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
