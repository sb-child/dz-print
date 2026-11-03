// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 任务表：对外暴露的 status / cancel，内部保存终态和取消 token。
//!
//! 状态和失败原因的分层是刻意的：`JobState` 只回答“现在到哪一步了”，
//! 终态是否成功由 `Succeeded` / `Failed` / `Cancelled` 区分；失败细节放在
//! [`JobFailure`] 里，不再借 `PrinterStatus` 表达 job 结局。

use dz_print::command::status::PrinterStatus;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub type JobId = [u8; 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    /// 在入口队列等渲染。
    Queued,
    /// 正在解码 / 渲染。
    Rendering,
    /// 等打印机回到可打印状态。
    WaitingForPrinter,
    /// 正在写给打印机并等页完成。
    Printing,
    /// （终态）打印完成。
    Succeeded,
    /// （终态）打印失败或任务作废，原因见 [`JobFailure`]。
    Failed,
    /// （终态）被取消。
    Cancelled,
}

impl JobState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// 失败发生在哪个阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureStage {
    Render,
    Connect,
    WaitForPrinter,
    Print,
}

/// 机器可判的失败原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    /// 输入处理失败：图片解码、Typst 编译、tar 结构。
    Render,
    /// 一直找不到 / 连不上设备。
    DeviceNotFound,
    /// 设备连着，但一直回不到可打印状态。
    DeviceNotReady,
    /// 打印过程中设备明确回了错误状态。
    PrinterStatus,
    /// 设备不再应答 USB 命令。
    PrinterSilent,
    /// 整份任务超时。
    PrintTimeout,
    /// 数据发完了，但设备始终没开始打印。
    PrintDidNotStart,
    /// 主机侧 IO 错误：USB 读写、设备被拔、读写线程无响应。
    Io,
    /// 归类不了的内部错误。客户端按“可以重试但要记录”处理。
    Internal,
}

#[derive(Debug, Clone)]
pub struct JobFailure {
    pub stage: FailureStage,
    pub reason: FailureReason,
    /// 给人看的完整错误描述（含原因链），不要解析。
    pub message: String,
}

