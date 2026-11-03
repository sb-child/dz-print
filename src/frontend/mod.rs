// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 设备 frontend。
//!
//! [`Device`] 对调用方暴露连接、批量发送、按类型接收、等待打印完成和关闭；
//! 真正的 USB 读写与切帧在 `worker` 的阻塞线程里。前台只拿到原始 payload，
//! 解析失败时不会消费队列，可以换一个响应类型重试。
//!
//! ```text
//! 调用方 --try_send_batch--> Assembler::encode --> worker --> Transport
//! 调用方 <--try_recv_one--- payload 队列 <-- worker::drain_frames <-- Transport
//! ```
//!
//! worker 由 [`mytm`] 托管：[`Device::ready`] 等它的 `Running` 状态，
//! [`Device::shutdown`] 取消它并等阻塞线程真正退出（USB 句柄释放）。
//! 两边的通道都由 [`crossfire`] 提供，选型见 `worker` 模块文档。
//!
//! 打印任务的流控和分页见 `print` 模块（[`Device::print_bitmap_job`]）。
//!
//! # 用法
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use dz_print::{
//!     backend::usb::UsbSelector,
//!     frontend::{Device, DeviceConfig},
//! };
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let tm = mytm::TaskManager::new();
//! let device = Device::new(
//!     &tm,
//!     DeviceConfig {
//!         selector: UsbSelector::DeviceName("DP27P-Y4094C023".into()),
//!         ..Default::default()
//!     },
//! );
//! device.ready().await?;
//!
//! // 低层：批量发命令 + 按类型收响应
//! device
//!     .try_send_batch(&[&dz_print::command::req::status::Status {}])
//!     .await?;
//! let status = device
//!     .try_recv_one::<dz_print::command::resp::status::Status>()
//!     .await?;
//! println!("{:?}", status.status);
//!
//! device.shutdown(Duration::from_secs(3)).await?; // 等 worker 退出
//! tm.close();
//! # Ok(())
//! # }
//! ```
//!
//! - `DeviceConfig::default()` 用 `UsbSelector::Auto` 选第一台德佟设备
//! - 打印整张位图用 [`Device::print_bitmap_job`]，见 `print` 模块
//! - `shutdown` 之后 USB 句柄才释放；只 drop `Device` 不会等线程退出
//!
//! # 并发
//!
//! `Device` 是 `Send + Sync`，可以放进 `Arc` 跨 task 共享，但不同接口的并发
//! 约束不一样：
//!
//! - [`Device::try_send_batch`] / [`Device::send_cmd_area`]：可以并发；worker
//!   串行写，每个调用是一个完整写任务，内部不会和其它调用交叉，但调用之间的
//!   先后顺序不保证。
//! - [`Device::print_bitmap_job`]：同一时刻只允许一个，第二个调用返回
//!   [`FrontendError::PrintBusy`]。
//! - [`Device::recv_cmd_timeout`]：按命令字节取帧，可以多个 task 同时等不同
//!   命令；同一命令的帧只会交给其中一个调用者。
//! - [`Device::try_recv_one`] / [`Device::try_recv_one_timeout`] /
//!   [`Device::try_pop_raw`] / [`Device::recv_matching`]：从队列头部消费，
//!   不要和别的接收接口并发，否则会抢走别人的帧。
//! - [`Device::wait_for_print_done`]：只由结束打印的一方调用；
//!   `print_bitmap_job` 内部已经调用，外部不要再并发调用。
//! - [`Device::shutdown`]：可以重复调用；会让正在进行的打印返回错误。

use crate::{
    backend::usb::UsbSelector,
    command::{
        ModelVersion,
        req::{self, PackReqError, ReqTrait},
        resp::{self, ParseRespError, RespTrait},
        status::PrinterStatus,
    },
    planner::{Assembler, flow::FlowError, print::PrintError},
};
use crossfire::oneshot;
use futures_util::FutureExt;
use mytm::{TypedTaskState, TypedTaskStateRef};
use pretty_hex::simple_hex;
use snafu::{OptionExt, Snafu};
use std::{collections::VecDeque, time::Duration};
use tokio::sync::Mutex;
use tracing::{debug, error, info, trace, warn};

mod print;
mod worker;

pub use print::PrintJobOptions;

/// 响应接收错误。
#[derive(Debug, Snafu)]
pub enum RecvError {
    #[snafu(display("device thread is gone"))]
    Closed,
    #[snafu(display("timed out after {timeout:?} waiting for a response"))]
    Timeout { timeout: Duration },
    #[snafu(display("front of the buffer is not '{expected}': {source}"))]
    Parse {
        expected: &'static str,
        source: ParseRespError,
    },
}

/// frontend 错误。
#[derive(Debug, Snafu)]
pub enum FrontendError {
    #[snafu(display("device thread is gone"))]
    WorkerGone,
    #[snafu(display("device is not ready: {message}"))]
    NotReady { message: String },
    #[snafu(display("device write failed: {message}"))]
    Write { message: String },
    #[snafu(transparent)]
    Recv { source: RecvError },
    #[snafu(transparent)]
    Pack { source: PackReqError },
    #[snafu(transparent)]
    Print { source: PrintError },
    #[snafu(transparent)]
    Flow { source: FlowError },
    #[snafu(display("bitmap width {width} does not fit in u16"))]
    WidthOverflow { width: usize },
    #[snafu(display("bitmap needs {pages} pages, the device supports at most 65534"))]
    TooManyPages { pages: usize },
    #[snafu(display("page has {lines} lines, does not fit in u16"))]
    PageTooTall { lines: usize },
    #[snafu(display("printer reported {status} while waiting for print completion"))]
    PrinterStatus { status: PrinterStatus },
    #[snafu(display("print did not finish within {timeout:?}"))]
    PrintTimeout { timeout: Duration },
    #[snafu(display(
        "printer stopped answering status polls ({polls} timeouts of {timeout:?} in a row); \
         the cover may be open, paper may be out, or the device may be unplugged"
    ))]
    PrinterSilent { polls: usize, timeout: Duration },
    #[snafu(display(
        "printer did not start within {timeout:?}: no busy status and no print counter progress"
    ))]
    PrintDidNotStart { timeout: Duration },
    #[snafu(display("device worker did not stop within {timeout:?}"))]
    ShutdownTimeout { timeout: Duration },
    #[snafu(display("device worker did not answer within {timeout:?}"))]
    WorkerTimeout { timeout: Duration },
    #[snafu(display("a print job is already running"))]
    PrintBusy,
}

/// 前台状态：响应队列 + 未取走的原始 payload。
struct RecvState {
    rx: worker::RecvRx,
    queue: VecDeque<Vec<u8>>,
}

/// 响应队列的上限。
///
/// 按命令字节取响应时，不匹配的帧会留在队列里（例如 `0x7b` 主动推送）。
/// 数量超过这个值就丢掉最旧的一帧并记录，避免长任务里无限增长。
const MAX_QUEUED_FRAMES: usize = 64;

