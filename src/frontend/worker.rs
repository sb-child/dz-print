// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 设备 worker：阻塞线程里的 USB 读写和命令区切帧。
//!
//! 前台只和 channel 打交道；worker 把切出来的原始 payload 回传给前台，
//! 由前台按响应类型解析。这样 `try_recv_one::<T>()` 解析失败时不会消费队列。
//!
//! 三个 channel 都来自 [`crossfire`]，按各自的跨线程方向选型：
//!
//! | 方向 | 通道 | 发送端 | 接收端 |
//! | --- | --- | --- | --- |
//! | 前台 → worker 写任务 | `mpmc::bounded_async_blocking(8)` | 异步、`Sync` | 阻塞 |
//! | worker → 前台 payload | `spsc::unbounded_async` | 阻塞 | 异步 |
//! | 单条写任务的 ack | `oneshot` | 阻塞 | 异步 |
//!
//! 写任务用多生产者是为了让 `Device` 在多任务里同时发送时仍然 `Sync`；
//! 同时只有一个 worker 在收，所以负载侧用单生产者的 `spsc` 就够了。
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

use crate::{
    backend::{ReadStatus, Transport, TransportError, WriteOutcome, usb::UsbTransport},
    command::ModelVersion,
    planner::Assembler,
};
use crossfire::{AsyncRx, MAsyncTx, MRx, TryRecvError, Tx, oneshot::TxOneshot, spsc};
use mytm::{ProgressUpdater, TaskHandle, TaskManager};
use pretty_hex::simple_hex;
use snafu::Snafu;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

/// 打开设备的工厂。测试可以注入 [`crate::backend`] 里的内存 transport。
pub(crate) type TransportFactory =
    Box<dyn FnMut() -> Result<Box<dyn Transport>, TransportError> + Send>;

/// worker 上报的状态。
///
/// 目前只有一个信号：设备已经打开。以后要加进度字段（已写字节、已切帧数）
/// 时，注意 `mytm` 只在 `Pending -> Running` 这一次通知等待者。
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WorkerProgress;

/// worker 的退出原因。
///
/// 单条命令的写失败通过 [`OutJob::ack`] 回给前台，不会作为 worker 的退出原因。
#[derive(Debug, Snafu)]
pub(crate) enum WorkerError {
    #[snafu(display("open device failed: {message}"))]
    Open { message: String },
    #[snafu(display("read failed: {message}"))]
    Read { message: String },
    #[snafu(display("device closed"))]
    Closed,
}

/// 写任务通道的容量。
const OUT_CAPACITY: usize = 8;

/// 前台 → worker 的写任务发送端（异步、可克隆、`Sync`）。
pub(crate) type OutTx = MAsyncTx<crossfire::mpmc::Array<OutJob>>;
/// worker 侧的写任务接收端（阻塞）。
pub(crate) type OutRx = MRx<crossfire::mpmc::Array<OutJob>>;
/// worker → 前台的原始 payload 发送端（阻塞）。
pub(crate) type RecvTx = Tx<spsc::List<Vec<u8>>>;
/// 前台侧的 payload 接收端（异步）。
pub(crate) type RecvRx = AsyncRx<spsc::List<Vec<u8>>>;
/// 单条写任务完成后回给前台的 ack。
pub(crate) type JobAck = TxOneshot<Result<(), String>>;

/// 建立前台与 worker 之间的全部通道。
pub(crate) fn channels() -> (OutTx, OutRx, RecvTx, RecvRx) {
    let (out_tx, out_rx) = crossfire::mpmc::bounded_async_blocking(OUT_CAPACITY);
    let (recv_tx, recv_rx) = spsc::unbounded_async();
    (out_tx, out_rx, recv_tx, recv_rx)
}

/// 后台线程收到的任务。
pub(crate) enum OutJobKind {
    /// 已编码的命令区（不含 USB 外层 `1e <长度>`）。
    Write(Vec<u8>),
    /// 丢弃当前 transport，重新 open 一次，相当于重启程序里的连接。
    Reset,
}

/// 一个待执行的任务。
pub(crate) struct OutJob {
    pub kind: OutJobKind,
    /// 完成后回报结果。
    pub ack: JobAck,
}

