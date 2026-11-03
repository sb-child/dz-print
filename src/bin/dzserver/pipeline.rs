// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 流水线：入口队列 -> N 个渲染 worker -> 打印队列 -> 1 个打印 worker。

use crate::{
    api::{ApiConfig, ApiState},
    common::{PrintArgs, typst::FontSet},
    job::{JobState, JobStore},
    printer::{PrinterProfile, PrinterWorker},
    render::{JobSpec, RenderedJob, run_blocking},
};
use anyhow::Context as _;
use mytm::{TaskHandle, TaskManager};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, mpsc, watch};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
const GC_INTERVAL: Duration = Duration::from_secs(60);

/// `Pipeline::new` 的全部外部参数。
///
/// 参数都来自命令行；打包成一个结构体后，新增选项只改这里和 `main.rs`，
/// 不用再动那串 11 个位置参数。
pub struct PipelineConfig {
    pub args: PrintArgs,
    pub fonts: Arc<FontSet>,
    pub state_dir: PathBuf,
    pub render_workers: usize,
    pub queue_capacity: usize,
    pub print_queue_capacity: usize,
    pub max_job_bytes: usize,
    pub max_repeats: u32,
    pub job_ttl: Duration,
}

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
    pub fn new(
        tm: &TaskManager,
        root: CancellationToken,
        config: PipelineConfig,
    ) -> anyhow::Result<Self> {
        let PipelineConfig {
            args,
            fonts,
            state_dir,
            render_workers,
            queue_capacity,
            print_queue_capacity,
            max_job_bytes,
            max_repeats,
            job_ttl,
        } = config;

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
            let ctx = RenderCtx {
                tm: tm.clone(),
                store: store.clone(),
                ingress: ingress_rx.clone(),
                print_tx: print_tx.clone(),
                profile_rx: profile_rx.clone(),
                args: args.clone(),
                fonts: fonts.clone(),
                package_cache: package_cache.clone(),
                token: root.child_token(),
            };

            let handle = tm.spawn_typed(move |_tm, _progress, task_token| async move {
                info!(worker = index, "render worker started");
                let result = render_worker(ctx, task_token).await;
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
            ApiConfig {
                temp_root: self.temp_root.clone(),
                accepting: self.accepting.clone(),
                max_job_bytes: self.max_job_bytes,
                max_repeats: self.max_repeats,
            },
        )
    }

    pub async fn shutdown(self) {
        self.accepting.store(false, Ordering::SeqCst);
        self.root.cancel();

        for handle in &self.render_handles {
            if tokio::time::timeout(SHUTDOWN_GRACE, handle.wait_terminal())
                .await
                .is_err()
            {
                warn!(timeout = ?SHUTDOWN_GRACE, "render worker did not stop in time");
            }
        }
        if tokio::time::timeout(SHUTDOWN_GRACE, self.gc_handle.wait_terminal())
            .await
            .is_err()
        {
            warn!(timeout = ?SHUTDOWN_GRACE, "gc worker did not stop in time");
        }
        if tokio::time::timeout(SHUTDOWN_GRACE, self.print_handle.wait_terminal())
            .await
            .is_err()
        {
            warn!(timeout = ?SHUTDOWN_GRACE, "print worker did not stop in time");
        }

        self.tm.close();
        info!(jobs = self.store.len(), "pipeline stopped");
    }
}

/// 一个渲染 worker 的固定依赖。
///
/// `token` 是 `root` 的子 token，root 取消时它一定也会取消，所以这里不再
/// 单独保存 root。
struct RenderCtx {
    tm: TaskManager,
    store: JobStore,
    ingress: Arc<AsyncMutex<mpsc::Receiver<JobSpec>>>,
    print_tx: mpsc::Sender<RenderedJob>,
    profile_rx: watch::Receiver<Option<PrinterProfile>>,
    args: PrintArgs,
    fonts: Arc<FontSet>,
    package_cache: PathBuf,
    token: CancellationToken,
}

async fn render_worker(ctx: RenderCtx, task_token: CancellationToken) -> anyhow::Result<()> {
    let RenderCtx {
        tm,
        store,
        ingress,
        print_tx,
        mut profile_rx,
        args,
        fonts,
        package_cache,
        token,
    } = ctx;

    loop {
        let job = {
            let mut rx = ingress.lock().await;
            tokio::select! {
                _ = cancelled(&task_token, &token) => break,
                job = rx.recv() => job,
            }
        };
        let Some(job) = job else {
            break;
        };

        let id = job.id();
        let cancel = job.cancel().clone();
        if is_cancelled(&cancel, &task_token, &token) {
            store.finish_cancelled(id);
            continue;
        }

        let Some(profile) = wait_profile(&mut profile_rx, &cancel, &task_token, &token).await
        else {
            store.finish_cancelled(id);
            if task_token.is_cancelled() || token.is_cancelled() {
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
                if is_cancelled(&rendered.cancel, &task_token, &token) {
                    store.finish_cancelled(id);
                    continue;
                }

                let send = print_tx.send(rendered);
                tokio::select! {
                    // 只是这个 job 被取消：打印 worker 还活着，继续接下一个。
                    _ = cancel.cancelled() => {
                        store.finish_cancelled(id);
                    }
                    // worker / 整个 pipeline 在退出：这个 job 作废后不再取新任务。
                    _ = cancelled(&task_token, &token) => {
                        store.finish_cancelled(id);
                        break;
                    }
                    result = send => {
                        if result.is_err() {
                            // 打印队列关闭说明打印 worker 已经退出，继续渲染没有意义。
                            store.finish_cancelled(id);
                            break;
                        }
                    }
                }
            }
            Err(error) => {
                if is_cancelled(&cancel, &task_token, &token) {
                    store.finish_cancelled(id);
                } else {
                    error!(job = %uuid::Uuid::from_bytes(id), %error, "render failed");
                    store.finish_render_failed(id, format!("{error:#}"));
                }
            }
        }
    }

    Ok(())
}

/// 等 worker token 或 pipeline token 任意一个取消。
///
/// 两个 token 的优先级在调用方看来是一样的（都是“该收摊了”），合并成一个
/// future 之后 `select!` 里不用重复写两遍相同的分支。
async fn cancelled(task_token: &CancellationToken, root: &CancellationToken) {
    tokio::select! {
        _ = task_token.cancelled() => {}
        _ = root.cancelled() => {}
    }
}

/// job token、worker token、pipeline token 任意一个已取消。
fn is_cancelled(
    job: &CancellationToken,
    task_token: &CancellationToken,
    root: &CancellationToken,
) -> bool {
    job.is_cancelled() || task_token.is_cancelled() || root.is_cancelled()
}

async fn wait_profile(
    rx: &mut watch::Receiver<Option<PrinterProfile>>,
    cancel: &CancellationToken,
    task_token: &CancellationToken,
    token: &CancellationToken,
) -> Option<PrinterProfile> {
    loop {
        if let Some(profile) = *rx.borrow() {
            return Some(profile);
        }
        tokio::select! {
            _ = cancel.cancelled() => return None,
            _ = cancelled(task_token, token) => return None,
            result = rx.changed() => {
                if result.is_err() {
                    warn!("printer profile channel closed");
                    return None;
                }
            }
        }
    }
}