impl RecvState {
    fn push(&mut self, payload: Vec<u8>) {
        if self.queue.len() >= MAX_QUEUED_FRAMES
            && let Some(dropped) = self.queue.pop_front()
        {
            warn!(
                "response queue is full, drop the oldest frame: {}",
                simple_hex(&dropped)
            );
        }
        self.queue.push_back(payload);
    }
}

/// 任意 payload，用于按命令字节过滤响应。
struct RawPayload(Vec<u8>);

impl RespTrait for RawPayload {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        Ok(Self(payload.to_vec()))
    }
}

/// 设备连接参数。
#[derive(Debug, Clone)]
pub struct DeviceConfig {
    /// 设备选择方式。
    pub selector: UsbSelector,
    /// 机型版本。
    pub model_version: ModelVersion,
    /// 单次接收的默认超时。
    pub recv_timeout: Duration,
    /// 等待打印完成时轮询 `0x70` 的间隔。
    pub print_poll: Duration,
    /// `EndPage` 后的启动宽限期。过了这个时间还没看到忙碌状态，才会开始
    /// 用 `0x73` 判断这一页是不是已经打印过了。
    pub print_start_grace: Duration,
    /// 没看到忙碌状态、`0x73` 也没有增长时，最多等多久就判定打印没有开始。
    ///
    /// 固件从 `EndPage` 到电机真正启动可能超过 1 秒，所以不能一过
    /// [`Self::print_start_grace`] 就返回成功。超过这个时间仍然没有
    /// `Printing` / `Rotating`，也没有打印计数增量，返回
    /// [`FrontendError::PrintDidNotStart`]。
    pub print_start_timeout: Duration,
    /// 没看到忙碌状态时，两次 `0x73` 校验之间的间隔。
    pub print_verify_interval: Duration,
    /// 是否用 `0x73` 校验打印是否真的发生。
    ///
    /// 打开后，如果一直没看到 `Printing` / `Rotating`，会在
    /// [`Self::print_start_grace`] 之后比较 `0x73` 的累计值：有增长说明页面
    /// 打得太快、只是没采样到忙碌状态；没有增长就继续等，直到
    /// [`Self::print_start_timeout`] 返回 [`FrontendError::PrintDidNotStart`]。
    ///
    /// `EndPage` 前的 baseline 查询失败时退回旧行为（过了宽限期就接受空闲）；
    /// 后续校验查询失败不会直接判失败，而是继续轮询到超时。
    pub print_verify: bool,
    /// 至少连续几次空闲才认为打印完成。
    pub print_idle_confirm: usize,
    /// 连续多少次状态查询没有任何回应就判定打印机失联。
    ///
    /// 每次查询最长等 [`Self::recv_timeout`]，所以默认 `5` 次大约 15 秒。
    /// 打印中途固件偶尔会停一小会儿不回应，但连续十几秒完全沉默说明它卡住了，
    /// 继续等只会让整个任务挂到 [`Self::print_done_timeout`]。
    pub print_silent_limit: usize,
    /// 等待打印完成的总超时。
    pub print_done_timeout: Duration,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            selector: UsbSelector::Auto,
            model_version: ModelVersion::V2,
            recv_timeout: Duration::from_secs(3),
            print_poll: Duration::from_millis(100),
            print_start_grace: Duration::from_secs(1),
            print_start_timeout: Duration::from_secs(3),
            print_verify_interval: Duration::from_millis(500),
            print_verify: true,
            print_idle_confirm: 2,
            print_silent_limit: 5,
            print_done_timeout: Duration::from_secs(600),
        }
    }
}

/// 一台打印机。
///
/// `Device` 是 `Send + Sync`，接口之间的并发约束见模块文档的「并发」一节。
pub struct Device {
    mv: ModelVersion,
    assembler: Assembler,
    config: DeviceConfig,
    out_tx: worker::OutTx,
    recv: Mutex<RecvState>,
    /// 同一时刻只允许一个 [`Device::print_bitmap_job`]。
    print_lock: Mutex<()>,
    worker: worker::WorkerHandle,
}

impl Device {
    /// 按配置连接设备。
    ///
    /// `tm` 是调用方在最外层持有的 [`mytm::TaskManager`]，worker 会挂到它下面；
    /// 退出前调用 `tm.close()` 可以统一取消所有任务。
    ///
    /// 必须在 tokio runtime 里调用：worker 内部仍用 `spawn_blocking`。
    pub fn new(tm: &mytm::TaskManager, config: DeviceConfig) -> Self {
        let (out_tx, out_rx, recv_tx, recv_rx) = worker::channels();

        let worker = worker::spawn_usb(
            tm,
            config.selector.clone(),
            config.model_version,
            out_rx,
            recv_tx,
        );

        Self::from_parts(config, out_tx, recv_rx, worker)
    }

    /// 按 USB 设备名（`型号-序列号`）连接，按 `v2` 机型处理。
    pub fn new_by_name(tm: &mytm::TaskManager, name: impl Into<String>) -> Self {
        Self::new(
            tm,
            DeviceConfig {
                selector: UsbSelector::DeviceName(name.into()),
                ..Default::default()
            },
        )
    }

    /// 指定设备选择和机型版本。
    pub fn new_with(tm: &mytm::TaskManager, selector: UsbSelector, mv: ModelVersion) -> Self {
        Self::new(
            tm,
            DeviceConfig {
                selector,
                model_version: mv,
                ..Default::default()
            },
        )
    }

    /// 用现成的 transport 创建设备，主要给单元测试使用。
    #[cfg(test)]
    fn new_with_transport(
        tm: &mytm::TaskManager,
        transport: impl crate::backend::Transport + 'static,
        config: DeviceConfig,
    ) -> Self {
        Self::new_with_factory(tm, move || Ok(Box::new(transport)), config)
    }

    /// 用可能失败的 transport 工厂创建设备，用来测打开失败。
    #[cfg(test)]
    fn new_with_factory(
        tm: &mytm::TaskManager,
        factory: impl FnOnce() -> Result<
            Box<dyn crate::backend::Transport>,
            crate::backend::TransportError,
        > + Send
        + 'static,
        config: DeviceConfig,
    ) -> Self {
        let (out_tx, out_rx, recv_tx, recv_rx) = worker::channels();

        let factory: worker::TransportFactory = Box::new(factory);
        let worker = worker::spawn(tm, factory, config.model_version, out_rx, recv_tx);

        Self::from_parts(config, out_tx, recv_rx, worker)
    }

    fn from_parts(
        config: DeviceConfig,
        out_tx: worker::OutTx,
        recv_rx: worker::RecvRx,
        worker: worker::WorkerHandle,
    ) -> Self {
        Self {
            mv: config.model_version,
            assembler: Assembler::new(config.model_version),
            config,
            out_tx,
            recv: Mutex::new(RecvState {
                rx: recv_rx,
                queue: VecDeque::new(),
            }),
            print_lock: Mutex::new(()),
            worker,
        }
    }

    pub fn model_version(&self) -> ModelVersion {
        self.mv
    }