/// 托管的 worker 句柄。
pub(crate) type WorkerHandle = TaskHandle<WorkerProgress, (), WorkerError>;

/// 丢掉旧 transport 之后、重新 open 之前的静默时间。
///
/// DP27P 在开盖中止打印后会停止处理命令，重新 open 本身不能让它恢复，必须
/// 让连接真正断开一会儿。真机实测：断开 2s / 1s 后重连，第一次 `0x70` 就能
/// 读到 `CoverOpened`；几百毫秒就重连的话，连上以后 30s 也不回包。这里取 2s
/// 留余量。
#[cfg(not(test))]
pub(crate) const RESET_SETTLE: Duration = Duration::from_secs(2);
/// 单元测试用假 transport，没有固件要等。
#[cfg(test)]
pub(crate) const RESET_SETTLE: Duration = Duration::ZERO;

/// 用 USB transport 打开设备并启动 worker。
///
/// `tm` 由调用方在最外层创建并持有，worker 只是挂到它下面；这样退出时
/// `TaskManager::close()` 能统一取消所有任务。
pub(crate) fn spawn_usb(
    tm: &TaskManager,
    selector: crate::backend::usb::UsbSelector,
    mv: ModelVersion,
    out_rx: OutRx,
    recv_tx: RecvTx,
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
    out_rx: OutRx,
    recv_tx: RecvTx,
) -> WorkerHandle {
    tm.spawn_blocking_typed(move |_tm, progress, ct| {
        run(factory, mv, out_rx, recv_tx, progress, ct)
    })
}

fn run(
    mut factory: TransportFactory,
    mv: ModelVersion,
    out_rx: OutRx,
    recv_tx: RecvTx,
    progress: ProgressUpdater<WorkerProgress>,
    ct: CancellationToken,
) -> Result<(), WorkerError> {
    let mut transport = Some(match factory() {
        Ok(transport) => transport,
        Err(e) => {
            error!("open device failed: {e:?}");
            return Err(WorkerError::Open {
                message: e.to_string(),
            });
        }
    });
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
                    let mut fatal_reset = None;
                    let result = match job.kind {
                        OutJobKind::Write(cmd_area) => {
                            let transport = transport.as_mut().expect("transport is open");
                            match transport.write(&cmd_area) {
                                Ok(WriteOutcome::Written) => Ok(()),
                                // USB 不会返回 Blocked；真出现就先报错，避免静默丢包。
                                Ok(WriteOutcome::Blocked) => {
                                    Err("USB transport unexpectedly blocked".to_string())
                                }
                                Err(e) => Err(e.to_string()),
                            }
                        }
                        OutJobKind::Reset => {
                            // 先 drop 旧 transport：release interface / attach
                            // kernel driver，让连接真正断开。等固件把上一页
                            // 收尾之后再重新 open，否则新连接上它也不回包。
                            debug!("resetting device transport");
                            drop(transport.take());
                            info!(settle = ?RESET_SETTLE, "waiting before reopening the device");
                            std::thread::sleep(RESET_SETTLE);
                            match factory() {
                                Ok(new_transport) => {
                                    transport = Some(new_transport);
                                    received.clear();
                                    info!("device transport reopened");
                                    Ok(())
                                }
                                Err(error) => {
                                    let message = error.to_string();
                                    fatal_reset = Some(message.clone());
                                    Err(message)
                                }
                            }
                        }
                    };
                    if let Err(e) = &result {
                        error!("command failed: {e}");
                    }
                    job.ack.send(result);
                    if let Some(message) = fatal_reset {
                        return Err(WorkerError::Open { message });
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    debug!("frontend closed, device thread exits");
                    return Ok(());
                }
            }
        }

        match transport
            .as_mut()
            .expect("transport is open")
            .read_chunk(&mut received)
        {
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
                return Err(WorkerError::Read {
                    message: e.to_string(),
                });
            }
        }
    }
}

/// 从接收缓冲区里切出所有完整响应，回传给前台。
///
/// 返回 `false` 表示前台已经消失，后台线程该退出了。
fn drain_frames(assembler: &Assembler, received: &mut Vec<u8>, tx: &RecvTx) -> bool {
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
