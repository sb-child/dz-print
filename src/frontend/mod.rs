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
use std::{collections::VecDeque, fmt, time::Duration};
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
    /// 打印卡住了：job 已经作废，连接已经重置过一次。
    ///
    /// `cause` 是检测到的卡住原因（沉默、总超时、打印没开始、worker 不响应），
    /// `outcome` 是重置之后读回的状态。这一层包装是为了让调用方同时看到
    /// “为什么卡住”和“现在打印机到底怎么样”。
    #[snafu(display("{cause}; the device connection was reset, {outcome}"))]
    Stalled {
        cause: Box<FrontendError>,
        outcome: StallOutcome,
    },
    #[snafu(display("a print job is already running"))]
    PrintBusy,
}

/// 重置连接之后打印机的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StallOutcome {
    /// 读到了状态：可能是具体错误，也可能已经恢复正常。
    Status(PrinterStatus),
    /// 新连接上仍然没有任何回包。
    Silent,
}

impl fmt::Display for StallOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(status) if status.is_error() => write!(f, "the printer reports {status}"),
            // 盖子松了一下又合上就是这种情况：job 已经废了，但打印机没事。
            Self::Status(status) => {
                write!(
                    f,
                    "the printer reports {status} again and can take the next job"
                )
            }
            Self::Silent => write!(f, "the printer still does not answer"),
        }
    }
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

/// [`Device::wait_for_print_done_with`] 的轮询状态。
///
/// 拆出来是为了让主循环只表达“取一条状态 → 按状态走哪条路”；计数器本身
/// 不参与分支判断以外的逻辑。
struct PrintWait {
    /// 是否见过忙碌状态。只有见过忙之后，空闲才能当成打印完成。
    seen_busy: bool,
    /// 连续空闲次数。
    idle_polls: usize,
    /// 连续多少次状态查询没有回包。
    silent_polls: usize,
    /// 上一次打过的状态，用来只在状态变化时记录日志。
    logged: Option<PrinterStatus>,
    /// 上一次 `0x73` 校验的时间。
    last_verify: tokio::time::Instant,
}

impl PrintWait {
    fn new(now: tokio::time::Instant) -> Self {
        Self {
            seen_busy: false,
            idle_polls: 0,
            silent_polls: 0,
            logged: None,
            last_verify: now,
        }
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
    /// `EndPage` 之后连续多少次状态查询没有回应就判定打印失败。
    ///
    /// 只用来容忍固件偶尔漏答：每次查询最长等 [`Self::recv_timeout`]，默认
    /// `2` 次在 500ms 超时下约 1 秒、默认 3s 超时下约 6 秒。调大不会提高
    /// 恢复概率——判定失联后会重置连接读一次真实状态，job 一律作废。
    ///
    /// 发送阶段不数这个值：那里靠 `0x77` 和 `0x70` 一起超时来判断，见
    /// `frontend::print`。
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
            print_silent_limit: 2,
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
        let mut transport = Some(transport);
        Self::new_with_factory(
            tm,
            move || {
                let transport = transport
                    .take()
                    .ok_or(crate::backend::TransportError::NotFound)?;
                Ok(Box::new(transport))
            },
            config,
        )
    }