    /// 等待设备就绪。
    ///
    /// worker 在 transport 打开成功后上报一次 `WorkerProgress`，
    /// 任务因此进入 `Running`；打开失败、panic 或提前退出都会转换成
    /// [`FrontendError`]。
    pub async fn ready(&self) -> Result<(), FrontendError> {
        match self
            .worker
            .wait_for(|state| matches!(state, TypedTaskStateRef::Running(_)))
            .await
        {
            TypedTaskState::Running(_) => Ok(()),
            TypedTaskState::Failed(e) => Err(FrontendError::NotReady {
                message: e.to_string(),
            }),
            TypedTaskState::Panicked(msg) => Err(FrontendError::NotReady {
                message: format!("worker panicked: {msg}"),
            }),
            TypedTaskState::Pending => Err(FrontendError::NotReady {
                message: "worker is still pending".into(),
            }),
            TypedTaskState::Cancelling | TypedTaskState::Cancelled => {
                Err(FrontendError::WorkerGone)
            }
            TypedTaskState::Completed(_) | TypedTaskState::Invalid => {
                Err(FrontendError::WorkerGone)
            }
        }
    }

    /// 取消 worker 并等它真正退出。
    ///
    /// 阻塞线程返回后 USB 句柄才释放，所以退出程序前应该调用一次。等待超过
    /// `timeout` 会返回 [`FrontendError::ShutdownTimeout`]；worker 已经因为
    /// IO 错误或 panic 停下时，只记日志并视为已停止。
    ///
    /// 可以重复调用。正在进行的 [`Device::print_bitmap_job`] 会因为通道关闭
    /// 返回错误，不会在 shutdown 之后继续往设备写。
    pub async fn shutdown(&self, timeout: Duration) -> Result<(), FrontendError> {
        self.worker.cancel();
        match tokio::time::timeout(timeout, self.worker.wait_terminal()).await {
            Err(_) => Err(FrontendError::ShutdownTimeout { timeout }),
            Ok(TypedTaskState::Failed(e)) => {
                warn!("device worker stopped with error: {e}");
                Ok(())
            }
            Ok(TypedTaskState::Panicked(msg)) => {
                warn!("device worker panicked: {msg}");
                Ok(())
            }
            Ok(_) => Ok(()),
        }
    }

    /// 批量发送命令。
    ///
    /// 所有命令会被 [`Assembler::encode`] 拼成一段命令区，再交给设备线程按
    /// `Transport::write` 发出；写失败会立即返回错误。
    ///
    /// 可以在多个 task 里并发调用：worker 串行写，每批是一个完整写任务，
    /// 内部不会和别的调用交叉；不同批之间的先后顺序不保证。
    pub async fn try_send_batch(&self, cmds: &[&dyn ReqTrait]) -> Result<(), FrontendError> {
        let cmd_area = self.assembler.encode(cmds)?;
        debug!(
            commands = cmds.len(),
            bytes = cmd_area.len(),
            "encoded command area"
        );
        self.send_cmd_area(cmd_area).await
    }

    /// 直接发送一段已编码的命令区。
    ///
    /// 写任务在 [`DeviceConfig::recv_timeout`] 内没有完成时返回
    /// [`FrontendError::WorkerTimeout`]：worker 卡在 USB 清理或设备掉线时，
    /// 前台不能无限等 ack。
    ///
    /// 和 [`Device::try_send_batch`] 一样可以并发调用；每次调用是一个完整
    /// 写任务，和别的调用之间不会交叉。
    pub async fn send_cmd_area(&self, cmd_area: Vec<u8>) -> Result<(), FrontendError> {
        if cmd_area.is_empty() {
            return Ok(());
        }
        debug!(bytes = cmd_area.len(), "write command area");
        trace!("write command area: {}", simple_hex(&cmd_area));

        let (ack_tx, ack_rx) = oneshot::oneshot();

        let sent = async {
            self.out_tx
                .send(worker::OutJob {
                    cmd_area,
                    ack: ack_tx,
                })
                .map(|r| r.ok().context(WorkerGoneSnafu))
                .await?;
            ack_rx
                .map(|r| r.ok().context(WorkerGoneSnafu))
                .await?
                .map_err(|message| FrontendError::Write { message })
        };
        let timeout = self.config.recv_timeout;
        match tokio::time::timeout(timeout, sent).await {
            Ok(result) => result,
            Err(_) => Err(FrontendError::WorkerTimeout { timeout }),
        }
    }

    /// 等打印机把已经发送的页真正打完。
    ///
    /// 每 [`DeviceConfig::print_poll`] 查一次 `0x70`：先看到忙、再连续几次空闲
    /// 才算完成；错误状态直接返回。只认 `0x70` 的帧，`0x7b` 之类的推送会跳过。
    ///
    /// 如果一直没有看到忙，不会立刻认为完成：
    ///
    /// - `print_verify` 打开时，在 [`DeviceConfig::print_start_grace`] 之后
    ///   比较 `0x73` 的累计值；有增长说明页面已经打印（只是太快没采样到忙），
    ///   没有增长就继续等，直到 [`DeviceConfig::print_start_timeout`] 返回
    ///   [`FrontendError::PrintDidNotStart`]。
    /// - `print_verify` 关闭或 `0x73` 查询失败时退回旧行为：过了宽限期就接受空闲。
    ///
    /// 只在最后一页的 `EndPage` 之后调用，由结束打印的一方独占；期间不要用
    /// `try_recv_one` / `try_pop_raw` 从队头取帧，否则会抢走 `0x70` 的响应。
    pub async fn wait_for_print_done(
        &self,
        timeout: Duration,
    ) -> Result<resp::status::Status, FrontendError> {
        let baseline = if self.config.print_verify {
            self.read_statistics(self.config.recv_timeout.min(timeout))
                .await
        } else {
            None
        };
        self.wait_for_print_done_with(baseline, timeout).await
    }

