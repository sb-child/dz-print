// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印队列的唯一消费者：连接、等就绪、逐页打印、错误后作废当前 job 并重连。

use crate::{
    common::{
        PrintArgs, PrintSettings, SHUTDOWN_TIMEOUT,
        printer::{PagePrintError, print_pages, query_info, query_status},
    },
    job::{JobId, JobState, JobStore},
    render::RenderedJob,
};
use dz_print::{
    command::status::PrinterStatus,
    frontend::{Device, DeviceConfig, FrontendError},
};
use mytm::TaskManager;
use std::time::{Duration, Instant};
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
        if !self.ensure_device(&shutdown, &shutdown, None).await {
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
            self.store.finish_cancelled(id);
            return;
        }

        self.store.set_state(id, JobState::PrintPending);
        match self.wait_ready(&job.cancel, token).await {
            ReadyState::Ready => {}
            ReadyState::Cancelled => {
                self.store.finish_cancelled(id);
                return;
            }
            ReadyState::TimedOut => {
                // 打印机长时间回不到可打印状态时，不要让后面的 job 一直
                // 排在它后面。作废当前 job，丢掉设备，下一个 job 重新连接。
                warn!(
                    job = %job_label(id),
                    timeout = ?self.config.print_done_timeout,
                    "printer did not become ready; failing this job"
                );
                self.store.finish(
                    id,
                    JobState::PrintCompleted,
                    Some(PrinterStatus::EnvNotReady),
                    false,
                );
                self.drop_device().await;
                return;
            }
        }

        self.store.set_state(id, JobState::Printing);
        info!(
            job = %job_label(id),
            pages = job.pages.len(),
            repeats = job.repeats,
            "print started"
        );

        match self.print_pages(&job).await {
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
                self.store.finish_cancelled(id);
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

    /// 等打印机回到可打印状态。
    ///
    /// 最多等 [`DeviceConfig::print_done_timeout`](dz_print::frontend::DeviceConfig::print_done_timeout)；
    /// 超时后当前 job 失败，避免打印机一直不可用时把打印队列堵死。
    async fn wait_ready(
        &mut self,
        job_cancel: &CancellationToken,
        token: &CancellationToken,
    ) -> ReadyState {
        let deadline = Instant::now() + self.config.print_done_timeout;
        let mut logged_error: Option<PrinterStatus> = None;

        loop {
            if job_cancel.is_cancelled() || token.is_cancelled() {
                return ReadyState::Cancelled;
            }
            if Instant::now() >= deadline {
                return ReadyState::TimedOut;
            }
            if !self.ensure_device(job_cancel, token, Some(deadline)).await {
                if job_cancel.is_cancelled() || token.is_cancelled() {
                    return ReadyState::Cancelled;
                }
                if Instant::now() >= deadline {
                    return ReadyState::TimedOut;
                }
                return ReadyState::Cancelled;
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
                    return ReadyState::Ready;
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

            let remaining = deadline.saturating_duration_since(Instant::now());
            tokio::select! {
                _ = job_cancel.cancelled() => return ReadyState::Cancelled,
                _ = token.cancelled() => return ReadyState::Cancelled,
                _ = tokio::time::sleep(STATUS_POLL.min(remaining)) => {}
            }
        }
    }

    /// 没有设备就连接并查询打印头点数；失败会退避重试。
    ///
    /// `deadline` 为 `Some` 时，超时后停止重试并返回 `false`。
    async fn ensure_device(
        &mut self,
        job_cancel: &CancellationToken,
        token: &CancellationToken,
        deadline: Option<Instant>,
    ) -> bool {
        if self.device.is_some() {
            return true;
        }

        loop {
            if job_cancel.is_cancelled() || token.is_cancelled() {
                return false;
            }
            let remaining = deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .unwrap_or(CONNECT_TIMEOUT);
            if remaining.is_zero() {
                return false;
            }
            let connect_timeout = CONNECT_TIMEOUT.min(remaining);

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
                result = tokio::time::timeout(connect_timeout, dev.ready()) => result,
            };

            match ready {
                Ok(Ok(())) => match query_info(&dev, self.config.recv_timeout).await {
                    Ok(printer_info) => {
                        printer_info.log_connected();
                        let head_dots = printer_info.head_dots;
                        let _ = self.profile_tx.send(Some(PrinterProfile { head_dots }));
                        self.device = Some(dev);
                        self.sent_settings = None;
                        return true;
                    }
                    Err(error) => {
                        warn!(%error, "query printer info failed");
                        let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
                    }
                },
                Ok(Err(error)) => {
                    warn!(%error, "printer open failed");
                    let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
                }
                Err(_) => {
                    warn!(timeout = ?connect_timeout, "printer open timed out");
                    let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
                }
            }

            let delay = deadline
                .map(|deadline| {
                    RECONNECT_DELAY.min(deadline.saturating_duration_since(Instant::now()))
                })
                .unwrap_or(RECONNECT_DELAY);
            if delay.is_zero() {
                return false;
            }
            tokio::select! {
                _ = job_cancel.cancelled() => return false,
                _ = token.cancelled() => return false,
                _ = tokio::time::sleep(delay) => {}
            }
        }
    }

    async fn print_pages(&mut self, job: &RenderedJob) -> Result<(), PrintFailure> {
        let dev = self.device.as_ref().expect("device is ready");
        let label = job_label(job.id).to_string();
        let mut sent_settings = self.sent_settings.take();

        let result = print_pages(
            dev,
            &label,
            &job.pages,
            job.repeats,
            &mut sent_settings,
            &job.cancel,
        )
        .await;

        self.sent_settings = sent_settings;
        match result {
            Ok(()) => Ok(()),
            Err(PagePrintError::Cancelled) => Err(PrintFailure::Cancelled),
            Err(PagePrintError::Device(error)) => {
                Err(PrintFailure::Device(anyhow::Error::new(error)))
            }
        }
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

/// `wait_ready` 的结果。
enum ReadyState {
    Ready,
    Cancelled,
    TimedOut,
}

fn classify_error(error: &anyhow::Error) -> (Option<PrinterStatus>, bool) {
    match error.downcast_ref::<FrontendError>() {
        Some(FrontendError::PrinterStatus { status }) => (Some(*status), false),
        // 页面没开始打印，但没有具体的打印机状态码；让 gRPC 侧落到
        // PS_OTHER_ERROR，而不是 PS_PRINTABLE。
        Some(FrontendError::PrintDidNotStart { .. }) => (None, false),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_did_not_start_is_not_an_io_error() {
        let error = anyhow::Error::new(FrontendError::PrintDidNotStart {
            timeout: Duration::from_secs(3),
        });
        let (status, io_error) = classify_error(&error);
        assert!(status.is_none());
        assert!(!io_error);
    }
}
