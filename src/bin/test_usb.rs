// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 真机 USB 测试平台。
//!
//! ```text
//! test_usb [DEVICE] [--phase <PHASE>] [--rle <MODE>] [--rows <N>]
//! ```
//!
//! - `DEVICE`：USB 设备名（`型号-序列号`），省略时取第一台德佟设备。
//! - `--phase`：要执行的测试阶段，可重复或用逗号分隔，默认 `device-info`。
//! - `--rle`：打印测试使用的 RLE 模式。
//! - `--rows`：`print` 阶段测试图的行数。
//! - `--short-rows` / `--short-pages`：`short-print` 阶段每页行数和页数。
//! - `--poll-ms` / `--grace-ms`：等待打印完成时的 `0x70` 轮询间隔和启动宽限期。
//!
//! `--phase print` 和 `--phase short-print` 会真的驱动打印头并走纸。

use std::time::Duration;

use anyhow::{Result, bail};
use clap::{Parser, ValueEnum};
use mytm::TaskManager;
use pretty_hex::simple_hex;
use tracing::{debug, info, warn};

use dz_print::{
    backend::usb::UsbSelector,
    bitmap::Bitmap,
    command::{
        ModelVersion,
        envelope::{Envelope, EnvelopeKind},
        high_command::HighCommandMode,
        req::{self, PackReqError, ReqTrait},
        resp::{self, RespTrait},
    },
    frontend::{Device, DeviceConfig, RecvError},
    planner::print::RleMode,
};

/// 单条探测命令等响应的超时。
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
/// 被动监听 `0x7b` 的时间。
const PUSH_TIMEOUT: Duration = Duration::from_secs(1);
/// 退出前等 worker 线程真正停下（释放 USB 句柄）的超时。
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// 测试阶段。
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// 单条查询 + 错误类型重试
    #[value(name = "single")]
    Single,
    /// 设备信息批量查询
    #[value(name = "device-info")]
    DeviceInfo,
    /// `0x77` 流控
    #[value(name = "flow-control")]
    FlowControl,
    /// `0x80` 激活 / 结束
    #[value(name = "high-command")]
    HighCommand,
    /// `0x7f` 设备信息探测
    #[value(name = "device-info-probe")]
    DeviceInfoProbe,
    /// `0x43` / `0x44` / `0x45` 设置读取
    #[value(name = "settings")]
    Settings,
    /// 未建模命令探测
    #[value(name = "raw-probes")]
    RawProbes,
    /// 被动监听 `0x7b`
    #[value(name = "push-listen")]
    PushListen,
    /// 打印测试页（会真的走纸）
    #[value(name = "print")]
    Print,
    /// 连续打印短页面并对账统计（复现 pogo pin 一工作就断开；会真的走纸）
    #[value(name = "short-print")]
    ShortPrint,
}

/// 打印测试使用的 RLE 模式。
#[derive(ValueEnum, Clone, Copy, Debug, Default)]
enum RleArg {
    #[default]
    #[value(name = "off")]
    Off,
    #[value(name = "byte")]
    Byte,
    #[value(name = "bits4")]
    Bits4,
    #[value(name = "bits5")]
    Bits5,
    #[value(name = "bits6")]
    Bits6,
    #[value(name = "bits4x")]
    Bits4x,
    #[value(name = "bits5x")]
    Bits5x,
    #[value(name = "bits6x")]
    Bits6x,
}

impl From<RleArg> for RleMode {
    fn from(value: RleArg) -> Self {
        match value {
            RleArg::Off => RleMode::Off,
            RleArg::Byte => RleMode::Byte,
            RleArg::Bits4 => RleMode::Bits4,
            RleArg::Bits5 => RleMode::Bits5,
            RleArg::Bits6 => RleMode::Bits6,
            RleArg::Bits4x => RleMode::Bits4XOnly,
            RleArg::Bits5x => RleMode::Bits5XOnly,
            RleArg::Bits6x => RleMode::Bits6XOnly,
        }
    }
}

#[derive(Parser, Debug)]
#[command(version, about = "真机 USB 测试平台")]
struct Cli {
    /// 按 USB 设备名（`型号-序列号`）选择；省略时取第一台德佟设备
    device: Option<String>,

    /// 要执行的测试阶段，可重复或用逗号分隔
    #[arg(long, value_enum, value_delimiter = ',', default_value = "device-info")]
    phase: Vec<Phase>,

