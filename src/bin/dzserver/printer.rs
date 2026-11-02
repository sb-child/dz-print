// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印队列的唯一消费者：连接、等就绪、逐页打印、错误后作废当前 job 并重连。

use crate::{
    common::{PrintArgs, PrintSettings, SHUTDOWN_TIMEOUT},
    job::{JobId, JobState, JobStore},
    render::RenderedJob,
};
use dz_print::{
    command::{req, resp, status::PrinterStatus},
    frontend::{Device, DeviceConfig, FrontendError, RecvError},
};
use mytm::TaskManager;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const STATUS_POLL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy)]
pub struct PrinterProfile {
    pub head_dots: u16,
}

pub struct PrinterWorker {
    tm: TaskManager,
    config: DeviceConfig,
    store: JobStore,
    rx: mpsc::Receiver<RenderedJob>,
    profile_tx: watch::Sender<Option<PrinterProfile>>,
    device: Option<Device>,
    sent_settings: Option<PrintSettings>,
}

impl PrinterWorker {
    pub fn new(
        tm: TaskManager,
        args: PrintArgs,
        store: JobStore,
        rx: mpsc::Receiver<RenderedJob>,
        profile_tx: watch::Sender<Option<PrinterProfile>>,
    ) -> Self {
        let config = args.device_config();
        Self {
            tm,
            config,
            store,
            rx,
            profile_tx,
            device: None,
            sent_settings: None,
        }
    }

    pub async fn run(
        mut self,
        task_token: CancellationToken,
        shutdown: CancellationToken,
    ) -> anyhow::Result<()> {
        // 渲染 worker 要等打印头点数才能开始，所以先连一次并发布 profile；
        // 打印机不在线时这里会一直重试，job 留在入口队列里。
        if !self.ensure_device(&shutdown, &shutdown).await {
            self.drop_device().await;
            return Ok(());
        }

        loop {
            let job = tokio::select! {
                _ = task_token.cancelled() => break,
                _ = shutdown.cancelled() => break,
                job = self.rx.recv() => match job {
                    Some(job) => job,
                    None => break,
                },
            };
            self.handle_job(job, &shutdown).await;
        }

        self.drop_device().await;
        Ok(())
    }

    async fn handle_job(&mut self, job: RenderedJob, token: &CancellationToken) {
        let id = job.id;

        if job.cancel.is_cancelled() || token.is_cancelled() {
            self.store.finish(id, JobState::Cancelled, None, false);
            return;
        }

        self.store.set_state(id, JobState::PrintPending);
        if !self.wait_ready(&job.cancel, token).await {
            self.store.finish(id, JobState::Cancelled, None, false);
            return;
        }

        self.store.set_state(id, JobState::Printing);
        info!(
            job = %job_label(id),
            pages = job.pages.len(),
            repeats = job.repeats,
            "print started"
        );

        match self.print_pages(&job, token).await {
            Ok(()) => {
                self.store.finish(
                    id,
                    JobState::PrintCompleted,
                    Some(PrinterStatus::Printable),
                    false,
                );
                info!(job = %job_label(id), "print completed");
            }
            Err(PrintFailure::Cancelled) => {
                self.store.finish(id, JobState::Cancelled, None, false);
                warn!(job = %job_label(id), "print cancelled");
                self.drop_device().await;
            }
            Err(PrintFailure::Device(error)) => {
                let (status, io_error) = classify_error(&error);
                error!(job = %job_label(id), %error, "print failed");
                self.store
                    .finish(id, JobState::PrintCompleted, status, io_error);
                self.drop_device().await;
            }
        }
    }

    /// 等打印机回到可打印状态；`false` 表示 job 或 server 被取消。
    async fn wait_ready(
        &mut self,
        job_cancel: &CancellationToken,
        token: &CancellationToken,
    ) -> bool {
        let mut logged_error: Option<PrinterStatus> = None;

        loop {
            if job_cancel.is_cancelled() || token.is_cancelled() {
                return false;
            }
            if !self.ensure_device(job_cancel, token).await {
                return false;
            }

            let status = {
                let dev = self.device.as_ref().expect("device just connected");
                query_status(dev, self.config.recv_timeout).await
            };

            match status {
                Ok(status) if matches!(status, PrinterStatus::Printable | PrinterStatus::NoJob) => {
                    if logged_error.take().is_some() {
                        info!(?status, "printer ready");
                    }
                    return true;
                }
                Ok(status) if status.is_error() => {
                    if logged_error != Some(status) {
                        warn!(?status, "printer not ready");
                        logged_error = Some(status);
                    }
                }
                Ok(status) => debug!(?status, "printer busy"),
                Err(error) => {
                    warn!(%error, "status query failed, reconnecting");
                    self.drop_device().await;
                }
            }

            tokio::select! {
                _ = job_cancel.cancelled() => return false,
                _ = token.cancelled() => return false,
                _ = tokio::time::sleep(STATUS_POLL) => {}
            }
        }
    }

