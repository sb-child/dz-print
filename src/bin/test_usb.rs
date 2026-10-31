// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! USB 测试平台。
//!
//! 一个最小可用的设备端到端测试入口：
//!
//! - [`TestFrontend`] 对调用方暴露 `ready` / `try_send_batch` / `try_recv_one`。
//! - `bg` 在阻塞线程里跑 USB 读写和命令区切帧，只把原始 payload 回传给前台。
//! - 响应解析放在前台，所以 `try_recv_one::<T>()` 解析失败时不会消费缓冲区，
//!   可以换一个响应类型重试。
//!
//! ```text
//! 调用方 --try_send_batch--> Assembler::encode --> USB 报告 --> 设备
//! 调用方 <--try_recv_one--- payload 队列 <-- Assembler::decode_frame <-- 设备
//! ```

use std::{collections::VecDeque, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use rusb::{Direction, TransferType, UsbContext};
use tokio::sync::{Mutex, mpsc, oneshot, watch};
use tracing::{debug, error, info, trace, warn};

use dz_print::{
    command::{
        ModelVersion,
        envelope::{Envelope, EnvelopeKind},
        high_command::HighCommandMode,
        req::{self, PackReqError, ReqTrait},
        resp::{self, ParseRespError, RespTrait},
    },
    planner::{Assembler, usb},
};

/// 德佟 USB 设备的 VID。
const DETONGER_VID: u16 = 0x3533;
/// 读中断端点超时。超时只代表这段时间没数据，不代表设备断开。
const READ_TIMEOUT: Duration = Duration::from_millis(100);
/// 写中断端点超时。
const WRITE_TIMEOUT: Duration = Duration::from_millis(500);
/// 前台等响应的超时。
const RECV_TIMEOUT: Duration = Duration::from_secs(3);
/// 单条探测命令等响应的超时。
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
/// 被动监听 `0x7b` 的时间。
const PUSH_TIMEOUT: Duration = Duration::from_secs(1);

/// 设备选择方式。
#[derive(Debug, Clone)]
pub enum USBSelector {
    /// 按 USB 设备名（`型号-序列号`）匹配，例如 `DP305P-Y60870279`。
    DeviceName(String),
    /// 不匹配名称，取第一台带中断端点对的德佟设备（VID `0x3533`）。
    Auto,
}

/// 一对中断端点及其所在接口。
#[derive(Debug, Clone, Copy)]
struct UsbEndpoints {
    in_iface: u8,
    in_setting: u8,
    ep_in: u8,
    out_iface: u8,
    out_setting: u8,
    ep_out: u8,
}

/// 后台线程收到的写任务。
struct OutJob {
    /// 已编码的命令区（不含 USB 外层 `1e <长度>`）。
    cmd_area: Vec<u8>,
    /// 写完后回报结果。
    ack: oneshot::Sender<Result<(), String>>,
}

/// 设备就绪状态。
#[derive(Debug, Clone)]
enum ReadyStatus {
    Pending,
    Ready,
    Failed(String),
}

/// 响应接受错误。
#[derive(Debug, thiserror::Error)]
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

/// 前台状态：响应队列 + 未取走的原始 payload。
struct RecvState {
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    queue: VecDeque<Vec<u8>>,
}

/// 无参数读取命令：`1f <cmd>`。
struct ReadCmd(u8);

impl ReqTrait for ReadCmd {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let kind = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        Ok(Envelope::new(vec![0x1f, self.0], kind).into())
    }
}

/// 带参数的原始命令：`1f <cmd> <data...>`。
struct DataCmd<'a> {
    cmd: u8,
    data: &'a [u8],
}

impl ReqTrait for DataCmd<'_> {
    fn pack(&self, mv: ModelVersion) -> Result<Vec<Envelope>, PackReqError> {
        let kind = match mv {
            ModelVersion::V0 => EnvelopeKind::Raw,
            ModelVersion::V1 => EnvelopeKind::AutoPackWithFixedChecksum,
            ModelVersion::V2 => EnvelopeKind::AutoPackWithChecksum,
        };
        let mut payload = Vec::with_capacity(2 + self.data.len());
        payload.extend_from_slice(&[0x1f, self.cmd]);
        payload.extend_from_slice(self.data);
        Ok(Envelope::new(payload, kind).into())
    }
}

