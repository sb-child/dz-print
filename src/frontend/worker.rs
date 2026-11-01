// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 设备 worker：阻塞线程里的 USB 读写和命令区切帧。
//!
//! 前台只和 channel 打交道；worker 把切出来的原始 payload 回传给前台，
//! 由前台按响应类型解析。这样 `try_recv_one::<T>()` 解析失败时不会消费队列。
//!
//! worker 由 [`mytm::TaskManager`] 托管，而不是裸的 `spawn_blocking`：
//! 调用方可以 `cancel()`，再用 `wait_terminal()` 等阻塞线程真正返回，
//! panic 也会被记录成 `TaskState::Panicked`，不会静默 detach。
//!
//! 约定：[`run`] 只在设备成功打开后调用一次 [`ProgressUpdater::update`]，
//! 任务因此从 `Pending` 进入 `Running`。[`crate::frontend::Device::ready`]
//! 就是等这个状态，所以不需要单独的 ready 通道。
//!
//! 注意 `mytm` 只在 `Pending -> Running` 这一次转换通知等待者；之后更新
//! `WorkerProgress` 只会刷新值，不会唤醒 `wait_for`，要读进度得轮询。

use mytm::{ProgressUpdater, TaskHandle, TaskManager};
use pretty_hex::simple_hex;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, trace, warn};

use crate::{
    backend::{ReadStatus, Transport, TransportError, WriteOutcome, usb::UsbTransport},
    command::ModelVersion,
    planner::Assembler,
};

/// 打开设备的工厂。测试可以注入 [`crate::backend`] 里的内存 transport。
pub(crate) type TransportFactory =
    Box<dyn FnOnce() -> Result<Box<dyn Transport>, TransportError> + Send>;

/// worker 上报的状态。
///
/// 目前只有一个信号：设备已经打开。以后要加进度字段（已写字节、已切帧数）
/// 时，注意 `mytm` 只在 `Pending -> Running` 这一次通知等待者。
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WorkerProgress;

/// worker 的退出原因。
///
/// 单条命令的写失败通过 [`OutJob::ack`] 回给前台，不会作为 worker 的退出原因。
#[derive(Debug, Error)]
pub(crate) enum WorkerError {
    #[error("open device failed: {0}")]
    Open(String),
    #[error("read failed: {0}")]
    Read(String),
    #[error("device closed")]
    Closed,
}

/// 后台线程收到的写任务。
pub(crate) struct OutJob {
    /// 已编码的命令区（不含 USB 外层 `1e <长度>`）。
    pub cmd_area: Vec<u8>,
    /// 写完后回报结果。
    pub ack: oneshot::Sender<Result<(), String>>,
}

/// 托管的 worker 句柄。
pub(crate) type WorkerHandle = TaskHandle<WorkerProgress, (), WorkerError>;

/// 用 USB transport 打开设备并启动 worker。
///
/// `tm` 由调用方在最外层创建并持有，worker 只是挂到它下面；这样退出时
/// `TaskManager::close()` 能统一取消所有任务。
pub(crate) fn spawn_usb(
    tm: &TaskManager,
    selector: crate::backend::usb::UsbSelector,
    mv: ModelVersion,
    out_rx: mpsc::Receiver<OutJob>,
    recv_tx: mpsc::UnboundedSender<Vec<u8>>,
) -> WorkerHandle {
    let factory: TransportFactory = Box::new(move || {
        UsbTransport::open(&selector).map(|transport| Box::new(transport) as Box<dyn Transport>)
    });
    spawn(tm, factory, mv, out_rx, recv_tx)
}

/// 用给定 transport 工厂启动 worker。
///
/// 必须在 tokio runtime 里调用：`mytm` 内部仍用 `tokio::task::spawn_blocking`。
pub(crate) fn spawn(
    tm: &TaskManager,
    factory: TransportFactory,
    mv: ModelVersion,
    out_rx: mpsc::Receiver<OutJob>,
    recv_tx: mpsc::UnboundedSender<Vec<u8>>,
) -> WorkerHandle {
    tm.spawn_blocking_typed(move |_tm, progress, ct| {
        run(factory, mv, out_rx, recv_tx, progress, ct)
    })
}

fn run(
    factory: TransportFactory,
    mv: ModelVersion,
    mut out_rx: mpsc::Receiver<OutJob>,
    recv_tx: mpsc::UnboundedSender<Vec<u8>>,
    progress: ProgressUpdater<WorkerProgress>,
    ct: CancellationToken,
) -> Result<(), WorkerError> {
    let mut transport = match factory() {
        Ok(transport) => transport,
        Err(e) => {
            error!("open device failed: {e:?}");
            return Err(WorkerError::Open(e.to_string()));
        }
    };
    // 唯一一次 `Pending -> Running`：`Device::ready()` 用它判断设备已打开。
    progress.update(WorkerProgress);

    let assembler = Assembler::new(mv);
    let mut received = Vec::new();

    loop {
        if ct.is_cancelled() {
            debug!("worker cancelled");
            return Ok(());
        }

        // 先处理所有待写任务。
        loop {
            match out_rx.try_recv() {
                Ok(job) => {
                    let result = match transport.write(&job.cmd_area) {
                        Ok(WriteOutcome::Written) => Ok(()),
                        // USB 不会返回 Blocked；真出现就先报错，避免静默丢包。
                        Ok(WriteOutcome::Blocked) => {
                            Err("USB transport unexpectedly blocked".to_string())
                        }
                        Err(e) => Err(e.to_string()),
                    };
                    if let Err(e) = &result {
                        error!("write failed: {e}");
                    }
                    job.ack.send(result).ok();
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    debug!("frontend closed, device thread exits");
                    return Ok(());
                }
            }
        }

        match transport.read_chunk(&mut received) {
            Ok(ReadStatus::Data) => {
                if !drain_frames(&assembler, &mut received, &recv_tx) {
                    debug!("frontend closed, device thread exits");
                    return Ok(());
                }
            }
            Ok(ReadStatus::Timeout) => {}
            Ok(ReadStatus::Closed) => {
                error!("device closed");
                return Err(WorkerError::Closed);
            }
            Err(e) => {
                error!("read failed: {e:?}");
                return Err(WorkerError::Read(e.to_string()));
            }
        }
    }
}

/// 从接收缓冲区里切出所有完整响应，回传给前台。
///
/// 返回 `false` 表示前台已经消失，后台线程该退出了。
fn drain_frames(
    assembler: &Assembler,
    received: &mut Vec<u8>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
) -> bool {
    loop {
        match assembler.decode_frame(received) {
            Ok(Some((payload, consumed))) => {
                received.drain(..consumed);
                trace!("response payload: {}", simple_hex(&payload));
                if tx.send(payload).is_err() {
                    return false;
                }
            }
            Ok(None) => return true,
            Err(e) => {
                // 测试平台不做重新同步，整段丢弃后继续读。
                warn!("drop invalid response frame: {e:?}");
                received.clear();
                return true;
            }
        }
    }
}
