// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 设备 worker：阻塞线程里的 USB 读写和命令区切帧。
//!
//! 前台只和 channel 打交道；worker 把切出来的原始 payload 回传给前台，
//! 由前台按响应类型解析。这样 `try_recv_one::<T>()` 解析失败时不会消费队列。

use pretty_hex::simple_hex;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, error, trace, warn};

use crate::{
    backend::{ReadStatus, Transport, TransportError, WriteOutcome, usb::UsbTransport},
    command::ModelVersion,
    planner::Assembler,
};

/// 打开设备的工厂。测试可以注入 [`crate::backend`] 里的内存 transport。
pub(crate) type TransportFactory =
    Box<dyn FnOnce() -> Result<Box<dyn Transport>, TransportError> + Send>;

/// 后台线程收到的写任务。
pub(crate) struct OutJob {
    /// 已编码的命令区（不含 USB 外层 `1e <长度>`）。
    pub cmd_area: Vec<u8>,
    /// 写完后回报结果。
    pub ack: oneshot::Sender<Result<(), String>>,
}

/// 设备就绪状态。
#[derive(Debug, Clone)]
pub(crate) enum ReadyStatus {
    Pending,
    Ready,
    Failed(String),
}

/// 用 USB transport 打开设备并启动 worker。
pub(crate) fn spawn_usb(
    selector: crate::backend::usb::UsbSelector,
    mv: ModelVersion,
    out_rx: mpsc::Receiver<OutJob>,
    recv_tx: mpsc::UnboundedSender<Vec<u8>>,
    ready_tx: watch::Sender<ReadyStatus>,
) -> tokio::task::JoinHandle<()> {
    let factory: TransportFactory = Box::new(move || {
        UsbTransport::open(&selector).map(|transport| Box::new(transport) as Box<dyn Transport>)
    });
    spawn(factory, mv, out_rx, recv_tx, ready_tx)
}

/// 用给定 transport 工厂启动 worker。
pub(crate) fn spawn(
    factory: TransportFactory,
    mv: ModelVersion,
    out_rx: mpsc::Receiver<OutJob>,
    recv_tx: mpsc::UnboundedSender<Vec<u8>>,
    ready_tx: watch::Sender<ReadyStatus>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || run(factory, mv, out_rx, recv_tx, ready_tx))
}

fn run(
    factory: TransportFactory,
    mv: ModelVersion,
    mut out_rx: mpsc::Receiver<OutJob>,
    recv_tx: mpsc::UnboundedSender<Vec<u8>>,
    ready_tx: watch::Sender<ReadyStatus>,
) {
    let mut transport = match factory() {
        Ok(transport) => {
            ready_tx.send_replace(ReadyStatus::Ready);
            transport
        }
        Err(e) => {
            error!("open device failed: {e:?}");
            ready_tx.send_replace(ReadyStatus::Failed(e.to_string()));
            return;
        }
    };

    let assembler = Assembler::new(mv);
    let mut received = Vec::new();

    loop {
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
                    return;
                }
            }
        }

        match transport.read_chunk(&mut received) {
            Ok(ReadStatus::Data) => {
                if !drain_frames(&assembler, &mut received, &recv_tx) {
                    debug!("frontend closed, device thread exits");
                    return;
                }
            }
            Ok(ReadStatus::Timeout) => {}
            Ok(ReadStatus::Closed) => {
                error!("device closed");
                return;
            }
            Err(e) => {
                error!("read failed: {e:?}");
                return;
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