    /// 打印测试使用的 RLE 模式
    #[arg(long, value_enum, default_value = "off")]
    rle: RleArg,

    /// 打印测试图的行数（`print` 阶段；超过 16383 会自动拆页）
    #[arg(long, default_value_t = 64 * 50)]
    rows: usize,

    /// `short-print` 每页的行数
    #[arg(long, default_value_t = 32)]
    short_rows: usize,

    /// `short-print` 连续打印的页数
    #[arg(long, default_value_t = 3)]
    short_pages: usize,

    /// 等待打印完成时轮询 `0x70` 的间隔（毫秒）
    ///
    /// 短页面想抓到转瞬即逝的 `Printing` / 错误状态时调到 1~5。
    #[arg(long, default_value_t = 100)]
    poll_ms: u64,

    /// `EndPage` 后至少等多久才接受空闲（毫秒）
    #[arg(long, default_value_t = 1000)]
    grace_ms: u64,

    /// 连续几次空闲才认为打印完成
    #[arg(long, default_value_t = 2)]
    idle_confirm: usize,

    /// 连续几次状态查询没有响应就判定打印机失联
    #[arg(long, default_value_t = 5)]
    silent_limit: usize,

    /// 等待打印完成的总超时（秒）
    #[arg(long, default_value_t = 600)]
    print_timeout: u64,

    /// 单条响应的等待超时（毫秒）
    #[arg(long, default_value_t = 3000)]
    recv_timeout_ms: u64,

    /// `short-print` 在打印返回后再等多久读 `0x73`（毫秒）
    ///
    /// 给固件留出更新累计计数器的时间，避免把「还没记上」误判成「没打印」。
    #[arg(long, default_value_t = 500)]
    settle_ms: u64,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_file(true)
        .with_line_number(true)
        .with_target(false)
        .init();

    let cli = Cli::parse();
    let selector = match cli.device.as_deref() {
        Some(name) => {
            info!("select device by name: {name}");
            UsbSelector::DeviceName(name.to_owned())
        }
        None => UsbSelector::Auto,
    };

    // 整个程序共用一个 TaskManager：worker 挂在它下面，退出前统一 close。
    let tm = TaskManager::new();
    let dev = Device::new(
        &tm,
        DeviceConfig {
            selector,
            recv_timeout: Duration::from_millis(cli.recv_timeout_ms.max(1)),
            print_poll: Duration::from_millis(cli.poll_ms.max(1)),
            print_start_grace: Duration::from_millis(cli.grace_ms),
            print_idle_confirm: cli.idle_confirm.max(1),
            print_silent_limit: cli.silent_limit.max(1),
            print_done_timeout: Duration::from_secs(cli.print_timeout.max(1)),
            ..Default::default()
        },
    );

    let result = async {
        dev.ready().await?;
        info!("device ready (model version: {:?})", dev.model_version());
        run_phases(&dev, &cli).await
    }
    .await;

    // 无论阶段成功与否，都要等 worker 线程退出后再让进程结束。
    let stopped = dev.shutdown(SHUTDOWN_TIMEOUT).await;
    tm.close();
    result?;
    stopped?;
    Ok(())
}

/// 按顺序执行命令行里所有阶段。
async fn run_phases(dev: &Device, cli: &Cli) -> Result<()> {
    for &phase in &cli.phase {
        info!(?phase, "running phase");
        run_phase(dev, phase, cli).await?;
    }
    Ok(())
}

async fn run_phase(dev: &Device, phase: Phase, cli: &Cli) -> Result<()> {
    match phase {
        Phase::Single => phase_single(dev).await,
        Phase::DeviceInfo => phase_device_info(dev).await,
        Phase::FlowControl => phase_flow_control(dev).await,
        Phase::HighCommand => phase_high_command(dev).await,
        Phase::DeviceInfoProbe => phase_device_info_probe(dev).await,
        Phase::Settings => phase_settings(dev).await,
        Phase::RawProbes => phase_raw_probes(dev).await,
        Phase::PushListen => phase_push_listen(dev).await,
        Phase::Print => phase_print(dev, cli.rle.into(), cli.rows).await,
        Phase::ShortPrint => {
            phase_short_print(
                dev,
                cli.rle.into(),
                cli.short_rows,
                cli.short_pages,
                Duration::from_millis(cli.settle_ms),
            )
            .await
        }
    }
}