/// 原始响应，用于还没建模的命令。
///
/// 它接受任何 payload，所以只适合「一次只发一条、按已知顺序取」的探测。
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawPayload(Vec<u8>);

impl RespTrait for RawPayload {
    fn parse(payload: &[u8], _mv: ModelVersion) -> Result<Self, ParseRespError> {
        Ok(Self(payload.to_vec()))
    }
}

/// 测试用前端。
pub struct TestFrontend {
    mv: ModelVersion,
    assembler: Assembler,
    out_tx: mpsc::Sender<OutJob>,
    recv: Mutex<RecvState>,
    ready_rx: watch::Receiver<ReadyStatus>,
    _bg: tokio::task::JoinHandle<()>,
}

impl TestFrontend {
    /// 连接第一台德佟设备，按 `v2` 机型处理。
    pub fn new() -> Self {
        Self::new_with(USBSelector::Auto, ModelVersion::V2)
    }

    /// 按 USB 设备名（`型号-序列号`）连接，按 `v2` 机型处理。
    pub fn new_by_name(name: impl Into<String>) -> Self {
        Self::new_with(USBSelector::DeviceName(name.into()), ModelVersion::V2)
    }

    /// 指定设备选择和机型版本。
    pub fn new_with(selector: USBSelector, mv: ModelVersion) -> Self {
        let (out_tx, out_rx) = mpsc::channel(8);
        let (recv_tx, recv_rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = watch::channel(ReadyStatus::Pending);

        let bg = tokio::task::spawn_blocking(move || bg(selector, mv, out_rx, recv_tx, ready_tx));

        Self {
            mv,
            assembler: Assembler::new(mv),
            out_tx,
            recv: Mutex::new(RecvState {
                rx: recv_rx,
                queue: VecDeque::new(),
            }),
            ready_rx,
            _bg: bg,
        }
    }

    /// 当前机型版本。
    pub fn model_version(&self) -> ModelVersion {
        self.mv
    }

    /// 等待设备就绪。
    pub async fn ready(&self) -> Result<()> {
        let mut rx = self.ready_rx.clone();
        loop {
            match rx.borrow().clone() {
                ReadyStatus::Ready => return Ok(()),
                ReadyStatus::Failed(e) => return Err(anyhow!(e)),
                ReadyStatus::Pending => {}
            }
            rx.changed()
                .await
                .context("device thread exited before the device became ready")?;
        }
    }

    /// 批量发送命令。
    ///
    /// 所有命令会被 [`Assembler::encode`] 拼成一段命令区，再按 62 字节切片成
    /// HID 报告发出；写失败会立即返回错误。
    pub async fn try_send_batch(&self, cmds: &[&dyn ReqTrait]) -> Result<()> {
        let cmd_area = self
            .assembler
            .encode(cmds)
            .context("encode command batch")?;
        debug!(
            commands = cmds.len(),
            bytes = cmd_area.len(),
            "encoded command area: {:02X?}",
            cmd_area
        );
        if cmd_area.is_empty() {
            return Ok(());
        }

        let (ack_tx, ack_rx) = oneshot::channel();
        self.out_tx
            .send(OutJob {
                cmd_area,
                ack: ack_tx,
            })
            .await
            .map_err(|_| anyhow!("device thread is gone"))?;
        let result = ack_rx
            .await
            .context("device thread dropped the write job")?;
        result.map_err(|e| anyhow!(e))?;
        Ok(())
    }

    /// 接收指定类型的回复。
    ///
    /// 解析失败时不会弹出缓冲区里的 payload，方便换一个类型重试。例如设备先返回
    /// `0x70`，用 `Speed` 解析会得到 [`RecvError::Parse`]，再换 `Status` 即可。
    pub async fn try_recv_one<T: RespTrait>(&self) -> Result<T, RecvError> {
        self.try_recv_one_timeout::<T>(RECV_TIMEOUT).await
    }

    /// 带超时的 [`Self::try_recv_one`]。
    pub async fn try_recv_one_timeout<T: RespTrait>(
        &self,
        timeout: Duration,
    ) -> Result<T, RecvError> {
        let mut state = self.recv.lock().await;
        loop {
            while let Ok(payload) = state.rx.try_recv() {
                state.queue.push_back(payload);
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
                Ok(Some(payload)) => state.queue.push_back(payload),
                Ok(None) => return Err(RecvError::Closed),
                Err(_) => return Err(RecvError::Timeout(timeout)),
            }
        }
    }

    /// 丢弃并返回缓冲区头部的一条原始 payload。
    ///
    /// 只用于测试平台在解析失败后恢复对齐；正常业务不应该依赖它。
    pub async fn try_pop_raw(&self) -> Option<Vec<u8>> {
        let mut state = self.recv.lock().await;
        while let Ok(payload) = state.rx.try_recv() {
            state.queue.push_back(payload);
        }
        state.queue.pop_front()
    }
}

impl Default for TestFrontend {
    fn default() -> Self {
        Self::new()
    }
}

/// 在设备上找一对中断端点。
///
/// 优先选同一个接口上同时具备 IN/OUT 的接口；没有这种接口时，退回到
/// 「任意接口的 IN + 任意接口的 OUT」。DP27P / DP305P 都走第一种。
fn find_endpoints(device: &rusb::Device<rusb::Context>) -> Result<Option<UsbEndpoints>> {
    let config = device
        .config_descriptor(0)
        .context("read config descriptor")?;

    // 第一遍：同一个接口内的端点对。
    for iface in config.interfaces() {
        for desc in iface.descriptors() {
            let mut ep_in = None;
            let mut ep_out = None;
            for ep in desc.endpoint_descriptors() {
                if ep.transfer_type() != TransferType::Interrupt {
                    continue;
                }
                match ep.direction() {
                    Direction::In => ep_in = Some(ep.address()),
                    Direction::Out => ep_out = Some(ep.address()),
                }
            }
            if let (Some(ep_in), Some(ep_out)) = (ep_in, ep_out) {
                let iface = desc.interface_number();
                let setting = desc.setting_number();
                return Ok(Some(UsbEndpoints {
                    in_iface: iface,
                    in_setting: setting,
                    ep_in,
                    out_iface: iface,
                    out_setting: setting,
                    ep_out,
                }));
            }
        }
    }

    // 第二遍：跨接口凑一对。
    let mut ep_in = None;
    let mut ep_out = None;
    for iface in config.interfaces() {
        for desc in iface.descriptors() {
            for ep in desc.endpoint_descriptors() {
                if ep.transfer_type() != TransferType::Interrupt {
                    continue;
                }
                match ep.direction() {
                    Direction::In if ep_in.is_none() => {
                        ep_in =
                            Some((ep.address(), desc.interface_number(), desc.setting_number()));
                    }
                    Direction::Out if ep_out.is_none() => {
                        ep_out =
                            Some((ep.address(), desc.interface_number(), desc.setting_number()));
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(match (ep_in, ep_out) {
        (Some((ep_in, in_iface, in_setting)), Some((ep_out, out_iface, out_setting))) => {
            Some(UsbEndpoints {
                in_iface,
                in_setting,
                ep_in,
                out_iface,
                out_setting,
                ep_out,
            })
        }
        _ => None,
    })
}

/// 从设备读取所有可用的 USB 字符串：产品名 + 各接口字符串。
fn read_device_names(
    handle: &rusb::DeviceHandle<rusb::Context>,
    device: &rusb::Device<rusb::Context>,
    desc: &rusb::DeviceDescriptor,
) -> Vec<String> {
    let timeout = Duration::from_secs(1);
    let mut names = Vec::new();

    let Ok(langs) = handle.read_languages(timeout) else {
        return names;
    };
    let Some(lang) = langs.into_iter().next() else {
        return names;
    };

    if let Ok(name) = handle.read_product_string(lang, desc, timeout) {
        names.push(name);
    }

    if let Ok(config) = device.config_descriptor(0) {
        for iface in config.interfaces() {
            for idesc in iface.descriptors() {
                if let Ok(name) = handle.read_interface_string(lang, &idesc, timeout) {
                    names.push(name);
                }
            }
        }
    }

    names
}

/// USB 字符串是否就是目标设备名。
///
/// 部分机型的接口字符串是 `xxx @ 型号-序列号`，所以除完全相等外也接受后缀匹配。
fn usb_name_matches(candidate: &str, expected: &str) -> bool {
    let candidate = candidate.trim();
    candidate == expected || candidate.ends_with(expected)
}

/// 打开设备并声明接口。
fn open_device(
    selector: &USBSelector,
) -> Result<(rusb::DeviceHandle<rusb::Context>, UsbEndpoints)> {
    let ctx = rusb::Context::new().context("create rusb context")?;
    let devices = ctx.devices().context("enumerate USB devices")?;

    for device in devices.iter() {
        let desc = match device.device_descriptor() {
            Ok(desc) => desc,
            Err(e) => {
                debug!("skip device: read descriptor failed: {e:?}");
                continue;
            }
        };

        let vid = format!("{:#06x}", desc.vendor_id());
        let pid = format!("{:#06x}", desc.product_id());

        // 按名称选择时不预先按 VID 过滤，最终以字符串匹配为准。
        match selector {
            USBSelector::DeviceName(_) => {}
            USBSelector::Auto => {
                if desc.vendor_id() != DETONGER_VID {
                    continue;
                }
            }
        }

        let handle = match device.open() {
            Ok(handle) => handle,
            Err(e) => {
                debug!(vid, pid, "skip device: open failed: {e:?}");
                continue;
            }
        };

        if let USBSelector::DeviceName(expected) = selector {
            let names = read_device_names(&handle, &device, &desc);
            if !names.iter().any(|name| usb_name_matches(name, expected)) {
                debug!(vid, pid, names = ?names, "skip device: name mismatch");
                continue;
            }
            info!(vid, pid, name = expected, "matched USB device name");
        }

        let Some(endpoints) = find_endpoints(&device)? else {
            debug!(vid, pid, "skip device: no interrupt endpoint pair");
            continue;
        };

        info!(
            vid,
            pid,
            bus = device.bus_number(),
            address = device.address(),
            in_iface = endpoints.in_iface,
            ep_in = format!("{:#04x}", endpoints.ep_in),
            out_iface = endpoints.out_iface,
            ep_out = format!("{:#04x}", endpoints.ep_out),
            "found USB printer"
        );

        // 同一个接口只声明一次；跨接口时两个都要声明。
        let mut claimed: Vec<(u8, u8)> = Vec::new();
        for (iface, setting) in [
            (endpoints.out_iface, endpoints.out_setting),
            (endpoints.in_iface, endpoints.in_setting),
        ] {
            if claimed.contains(&(iface, setting)) {
                continue;
            }
            if handle.kernel_driver_active(iface).unwrap_or(false) {
                handle
                    .detach_kernel_driver(iface)
                    .context("detach kernel driver")?;
            }
            handle
                .claim_interface(iface)
                .context("claim USB interface")?;
            handle
                .set_alternate_setting(iface, setting)
                .context("set alternate setting")?;
            claimed.push((iface, setting));
        }

        return Ok((handle, endpoints));
    }

    bail!("no matching USB printer found")
}

/// 把一段命令区按 HID 报告写出去。
fn write_cmd_area(
    handle: &rusb::DeviceHandle<rusb::Context>,
    endpoints: &UsbEndpoints,
    cmd_area: &[u8],
) -> Result<(), String> {
    let reports = usb::encode_reports(cmd_area).map_err(|e| e.to_string())?;
    for report in reports {
        let written = handle
            .write_interrupt(endpoints.ep_out, &report, WRITE_TIMEOUT)
            .map_err(|e| format!("write interrupt failed: {e:?}"))?;
        if written != report.len() {
            return Err(format!(
                "short write: {written} of {} byte(s)",
                report.len()
            ));
        }
    }
    Ok(())
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
                trace!("response payload: {:02X?}", payload);
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

/// 后台线程：USB 读写 + 命令区切帧。
fn bg(
    selector: USBSelector,
    mv: ModelVersion,
    mut out_rx: mpsc::Receiver<OutJob>,
    recv_tx: mpsc::UnboundedSender<Vec<u8>>,
    ready_tx: watch::Sender<ReadyStatus>,
) {
    let (handle, endpoints) = match open_device(&selector) {
        Ok(v) => {
            ready_tx.send_replace(ReadyStatus::Ready);
            v
        }
        Err(e) => {
            error!("open device failed: {e:?}");
            ready_tx.send_replace(ReadyStatus::Failed(e.to_string()));
            return;
        }
    };

    let assembler = Assembler::new(mv);
    let mut received = Vec::new();
    let mut report = vec![0u8; usb::REPORT_SIZE];

    loop {
        // 先处理所有待写任务。
        loop {
            match out_rx.try_recv() {
                Ok(job) => {
                    let result = write_cmd_area(&handle, &endpoints, &job.cmd_area);
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

        match handle.read_interrupt(endpoints.ep_in, &mut report, READ_TIMEOUT) {
            Ok(n) => match usb::decode_report(&report[..n]) {
                Ok(chunk) => {
                    received.extend_from_slice(chunk);
                    if !drain_frames(&assembler, &mut received, &recv_tx) {
                        debug!("frontend closed, device thread exits");
                        return;
                    }
                }
                Err(e) => warn!("invalid USB report: {e:?}"),
            },
            Err(rusb::Error::Timeout) => {}
            Err(e) => {
                error!("read interrupt failed: {e:?}");
                return;
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    // 用法：`test_usb [型号-序列号]`，不带参数时取第一台德佟设备。
    let tf = match std::env::args().nth(1) {
        Some(name) => {
            info!("select device by name: {name}");
            TestFrontend::new_by_name(name)
        }
        None => TestFrontend::new(),
    };
    tf.ready().await?;
    info!("device ready (model version: {:?})", tf.model_version());

    app(&tf).await?;
    Ok(())
}

async fn app(tf: &TestFrontend) -> anyhow::Result<()> {
    phase_single(tf).await?;
    phase_device_info(tf).await?;
    phase_flow_control(tf).await?;
    phase_high_command(tf).await?;
    phase_device_info_probe(tf).await?;
    phase_settings(tf).await?;
    phase_raw_probes(tf).await?;
    phase_push_listen(tf).await?;
    Ok(())
}

/// `0x71` 单条查询，顺带验证「解析失败不消费缓冲区」。
async fn phase_single(tf: &TestFrontend) -> anyhow::Result<()> {
    info!("[1] 单条查询 + 错误类型重试");
    tf.try_send_batch(&[&req::dpi::Dpi {}]).await?;

    match tf.try_recv_one::<resp::speed::Speed>().await {
        Err(RecvError::Parse { expected, .. }) => {
            info!(expected, "错误类型被拒绝，缓冲区未被消费");
        }
        Ok(_) => bail!("用 Speed 解析 DPI 居然成功了"),
        Err(e) => return Err(e.into()),
    }

    let dpi: resp::dpi::Dpi = tf.try_recv_one().await?;
    info!("DPI = {}", dpi.dpi);
    Ok(())
}

/// `0x70` / `0x72` / `0x73` / `0x75` / `0x79` / `0x7c` / `0x7d` / `0x7f` 批量查询。
async fn phase_device_info(tf: &TestFrontend) -> anyhow::Result<()> {
    info!("[2] 设备信息批量查询");

    tf.try_send_batch(&[
        &req::status::Status {},
        &req::print_width::PrintWidth {},
        &req::statistics::Statistics {},
        &req::manufacturer::Manufacturer {},
        &req::device_name::DeviceName {},
        &req::software_version::SoftwareVersion {},
        &req::bluetooth_mac::BluetoothMac {},
        &req::device_info::DeviceInfo {},
    ])
    .await?;

    let status: resp::status::Status = tf.try_recv_one().await?;
    info!(
        "status = {:?}, page_key = {:#06x}, tail = {:02X?}",
        status.status, status.page_key, status.tail
    );

    let width: resp::print_width::PrintWidth = tf.try_recv_one().await?;
    info!(
        "print width = {}, paper width = {:.1}mm, tail = {:02X?}",
        width.print_width,
        width.paper_width as f32 / 10.0,
        width.tail
    );

    let stats: resp::statistics::Statistics = tf.try_recv_one().await?;
    info!(
        "work lines = {}, print lines = {}, null lines = {}, pages = {}",
        stats.work_lines, stats.print_lines, stats.null_lines, stats.print_pages
    );

    let manufacturer: resp::manufacturer::Manufacturer = tf.try_recv_one().await?;
    info!("manufacturer = {}", manufacturer.manufacturer);

    let name: resp::device_name::DeviceName = tf.try_recv_one().await?;
    info!("device name = {}", name.device_name);

    let version: resp::software_version::SoftwareVersion = tf.try_recv_one().await?;
    info!("software version = {}", version.software_version);

    let mac: resp::bluetooth_mac::BluetoothMac = tf.try_recv_one().await?;
    info!(
        "bluetooth mac = classic {}, ble {}",
        mac.format_classic(),
        mac.format_ble()
    );

    let info: resp::device_info::DeviceInfo = tf.try_recv_one().await?;
    info!("0x7f device info = {:02X?}", info.data);

    Ok(())
}

/// `0x77` 流控：单独查询、SDK 的 `0x70 + 0x77` 组合、`0x42 + 0x77` 组合。
async fn phase_flow_control(tf: &TestFrontend) -> anyhow::Result<()> {
    info!("[3] 流控 0x77");

    tf.try_send_batch(&[&req::buffer_size::BufferSize {}])
        .await?;
    let buf: resp::buffer_size::BufferSize = tf.try_recv_one().await?;
    log_buffer_size("0x77 单独", &buf);

    // SDK 预算耗尽时的组合：0x70 + 0x77 打在同一个命令区里。
    tf.try_send_batch(&[&req::status::Status {}, &req::buffer_size::BufferSize {}])
        .await?;
    let status: resp::status::Status = tf.try_recv_one().await?;
    let buf: resp::buffer_size::BufferSize = tf.try_recv_one().await?;
    info!(
        "0x70 + 0x77: status = {:?}, budget = {} byte(s)",
        status.status,
        buf.budget()
    );

    // SDK 的 p1()：读纸张类型 + 取预算。
    tf.try_send_batch(&[&ReadCmd(0x42), &req::buffer_size::BufferSize {}])
        .await?;
    let gap: resp::gap_type::GapType = tf.try_recv_one().await?;
    let buf: resp::buffer_size::BufferSize = tf.try_recv_one().await?;
    info!(
        "0x42 + 0x77: gap_type = {:?}, budget = {} byte(s)",
        gap.gap_type,
        buf.budget()
    );

    Ok(())
}

/// `0x80` 激活 / 结束 `> 0x80` 命令。
async fn phase_high_command(tf: &TestFrontend) -> anyhow::Result<()> {
    info!("[4] 0x80 激活 >0x80 命令");

    tf.try_send_batch(&[&req::high_command::HighCommand::default()])
        .await?;
    match tf
        .try_recv_one_timeout::<resp::high_command::HighCommand>(PROBE_TIMEOUT)
        .await
    {
        Ok(h) => info!("0x80 enable -> {:?}, tail = {:02X?}", h.mode, h.tail),
        Err(RecvError::Parse { .. }) => {
            if let Some(raw) = tf.try_pop_raw().await {
                warn!("0x80 enable 解析失败，丢弃: {:02X?}", raw);
            }
        }
        Err(e) => warn!("0x80 enable: {e}"),
    }

    tf.try_send_batch(&[&req::high_command::HighCommand {
        mode: HighCommandMode::Disable,
    }])
    .await?;
    match tf
        .try_recv_one_timeout::<resp::high_command::HighCommand>(PROBE_TIMEOUT)
        .await
    {
        Ok(h) => info!("0x80 disable -> {:?}, tail = {:02X?}", h.mode, h.tail),
        Err(RecvError::Parse { .. }) => {
            if let Some(raw) = tf.try_pop_raw().await {
                warn!("0x80 disable 解析失败，丢弃: {:02X?}", raw);
            }
        }
        Err(e) => warn!("0x80 disable: {e}"),
    }

    Ok(())
}

/// `0x7f` 设备信息探测。
///
/// DP27P 无参数读取固定回 16 字节全 `0x00`，所以再试两种可能：
/// 需要先激活 `> 0x80` 命令，或者需要带子索引参数。
async fn phase_device_info_probe(tf: &TestFrontend) -> anyhow::Result<()> {
    info!("[4b] 0x7f 设备信息探测");

    tf.try_send_batch(&[&req::high_command::HighCommand::default()])
        .await?;
    match recv_matching(tf, 0x80, PROBE_TIMEOUT).await? {
        Some(raw) => info!("0x7f 前激活: {:02X?}", raw.0),
        None => warn!("0x7f 前激活: 无响应"),
    }

    info!("激活后无参数读 0x7f");
    read_device_info(tf, "").await?;

    // 可能是带子索引的读取：把常见索引宽度都试一遍。
    let probes: &[(&str, &[u8])] = &[
        ("1 字节 00", &[0x00]),
        ("1 字节 01", &[0x01]),
        ("1 字节 02", &[0x02]),
        ("1 字节 03", &[0x03]),
        ("1 字节 04", &[0x04]),
        ("1 字节 10", &[0x10]),
        ("1 字节 7f", &[0x7f]),
        ("1 字节 ff", &[0xff]),
        ("2 字节 00 00", &[0x00, 0x00]),
        ("2 字节 00 01", &[0x00, 0x01]),
        ("2 字节 01 00", &[0x01, 0x00]),
        ("16 字节 00", &[0x00; 16]),
    ];

    for (label, data) in probes {
        tf.try_send_batch(&[&DataCmd { cmd: 0x7f, data }]).await?;
        match recv_matching(tf, 0x7f, PROBE_TIMEOUT).await? {
            Some(raw) => log_device_info(tf, label, &raw.0),
            None => warn!("0x7f {label}: 无响应"),
        }
    }

    tf.try_send_batch(&[&req::high_command::HighCommand {
        mode: HighCommandMode::Disable,
    }])
    .await?;
    match recv_matching(tf, 0x80, PROBE_TIMEOUT).await? {
        Some(raw) => info!("0x7f 后结束激活: {:02X?}", raw.0),
        None => warn!("0x7f 后结束激活: 无响应"),
    }

    Ok(())
}

/// 无参数读一次 `0x7f` 并记录。
async fn read_device_info(tf: &TestFrontend, label: &str) -> anyhow::Result<()> {
    tf.try_send_batch(&[&req::device_info::DeviceInfo {}])
        .await?;
    match recv_matching(tf, 0x7f, PROBE_TIMEOUT).await? {
        Some(raw) => log_device_info(tf, label, &raw.0),
        None => warn!("0x7f {label}: 无响应"),
    }
    Ok(())
}

fn log_device_info(tf: &TestFrontend, label: &str, raw: &[u8]) {
    match resp::device_info::DeviceInfo::parse(raw, tf.model_version()) {
        Ok(info) => info!(
            "0x7f {label} -> {} byte(s): {:02X?}",
            info.data.len(),
            info.data
        ),
        Err(e) => warn!("0x7f {label} 解析失败: {e}; raw = {:02X?}", raw),
    }
}

/// 打印设置读取：`0x43` / `0x44` / `0x45` 都是无参数读取。
async fn phase_settings(tf: &TestFrontend) -> anyhow::Result<()> {
    info!("[5] 打印设置读取");

    if let Some(raw) = read_raw(tf, 0x43).await? {
        match resp::darkness::Darkness::parse(&raw.0, tf.model_version()) {
            Ok(v) => info!("0x43 浓度 = {:?}, raw = {:02X?}", v.darkness, raw.0),
            Err(e) => warn!("0x43 解析失败: {e}; raw = {:02X?}", raw.0),
        }
    }

    if let Some(raw) = read_raw(tf, 0x44).await? {
        match resp::speed::Speed::parse(&raw.0, tf.model_version()) {
            Ok(v) => info!("0x44 速度 = {:?}, raw = {:02X?}", v.speed, raw.0),
            Err(e) => warn!("0x44 解析失败: {e}; raw = {:02X?}", raw.0),
        }
    }

    if let Some(raw) = read_raw(tf, 0x45).await? {
        match resp::gap_length::GapLength::parse(&raw.0, tf.model_version()) {
            Ok(v) => info!("0x45 纸间隔 = {} (0.01mm), raw = {:02X?}", v.gap, raw.0),
            Err(e) => warn!("0x45 解析失败: {e}; raw = {:02X?}", raw.0),
        }
    }

    Ok(())
}

/// 还没建模的读取命令，逐个发、逐个记录原始响应。
///
/// 以下命令**不发**：
/// - `0x7e`：会让打印机重启。
/// - `0x78`：会写时间。
/// - `0x80` / `0x84` / `0x88` / `0x9e`：需要先发 `0x80` 激活 `> 0x80` 命令。
/// - `0x7b`：固件主动上报，主机不发。
async fn phase_raw_probes(tf: &TestFrontend) -> anyhow::Result<()> {
    const PROBES: &[(u8, &str)] = &[
        (0x48, "自动关机时间"),
        (0x52, "支持的纸张类型"),
        (0x57, "支持的电机模式"),
        (0x59, "支持的语言"),
        (0x5b, "未确认"),
        (0x7a, "硬件版本"),
    ];

    info!("[6] 未建模命令探测");

    for &(code, name) in PROBES {
        tf.try_send_batch(&[&ReadCmd(code)]).await?;
        match recv_matching(tf, code, PROBE_TIMEOUT).await? {
            Some(raw) => info!("{code:#04x} {name}: {:02X?}", raw.0),
            None => warn!("{code:#04x} {name}: 无响应"),
        }
    }

    Ok(())
}

/// 被动等 `0x7b`。它是固件主动上报，主机不发请求。
async fn phase_push_listen(tf: &TestFrontend) -> anyhow::Result<()> {
    info!("[7] 被动监听 0x7b");

    match recv_matching(tf, 0x7b, PUSH_TIMEOUT).await? {
        Some(raw) => match resp::buffer_size2::BufferSize2::parse(&raw.0, tf.model_version()) {
            Ok(buf) => info!(
                "收到 0x7b 推送: size = {}, budget = {} byte(s)",
                buf.size,
                buf.budget()
            ),
            Err(e) => warn!("0x7b 解析失败: {e}; raw = {:02X?}", raw.0),
        },
        None => info!("这段时间没有 0x7b 推送"),
    }

    Ok(())
}

/// 发一条无参数读取命令，拿回原始 payload。
async fn read_raw(tf: &TestFrontend, code: u8) -> anyhow::Result<Option<RawPayload>> {
    tf.try_send_batch(&[&ReadCmd(code)]).await?;
    let raw = recv_matching(tf, code, PROBE_TIMEOUT).await?;
    if raw.is_none() {
        warn!("{code:#04x}: 无响应");
    }
    Ok(raw)
}

/// 等一条命令字节匹配的原始响应；前面不匹配的帧会被丢弃并记录。
///
/// 用来在解析失败或晚到时恢复缓冲区对齐，避免把上一条响应当成下一条。
async fn recv_matching(
    tf: &TestFrontend,
    cmd: u8,
    timeout: Duration,
) -> anyhow::Result<Option<RawPayload>> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        match tf.try_recv_one_timeout::<RawPayload>(remaining).await {
            Ok(raw) => {
                let actual = raw.0.get(1).copied();
                if actual == Some(cmd) {
                    return Ok(Some(raw));
                }
                warn!(
                    "期望 {cmd:#04x}，丢弃不匹配的头部帧 {:#04x}: {:02X?}",
                    actual.unwrap_or(0x00),
                    raw.0
                );
            }
            Err(RecvError::Timeout(_)) => return Ok(None),
            Err(RecvError::Closed) => bail!("device thread is gone"),
            Err(e) => return Err(e.into()),
        }
    }
}

fn log_buffer_size(tag: &str, buf: &resp::buffer_size::BufferSize) {
    info!(
        "{tag}: size = {} ({} byte(s)), status = {:#010x}, sync_token = {:#06x}, \
         ok = {}, waiting_data = {}, error = {}",
        buf.size,
        buf.budget(),
        buf.status,
        buf.sync_token,
        buf.is_ok(),
        buf.is_waiting_data(),
        buf.is_error()
    );
}