impl JobFailure {
    pub fn new(stage: FailureStage, reason: FailureReason, message: impl Into<String>) -> Self {
        Self {
            stage,
            reason,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct JobSnapshot {
    pub state: JobState,
    pub failure: Option<JobFailure>,
    /// 服务端为这个任务读到的设备状态，只用于展示。
    pub printer_status: Option<PrinterStatus>,
}

#[derive(Clone, Default)]
pub struct JobStore {
    inner: Arc<RwLock<HashMap<JobId, JobRecord>>>,
}

struct JobRecord {
    state: JobState,
    failure: Option<JobFailure>,
    printer_status: Option<PrinterStatus>,
    cancel: CancellationToken,
    updated: Instant,
}

impl JobStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn read(&self) -> RwLockReadGuard<'_, HashMap<JobId, JobRecord>> {
        self.inner.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, HashMap<JobId, JobRecord>> {
        self.inner.write().unwrap_or_else(|e| e.into_inner())
    }

    pub fn insert(&self, id: JobId, cancel: CancellationToken) {
        self.write().insert(
            id,
            JobRecord {
                state: JobState::Queued,
                failure: None,
                printer_status: None,
                cancel,
                updated: Instant::now(),
            },
        );
    }

    pub fn remove(&self, id: &JobId) {
        self.write().remove(id);
    }

    pub fn snapshot(&self, id: &JobId) -> Option<JobSnapshot> {
        let map = self.read();
        let record = map.get(id)?;
        Some(JobSnapshot {
            state: record.state,
            failure: record.failure.clone(),
            printer_status: record.printer_status,
        })
    }

    pub fn set_state(&self, id: JobId, state: JobState) {
        if let Some(record) = self.write().get_mut(&id)
            && !record.state.is_terminal()
        {
            record.state = state;
            record.updated = Instant::now();
        }
    }

    /// 记录这次任务读到的设备状态，仅用于展示。
    pub fn set_printer_status(&self, id: JobId, status: PrinterStatus) {
        if let Some(record) = self.write().get_mut(&id) {
            record.printer_status = Some(status);
        }
    }

    pub fn finish_succeeded(&self, id: JobId, printer_status: Option<PrinterStatus>) {
        self.finish(id, JobState::Succeeded, None, printer_status);
    }

    /// 把任务标记为取消。
    ///
    /// 调用点很多，单独包一层避免每次都写四个参数。
    pub fn finish_cancelled(&self, id: JobId) {
        self.finish(id, JobState::Cancelled, None, None);
    }

    /// 把任务标记为渲染失败。
    pub fn finish_render_failed(&self, id: JobId, message: impl Into<String>) {
        self.finish(
            id,
            JobState::Failed,
            Some(JobFailure::new(
                FailureStage::Render,
                FailureReason::Render,
                message,
            )),
            None,
        );
    }

    pub fn finish_failed(
        &self,
        id: JobId,
        failure: JobFailure,
        printer_status: Option<PrinterStatus>,
    ) {
        self.finish(id, JobState::Failed, Some(failure), printer_status);
    }

    pub fn finish(
        &self,
        id: JobId,
        state: JobState,
        failure: Option<JobFailure>,
        printer_status: Option<PrinterStatus>,
    ) {
        if let Some(record) = self.write().get_mut(&id) {
            if record.state.is_terminal() {
                return;
            }
            record.state = state;
            record.failure = failure;
            record.printer_status = printer_status;
            record.updated = Instant::now();
        }
    }

    /// 尝试取消。
    ///
    /// - `None`：任务不存在（调用方回 `NOT_FOUND`）；
    /// - `Some((true, Cancelled))`：接受了，任务会走到 `Cancelled`；
    /// - `Some((false, state))`：没接受，`state` 是任务当时的状态。
    pub fn cancel(&self, id: &JobId) -> Option<(bool, JobState)> {
        let mut map = self.write();
        let record = map.get_mut(id)?;
        if record.state.is_terminal() || record.cancel.is_cancelled() {
            return Some((false, record.state));
        }
        record.cancel.cancel();
        record.state = JobState::Cancelled;
        record.updated = Instant::now();
        Some((true, record.state))
    }

    pub fn gc(&self, ttl: Duration) {
        let now = Instant::now();
        self.write().retain(|_, record| {
            !record.state.is_terminal() || now.duration_since(record.updated) < ttl
        });
    }

    pub fn len(&self) -> usize {
        self.read().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_job(store: &JobStore, id: JobId) -> CancellationToken {
        let token = CancellationToken::new();
        store.insert(id, token.clone());
        token
    }

    #[test]
    fn cancel_is_idempotent_and_terminal_jobs_reject_it() {
        let store = JobStore::new();
        let id = [7u8; 16];
        let token = new_job(&store, id);
        assert_eq!(store.snapshot(&id).unwrap().state, JobState::Queued);
        assert_eq!(store.cancel(&id), Some((true, JobState::Cancelled)));
        assert!(token.is_cancelled());
        assert_eq!(store.cancel(&id), Some((false, JobState::Cancelled)));

        store.finish_cancelled(id);
        assert_eq!(store.snapshot(&id).unwrap().state, JobState::Cancelled);
        assert_eq!(store.cancel(&id), Some((false, JobState::Cancelled)));
    }

    #[test]
    fn cancel_reports_the_state_that_blocked_it() {
        let store = JobStore::new();
        let id = [9u8; 16];
        new_job(&store, id);
        store.finish_succeeded(id, None);

        assert_eq!(store.cancel(&id), Some((false, JobState::Succeeded)));
    }

    #[test]
    fn unknown_jobs_have_no_cancel_result() {
        let store = JobStore::new();
        assert_eq!(store.cancel(&[1u8; 16]), None);
    }

    #[test]
    fn failure_is_only_reported_until_the_job_reaches_a_terminal_state() {
        let store = JobStore::new();
        let id = [3u8; 16];
        new_job(&store, id);

        store.finish_failed(
            id,
            JobFailure::new(FailureStage::Print, FailureReason::PrinterSilent, "mute"),
            Some(PrinterStatus::CoverOpened),
        );
        let snapshot = store.snapshot(&id).unwrap();
        assert_eq!(snapshot.state, JobState::Failed);
        assert_eq!(
            snapshot.failure.unwrap().reason,
            FailureReason::PrinterSilent
        );
        assert_eq!(snapshot.printer_status, Some(PrinterStatus::CoverOpened));

        // 终态之后再来一次 finish 不会覆盖。
        store.finish_succeeded(id, None);
        assert_eq!(store.snapshot(&id).unwrap().state, JobState::Failed);
    }

    #[test]
    fn terminal_jobs_are_garbage_collected() {
        let store = JobStore::new();
        let id = [8u8; 16];
        new_job(&store, id);
        store.finish_render_failed(id, "bad typst");

        store.gc(Duration::ZERO);
        assert!(store.snapshot(&id).is_none());

        new_job(&store, id);
        store.gc(Duration::ZERO);
        assert!(store.snapshot(&id).is_some());
    }
}