/// 一张 384 x `rows` 的测试图：外框 + 每 8 行一条横线。
fn test_bitmap(rows: usize) -> Bitmap {
    let height = rows.max(1);
    let mut bm = Bitmap::new(384, height).unwrap();
    let width = bm.width();
    for y in 0..bm.height() {
        let mut line = bm.line_mut(y).unwrap();
        line.set(0, true);
        line.set(width - 1, true);
        if y % 8 == 0 {
            for x in 0..width {
                line.set(x, true);
            }
        }
        line.set((y * 6) % width, true);
    }
    bm
}

/// 用 `FlowPlanner` 打印一张测试图。**会真的走纸。**
async fn phase_print(dev: &Device, rle: RleMode, rows: usize) -> anyhow::Result<()> {
    info!("[8] 打印测试页（会真的走纸，rle = {rle:?}, rows = {rows}）");

    let bm = test_bitmap(rows);
    let report = dev.print_bitmap_job(&bm, rle).await?;
    info!(
        chunks = report.chunks,
        bytes = report.bytes,
        budget_queries = report.budget_queries,
        degraded = report.degraded,
        "print finished"
    );
    Ok(())
}

/// 连续打印短页面并在每页结束后对账，用来复现「pogo pin 一工作就断开」。
///
/// 关键点是页面要短：数据发完、`EndPage` 之后的空窗期很短，`wait_for_print_done`
/// 可能在固件真正启动打印前后就直接看到空闲并返回成功。这个阶段会把
/// `--poll-ms` 压到 1~5ms 时观察到的最终结果、`0x73` 统计增量和队列里残留的
/// 响应帧都打出来，方便判断固件到底有没有报错。
///
/// **会真的走纸。**
async fn phase_short_print(
    dev: &Device,
    rle: RleMode,
    rows: usize,
    pages: usize,
    settle: Duration,
) -> anyhow::Result<()> {
    let rows = rows.max(1);
    let pages = pages.max(1);
    info!("[9] 短页面打印 + 统计对账（会真的走纸，rle = {rle:?}, rows = {rows}, pages = {pages}）");

    let stale = drain_frames(dev).await;
    if !stale.is_empty() {
        debug!(frames = stale.len(), "清掉开始前队列里的残留帧");
    }
    let mut prev = read_statistics(dev).await?;
    info!(
        work_lines = prev.work_lines,
        print_lines = prev.print_lines,
        null_lines = prev.null_lines,
        print_pages = prev.print_pages,
        "0x73 before"
    );

    let mut suspicious = 0usize;
    let mut errored = 0usize;

    for page in 1..=pages {
        info!(page, pages, "短页面开始");
        let bm = test_bitmap(rows);
        let started = std::time::Instant::now();
        let outcome = dev.print_bitmap_job(&bm, rle).await;
        let elapsed = started.elapsed();

        // 给固件更新 0x73 的时间，同时让打印期间积压的推送都进队列。
        if !settle.is_zero() {
            tokio::time::sleep(settle).await;
        }
        let leftovers = drain_frames(dev).await;
        log_leftover_frames(dev, page, &leftovers);

        match &outcome {
            Ok(report) => info!(
                page,
                elapsed = ?elapsed,
                chunks = report.chunks,
                bytes = report.bytes,
                budget_queries = report.budget_queries,
                degraded = report.degraded,
                "print_bitmap_job 返回成功"
            ),
            Err(error) => {
                errored += 1;
                warn!(page, elapsed = ?elapsed, %error, "print_bitmap_job 返回错误");
            }
        }

        let after = match read_statistics(dev).await {
            Ok(stats) => stats,
            Err(error) => {
                warn!(page, %error, "读取 0x73 失败，设备可能已经掉线");
                return Err(error);
            }
        };
        let reset = after.work_lines < prev.work_lines
            || after.print_lines < prev.print_lines
            || after.null_lines < prev.null_lines
            || after.print_pages < prev.print_pages;
        let work = after.work_lines.saturating_sub(prev.work_lines);
        let printed = after.print_lines.saturating_sub(prev.print_lines);
        let null = after.null_lines.saturating_sub(prev.null_lines);
        let printed_pages = after.print_pages.saturating_sub(prev.print_pages);

        info!(
            page,
            work_lines = work,
            print_lines = printed,
            null_lines = null,
            print_pages = printed_pages,
            reset,
            "0x73 delta"
        );

        if reset {
            warn!(
                page,
                "0x73 计数器变小了，固件可能重启过；这一页的增量不可信"
            );
        } else if work > 0 && printed == 0 {
            suspicious += 1;
            warn!(
                page,
                work_lines = work,
                "电机走了但没有打印行：打印头 / pogo pin 很可能在开始打印时断开"
            );
        } else if work == 0 && printed == 0 {
            suspicious += 1;
            warn!(page, "走纸和打印行都没有增加：这一页实际没有打印");
        } else if printed < rows as u32 {
            warn!(
                page,
                printed, rows, "打印行数少于发送行数，这一页可能只打了一部分"
            );
        } else {
            info!(page, "这一页的统计有增量（0x73 只作参考，最终仍要看纸面）");
        }

        prev = after;
    }

    if errored > 0 {
        info!(
            errored,
            "有页面返回错误；如果纸面是空白的，错误本身也可作为检测信号"
        );
    }
    if suspicious > 0 {
        bail!(
            "{pages} 页里有 {suspicious} 页发出后没有打印行增量；\
             如果纸面是空白的，说明打印头 / pogo pin 在开始打印时断开了"
        );
    }
    Ok(())
}

