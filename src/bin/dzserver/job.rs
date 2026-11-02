// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 任务表：对外暴露的 status / cancel，内部保存终态和取消 token。

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
    InQueue,
    Rendering,
    RenderFailed,
    PrintPending,
    Printing,
    PrintCompleted,
    Cancelled,
}

impl JobState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::RenderFailed | Self::PrintCompleted | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone)]
pub struct JobSnapshot {
    pub state: JobState,
    pub printer_status: Option<PrinterStatus>,
    pub io_error: bool,
}

#[derive(Clone, Default)]
pub struct JobStore {
    inner: Arc<RwLock<HashMap<JobId, JobRecord>>>,
}

struct JobRecord {
    state: JobState,
    printer_status: Option<PrinterStatus>,
    io_error: bool,
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
                state: JobState::InQueue,
                printer_status: None,
                io_error: false,
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
            printer_status: record.printer_status,
            io_error: record.io_error,
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

    pub fn finish(
        &self,
        id: JobId,
        state: JobState,
        printer_status: Option<PrinterStatus>,
        io_error: bool,
    ) {
        if let Some(record) = self.write().get_mut(&id) {
            if record.state.is_terminal() {
                return;
            }
            record.state = state;
            record.printer_status = printer_status;
            record.io_error = io_error;
            record.updated = Instant::now();
        }
    }

    /// 只有非终态任务可以被取消；已经请求过取消的返回 `false`。
    pub fn cancel(&self, id: &JobId) -> bool {
        let mut map = self.write();
        let Some(record) = map.get_mut(id) else {
            return false;
        };
        if record.state.is_terminal() || record.cancel.is_cancelled() {
            return false;
        }
        record.cancel.cancel();
        record.state = JobState::Cancelled;
        record.updated = Instant::now();
        true
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

    #[test]
    fn cancel_is_idempotent_and_terminal_jobs_reject_it() {
        let store = JobStore::new();
        let token = CancellationToken::new();
        let id = [7u8; 16];

        store.insert(id, token.clone());
        assert_eq!(store.snapshot(&id).unwrap().state, JobState::InQueue);
        assert!(store.cancel(&id));
        assert!(token.is_cancelled());
        assert!(!store.cancel(&id));

        store.finish(id, JobState::Cancelled, None, false);
        assert_eq!(store.snapshot(&id).unwrap().state, JobState::Cancelled);
        assert!(!store.cancel(&id));
    }

    #[test]
    fn terminal_jobs_are_garbage_collected() {
        let store = JobStore::new();
        let id = [8u8; 16];
        store.insert(id, CancellationToken::new());
        store.finish(id, JobState::RenderFailed, None, false);

        store.gc(Duration::ZERO);
        assert!(store.snapshot(&id).is_none());

        store.insert(id, CancellationToken::new());
        store.gc(Duration::ZERO);
        assert!(store.snapshot(&id).is_some());
    }
}