    /// 没有设备就连接并查询打印头点数；失败会退避重试。
    async fn ensure_device(
        &mut self,
        job_cancel: &CancellationToken,
        token: &CancellationToken,
    ) -> bool {
        if self.device.is_some() {
            return true;
        }

        loop {
            if job_cancel.is_cancelled() || token.is_cancelled() {
                return false;
            }

            let dev = Device::new(&self.tm, self.config.clone());
            let ready = tokio::select! {
                _ = job_cancel.cancelled() => {
                    let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
                    return false;
                }
                _ = token.cancelled() => {
                    let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
                    return false;
                }
                result = tokio::time::timeout(CONNECT_TIMEOUT, dev.ready()) => result,
            };

            match ready {
                Ok(Ok(())) => match query_head_dots(&dev, self.config.recv_timeout).await {
                    Ok(head_dots) => {
                        info!(head_dots, "printer connected");
                        let _ = self.profile_tx.send(Some(PrinterProfile { head_dots }));
                        self.device = Some(dev);
                        self.sent_settings = None;
                        return true;
                    }
                    Err(error) => {
                        warn!(%error, "read print head width failed");
                        let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
                    }
                },
                Ok(Err(error)) => {
                    warn!(%error, "printer open failed");
                    let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
                }
                Err(_) => {
                    warn!(timeout = ?CONNECT_TIMEOUT, "printer open timed out");
                    let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
                }
            }

            tokio::select! {
                _ = job_cancel.cancelled() => return false,
                _ = token.cancelled() => return false,
                _ = tokio::time::sleep(RECONNECT_DELAY) => {}
            }
        }
    }

    async fn print_pages(
        &mut self,
        job: &RenderedJob,
        token: &CancellationToken,
    ) -> Result<(), PrintFailure> {
        let dev = self.device.as_ref().expect("device is ready");
        for repeat in 0..=job.repeats {
            for (index, page) in job.pages.iter().enumerate() {
                if job.cancel.is_cancelled() || token.is_cancelled() {
                    return Err(PrintFailure::Cancelled);
                }

                if self.sent_settings.as_ref() != Some(&page.settings) {
                    page.settings
                        .send(dev)
                        .await
                        .map_err(PrintFailure::Device)?;
                    self.sent_settings = Some(page.settings.clone());
                }

                let print = dev.print_bitmap_job_with(&page.bitmap, page.rle, page.options);
                let result = tokio::select! {
                    _ = job.cancel.cancelled() => return Err(PrintFailure::Cancelled),
                    _ = token.cancelled() => return Err(PrintFailure::Cancelled),
                    result = print => result,
                };

                let report =
                    result.map_err(|error| PrintFailure::Device(anyhow::Error::new(error)))?;
                info!(
                    job = %job_label(job.id),
                    repeat = repeat + 1,
                    page = index + 1,
                    chunks = report.chunks,
                    bytes = report.bytes,
                    "page printed"
                );
            }
        }

        Ok(())
    }

    async fn drop_device(&mut self) {
        if let Some(dev) = self.device.take()
            && let Err(error) = dev.shutdown(SHUTDOWN_TIMEOUT).await
        {
            warn!(%error, "device shutdown failed");
        }
        self.sent_settings = None;
        let _ = self.profile_tx.send(None);
    }
}

enum PrintFailure {
    Cancelled,
    Device(anyhow::Error),
}

async fn query_head_dots(dev: &Device, timeout: Duration) -> Result<u16, FrontendError> {
    dev.try_send_batch(&[&req::print_width::PrintWidth {}])
        .await?;
    let width = dev
        .recv_cmd_timeout::<resp::print_width::PrintWidth>(resp::print_width::CMD, timeout)
        .await?;
    let Some(width) = width else {
        return Err(FrontendError::Recv {
            source: RecvError::Timeout { timeout },
        });
    };
    if width.print_width == 0 {
        return Err(FrontendError::WidthOverflow { width: 0 });
    }
    Ok(width.print_width)
}

async fn query_status(dev: &Device, timeout: Duration) -> Result<PrinterStatus, FrontendError> {
    dev.try_send_batch(&[&req::status::Status {}]).await?;
    let status = dev
        .recv_cmd_timeout::<resp::status::Status>(resp::status::CMD, timeout)
        .await?;
    match status {
        Some(status) => Ok(status.status),
        None => Err(FrontendError::Recv {
            source: RecvError::Timeout { timeout },
        }),
    }
}

fn classify_error(error: &anyhow::Error) -> (Option<PrinterStatus>, bool) {
    match error.downcast_ref::<FrontendError>() {
        Some(FrontendError::PrinterStatus { status }) => (Some(*status), false),
        Some(
            FrontendError::PrinterSilent { .. }
            | FrontendError::PrintTimeout { .. }
            | FrontendError::Write { .. }
            | FrontendError::Recv { .. }
            | FrontendError::WorkerGone
            | FrontendError::WorkerTimeout { .. }
            | FrontendError::ShutdownTimeout { .. },
        ) => (None, true),
        _ => (None, false),
    }
}

fn job_label(id: JobId) -> Uuid {
    Uuid::from_bytes(id)
}