/// 读一次 `0x73` 打印统计。
async fn read_statistics(dev: &Device) -> anyhow::Result<resp::statistics::Statistics> {
    dev.try_send_batch(&[&req::statistics::Statistics {}])
        .await?;
    match dev
        .recv_cmd_timeout::<resp::statistics::Statistics>(resp::statistics::CMD, PROBE_TIMEOUT)
        .await?
    {
        Some(stats) => Ok(stats),
        None => bail!("0x73 统计查询超时"),
    }
}

/// 把接收队列里剩余的帧全部取出来。
///
/// 打印期间 `wait_for_print_done` 只消费 `0x70`，`0x7b` 之类的推送会留在队列里；
/// 打印刚结束时也可能还有没被读走的状态帧。
async fn drain_frames(dev: &Device) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    while let Some(raw) = dev.try_pop_raw().await {
        frames.push(raw);
    }
    frames
}

fn log_leftover_frames(dev: &Device, page: usize, frames: &[Vec<u8>]) {
    if frames.is_empty() {
        debug!(page, "打印结束后没有残留帧");
        return;
    }
    for raw in frames {
        let cmd = raw.get(1).copied().unwrap_or(0x00);
        if cmd == resp::status::CMD {
            match resp::status::Status::parse(raw, dev.model_version()) {
                Ok(status) => warn!(
                    page,
                    status = ?status.status,
                    reason = %status.status,
                    page_key = status.page_key,
                    raw = %simple_hex(raw),
                    "打印结束后队列里残留 0x70 状态帧"
                ),
                Err(error) => warn!(
                    page,
                    %error,
                    raw = %simple_hex(raw),
                    "打印结束后队列里残留无法解析的 0x70 帧"
                ),
            }
        } else {
            info!(
                page,
                cmd = format!("{cmd:#04x}"),
                raw = %simple_hex(raw),
                "打印结束后队列里残留响应帧"
            );
        }
    }
}

/// `0x71` 单条查询，顺带验证「解析失败不消费缓冲区」。
async fn phase_single(dev: &Device) -> anyhow::Result<()> {
    info!("[1] 单条查询 + 错误类型重试");
    dev.try_send_batch(&[&req::dpi::Dpi {}]).await?;

    match dev.try_recv_one::<resp::speed::Speed>().await {
        Err(RecvError::Parse { expected, .. }) => {
            info!(expected, "错误类型被拒绝，缓冲区未被消费");
        }
        Ok(_) => bail!("用 Speed 解析 DPI 居然成功了"),
        Err(e) => return Err(e.into()),
    }

    let dpi: resp::dpi::Dpi = dev.try_recv_one().await?;
    info!("DPI = {}", dpi.dpi);
    Ok(())
}

