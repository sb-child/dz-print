// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 流水线：入口队列 -> N 个渲染 worker -> 打印队列 -> 1 个打印 worker。

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Context as _;
use mytm::{TaskHandle, TaskManager};
use tokio::sync::{Mutex as AsyncMutex, mpsc, watch};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::{
    api::ApiState,
    common::{PrintArgs, typst::FontSet},
    job::{JobState, JobStore},
    printer::{PrinterProfile, PrinterWorker},
    render::{JobSpec, RenderedJob, run_blocking},
};

const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);
const GC_INTERVAL: Duration = Duration::from_secs(60);

pub struct Pipeline {
    store: JobStore,
    ingress_tx: mpsc::Sender<JobSpec>,
    root: CancellationToken,
    tm: TaskManager,
    accepting: Arc<AtomicBool>,
    temp_root: PathBuf,
    max_job_bytes: usize,
    max_repeats: u32,
    render_handles: Vec<TaskHandle<(), (), anyhow::Error>>,
    gc_handle: TaskHandle<(), (), anyhow::Error>,
    print_handle: TaskHandle<(), (), anyhow::Error>,
}

impl Pipeline {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        tm: &TaskManager,
        root: CancellationToken,
        args: PrintArgs,
        fonts: Arc<FontSet>,
        state_dir: &Path,
        render_workers: usize,
        queue_capacity: usize,
        print_queue_capacity: usize,
        max_job_bytes: usize,
        max_repeats: u32,
        job_ttl: Duration,
    ) -> anyhow::Result<Self> {
        let temp_root = state_dir.join("jobs");
        let package_cache = state_dir.join("typst-packages");
        std::fs::create_dir_all(&temp_root)
            .with_context(|| format!("创建 {} 失败", temp_root.display()))?;
        std::fs::create_dir_all(&package_cache)
            .with_context(|| format!("创建 {} 失败", package_cache.display()))?;

        let store = JobStore::new();
        let accepting = Arc::new(AtomicBool::new(true));

        let (ingress_tx, ingress_rx) = mpsc::channel(queue_capacity.max(1));
        let (print_tx, print_rx) = mpsc::channel(print_queue_capacity.max(1));
        let (profile_tx, profile_rx) = watch::channel::<Option<PrinterProfile>>(None);

        let ingress_rx = Arc::new(AsyncMutex::new(ingress_rx));
        let mut render_handles = Vec::with_capacity(render_workers.max(1));

        for index in 0..render_workers.max(1) {
            let tm = tm.clone();
            let store = store.clone();
            let ingress_rx = ingress_rx.clone();
            let print_tx = print_tx.clone();
            let profile_rx = profile_rx.clone();
            let args = args.clone();
            let fonts = fonts.clone();
            let package_cache = package_cache.clone();
            let token = root.child_token();

            let handle = tm.spawn_typed(move |tm, _progress, task_token| async move {
                info!(worker = index, "render worker started");
                let result = render_worker(
                    tm,
                    store,
                    ingress_rx,
                    print_tx,
                    profile_rx,
                    args,
                    fonts,
                    package_cache,
                    task_token,
                    token,
                )
                .await;
                info!(worker = index, "render worker stopped");
                result
            });
            render_handles.push(handle);
        }

        let gc_handle = {
            let tm = tm.clone();
            let store = store.clone();
            let token = root.child_token();
            tm.spawn_typed(move |_tm, _progress, task_token| async move {
                loop {
                    tokio::select! {
                        _ = task_token.cancelled() => break,
                        _ = token.cancelled() => break,
                        _ = tokio::time::sleep(GC_INTERVAL) => store.gc(job_ttl),
                    }
                }
                Ok(())
            })
        };

        let print_handle = {
            let tm = tm.clone();
            let printer = PrinterWorker::new(tm.clone(), args, store.clone(), print_rx, profile_tx);
            let token = root.child_token();
            tm.spawn_typed(move |_tm, _progress, task_token| async move {
                printer.run(task_token, token).await
            })
        };

        Ok(Self {
            store,
            ingress_tx,
            root,
            tm: tm.clone(),
            accepting,
            temp_root,
            max_job_bytes,
            max_repeats,
            render_handles,
            gc_handle,
            print_handle,
        })
    }

    pub fn api_state(&self) -> ApiState {
        ApiState::new(
            self.store.clone(),
            self.ingress_tx.clone(),
            self.root.clone(),
            self.tm.clone(),
            self.temp_root.clone(),
            self.accepting.clone(),
            self.max_job_bytes,
            self.max_repeats,
        )
    }

    pub async fn shutdown(self) {
        self.accepting.store(false, Ordering::SeqCst);
        self.root.cancel();

        for handle in &self.render_handles {
            let _ = tokio::time::timeout(SHUTDOWN_GRACE, handle.wait_terminal()).await;
        }
        let _ = tokio::time::timeout(SHUTDOWN_GRACE, self.gc_handle.wait_terminal()).await;
        let _ = tokio::time::timeout(SHUTDOWN_GRACE, self.print_handle.wait_terminal()).await;

        self.tm.close();
        info!(jobs = self.store.len(), "pipeline stopped");
    }
}