    /// 用可能失败的 transport 工厂创建设备，用来测打开失败或 reset。
    #[cfg(test)]
    fn new_with_factory(
        tm: &mytm::TaskManager,
        factory: impl FnMut() -> Result<
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
                    kind: worker::OutJobKind::Write(cmd_area),
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

    /// 彻底重置设备连接。
    ///
    /// worker 会丢弃当前 transport（释放接口、把内核驱动接回去），让连接真正
    /// 断开 `RESET_SETTLE`（2 秒）之后，再用原来的选择器重新 open：新的
    /// libusb context / DeviceHandle、claim 之前再 `handle.reset()`。等价于
    /// 重启进程后的第一次连接，不会复用旧 handle。
    ///
    /// 中间那段静默是必须的：DP27P 在开盖中止打印后会停止处理命令，立刻重连
    /// 读不到状态，断开一会儿再连才行（真机实测 1s 足够，几百毫秒不行）。
    ///
    /// 等待 ack 的超时下限由静默窗口、`recv_timeout` 和一次 open 的余量推出，
    /// 不是固定的 5 秒；传入更小的值会被抬到这个下限。
    ///
    /// 只用于清掉卡住的 parser 和残留响应；当前 job 仍然作废。
    pub async fn reset(&self, timeout: Duration) -> Result<(), FrontendError> {
        // reset 至少要花：静默窗口 + 重新 open + 一次状态查询。前面还可能
        // 排着一个写任务，所以再留一次 open 的余量，而不是另写一个 5 秒。
        const OPEN_MARGIN: Duration = Duration::from_secs(1);
        let min_timeout = worker::RESET_SETTLE + self.config.recv_timeout + OPEN_MARGIN;
        let timeout = timeout.max(min_timeout);

        // 先把缓存里的旧响应丢掉，reset 之后只认新连接上的回包。
        // 必须在换 transport 之前清：换完之后打印机的第一个应答可能已经
        // 进了队列，那时再清就会把真正想要的响应也丢掉。
        {
            let mut state = self.recv.lock().await;
            while state.rx.try_recv().is_ok() {}
            state.queue.clear();
        }

        let (ack_tx, ack_rx) = oneshot::oneshot();
        let sent = async {
            self.out_tx
                .send(worker::OutJob {
                    kind: worker::OutJobKind::Reset,
                    ack: ack_tx,
                })
                .map(|r| r.ok().context(WorkerGoneSnafu))
                .await?;
            ack_rx
                .map(|r| r.ok().context(WorkerGoneSnafu))
                .await?
                .map_err(|message| FrontendError::Write { message })
        };

        match tokio::time::timeout(timeout, sent).await {
            Ok(result) => result,
            Err(_) => Err(FrontendError::WorkerTimeout { timeout }),
        }
    }

    /// 发一次 `0x70` 并等一条状态；没有回包时返回 `Ok(None)`。
    ///
    /// 每次调用都自己发请求：打印机沉默后，上一轮的请求可能已经超时或者跟着
    /// 旧 transport 一起丢了，只 `recv` 等于等一个不存在的响应。
    pub(crate) async fn query_status(
        &self,
        timeout: Duration,
    ) -> Result<Option<PrinterStatus>, FrontendError> {
        self.try_send_batch(&[&req::status::Status {}]).await?;
        match self
            .recv_cmd_timeout::<resp::status::Status>(resp::status::CMD, timeout)
            .await
        {
            Ok(Some(status)) => Ok(Some(status.status)),
            Ok(None) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// 打印机沉默后重置连接，并在新连接上读一次状态。
    ///
    /// 返回 `None` 表示 reset 失败，或者新连接上仍然没有回包。
    pub(crate) async fn status_after_reset(&self) -> Option<PrinterStatus> {
        if let Err(error) = self.reset(self.config.recv_timeout).await {
            warn!(%error, "device reset failed");
            return None;
        }
        match self.query_status(self.config.recv_timeout).await {
            Ok(status) => status,
            Err(error) => {
                warn!(%error, "status query after reset failed");
                None
            }
        }
    }

    /// 打印卡住后的统一收尾：重置连接、读回状态，包装成 [`FrontendError::Stalled`]。
    ///
    /// 不管读回的状态是错误、正常还是仍然沉默，job 都不会继续：已经发出去的
    /// 数据撤不回来，而 reset 之后旧连接上的进度也没了。读回正常状态是有意义
    /// 的结果（例如盖子松了一下又合上），调用方据此决定下一个 job 还能不能跑。
    pub(crate) async fn abort_stalled(&self, cause: FrontendError) -> FrontendError {
        info!(%cause, "print stalled; resetting the device connection");

        let outcome = match self.status_after_reset().await {
            Some(status) if status.is_error() => {
                warn!(
                    status = ?status,
                    reason = %status,
                    "printer status after reset"
                );
                StallOutcome::Status(status)
            }
            Some(status) => {
                info!(
                    status = ?status,
                    reason = %status,
                    "printer is ready again after reset"
                );
                StallOutcome::Status(status)
            }
            None => {
                warn!("printer still silent after reset");
                StallOutcome::Silent
            }
        };

        FrontendError::Stalled {
            cause: Box::new(cause),
            outcome,
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
    ///
    /// 这里只轮询状态，不做卡住后的收尾；重置连接、读回状态在
    /// [`Self::print_bitmap_job`] 的出口统一处理。
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
    /// `baseline` 是 `EndPage` 之前的 `0x73` 快照；`None` 表示没有可用的
    /// 校验基准，只能退回“过了启动宽限期就接受空闲”的旧行为。
    async fn wait_for_print_done_with(
        &self,
        baseline: Option<resp::statistics::Statistics>,
        timeout: Duration,
    ) -> Result<resp::status::Status, FrontendError> {
        let started = tokio::time::Instant::now();
        let deadline = started + timeout;
        let poll = self.config.print_poll.max(Duration::from_millis(1));
        let mut wait = PrintWait::new(started);

        // `print_verify` 打开但没有 baseline，说明 `EndPage` 前的 `0x73`
        // 没读回来。这时没有别的判据，只能退回旧行为；至少要在日志里说明
        // 这次并没有校验过。
        let verify = if self.config.print_verify {
            if baseline.is_none() {
                warn!(
                    timeout = ?self.config.recv_timeout,
                    "print statistics are unavailable; accepting idle after the start grace \
                     without verification"
                );
            }
            baseline.as_ref()
        } else {
            debug!("print verification is disabled");
            None
        };

        loop {
            // 睡眠前后各查一次 deadline：`poll` 可能刚好跨过整页超时。
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(FrontendError::PrintTimeout { timeout });
            }
            tokio::time::sleep(poll.min(remaining)).await;

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(FrontendError::PrintTimeout { timeout });
            }

            // 单次漏答不算失败；连续 `print_silent_limit` 次才返回沉默。
            // 真正的 reset / 状态读回由 `print_bitmap_job` 的出口统一做。
            let Some(status) = self.poll_print_status(deadline, &mut wait).await? else {
                continue;
            };

            debug!(
                status = ?status.status,
                page_key = status.page_key,
                seen_busy = wait.seen_busy,
                idle_polls = wait.idle_polls,
                "print status"
            );
            log_status_change(&mut wait, &status);

            // 打印机明确报错：不再等，也不 reset（它还在正常通信）。
            if status.status.is_error() {
                return Err(FrontendError::PrinterStatus {
                    status: status.status,
                });
            }

            // 忙：重置空闲计数。`PageNotReady` 也算忙，固件在为下一页做准备。
            if is_busy(status.status) {
                wait.seen_busy = true;
                wait.idle_polls = 0;
                continue;
            }

            wait.idle_polls += 1;

            // 见过忙之后，连续几次空闲才算完成，避免采样到两次忙之间的缝。
            if wait.seen_busy {
                if wait.idle_polls >= self.config.print_idle_confirm {
                    return Ok(status);
                }
                continue;
            }

            // 还没见过忙：可能只是电机还没启动，交给启动判定。
            if let Some(done) = self
                .accept_unstarted(status, &mut wait, started, deadline, verify)
                .await?
            {
                return Ok(done);
            }
        }
    }

    /// 发一次 `0x70` 并等状态，负责维护“连续沉默”计数。
    ///
    /// 返回 `Ok(None)` 表示这一轮没有回包但还没到失联阈值；达到
    /// [`DeviceConfig::print_silent_limit`] 时返回
    /// [`FrontendError::PrinterSilent`]。
    async fn poll_print_status(
        &self,
        deadline: tokio::time::Instant,
        wait: &mut PrintWait,
    ) -> Result<Option<resp::status::Status>, FrontendError> {
        self.try_send_batch(&[&req::status::Status {}]).await?;

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match self
            .recv_cmd_timeout::<resp::status::Status>(
                resp::status::CMD,
                remaining.min(self.config.recv_timeout),
            )
            .await
        {
            Ok(Some(status)) => {
                wait.silent_polls = 0;
                Ok(Some(status))
            }
            // 单次查询超时、或只收到 `0x7b` 之类的推送，不代表打印结束。
            Ok(None) => {
                wait.silent_polls += 1;
                // 页面刚结束时固件偶尔会漏答一次，第一轮只记 debug；
                // 到阈值或每 10 次再警告，避免长超时把日志刷满。
                if wait.silent_polls == 1 {
                    debug!(
                        limit = self.config.print_silent_limit,
                        timeout = ?self.config.recv_timeout,
                        "printer did not answer the first status poll"
                    );
                } else if wait.silent_polls == self.config.print_silent_limit
                    || wait.silent_polls % 10 == 0
                {
                    warn!(
                        silent_polls = wait.silent_polls,
                        limit = self.config.print_silent_limit,
                        timeout = ?self.config.recv_timeout,
                        "printer did not answer the status poll"
                    );
                }

                if wait.silent_polls >= self.config.print_silent_limit {
                    return Err(FrontendError::PrinterSilent {
                        polls: wait.silent_polls,
                        timeout: self.config.recv_timeout,
                    });
                }
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// 在“还没见过忙”的情况下判断这一页是完成了、没开始，还是继续等。
    ///
    /// 返回 `Some(status)` 表示接受当前空闲状态为“打印完成”；`None` 表示
    /// 继续轮询。`baseline` 为 `None` 时没有 `0x73` 校验手段，退回旧行为。
    async fn accept_unstarted(
        &self,
        status: resp::status::Status,
        wait: &mut PrintWait,
        started: tokio::time::Instant,
        deadline: tokio::time::Instant,
        baseline: Option<&resp::statistics::Statistics>,
    ) -> Result<Option<resp::status::Status>, FrontendError> {
        let elapsed = started.elapsed();

        // 没有校验手段时只能保持旧行为：启动宽限期之后，连续几次空闲就
        // 认为页面已经打完。
        let Some(before) = baseline else {
            if elapsed >= self.config.print_start_grace
                && wait.idle_polls >= self.config.print_idle_confirm
            {
                return Ok(Some(status));
            }
            return Ok(None);
        };

        // 宽限期内不查 `0x73`：固件从 `EndPage` 到电机启动可能超过 1 秒，
        // 这时候的空闲只是还没开始，不是打完。
        if elapsed < self.config.print_start_grace {
            return Ok(None);
        }

        // 校验查询本身也要几百毫秒，按 `print_verify_interval` 限流。
        if tokio::time::Instant::now().duration_since(wait.last_verify)
            < self.config.print_verify_interval
        {
            return Ok(None);
        }
        wait.last_verify = tokio::time::Instant::now();

        // 用累计计数器判断页面到底有没有打印：有增量说明只是太快、没采样到
        // 忙碌状态，按正常完成逻辑再确认几次空闲，避免在它走纸时提前返回。
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
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
            wait.seen_busy = true;
            wait.idle_polls = 0;
            return Ok(None);
        }

        // 一直没有忙、计数器也没有前进：这一页没有真正开始。
        if started.elapsed() >= self.config.print_start_timeout {
            return Err(FrontendError::PrintDidNotStart {
                timeout: self.config.print_start_timeout,
            });
        }
        Ok(None)
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

/// 这些状态表示打印头 / 电机在动，或者固件正在为下一页做准备。
fn is_busy(status: PrinterStatus) -> bool {
    matches!(
        status,
        PrinterStatus::Printing | PrinterStatus::Rotating | PrinterStatus::PageNotReady
    )
}

/// 只在状态变化时打日志，避免 `print_poll` 把日志刷满。
fn log_status_change(wait: &mut PrintWait, status: &resp::status::Status) {
    if wait.logged == Some(status.status) {
        return;
    }
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
    wait.logged = Some(status.status);
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
        sync::{Arc, Mutex as StdMutex, atomic::AtomicBool, atomic::Ordering},
        thread,
        time::Duration,
    };

    use crate::{
        backend::{ReadStatus, Transport, TransportError, WriteOutcome},
        bitmap::Bitmap,
        command::{calculate_checksum, req, resp, status::PrinterStatus},
        frontend::StallOutcome,
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
    }

    /// 正常应答，但收到 `0x25`（加热统计）之后开始沉默。
    ///
    /// `0x25` 是数据发完、`EndPage` 之前最后一条命令，正好复现抓包里“所有数据
    /// 都灌完才开盖”的时序：在此之前每条查询都有回包，之后一条都没有。
    #[derive(Clone, Default)]
    struct MuteAfterHeat {
        written: Arc<StdMutex<Vec<u8>>>,
        incoming: Arc<StdMutex<VecDeque<Vec<u8>>>>,
        muted: Arc<AtomicBool>,
        /// 关掉之后这份 transport 永远应答，用来验证正常路径。
        mute_after_heat: bool,
    }

    impl MuteAfterHeat {
        fn muting() -> Self {
            Self {
                mute_after_heat: true,
                ..Self::default()
            }
        }

        fn written(&self) -> Vec<u8> {
            self.written.lock().unwrap().clone()
        }
    }

    impl Transport for MuteAfterHeat {
        fn write(&mut self, cmd_area: &[u8]) -> Result<WriteOutcome, TransportError> {
            self.written.lock().unwrap().extend_from_slice(cmd_area);
            if self.mute_after_heat && cmd_area.windows(2).any(|w| w == [0x1f, 0x25]) {
                self.muted.store(true, Ordering::SeqCst);
            }
            if self.muted.load(Ordering::SeqCst) {
                return Ok(WriteOutcome::Written);
            }
            let mut incoming = self.incoming.lock().unwrap();
            if cmd_area.windows(2).any(|w| w == [0x1f, 0x70]) {
                incoming.push_back(status_frame(0x00));
            }
            if cmd_area.windows(2).any(|w| w == [0x1f, 0x77]) {
                incoming.push_back(buffer_frame(50));
            }
            if cmd_area.windows(2).any(|w| w == [0x1f, 0x73]) {
                incoming.push_back(stats_frame(&resp::statistics::Statistics::default()));
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
            // 让“纯超时”类测试先撞到 print_done_timeout，而不是静默计数。
            print_silent_limit: 100,
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
        let factory_transport = transport.clone();
        let device = Device::new_with_factory(
            &tm,
            move || Ok(Box::new(factory_transport.clone())),
            test_config(),
        );
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
        let (device, transport) = device_with(&[
            status_frame(0x00),
            buffer_frame(50),
            // 之后的 `EndPage` 前置检查、以及等打印完成的状态轮询。
            status_frame(0x00),
            status_frame(0x00),
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

        let written = transport.written();
        assert!(written.windows(2).any(|w| w == [0x1f, 0x20]), "StartPage");
        assert!(written.windows(2).any(|w| w == [0x1f, 0x26]), "TotalLines");
        assert!(written.windows(2).any(|w| w == [0x1f, 0x21]), "PrintLine");
        assert!(written.windows(2).any(|w| w == [0x1f, 0x28]), "EndPage");
    }

    /// 同一时刻只能有一个打印任务；第二个任务立即失败，不排队。
    #[tokio::test]
    async fn print_bitmap_job_rejects_a_second_job() {
        let (device, _transport) = device_with(&[
            status_frame(0x00),
            buffer_frame(50),
            status_frame(0x00),
            status_frame(0x00),
            status_frame(0x00),
        ]);

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
        let first = TestTransport::default();
        let second = TestTransport::default();
        let config = DeviceConfig {
            recv_timeout: Duration::from_millis(5),
            print_silent_limit: 2,
            ..test_config()
        };
        let tm = mytm::TaskManager::new();
        let mut transports = vec![first, second.clone()].into_iter();
        let device = Device::new_with_factory(
            &tm,
            move || {
                transports
                    .next()
                    .map(|transport| Box::new(transport) as Box<dyn crate::backend::Transport>)
                    .ok_or(crate::backend::TransportError::NotFound)
            },
            config,
        );

        // 页要足够长，才能在 `0x77` 超时 fallback 之后再次检查状态，
        // 覆盖“预算超时 + 状态超时 -> reset -> 重新发 0x70”的路径。
        let mut bitmap = Bitmap::new(576, 200).unwrap();
        for y in 0..200 {
            bitmap.line_mut(y).unwrap().set(0, true);
        }

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            device.print_bitmap_job(&bitmap, RleMode::Off),
        )
        .await
        .expect("print job hung on a silent printer");
        assert!(
            matches!(
                result,
                Err(FrontendError::Stalled {
                    outcome: StallOutcome::Silent,
                    ..
                })
            ),
            "{result:?}"
        );
        // reset 之后必须在新 transport 上重新发 0x70，而不是干等。
        let status_queries = second
            .written()
            .windows(2)
            .filter(|bytes| *bytes == [0x1f, 0x70])
            .count();
        assert!(
            status_queries >= 1,
            "expected a fresh 0x70 on the reopened transport, saw {status_queries}"
        );
    }

    /// 打印机在数据发完之后沉默时，绝不能把 `EndPage` 写进去。
    ///
    /// 真机上这一条会让固件持续不回包：reset、等 1.3 秒重连，20 秒读 50 次
    /// `0x70` 都不回；不发 `EndPage` 的话，重连后第一次查询就能读到
    /// `CoverOpened`。
    #[tokio::test]
    async fn end_page_is_not_sent_to_a_mute_printer() {
        let transport = MuteAfterHeat::muting();
        let config = DeviceConfig {
            recv_timeout: Duration::from_millis(5),
            print_verify: false,
            ..test_config()
        };
        let tm = mytm::TaskManager::new();
        let mut transports = vec![transport.clone(), transport.clone()].into_iter();
        let device = Device::new_with_factory(
            &tm,
            move || {
                transports
                    .next()
                    .map(|transport| Box::new(transport) as Box<dyn Transport>)
                    .ok_or(TransportError::NotFound)
            },
            config,
        );

        let mut bitmap = Bitmap::new(64, 8).unwrap();
        bitmap.line_mut(0).unwrap().set(0, true);

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            device.print_bitmap_job(&bitmap, RleMode::Off),
        )
        .await
        .expect("print job hung on a mute printer");
        assert!(
            matches!(
                result,
                Err(FrontendError::Stalled {
                    ref cause,
                    outcome: StallOutcome::Silent,
                }) if matches!(**cause, FrontendError::PrinterSilent { .. })
            ),
            "{result:?}"
        );

        let written = transport.written();
        assert!(
            written.windows(2).any(|w| w == [0x1f, 0x25]),
            "job never reached the heat command"
        );
        assert!(
            !written.windows(2).any(|w| w == [0x1f, 0x28]),
            "EndPage was written to a mute printer"
        );
    }

    /// 打印机正常应答时必须照旧发 `EndPage`，而且是在确认它还活着之后。
    #[tokio::test]
    async fn end_page_is_sent_after_a_liveness_check() {
        let transport = MuteAfterHeat::default();
        // `mute_after_heat` 关着，这份 transport 在整页流程里一直应答。
        let tm = mytm::TaskManager::new();
        let mut transports = vec![transport.clone()].into_iter();
        let device = Device::new_with_factory(
            &tm,
            move || {
                transports
                    .next()
                    .map(|transport| Box::new(transport) as Box<dyn Transport>)
                    .ok_or(TransportError::NotFound)
            },
            DeviceConfig {
                recv_timeout: Duration::from_millis(5),
                print_verify: false,
                ..test_config()
            },
        );

        let mut bitmap = Bitmap::new(64, 8).unwrap();
        bitmap.line_mut(0).unwrap().set(0, true);
        // 忽略等待打印完成的结果：这里只关心写出的命令顺序。
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            device.print_bitmap_job(&bitmap, RleMode::Off),
        )
        .await;

        let written = transport.written();
        let end_page = written
            .windows(2)
            .position(|w| w == [0x1f, 0x28])
            .expect("EndPage was never written to a healthy printer");
        let before = &written[..end_page];
        let query = before
            .windows(2)
            .rposition(|w| w == [0x1f, 0x70])
            .expect("no 0x70 before EndPage");
        let heat = before
            .windows(2)
            .rposition(|w| w == [0x1f, 0x25])
            .expect("no heat command before EndPage");
        assert!(query > heat, "liveness check must run right before EndPage");
    }

    /// reset 之后的新连接如果开始回 `0x70`，要把真实状态报出来。
    #[tokio::test]
    async fn status_from_the_reopened_transport_is_reported() {
        let silent = TestTransport::default();
        let recovered = TestTransport::default();
        // 新连接一装好就已经能回答 `0x70`：开盖。
        recovered.push_frame(status_frame(0x22));

        let config = DeviceConfig {
            recv_timeout: Duration::from_millis(5),
            print_silent_limit: 2,
            ..test_config()
        };
        let tm = mytm::TaskManager::new();
        let mut transports = vec![silent, recovered].into_iter();
        let device = Device::new_with_factory(
            &tm,
            move || {
                transports
                    .next()
                    .map(|transport| Box::new(transport) as Box<dyn crate::backend::Transport>)
                    .ok_or(crate::backend::TransportError::NotFound)
            },
            config,
        );

        let mut bitmap = Bitmap::new(576, 200).unwrap();
        for y in 0..200 {
            bitmap.line_mut(y).unwrap().set(0, true);
        }

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            device.print_bitmap_job(&bitmap, RleMode::Off),
        )
        .await
        .expect("print job hung");
        assert!(
            matches!(
                result,
                Err(FrontendError::Stalled {
                    outcome: StallOutcome::Status(PrinterStatus::CoverOpened),
                    ..
                })
            ),
            "{result:?}"
        );
    }

    /// 卡住之后 reset 读回正常状态（盖子松了一下又合上）也要报出来：
    /// job 作废，但调用方要能看到打印机已经可以接下一个任务。
    #[tokio::test]
    async fn status_after_reset_can_be_printable_again() {
        let silent = TestTransport::default();
        let recovered = TestTransport::default();
        recovered.push_frame(status_frame(0x00));

        let config = DeviceConfig {
            recv_timeout: Duration::from_millis(5),
            print_silent_limit: 2,
            ..test_config()
        };
        let tm = mytm::TaskManager::new();
        let mut transports = vec![silent, recovered].into_iter();
        let device = Device::new_with_factory(
            &tm,
            move || {
                transports
                    .next()
                    .map(|transport| Box::new(transport) as Box<dyn crate::backend::Transport>)
                    .ok_or(crate::backend::TransportError::NotFound)
            },
            config,
        );

        let mut bitmap = Bitmap::new(576, 200).unwrap();
        for y in 0..200 {
            bitmap.line_mut(y).unwrap().set(0, true);
        }

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            device.print_bitmap_job(&bitmap, RleMode::Off),
        )
        .await
        .expect("print job hung");
        match result {
            Err(FrontendError::Stalled {
                outcome: StallOutcome::Status(PrinterStatus::Printable),
                ..
            }) => {}
            other => panic!("{other:?}"),
        }
    }

    /// `Device::reset` 必须丢弃旧 transport，重新调用 factory。
    #[tokio::test]
    async fn reset_reopens_the_transport() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let opens = Arc::new(AtomicUsize::new(0));
        let transport = TestTransport::default();
        let factory_transport = transport.clone();
        let factory_opens = opens.clone();
        let tm = mytm::TaskManager::new();
        let device = Device::new_with_factory(
            &tm,
            move || {
                factory_opens.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(factory_transport.clone()))
            },
            test_config(),
        );

        device.ready().await.unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 1);

        device.reset(Duration::from_secs(1)).await.unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 2);
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
            // 之后的 `EndPage` 前置检查、以及等打印完成的状态轮询。
            status_frame(0x00),
            status_frame(0x00),
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