/// `0x70` / `0x72` / `0x73` / `0x75` / `0x79` / `0x7c` / `0x7d` / `0x7f` 批量查询。
async fn phase_device_info(dev: &Device) -> anyhow::Result<()> {
    info!("[2] 设备信息批量查询");

    dev.try_send_batch(&[
        &req::status::Status {},
        &req::print_width::PrintWidth {},
        &req::dpi::Dpi {},
        &req::statistics::Statistics {},
        &req::manufacturer::Manufacturer {},
        &req::device_name::DeviceName {},
        &req::software_version::SoftwareVersion {},
        &req::bluetooth_mac::BluetoothMac {},
        &req::device_info::DeviceInfo {},
    ])
    .await?;

    let status: resp::status::Status = dev.try_recv_one().await?;
    info!(
        "status = {:?}, page_key = {:#06x}, tail = {}",
        status.status,
        status.page_key,
        simple_hex(&status.tail)
    );

    let width: resp::print_width::PrintWidth = dev.try_recv_one().await?;
    info!(
        "print width = {}, paper width = {:.1}mm, tail = {}",
        width.print_width,
        width.paper_width as f32 / 10.0,
        simple_hex(&width.tail)
    );

    let dpi: resp::dpi::Dpi = dev.try_recv_one().await?;
    info!("dpi = {}", dpi.dpi);

    let stats: resp::statistics::Statistics = dev.try_recv_one().await?;
    info!(
        "work lines = {}, print lines = {}, null lines = {}, pages = {}",
        stats.work_lines, stats.print_lines, stats.null_lines, stats.print_pages
    );

    let manufacturer: resp::manufacturer::Manufacturer = dev.try_recv_one().await?;
    info!("manufacturer = {}", manufacturer.manufacturer);

    let name: resp::device_name::DeviceName = dev.try_recv_one().await?;
    info!("device name = {}", name.device_name);

    let version: resp::software_version::SoftwareVersion = dev.try_recv_one().await?;
    info!("software version = {}", version.software_version);

    let mac: resp::bluetooth_mac::BluetoothMac = dev.try_recv_one().await?;
    info!(
        "bluetooth mac = classic {}, ble {}",
        mac.format_classic(),
        mac.format_ble()
    );

    let info: resp::device_info::DeviceInfo = dev.try_recv_one().await?;
    info!("0x7f device info = {}", simple_hex(&info.data));

    Ok(())
}

/// `0x77` 流控：单独查询、SDK 的 `0x70 + 0x77` 组合、`0x42 + 0x77` 组合。
async fn phase_flow_control(dev: &Device) -> anyhow::Result<()> {
    info!("[3] 流控 0x77");

    dev.try_send_batch(&[&req::buffer_size::BufferSize {}])
        .await?;
    let buf: resp::buffer_size::BufferSize = dev.try_recv_one().await?;
    log_buffer_size("0x77 单独", &buf);

    // SDK 预算耗尽时的组合：0x70 + 0x77 打在同一个命令区里。
    dev.try_send_batch(&[&req::status::Status {}, &req::buffer_size::BufferSize {}])
        .await?;
    let status: resp::status::Status = dev.try_recv_one().await?;
    let buf: resp::buffer_size::BufferSize = dev.try_recv_one().await?;
    info!(
        "0x70 + 0x77: status = {:?}, budget = {} byte(s)",
        status.status,
        buf.budget()
    );

    // SDK 的 p1()：读纸张类型 + 取预算。
    dev.try_send_batch(&[&ReadCmd(0x42), &req::buffer_size::BufferSize {}])
        .await?;
    let gap: resp::gap_type::GapType = dev.try_recv_one().await?;
    let buf: resp::buffer_size::BufferSize = dev.try_recv_one().await?;
    info!(
        "0x42 + 0x77: gap_type = {:?}, budget = {} byte(s)",
        gap.gap_type,
        buf.budget()
    );

    Ok(())
}

