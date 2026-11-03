// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印队列的唯一消费者：连接、等就绪、逐页打印、错误后作废当前 job 并重连。

use crate::{
    common::{
        PrintArgs, PrintSettings, SHUTDOWN_TIMEOUT,
        printer::{PagePrintError, print_pages, query_info, query_status},
    },
    job::{FailureReason, FailureStage, JobFailure, JobId, JobState, JobStore},
    render::RenderedJob,
};
use dz_print::{
    command::status::PrinterStatus,
    frontend::{Device, DeviceConfig, FrontendError, StallOutcome},
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

        self.store.set_state(id, JobState::WaitingForPrinter);
        match self.wait_ready(id, &job.cancel, token).await {
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
                let failure =
                    not_ready_failure(self.device.is_some(), self.config.print_done_timeout);
                self.store
                    .finish_failed(id, failure, Some(PrinterStatus::EnvNotReady));
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
                self.store
                    .finish_succeeded(id, Some(PrinterStatus::Printable));
                info!(job = %job_label(id), "print completed");
            }
            Err(PrintFailure::Cancelled) => {
                self.store.finish_cancelled(id);
                warn!(job = %job_label(id), "print cancelled");
                self.drop_device().await;
            }
            Err(PrintFailure::Device(error)) => {
                let (failure, status) = classify_error(&error);
                error!(job = %job_label(id), %error, "print failed");
                self.store.finish_failed(id, failure, status);
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
        job_id: JobId,
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
            // 设备不在（或刚被上一次查询失败丢掉）就重连；重连也受同一个
            // deadline 约束，不会在打印机长期离线时无限等。
            if !self.ensure_device(job_cancel, token, Some(deadline)).await {
                if job_cancel.is_cancelled() || token.is_cancelled() {
                    return ReadyState::Cancelled;
                }
                if Instant::now() >= deadline {
                    return ReadyState::TimedOut;
                }
                // `ensure_device` 只有取消或预算用完才返回 false；取消已经在上
                // 面处理，走到这里只可能是 deadline 被夹到 0 的边界情况。
                return ReadyState::TimedOut;
            }

            // `ensure_device` 返回 true 时一定已经设置 `device`；这里仍然按
            // 可选处理，避免以后调整状态机时直接 panic。
            let Some(dev) = self.device.as_ref() else {
                error!("printer disappeared after a successful connect");
                return ReadyState::TimedOut;
            };
            let status = query_status(dev, self.config.recv_timeout).await;

            // 等待期间的状态也要让客户端看得到：等 600s 的任务应该能说明
            // 自己是因为开盖还是缺纸在等。
            if let Ok(status) = status {
                self.store.set_printer_status(job_id, status);
            }

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
            // `deadline = None` 表示调用方希望一直重试（启动时的第一次连接）；
            // 每次尝试仍只给 `CONNECT_TIMEOUT`，避免一次 open 卡死整轮。
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

            // 退避不能超过剩余预算，否则最后会多睡一段再报告超时。
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
        // 进入打印前 `wait_ready` 已经确认过连接，正常不会走到 else。
        let Some(dev) = self.device.as_ref() else {
            return Err(PrintFailure::Device(anyhow::anyhow!(
                "printer is not connected"
            )));
        };
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

/// 一直等不到可打印状态时生成的失败。
///
/// 设备根本没连上（`found = false`）和连上了但环境不正常，对客户端来说是
/// 两件事：前者要检查线 / 电源，后者要检查盖子 / 纸张。
fn not_ready_failure(found: bool, timeout: Duration) -> JobFailure {
    let (stage, reason, what) = if found {
        (
            FailureStage::WaitForPrinter,
            FailureReason::DeviceNotReady,
            "设备一直回不到可打印状态",
        )
    } else {
        (
            FailureStage::Connect,
            FailureReason::DeviceNotFound,
            "一直没有找到可用的设备",
        )
    };
    JobFailure::new(stage, reason, format!("{what}（等待 {timeout:?}）"))
}

/// 把打印错误翻译成对外的失败描述，以及当时读到的设备状态。
///
/// `reason` 从「卡住的原因」取，`printer_status` 从「重置之后读回的结果」取：
/// 这两件事经常不一样（例如设备沉默、但 reset 之后能读到 `CoverOpened`）。
fn classify_error(error: &anyhow::Error) -> (JobFailure, Option<PrinterStatus>) {
    let message = format!("{error:#}");
    let failure = |stage, reason| JobFailure::new(stage, reason, message.clone());

    match error.downcast_ref::<FrontendError>() {
        Some(FrontendError::PrinterStatus { status }) => (
            failure(FailureStage::Print, FailureReason::PrinterStatus),
            Some(*status),
        ),
        Some(FrontendError::Stalled { cause, outcome }) => {
            let reason = match cause.as_ref() {
                FrontendError::PrinterSilent { .. } => FailureReason::PrinterSilent,
                FrontendError::PrintTimeout { .. } => FailureReason::PrintTimeout,
                FrontendError::PrintDidNotStart { .. } => FailureReason::PrintDidNotStart,
                FrontendError::WorkerGone | FrontendError::WorkerTimeout { .. } => {
                    FailureReason::Io
                }
                _ => FailureReason::Internal,
            };
            let status = match outcome {
                StallOutcome::Status(status) => Some(*status),
                StallOutcome::Silent => None,
            };
            (failure(FailureStage::Print, reason), status)
        }
        Some(FrontendError::PrintDidNotStart { .. }) => (
            failure(FailureStage::Print, FailureReason::PrintDidNotStart),
            None,
        ),
        Some(FrontendError::PrinterSilent { .. }) => (
            failure(FailureStage::Print, FailureReason::PrinterSilent),
            None,
        ),
        Some(FrontendError::PrintTimeout { .. }) => (
            failure(FailureStage::Print, FailureReason::PrintTimeout),
            None,
        ),
        Some(
            FrontendError::Write { .. }
            | FrontendError::Recv { .. }
            | FrontendError::WorkerGone
            | FrontendError::WorkerTimeout { .. }
            | FrontendError::ShutdownTimeout { .. },
        ) => (failure(FailureStage::Print, FailureReason::Io), None),
        _ => (failure(FailureStage::Print, FailureReason::Internal), None),
    }
}

fn job_label(id: JobId) -> Uuid {
    Uuid::from_bytes(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stalled(cause: FrontendError, outcome: StallOutcome) -> anyhow::Error {
        anyhow::Error::new(FrontendError::Stalled {
            cause: Box::new(cause),
            outcome,
        })
    }

    fn silent_cause() -> FrontendError {
        FrontendError::PrinterSilent {
            polls: 2,
            timeout: Duration::from_millis(500),
        }
    }

    #[test]
    fn print_did_not_start_has_its_own_reason() {
        let (failure, status) =
            classify_error(&anyhow::Error::new(FrontendError::PrintDidNotStart {
                timeout: Duration::from_secs(3),
            }));
        assert_eq!(failure.reason, FailureReason::PrintDidNotStart);
        assert_eq!(failure.stage, FailureStage::Print);
        assert!(status.is_none());
    }

    #[test]
    fn stalled_keeps_a_concrete_printer_status() {
        let (failure, status) = classify_error(&stalled(
            silent_cause(),
            StallOutcome::Status(PrinterStatus::CoverOpened),
        ));
        assert_eq!(failure.reason, FailureReason::PrinterSilent);
        assert_eq!(status, Some(PrinterStatus::CoverOpened));
    }

    #[test]
    fn stalled_and_recovered_still_reports_the_silent_cause() {
        // 盖子松了一下又合上：打印机已经能应答，但这一页已经作废。
        let (failure, status) = classify_error(&stalled(
            silent_cause(),
            StallOutcome::Status(PrinterStatus::Printable),
        ));
        assert_eq!(failure.reason, FailureReason::PrinterSilent);
        assert_eq!(status, Some(PrinterStatus::Printable));
    }

    #[test]
    fn stalled_and_still_silent_has_no_status() {
        let (failure, status) = classify_error(&stalled(silent_cause(), StallOutcome::Silent));
        assert_eq!(failure.reason, FailureReason::PrinterSilent);
        assert!(status.is_none());
    }

    #[test]
    fn a_timeout_cause_is_not_reported_as_silence() {
        let (failure, _) = classify_error(&stalled(
            FrontendError::PrintTimeout {
                timeout: Duration::from_secs(600),
            },
            StallOutcome::Silent,
        ));
        assert_eq!(failure.reason, FailureReason::PrintTimeout);
    }

    #[test]
    fn usb_errors_are_io() {
        let (failure, _) = classify_error(&anyhow::Error::new(FrontendError::WorkerGone));
        assert_eq!(failure.reason, FailureReason::Io);
    }

    #[test]
    fn a_missing_device_is_a_connect_failure() {
        let failure = not_ready_failure(false, Duration::from_secs(600));
        assert_eq!(failure.stage, FailureStage::Connect);
        assert_eq!(failure.reason, FailureReason::DeviceNotFound);

        let failure = not_ready_failure(true, Duration::from_secs(600));
        assert_eq!(failure.stage, FailureStage::WaitForPrinter);
        assert_eq!(failure.reason, FailureReason::DeviceNotReady);
    }
}