    /// [`Self::wait_for_print_done`] 的内部实现。
    ///
    /// `baseline` 是 `EndPage` 之前的 `0x73` 快照；`None` 表示不做计数校验。
    async fn wait_for_print_done_with(
        &self,
        baseline: Option<resp::statistics::Statistics>,
        timeout: Duration,
    ) -> Result<resp::status::Status, FrontendError> {
        let started = tokio::time::Instant::now();
        let deadline = started + timeout;
        let poll = self.config.print_poll.max(Duration::from_millis(1));
        let mut seen_busy = false;
        let mut idle_polls = 0usize;
        // 连续多少次查询没有回包。固件卡住时它连 `0x70` 都不回，
        // 不设上限的话只能等到整个任务超时。
        let mut silent_polls = 0usize;
        // 只在状态变化时打日志，避免 100ms 轮询把日志刷满。
        let mut logged: Option<PrinterStatus> = None;
        // 没看到忙碌状态时，用 `0x73` 判断页面是不是已经打印过了。
        let mut last_verify = started;
        let verify = self.config.print_verify && baseline.is_some();

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(FrontendError::PrintTimeout { timeout });
            }
            tokio::time::sleep(poll.min(remaining)).await;

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(FrontendError::PrintTimeout { timeout });
            }
            self.try_send_batch(&[&req::status::Status {}]).await?;

            let status = match self
                .recv_cmd_timeout::<resp::status::Status>(
                    resp::status::CMD,
                    remaining.min(self.config.recv_timeout),
                )
                .await
            {
                Ok(Some(status)) => {
                    silent_polls = 0;
                    status
                }
                // 单次查询超时、或只收到 `0x7b` 之类的推送，不代表打印结束。
                Ok(None) => {
                    silent_polls += 1;
                    // 页面刚结束时固件偶尔会漏答一次，不算异常；连续两次再警告。
                    if silent_polls == 1 {
                        debug!(
                            limit = self.config.print_silent_limit,
                            timeout = ?self.config.recv_timeout,
                            "printer did not answer the first status poll"
                        );
                    } else {
                        warn!(
                            silent_polls,
                            limit = self.config.print_silent_limit,
                            timeout = ?self.config.recv_timeout,
                            "printer did not answer the status poll"
                        );
                    }
                    if silent_polls >= self.config.print_silent_limit {
                        return Err(FrontendError::PrinterSilent {
                            polls: silent_polls,
                            timeout: self.config.recv_timeout,
                        });
                    }
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            debug!(
                status = ?status.status,
                page_key = status.page_key,
                seen_busy,
                idle_polls,
                "print status"
            );

            // 状态变化对用户有意义：`printing` → `cover is open` 这类要能直接看到。
            if logged != Some(status.status) {
                if status.status.is_error() {
                    error!(
                        status = ?status.status,
                        reason = %status.status,
                        page_key = status.page_key,
                        "printer stopped with an error"
                    );
                } else if matches!(
                    status.status,
                    PrinterStatus::Printing | PrinterStatus::Rotating
                ) {
                    info!(
                        status = ?status.status,
                        reason = %status.status,
                        "printer is busy"
                    );
                } else {
                    debug!(
                        status = ?status.status,
                        reason = %status.status,
                        "print status"
                    );
                }
                logged = Some(status.status);
            }

            if status.status.is_error() {
                return Err(FrontendError::PrinterStatus {
                    status: status.status,
                });
            }

            if matches!(
                status.status,
                PrinterStatus::Printing | PrinterStatus::Rotating | PrinterStatus::PageNotReady
            ) {
                seen_busy = true;
                idle_polls = 0;
                continue;
            }

            idle_polls += 1;

            if seen_busy {
                if idle_polls >= self.config.print_idle_confirm {
                    return Ok(status);
                }
                continue;
            }

            // 还没看到忙碌状态。固件可能只是还没开始转电机，不能马上认为完成。
            let elapsed = started.elapsed();
            if !verify {
                // 没有校验手段时保持旧行为。
                if elapsed >= self.config.print_start_grace
                    && idle_polls >= self.config.print_idle_confirm
                {
                    return Ok(status);
                }
                continue;
            }

            if elapsed < self.config.print_start_grace {
                continue;
            }

            if tokio::time::Instant::now().duration_since(last_verify)
                < self.config.print_verify_interval
            {
                continue;
            }
            last_verify = tokio::time::Instant::now();

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let before = baseline.as_ref().expect("verify requires a baseline");
            if let Some(stats) = self
                .read_statistics(remaining.min(self.config.recv_timeout))
                .await
                && statistics_progress(before, &stats)
            {
                info!(
                    print_pages = stats.print_pages,
                    print_lines = stats.print_lines,
                    "printer made print progress without a visible busy status"
                );
                // 页面确实打印过；按正常完成逻辑再确认几次空闲，避免在它
                // 还在走纸时提前返回。
                seen_busy = true;
                idle_polls = 0;
                continue;
            }

            if started.elapsed() >= self.config.print_start_timeout {
                return Err(FrontendError::PrintDidNotStart {
                    timeout: self.config.print_start_timeout,
                });
            }
        }
    }

    /// 读一次 `0x73` 打印统计。
    ///
    /// 超时、解析失败或写失败都返回 `None`。打印结束判定把 `None` 当作
    /// “没有校验手段”，退回旧行为，而不是直接判失败。
    async fn read_statistics(&self, timeout: Duration) -> Option<resp::statistics::Statistics> {
        if self
            .try_send_batch(&[&req::statistics::Statistics {}])
            .await
            .is_err()
        {
            return None;
        }
        self.recv_cmd_timeout::<resp::statistics::Statistics>(resp::statistics::CMD, timeout)
            .await
            .ok()
            .flatten()
    }

    /// 接收指定类型的回复。
    ///
    /// 解析失败时不会弹出缓冲区里的 payload，方便换一个类型重试。
    ///
    /// 从队列头部消费，不按命令字节匹配。多个 task 同时调用时谁先拿到锁谁
    /// 取队头，可能拿走别人要的响应；需要多 task 并发接收时用
    /// [`Self::recv_cmd_timeout`]。
    pub async fn try_recv_one<T: RespTrait>(&self) -> Result<T, RecvError> {
        self.try_recv_one_timeout::<T>(self.config.recv_timeout)
            .await
    }

    /// 带超时的 [`Self::try_recv_one`]。
    ///
    /// 并发约束和 [`Self::try_recv_one`] 相同：从队头消费，不要和其它接收
    /// 接口并发。
    pub async fn try_recv_one_timeout<T: RespTrait>(
        &self,
        timeout: Duration,
    ) -> Result<T, RecvError> {
        let mut state = self.recv.lock().await;
        loop {
            while let Ok(payload) = state.rx.try_recv() {
                state.push(payload);
            }

            if let Some(payload) = state.queue.front() {
                match T::parse(payload, self.mv) {
                    Ok(value) => {
                        state.queue.pop_front();
                        return Ok(value);
                    }
                    Err(source) => {
                        return Err(RecvError::Parse {
                            expected: std::any::type_name::<T>(),
                            source,
                        });
                    }
                }
            }

            match tokio::time::timeout(timeout, state.rx.recv()).await {
                Ok(Ok(payload)) => state.push(payload),
                Ok(Err(_)) => return Err(RecvError::Closed),
                Err(_) => return Err(RecvError::Timeout { timeout }),
            }
        }
    }

    /// 按命令字节接收并解析响应。
    ///
    /// 和 [`Self::try_recv_one_timeout`] 不同，这里会在整个队列里找
    /// `payload[1] == cmd` 的帧，**不消费**其它命令的帧：
    ///
    /// - `0x70` / `0x77` 这类查询的应答可能被 `0x7b` 主动推送插在中间；
    /// - 预算查询同一帧里带两条命令，应答顺序也不保证。
    ///
    /// 找不到时等到 `timeout`，返回 `Ok(None)`；worker 已经退出时返回
    /// [`RecvError::Closed`]。
    ///
    /// 可以多个 task 并发等待不同命令；同一命令的帧只会交给其中一个调用者。
    /// [`Self::try_recv_one`] / [`Self::try_pop_raw`] 会从队头消费，和这里并发
    /// 时可能抢走帧。
    pub async fn recv_cmd_timeout<T: RespTrait>(
        &self,
        cmd: u8,
        timeout: Duration,
    ) -> Result<Option<T>, RecvError> {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut state = self.recv.lock().await;
        loop {
            while let Ok(payload) = state.rx.try_recv() {
                state.push(payload);
            }

            if let Some(index) = state.queue.iter().position(|p| p.get(1) == Some(&cmd)) {
                let payload = state.queue.remove(index).expect("index from position()");
                return T::parse(&payload, self.mv)
                    .map(Some)
                    .map_err(|source| RecvError::Parse {
                        expected: std::any::type_name::<T>(),
                        source,
                    });
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            match tokio::time::timeout(remaining, state.rx.recv()).await {
                Ok(Ok(payload)) => state.push(payload),
                Ok(Err(_)) => return Err(RecvError::Closed),
                Err(_) => return Ok(None),
            }
        }
    }

    /// 丢弃并返回缓冲区头部的一条原始 payload。
    ///
    /// 只用于解析失败后恢复对齐；正常业务不应该依赖它。会消费队头，不要和
    /// 其它接收接口并发。
    pub async fn try_pop_raw(&self) -> Option<Vec<u8>> {
        let mut state = self.recv.lock().await;
        while let Ok(payload) = state.rx.try_recv() {
            state.push(payload);
        }
        state.queue.pop_front()
    }

    /// 等一条命令字节匹配的原始响应；前面不匹配的帧会被丢弃并记录。
    ///
    /// 用来在解析失败或晚到时恢复缓冲区对齐，也用来跳过 `0x7b` 这类主动推送。
    /// 内部走 [`Self::try_recv_one_timeout`]，会消费/丢弃队头帧，不要和其它
    /// 接收接口并发。
    pub async fn recv_matching(
        &self,
        cmd: u8,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>, RecvError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            match self.try_recv_one_timeout::<RawPayload>(remaining).await {
                Ok(raw) => {
                    let actual = raw.0.get(1).copied();
                    if actual == Some(cmd) {
                        return Ok(Some(raw.0));
                    }
                    warn!(
                        "期望 {cmd:#04x}，丢弃不匹配的头部帧 {:#04x}: {}",
                        actual.unwrap_or(0x00),
                        simple_hex(&raw.0)
                    );
                }
                Err(RecvError::Timeout { .. }) => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }
}

/// 判断 `0x73` 计数器有没有前进。
///
/// `PrintLines` / `PrintPages` 是 `u32`，长时间使用后会溢出到 0，所以不能
/// 用 `after > before` 判断进度：只要值发生了变化就算有进展。打印机复位
/// 也会被当成进展，这里宁可漏报也不误杀正常打印。
fn statistics_progress(
    before: &resp::statistics::Statistics,
    after: &resp::statistics::Statistics,
) -> bool {
    after.print_pages != before.print_pages || after.print_lines != before.print_lines
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex as StdMutex},
        thread,
        time::Duration,
    };

    use crate::{
        backend::{ReadStatus, Transport, TransportError, WriteOutcome},
        bitmap::Bitmap,
        command::{calculate_checksum, req, resp, status::PrinterStatus},
        planner::print::RleMode,
    };

    use super::{Device, DeviceConfig, FrontendError, RecvError, statistics_progress};

    /// 内存 transport：记录写出的命令区，按队列回放响应帧。
    #[derive(Clone, Default)]
    struct TestTransport {
        written: Arc<StdMutex<Vec<u8>>>,
        incoming: Arc<StdMutex<VecDeque<Vec<u8>>>>,
    }

    impl TestTransport {
        fn push_frame(&self, frame: Vec<u8>) {
            self.incoming.lock().unwrap().push_back(frame);
        }

        fn written(&self) -> Vec<u8> {
            self.written.lock().unwrap().clone()
        }
    }

    impl Transport for TestTransport {
        fn write(&mut self, cmd_area: &[u8]) -> Result<WriteOutcome, TransportError> {
            self.written.lock().unwrap().extend_from_slice(cmd_area);
            Ok(WriteOutcome::Written)
        }

        fn read_chunk(&mut self, out: &mut Vec<u8>) -> Result<ReadStatus, TransportError> {
            if let Some(frame) = self.incoming.lock().unwrap().pop_front() {
                out.extend_from_slice(&frame);
                Ok(ReadStatus::Data)
            } else {
                // 让 worker 不要空转。
                thread::sleep(Duration::from_millis(1));
                Ok(ReadStatus::Timeout)
            }
        }

        fn reset(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    fn frame(cmd: u8, data: &[u8]) -> Vec<u8> {
        assert!(data.len() < 192);
        let mut frame = vec![0x1f, cmd, data.len() as u8];
        frame.extend_from_slice(data);
        let checksum = calculate_checksum(&frame, 1, frame.len());
        frame.push(checksum);
        frame
    }

    fn status_frame(status: u8) -> Vec<u8> {
        frame(0x70, &[status, 0x00, 0x01])
    }

    fn buffer_frame(size: u8) -> Vec<u8> {
        frame(0x77, &[size, 0, 0, 0, 0, 0, 0, 0])
    }

    fn stats_frame(stats: &resp::statistics::Statistics) -> Vec<u8> {
        let mut data = Vec::with_capacity(16);
        data.extend_from_slice(&stats.work_lines.to_be_bytes());
        data.extend_from_slice(&stats.print_lines.to_be_bytes());
        data.extend_from_slice(&stats.null_lines.to_be_bytes());
        data.extend_from_slice(&stats.print_pages.to_be_bytes());
        frame(0x73, &data)
    }

    /// 按写入的命令自动回包：`0x70` 回状态，`0x73` 回统计。
    ///
    /// 等待打印完成的测试需要控制“第几次 `0x73` 看到什么”，FIFO transport
    /// 很难和状态轮询对齐。
    #[derive(Clone, Default)]
    struct RespondingTransport {
        statuses: Arc<StdMutex<VecDeque<u8>>>,
        stats: Arc<StdMutex<VecDeque<resp::statistics::Statistics>>>,
        written: Arc<StdMutex<Vec<u8>>>,
        incoming: Arc<StdMutex<VecDeque<Vec<u8>>>>,
    }

    impl RespondingTransport {
        fn push_status(&self, status: u8) {
            self.statuses.lock().unwrap().push_back(status);
        }

        fn push_stats(&self, stats: resp::statistics::Statistics) {
            self.stats.lock().unwrap().push_back(stats);
        }
    }

    impl Transport for RespondingTransport {
        fn write(&mut self, cmd_area: &[u8]) -> Result<WriteOutcome, TransportError> {
            self.written.lock().unwrap().extend_from_slice(cmd_area);
            let mut incoming = self.incoming.lock().unwrap();
            if cmd_area.windows(2).any(|w| w == [0x1f, 0x70]) {
                let status = self.statuses.lock().unwrap().pop_front().unwrap_or(0x00);
                incoming.push_back(status_frame(status));
            }
            if cmd_area.windows(2).any(|w| w == [0x1f, 0x73]) {
                let stats = self.stats.lock().unwrap().pop_front().unwrap_or_default();
                incoming.push_back(stats_frame(&stats));
            }
            Ok(WriteOutcome::Written)
        }

        fn read_chunk(&mut self, out: &mut Vec<u8>) -> Result<ReadStatus, TransportError> {
            if let Some(frame) = self.incoming.lock().unwrap().pop_front() {
                out.extend_from_slice(&frame);
                Ok(ReadStatus::Data)
            } else {
                thread::sleep(Duration::from_millis(1));
                Ok(ReadStatus::Timeout)
            }
        }

        fn reset(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    fn respond(config: DeviceConfig) -> (Device, RespondingTransport) {
        let transport = RespondingTransport::default();
        let tm = mytm::TaskManager::new();
        let device = Device::new_with_transport(&tm, transport.clone(), config);
        (device, transport)
    }

    fn verify_config() -> DeviceConfig {
        DeviceConfig {
            recv_timeout: Duration::from_millis(50),
            print_poll: Duration::from_millis(1),
            print_start_grace: Duration::from_millis(10),
            print_start_timeout: Duration::from_millis(60),
            print_verify_interval: Duration::from_millis(5),
            print_verify: true,
            print_idle_confirm: 1,
            print_silent_limit: 100,
            print_done_timeout: Duration::from_millis(500),
            ..Default::default()
        }
    }

    fn test_config() -> DeviceConfig {
        DeviceConfig {
            recv_timeout: Duration::from_millis(200),
            print_poll: Duration::from_millis(1),
            print_start_grace: Duration::ZERO,
            print_verify: false,
            print_idle_confirm: 1,
            print_done_timeout: Duration::from_millis(500),
            ..Default::default()
        }
    }

    fn device_with(frames: &[Vec<u8>]) -> (Device, TestTransport) {
        let transport = TestTransport::default();
        for frame in frames {
            transport.push_frame(frame.clone());
        }
        // `WorkerHandle` 内部持有 `TaskManager` 的克隆，所以这里放下 `tm`
        // 不会影响 worker；`shutdown()` / `close()` 仍然能作用到它。
        let tm = mytm::TaskManager::new();
        let device = Device::new_with_transport(&tm, transport.clone(), test_config());
        (device, transport)
    }

    #[test]
    fn statistics_progress_handles_wraparound() {
        let before = resp::statistics::Statistics {
            print_lines: u32::MAX,
            print_pages: 7,
            ..Default::default()
        };
        let wrapped = resp::statistics::Statistics {
            print_lines: 0,
            print_pages: 7,
            ..Default::default()
        };
        assert!(statistics_progress(&before, &wrapped));
        assert!(!statistics_progress(&before, &before));
    }

    #[tokio::test]
    async fn send_writes_the_encoded_command_area() {
        let (device, transport) = device_with(&[]);
        device.ready().await.unwrap();

        device
            .try_send_batch(&[&req::status::Status {}])
            .await
            .unwrap();

        assert_eq!(transport.written(), vec![0x1f, 0x70, 0x00, 0x8f]);
    }

    #[tokio::test]
    async fn parse_failure_does_not_consume_the_queue() {
        let (device, _transport) = device_with(&[status_frame(0x00)]);

        let err = device
            .try_recv_one::<resp::speed::Speed>()
            .await
            .unwrap_err();
        assert!(matches!(err, RecvError::Parse { .. }));

        let status: resp::status::Status = device.try_recv_one().await.unwrap();
        assert_eq!(status.status, PrinterStatus::Printable);
    }

    #[tokio::test]
    async fn recv_matching_discards_other_commands() {
        let (device, _transport) = device_with(&[frame(0x7b, &[50]), status_frame(0x00)]);

        let payload = device
            .recv_matching(0x70, Duration::from_millis(200))
            .await
            .unwrap()
            .expect("status frame");
        assert_eq!(payload.get(1), Some(&0x70));
        assert!(device.try_pop_raw().await.is_none());
    }

    #[tokio::test]
    async fn wait_for_print_done_needs_busy_then_idle() {
        let (device, _transport) =
            device_with(&[status_frame(0x01), status_frame(0x00), status_frame(0x00)]);

        let status = device
            .wait_for_print_done(Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(status.status, PrinterStatus::Printable);
    }

    #[tokio::test]
    async fn wait_for_print_done_reports_printer_status() {
        let (device, _transport) = device_with(&[status_frame(0x23)]);

        let err = device
            .wait_for_print_done(Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                FrontendError::PrinterStatus {
                    status: PrinterStatus::NoPaper
                }
            ),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn wait_for_print_done_errors_when_nothing_starts() {
        let (device, transport) = respond(verify_config());

        // 状态永远是 Printable，统计永远不增长：页面根本没开始打印。
        let err = device
            .wait_for_print_done(Duration::from_millis(500))
            .await
            .unwrap_err();
        assert!(
            matches!(err, FrontendError::PrintDidNotStart { .. }),
            "{err:?}"
        );
        // 确认确实查过 0x73（baseline + 至少一次校验）。
        let written = transport.written.lock().unwrap().clone();
        let stats_queries = written.windows(2).filter(|w| *w == [0x1f, 0x73]).count();
        assert!(stats_queries >= 2, "written = {written:?}");
    }

    #[tokio::test]
    async fn wait_for_print_done_accepts_fast_print_confirmed_by_stats() {
        let (device, transport) = respond(verify_config());

        let before = resp::statistics::Statistics::default();
        let after = resp::statistics::Statistics {
            print_lines: 32,
            print_pages: 1,
            ..Default::default()
        };
        // 第一次 0x73 是 baseline，第二次是宽限期后的校验。
        transport.push_stats(before);
        transport.push_stats(after);

        let status = device
            .wait_for_print_done(Duration::from_millis(500))
            .await
            .unwrap();
        assert_eq!(status.status, PrinterStatus::Printable);
    }

    /// 固件启动慢时不能提前判完成：先空闲一段时间，再 Rotating。
    #[tokio::test]
    async fn wait_for_print_done_accepts_late_busy() {
        let config = DeviceConfig {
            print_start_grace: Duration::from_millis(20),
            print_start_timeout: Duration::from_millis(200),
            ..verify_config()
        };
        let (device, transport) = respond(config);

        for _ in 0..30 {
            transport.push_status(0x00);
        }
        transport.push_status(0x02); // Rotating
        transport.push_status(0x00); // Printable

        let status = device
            .wait_for_print_done(Duration::from_millis(500))
            .await
            .unwrap();
        assert_eq!(status.status, PrinterStatus::Printable);
    }

    /// 开盖错误在打印中途出现时仍然要立即返回，不能被宽限期吞掉。
    #[tokio::test]
    async fn wait_for_print_done_reports_cover_opened_during_verify() {
        let (device, transport) = respond(verify_config());

        transport.push_status(0x00);
        transport.push_status(0x02); // Rotating
        transport.push_status(0x22); // CoverOpened

        let err = device
            .wait_for_print_done(Duration::from_millis(500))
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                FrontendError::PrinterStatus {
                    status: PrinterStatus::CoverOpened
                }
            ),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn wait_for_print_done_times_out() {
        let transport = TestTransport::default();
        let config = DeviceConfig {
            recv_timeout: Duration::from_millis(5),
            print_poll: Duration::from_millis(1),
            print_done_timeout: Duration::from_millis(30),
            ..test_config()
        };
        let tm = mytm::TaskManager::new();
        let device = Device::new_with_transport(&tm, transport, config);

        let err = device
            .wait_for_print_done(Duration::from_millis(30))
            .await
            .unwrap_err();
        assert!(matches!(err, FrontendError::PrintTimeout { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn print_bitmap_job_queries_budget_then_finishes() {
        let (device, transport) =
            device_with(&[status_frame(0x00), buffer_frame(50), status_frame(0x00)]);

        let mut bitmap = Bitmap::new(8, 1).unwrap();
        bitmap.line_mut(0).unwrap().set(0, true);

        let report = device
            .print_bitmap_job(&bitmap, RleMode::Off)
            .await
            .unwrap();
        assert_eq!(report.chunks, 1);
        assert_eq!(report.budget_queries, 1);
        assert!(!report.degraded);

        let written = transport.written();
        assert!(written.windows(2).any(|w| w == [0x1f, 0x20]), "StartPage");
        assert!(written.windows(2).any(|w| w == [0x1f, 0x26]), "TotalLines");
        assert!(written.windows(2).any(|w| w == [0x1f, 0x21]), "PrintLine");
        assert!(written.windows(2).any(|w| w == [0x1f, 0x28]), "EndPage");
    }

    /// 同一时刻只能有一个打印任务；第二个任务立即失败，不排队。
    #[tokio::test]
    async fn print_bitmap_job_rejects_a_second_job() {
        let (device, _transport) =
            device_with(&[status_frame(0x00), buffer_frame(50), status_frame(0x00)]);

        let mut bitmap = Bitmap::new(8, 1).unwrap();
        bitmap.line_mut(0).unwrap().set(0, true);

        // 模拟已经有一个任务占着打印锁。
        let running = device.print_lock.try_lock().unwrap();
        let err = device
            .print_bitmap_job(&bitmap, RleMode::Off)
            .await
            .unwrap_err();
        assert!(matches!(err, FrontendError::PrintBusy), "{err:?}");

        // 第一个任务结束后锁释放，再打印就正常。
        drop(running);
        device
            .print_bitmap_job(&bitmap, RleMode::Off)
            .await
            .unwrap();
    }

    /// 拔线后 `wait_for_print_done` 和 `shutdown` 都必须返回，不能挂住。
    #[tokio::test]
    async fn unplug_does_not_hang_wait_for_print_done() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        struct DyingTransport {
            die: Arc<AtomicBool>,
            writes: Arc<AtomicUsize>,
        }

        impl Transport for DyingTransport {
            fn write(&mut self, _cmd_area: &[u8]) -> Result<WriteOutcome, TransportError> {
                self.writes.fetch_add(1, Ordering::SeqCst);
                Ok(WriteOutcome::Written)
            }

            fn read_chunk(&mut self, _out: &mut Vec<u8>) -> Result<ReadStatus, TransportError> {
                if self.die.load(Ordering::SeqCst) {
                    Err(TransportError::Disconnected {
                        message: "unplugged".into(),
                    })
                } else {
                    thread::sleep(Duration::from_millis(1));
                    Ok(ReadStatus::Timeout)
                }
            }

            fn reset(&mut self) -> Result<(), TransportError> {
                Ok(())
            }
        }

        let die = Arc::new(AtomicBool::new(false));
        let writes = Arc::new(AtomicUsize::new(0));
        let tm = mytm::TaskManager::new();
        let device = Device::new_with_transport(
            &tm,
            DyingTransport {
                die: die.clone(),
                writes: writes.clone(),
            },
            test_config(),
        );
        device.ready().await.unwrap();

        // 先让前台发出 0x70 并进入等待，再模拟拔线，和真机日志的顺序一致。
        let waiter = device.wait_for_print_done(Duration::from_secs(5));
        let unplug = async {
            while writes.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
            die.store(true, Ordering::SeqCst);
        };
        let err = tokio::time::timeout(Duration::from_secs(2), async {
            let (result, ()) = tokio::join!(waiter, unplug);
            result
        })
        .await
        .expect("wait_for_print_done hung after unplug")
        .expect_err("expected an error after unplug");
        assert!(
            matches!(err, FrontendError::Recv { .. } | FrontendError::WorkerGone),
            "{err:?}"
        );

        let stopped = tokio::time::timeout(
            Duration::from_secs(2),
            device.shutdown(Duration::from_secs(1)),
        )
        .await;
        stopped.expect("shutdown hung after unplug").unwrap();
    }

    /// 无纸时 `0x70` 一直回 `NoPaper`、`0x77` 回 `size = 0`，必须中止而不是死循环。
    #[tokio::test]
    async fn no_paper_aborts_instead_of_querying_forever() {
        let (device, _transport) = device_with(&[status_frame(0x23), buffer_frame(0)]);

        let mut bitmap = Bitmap::new(8, 1).unwrap();
        bitmap.line_mut(0).unwrap().set(0, true);

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            device.print_bitmap_job(&bitmap, RleMode::Off),
        )
        .await
        .expect("print job hung on NoPaper");
        let err = result.expect_err("expected an error on NoPaper");
        assert!(
            matches!(
                err,
                FrontendError::PrinterStatus {
                    status: PrinterStatus::NoPaper
                }
            ),
            "{err:?}"
        );
    }

    /// 开盖 / 缺纸时固件可能连 `0x70` 都不回；发送阶段不能一直灌数据。
    #[tokio::test]
    async fn silent_printer_aborts_during_send() {
        let transport = TestTransport::default();
        let config = DeviceConfig {
            recv_timeout: Duration::from_millis(5),
            print_silent_limit: 2,
            ..test_config()
        };
        let tm = mytm::TaskManager::new();
        let device = Device::new_with_transport(&tm, transport, config);

        let mut bitmap = Bitmap::new(8, 1).unwrap();
        bitmap.line_mut(0).unwrap().set(0, true);

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            device.print_bitmap_job(&bitmap, RleMode::Off),
        )
        .await
        .expect("print job hung on a silent printer");
        assert!(
            matches!(result, Err(FrontendError::PrinterSilent { .. })),
            "{result:?}"
        );
    }

    /// worker 卡在 USB 写里时，前台不能无限等 ack。
    #[tokio::test]
    async fn send_times_out_when_the_worker_is_stuck() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        };

        struct StuckTransport {
            entered: Arc<AtomicBool>,
            release: mpsc::Receiver<()>,
        }

        impl Transport for StuckTransport {
            fn write(&mut self, _cmd_area: &[u8]) -> Result<WriteOutcome, TransportError> {
                self.entered.store(true, Ordering::SeqCst);
                let _ = self.release.recv();
                Ok(WriteOutcome::Written)
            }

            fn read_chunk(&mut self, _out: &mut Vec<u8>) -> Result<ReadStatus, TransportError> {
                thread::sleep(Duration::from_millis(1));
                Ok(ReadStatus::Timeout)
            }

            fn reset(&mut self) -> Result<(), TransportError> {
                Ok(())
            }
        }

        let entered = Arc::new(AtomicBool::new(false));
        let (release_tx, release_rx) = mpsc::channel();
        let tm = mytm::TaskManager::new();
        let device = Device::new_with_transport(
            &tm,
            StuckTransport {
                entered: entered.clone(),
                release: release_rx,
            },
            test_config(),
        );
        device.ready().await.unwrap();

        let send = device.send_cmd_area(vec![0x1f, 0x70, 0x00, 0x8f]);
        let wait_entered = async {
            while !entered.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        };
        let (result, ()) = tokio::join!(send, wait_entered);
        let err = result.expect_err("expected a timeout");
        assert!(
            matches!(err, FrontendError::WorkerTimeout { .. }),
            "{err:?}"
        );

        // 放行 worker，让测试结束时线程能真正退出。
        release_tx.send(()).unwrap();
        device.shutdown(Duration::from_secs(1)).await.unwrap();
    }

    /// `Device` 需要能放进 `Arc` 跨 tokio task 共享。
    #[test]
    fn device_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Device>();
    }

    #[tokio::test]
    async fn ready_reports_open_failure() {
        let tm = mytm::TaskManager::new();
        let device = Device::new_with_factory(&tm, || Err(TransportError::NotFound), test_config());

        let err = device.ready().await.unwrap_err();
        assert!(
            matches!(&err, FrontendError::NotReady { message } if message.contains("no matching device")),
            "{err:?}"
        );
    }

    /// 按命令字节取响应：前面的 `0x7b` 推送不消费、也不影响解析。
    #[tokio::test]
    async fn recv_cmd_timeout_skips_other_frames() {
        let (device, _transport) = device_with(&[frame(0x7b, &[0x01]), status_frame(0x00)]);
        device.ready().await.unwrap();

        let status = device
            .recv_cmd_timeout::<resp::status::Status>(resp::status::CMD, Duration::from_millis(200))
            .await
            .unwrap()
            .expect("status frame");
        assert_eq!(status.status, PrinterStatus::Printable);

        // 推送帧还在队列里。
        let raw = device
            .recv_matching(0x7b, Duration::from_millis(50))
            .await
            .unwrap();
        assert!(raw.is_some());
    }

    /// `0x7b` 主动推送插在 `0x70` 应答之间时，等待打印完成不能失败。
    #[tokio::test]
    async fn wait_for_print_done_ignores_pushed_frames() {
        let (device, _transport) = device_with(&[
            frame(0x7b, &[0x00]),
            status_frame(0x01),
            frame(0x7b, &[0x00]),
            status_frame(0x00),
        ]);
        device.ready().await.unwrap();

        let status = device
            .wait_for_print_done(Duration::from_millis(500))
            .await
            .unwrap();
        assert_eq!(status.status, PrinterStatus::Printable);
    }

    /// 预算查询时混入推送帧，仍然能按命令字节取到 `0x70` / `0x77`。
    #[tokio::test]
    async fn budget_query_ignores_pushed_frames() {
        let (device, _transport) = device_with(&[
            frame(0x7b, &[0x00]),
            status_frame(0x00),
            frame(0x7b, &[0x00]),
            buffer_frame(50),
            status_frame(0x00),
        ]);

        let mut bitmap = Bitmap::new(8, 1).unwrap();
        bitmap.line_mut(0).unwrap().set(0, true);

        let report = device
            .print_bitmap_job(&bitmap, RleMode::Off)
            .await
            .unwrap();
        assert_eq!(report.chunks, 1);
        assert_eq!(report.budget_queries, 1);
        assert!(!report.degraded);
    }

    #[tokio::test]
    async fn shutdown_stops_the_worker() {
        let (device, transport) = device_with(&[]);
        device.ready().await.unwrap();

        device.shutdown(Duration::from_secs(1)).await.unwrap();

        let err = device
            .try_send_batch(&[&req::status::Status {}])
            .await
            .unwrap_err();
        assert!(matches!(err, FrontendError::WorkerGone), "{err:?}");
        assert!(transport.written().is_empty());

        // worker 退出后 payload 通道也关闭，前台不应该一直等到超时。
        let err = device
            .try_recv_one::<resp::status::Status>()
            .await
            .unwrap_err();
        assert!(matches!(err, RecvError::Closed), "{err:?}");
    }

    /// `shutdown` 重复调用不会报错，方便在退出路径里统一兜底。
    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let (device, _transport) = device_with(&[]);
        device.ready().await.unwrap();

        device.shutdown(Duration::from_secs(1)).await.unwrap();
        device.shutdown(Duration::from_secs(1)).await.unwrap();
    }

    /// 卡在 IO 里的 worker 不会让 `shutdown` 无限等下去。
    #[tokio::test]
    async fn shutdown_times_out_when_the_worker_is_stuck() {
        use std::sync::atomic::{AtomicBool, Ordering};

        struct SlowTransport {
            entered: Arc<AtomicBool>,
        }

        impl Transport for SlowTransport {
            fn write(&mut self, _cmd_area: &[u8]) -> Result<WriteOutcome, TransportError> {
                Ok(WriteOutcome::Written)
            }

            fn read_chunk(&mut self, _out: &mut Vec<u8>) -> Result<ReadStatus, TransportError> {
                self.entered.store(true, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(200));
                Ok(ReadStatus::Timeout)
            }

            fn reset(&mut self) -> Result<(), TransportError> {
                Ok(())
            }
        }

        let entered = Arc::new(AtomicBool::new(false));
        let tm = mytm::TaskManager::new();
        let device = Device::new_with_transport(
            &tm,
            SlowTransport {
                entered: entered.clone(),
            },
            test_config(),
        );
        device.ready().await.unwrap();

        // 等 worker 真的进入一次阻塞读，`shutdown` 的取消才不会提前命中循环顶部。
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        let err = device
            .shutdown(Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(
            matches!(err, FrontendError::ShutdownTimeout { .. }),
            "{err:?}"
        );
    }

    /// 外层 `TaskManager::close()` 能停掉 worker，和 `shutdown()` 配合得到确定终点。
    #[tokio::test]
    async fn close_stops_the_worker() {
        let tm = mytm::TaskManager::new();
        let device = Device::new_with_transport(&tm, TestTransport::default(), test_config());
        device.ready().await.unwrap();

        tm.close();

        device.shutdown(Duration::from_secs(1)).await.unwrap();
        let err = device
            .try_send_batch(&[&req::status::Status {}])
            .await
            .unwrap_err();
        assert!(matches!(err, FrontendError::WorkerGone), "{err:?}");
    }
}