/// `0x80` 激活 / 结束 `> 0x80` 命令。
async fn phase_high_command(dev: &Device) -> anyhow::Result<()> {
    info!("[4] 0x80 激活 >0x80 命令");

    dev.try_send_batch(&[&req::high_command::HighCommand::default()])
        .await?;
    match dev
        .try_recv_one_timeout::<resp::high_command::HighCommand>(PROBE_TIMEOUT)
        .await
    {
        Ok(h) => info!(
            "0x80 enable -> {:?}, tail = {}",
            h.mode,
            simple_hex(&h.tail)
        ),
        Err(RecvError::Parse { .. }) => {
            if let Some(raw) = dev.try_pop_raw().await {
                warn!("0x80 enable 解析失败，丢弃: {}", simple_hex(&raw));
            }
        }
        Err(e) => warn!("0x80 enable: {e}"),
    }

    dev.try_send_batch(&[&req::high_command::HighCommand {
        mode: HighCommandMode::Disable,
    }])
    .await?;
    match dev
        .try_recv_one_timeout::<resp::high_command::HighCommand>(PROBE_TIMEOUT)
        .await
    {
        Ok(h) => info!(
            "0x80 disable -> {:?}, tail = {}",
            h.mode,
            simple_hex(&h.tail)
        ),
        Err(RecvError::Parse { .. }) => {
            if let Some(raw) = dev.try_pop_raw().await {
                warn!("0x80 disable 解析失败，丢弃: {}", simple_hex(&raw));
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
async fn phase_device_info_probe(dev: &Device) -> anyhow::Result<()> {
    info!("[4b] 0x7f 设备信息探测");

    dev.try_send_batch(&[&req::high_command::HighCommand::default()])
        .await?;
    match dev.recv_matching(0x80, PROBE_TIMEOUT).await? {
        Some(raw) => info!("0x7f 前激活: {}", simple_hex(&raw)),
        None => warn!("0x7f 前激活: 无响应"),
    }

    info!("激活后无参数读 0x7f");
    read_device_info(dev, "").await?;

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
        dev.try_send_batch(&[&DataCmd { cmd: 0x7f, data }]).await?;
        match dev.recv_matching(0x7f, PROBE_TIMEOUT).await? {
            Some(raw) => log_device_info(dev, label, &raw),
            None => warn!("0x7f {label}: 无响应"),
        }
    }

    dev.try_send_batch(&[&req::high_command::HighCommand {
        mode: HighCommandMode::Disable,
    }])
    .await?;
    match dev.recv_matching(0x80, PROBE_TIMEOUT).await? {
        Some(raw) => info!("0x7f 后结束激活: {}", simple_hex(&raw)),
        None => warn!("0x7f 后结束激活: 无响应"),
    }

    Ok(())
}

/// 无参数读一次 `0x7f` 并记录。
async fn read_device_info(dev: &Device, label: &str) -> anyhow::Result<()> {
    dev.try_send_batch(&[&req::device_info::DeviceInfo {}])
        .await?;
    match dev.recv_matching(0x7f, PROBE_TIMEOUT).await? {
        Some(raw) => log_device_info(dev, label, &raw),
        None => warn!("0x7f {label}: 无响应"),
    }
    Ok(())
}

fn log_device_info(dev: &Device, label: &str, raw: &[u8]) {
    match resp::device_info::DeviceInfo::parse(raw, dev.model_version()) {
        Ok(info) => info!(
            "0x7f {label} -> {} byte(s): {}",
            info.data.len(),
            simple_hex(&info.data)
        ),
        Err(e) => warn!("0x7f {label} 解析失败: {e}; raw = {}", simple_hex(&raw)),
    }
}

/// 打印设置读取：`0x43` / `0x44` / `0x45` 都是无参数读取。
async fn phase_settings(dev: &Device) -> anyhow::Result<()> {
    info!("[5] 打印设置读取");

    if let Some(raw) = read_raw(dev, 0x43).await? {
        match resp::darkness::Darkness::parse(&raw, dev.model_version()) {
            Ok(v) => info!("0x43 浓度 = {:?}, raw = {}", v.darkness, simple_hex(&raw)),
            Err(e) => warn!("0x43 解析失败: {e}; raw = {}", simple_hex(&raw)),
        }
    }

    if let Some(raw) = read_raw(dev, 0x44).await? {
        match resp::speed::Speed::parse(&raw, dev.model_version()) {
            Ok(v) => info!("0x44 速度 = {:?}, raw = {}", v.speed, simple_hex(&raw)),
            Err(e) => warn!("0x44 解析失败: {e}; raw = {}", simple_hex(&raw)),
        }
    }

    if let Some(raw) = read_raw(dev, 0x45).await? {
        match resp::gap_length::GapLength::parse(&raw, dev.model_version()) {
            Ok(v) => info!(
                "0x45 纸间隔 = {} (0.01mm), raw = {}",
                v.gap,
                simple_hex(&raw)
            ),
            Err(e) => warn!("0x45 解析失败: {e}; raw = {}", simple_hex(&raw)),
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
async fn phase_raw_probes(dev: &Device) -> anyhow::Result<()> {
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
        dev.try_send_batch(&[&ReadCmd(code)]).await?;
        match dev.recv_matching(code, PROBE_TIMEOUT).await? {
            Some(raw) => info!("{code:#04x} {name}: {}", simple_hex(&raw)),
            None => warn!("{code:#04x} {name}: 无响应"),
        }
    }

    Ok(())
}

/// 被动等 `0x7b`。它是固件主动上报，主机不发请求。
async fn phase_push_listen(dev: &Device) -> anyhow::Result<()> {
    info!("[7] 被动监听 0x7b");

    match dev.recv_matching(0x7b, PUSH_TIMEOUT).await? {
        Some(raw) => match resp::buffer_size2::BufferSize2::parse(&raw, dev.model_version()) {
            Ok(buf) => info!(
                "收到 0x7b 推送: size = {}, budget = {} byte(s)",
                buf.size,
                buf.budget()
            ),
            Err(e) => warn!("0x7b 解析失败: {e}; raw = {}", simple_hex(&raw)),
        },
        None => info!("这段时间没有 0x7b 推送"),
    }

    Ok(())
}

/// 发一条无参数读取命令，拿回原始 payload。
async fn read_raw(dev: &Device, code: u8) -> anyhow::Result<Option<Vec<u8>>> {
    dev.try_send_batch(&[&ReadCmd(code)]).await?;
    let raw = dev.recv_matching(code, PROBE_TIMEOUT).await?;
    if raw.is_none() {
        warn!("{code:#04x}: 无响应");
    }
    Ok(raw)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_defaults_to_device_info() {
        let cli = Cli::try_parse_from(["test_usb"]).unwrap();
        assert_eq!(cli.phase, vec![Phase::DeviceInfo]);
        assert!(matches!(cli.rle, RleArg::Off));
        assert_eq!(cli.rows, 64 * 50);
        assert_eq!(cli.short_rows, 32);
        assert_eq!(cli.short_pages, 3);
        assert_eq!(cli.poll_ms, 100);
        assert_eq!(cli.grace_ms, 1000);
        assert_eq!(cli.idle_confirm, 2);
        assert_eq!(cli.silent_limit, 5);
        assert_eq!(cli.print_timeout, 600);
        assert_eq!(cli.recv_timeout_ms, 3000);
        assert_eq!(cli.settle_ms, 500);
    }

    #[test]
    fn cli_parses_phase_list_and_print_options() {
        let cli = Cli::try_parse_from([
            "test_usb",
            "DP27P-Y4094C023",
            "--phase",
            "print,settings",
            "--rle",
            "bits5x",
            "--rows",
            "20000",
        ])
        .unwrap();

        assert_eq!(cli.device.as_deref(), Some("DP27P-Y4094C023"));
        assert_eq!(cli.phase, vec![Phase::Print, Phase::Settings]);
        assert!(matches!(cli.rle, RleArg::Bits5x));
        assert_eq!(cli.rows, 20_000);
    }

    #[test]
    fn cli_parses_short_print_options() {
        let cli = Cli::try_parse_from([
            "test_usb",
            "--phase",
            "short-print",
            "--short-rows",
            "16",
            "--short-pages",
            "5",
            "--poll-ms",
            "1",
            "--grace-ms",
            "250",
            "--idle-confirm",
            "3",
            "--silent-limit",
            "2",
            "--print-timeout",
            "30",
            "--recv-timeout-ms",
            "500",
            "--settle-ms",
            "100",
        ])
        .unwrap();

        assert_eq!(cli.phase, vec![Phase::ShortPrint]);
        assert_eq!(cli.short_rows, 16);
        assert_eq!(cli.short_pages, 5);
        assert_eq!(cli.poll_ms, 1);
        assert_eq!(cli.grace_ms, 250);
        assert_eq!(cli.idle_confirm, 3);
        assert_eq!(cli.silent_limit, 2);
        assert_eq!(cli.print_timeout, 30);
        assert_eq!(cli.recv_timeout_ms, 500);
        assert_eq!(cli.settle_ms, 100);
    }
}
