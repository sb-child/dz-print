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
use mytm::{TypedTaskState, TypedTaskStateRef};
use pretty_hex::simple_hex;
use std::{collections::VecDeque, time::Duration};
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{debug, trace, warn};

mod print;
mod worker;

/// 响应接收错误。
#[derive(Debug, Error)]
pub enum RecvError {
    #[error("device thread is gone")]
    Closed,
    #[error("timed out after {0:?} waiting for a response")]
    Timeout(Duration),
    #[error("front of the buffer is not '{expected}': {source}")]
    Parse {
        expected: &'static str,
        source: ParseRespError,
    },
}

/// frontend 错误。
#[derive(Debug, Error)]
pub enum FrontendError {
    #[error("device thread is gone")]
    WorkerGone,
    #[error("device is not ready: {0}")]
    NotReady(String),
    #[error("device write failed: {0}")]
    Write(String),
    #[error(transparent)]
    Recv(#[from] RecvError),
    #[error(transparent)]
    Pack(#[from] PackReqError),
    #[error(transparent)]
    Print(#[from] PrintError),
    #[error(transparent)]
    Flow(#[from] FlowError),
    #[error("bitmap width {width} does not fit in u16")]
    WidthOverflow { width: usize },
    #[error("bitmap needs {pages} pages, the device supports at most 65534")]
    TooManyPages { pages: usize },
    #[error("page has {lines} lines, does not fit in u16")]
    PageTooTall { lines: usize },
    #[error("printer reported {0:?} while waiting for print completion")]
    PrinterStatus(PrinterStatus),
    #[error("print did not finish within {0:?}")]
    PrintTimeout(Duration),
    #[error("device worker did not stop within {0:?}")]
    ShutdownTimeout(Duration),
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
    /// `EndPage` 后的启动宽限期，期间即使空闲也继续等。
    pub print_start_grace: Duration,
    /// 至少连续几次空闲才认为打印完成。
    pub print_idle_confirm: usize,
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
            print_idle_confirm: 2,
            print_done_timeout: Duration::from_secs(600),
        }
    }
}

/// 一台打印机。
pub struct Device {
    mv: ModelVersion,
    assembler: Assembler,
    config: DeviceConfig,
    out_tx: worker::OutTx,
    recv: Mutex<RecvState>,
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
            worker,
        }
    }

    /// 当前机型版本。
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
            TypedTaskState::Failed(e) => Err(FrontendError::NotReady(e.to_string())),
            TypedTaskState::Panicked(msg) => {
                Err(FrontendError::NotReady(format!("worker panicked: {msg}")))
            }
            TypedTaskState::Pending => {
                Err(FrontendError::NotReady("worker is still pending".into()))
            }
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
    pub async fn shutdown(&self, timeout: Duration) -> Result<(), FrontendError> {
        self.worker.cancel();
        match tokio::time::timeout(timeout, self.worker.wait_terminal()).await {
            Err(_) => Err(FrontendError::ShutdownTimeout(timeout)),
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
    pub async fn send_cmd_area(&self, cmd_area: Vec<u8>) -> Result<(), FrontendError> {
        if cmd_area.is_empty() {
            return Ok(());
        }
        debug!(bytes = cmd_area.len(), "write command area");
        trace!("write command area: {}", simple_hex(&cmd_area));

        let (ack_tx, ack_rx) = oneshot::oneshot();
        self.out_tx
            .send(worker::OutJob {
                cmd_area,
                ack: ack_tx,
            })
            .await
            .map_err(|_| FrontendError::WorkerGone)?;
        ack_rx
            .await
            .map_err(|_| FrontendError::WorkerGone)?
            .map_err(FrontendError::Write)
    }

    /// 等打印机把已经发送的页真正打完。
    ///
    /// `EndPage` 只是把数据交给固件，返回时打印头可能还在走。这里每
    /// [`DeviceConfig::print_poll`] 查一次 `0x70`：
    ///
    /// - `Printing` / `Rotating` / `PageNotReady` 视为还在忙；
    /// - 至少观察到一次忙，之后回到 `Printable` / `NoJob` 才返回；
    /// - 如果打印很快就结束了，则等过 [`DeviceConfig::print_start_grace`] 后
    ///   再接受空闲状态；
    /// - `0x0c` 及以上的错误状态直接返回错误；
    /// - 只认命令字节为 `0x70` 的帧，`0x7b` 之类的主动推送会被跳过。
    ///
    /// 只应在最后一页的 `EndPage` 之后调用；不要在行数据中间插 `0x70`。
    pub async fn wait_for_print_done(
        &self,
        timeout: Duration,
    ) -> Result<resp::status::Status, FrontendError> {
        let started = tokio::time::Instant::now();
        let deadline = started + timeout;
        let poll = self.config.print_poll.max(Duration::from_millis(1));
        let mut seen_busy = false;
        let mut idle_polls = 0usize;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(FrontendError::PrintTimeout(timeout));
            }
            tokio::time::sleep(poll.min(remaining)).await;

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(FrontendError::PrintTimeout(timeout));
            }
            self.try_send_batch(&[&req::status::Status {}]).await?;

            let status = match self
                .recv_cmd_timeout::<resp::status::Status>(
                    resp::status::CMD,
                    remaining.min(self.config.recv_timeout),
                )
                .await
            {
                Ok(Some(status)) => status,
                // 单次查询超时、或只收到 `0x7b` 之类的推送，不代表打印结束。
                Ok(None) => continue,
                Err(e) => return Err(e.into()),
            };
            debug!(
                status = ?status.status,
                page_key = status.page_key,
                seen_busy,
                idle_polls,
                "print status"
            );

            if status.status.is_error() {
                return Err(FrontendError::PrinterStatus(status.status));
            }
            if matches!(
                status.status,
                PrinterStatus::Printing | PrinterStatus::Rotating | PrinterStatus::PageNotReady
            ) {
                seen_busy = true;
                idle_polls = 0;
            } else {
                idle_polls += 1;
                if (seen_busy || started.elapsed() >= self.config.print_start_grace)
                    && idle_polls >= self.config.print_idle_confirm
                {
                    return Ok(status);
                }
            }
        }
    }

    /// 接收指定类型的回复。
    ///
    /// 解析失败时不会弹出缓冲区里的 payload，方便换一个类型重试。
    pub async fn try_recv_one<T: RespTrait>(&self) -> Result<T, RecvError> {
        self.try_recv_one_timeout::<T>(self.config.recv_timeout)
            .await
    }

    /// 带超时的 [`Self::try_recv_one`]。
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
                Err(_) => return Err(RecvError::Timeout(timeout)),
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
    /// 只用于解析失败后恢复对齐；正常业务不应该依赖它。
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
                Err(RecvError::Timeout(_)) => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }
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

    use super::{Device, DeviceConfig, FrontendError, RecvError};

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

    fn test_config() -> DeviceConfig {
        DeviceConfig {
            recv_timeout: Duration::from_millis(200),
            print_poll: Duration::from_millis(1),
            print_start_grace: Duration::ZERO,
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
            matches!(err, FrontendError::PrinterStatus(PrinterStatus::NoPaper)),
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
        assert!(matches!(err, FrontendError::PrintTimeout(_)), "{err:?}");
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
            matches!(&err, FrontendError::NotReady(msg) if msg.contains("no matching device")),
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
        assert!(matches!(err, FrontendError::ShutdownTimeout(_)), "{err:?}");
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