#[allow(clippy::too_many_arguments)]
async fn render_worker(
    tm: TaskManager,
    store: JobStore,
    ingress: Arc<AsyncMutex<mpsc::Receiver<JobSpec>>>,
    print_tx: mpsc::Sender<RenderedJob>,
    mut profile_rx: watch::Receiver<Option<PrinterProfile>>,
    args: PrintArgs,
    fonts: Arc<FontSet>,
    package_cache: PathBuf,
    token: CancellationToken,
    root: CancellationToken,
) -> anyhow::Result<()> {
    loop {
        let job = {
            let mut rx = ingress.lock().await;
            tokio::select! {
                _ = token.cancelled() => break,
                _ = root.cancelled() => break,
                job = rx.recv() => job,
            }
        };
        let Some(job) = job else {
            break;
        };

        let id = job.id();
        let cancel = job.cancel().clone();
        if cancel.is_cancelled() || token.is_cancelled() || root.is_cancelled() {
            store.finish(id, JobState::Cancelled, None, false);
            continue;
        }

        let Some(profile) = wait_profile(&mut profile_rx, &cancel, &token, &root).await else {
            store.finish(id, JobState::Cancelled, None, false);
            if token.is_cancelled() || root.is_cancelled() {
                break;
            }
            if cancel.is_cancelled() {
                continue;
            }
            // profile channel 已经关闭，打印 worker 不在，继续等也没意义。
            break;
        };

        store.set_state(id, JobState::Rendering);
        let args = args.clone();
        let fonts = fonts.clone();
        let package_cache = package_cache.clone();
        let render_cancel = cancel.clone();
        let result = run_blocking(&tm, &cancel, move || {
            job.render(profile, &args, &fonts, &package_cache, &render_cancel)
        })
        .await;

        match result {
            Ok(rendered) => {
                if rendered.cancel.is_cancelled() || token.is_cancelled() || root.is_cancelled() {
                    store.finish(id, JobState::Cancelled, None, false);
                    continue;
                }

                let send = print_tx.send(rendered);
                tokio::select! {
                    _ = cancel.cancelled() => {
                        store.finish(id, JobState::Cancelled, None, false);
                    }
                    _ = token.cancelled() => {
                        store.finish(id, JobState::Cancelled, None, false);
                        break;
                    }
                    _ = root.cancelled() => {
                        store.finish(id, JobState::Cancelled, None, false);
                        break;
                    }
                    result = send => {
                        if result.is_err() {
                            store.finish(id, JobState::Cancelled, None, false);
                            break;
                        }
                    }
                }
            }
            Err(error) => {
                if cancel.is_cancelled() || token.is_cancelled() || root.is_cancelled() {
                    store.finish(id, JobState::Cancelled, None, false);
                } else {
                    error!(job = %uuid::Uuid::from_bytes(id), %error, "render failed");
                    store.finish(id, JobState::RenderFailed, None, false);
                }
            }
        }
    }

    Ok(())
}

async fn wait_profile(
    rx: &mut watch::Receiver<Option<PrinterProfile>>,
    cancel: &CancellationToken,
    token: &CancellationToken,
    root: &CancellationToken,
) -> Option<PrinterProfile> {
    loop {
        if let Some(profile) = *rx.borrow() {
            return Some(profile);
        }
        tokio::select! {
            _ = cancel.cancelled() => return None,
            _ = token.cancelled() => return None,
            _ = root.cancelled() => return None,
            result = rx.changed() => {
                if result.is_err() {
                    warn!("printer profile channel closed");
                    return None;
                }
            }
        }
    }
}
