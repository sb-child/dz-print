// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 验证「打印机沉默后还发 `EndPage`」是否会让恢复变慢。
//!
//! 这个 bin 会真的打印、走纸。它按模式控制 EndPage：
//!
//! - `early`：发送阶段的探测失败就停，**不发** `EndPage`；
//! - `early-end-page`：探测失败就停，但**仍然发**一条 `EndPage`；
//! - `late`：不理会探测失败，发完数据 + `0x25` + `0x73` + `EndPage`
//!   （复现失败抓包）；
//! - `late-skip`：发完数据 + `0x25`；`0x73` 失败且 `0x70` 也失败时
//!   **跳过** `EndPage`（候选修复）。
//!
//! 程序会在页面的指定百分比打印 `>>> OPEN THE COVER NOW <<<`，之后自动：
//! 记录第一次沉默、是否发了 EndPage、断开 → 等待 `--settle` → 重新连接 →
//! 连续读状态，最后打印一行 `RESULT ...` 方便复制。
//!
//! **开盖必须发生在打印机正在打印的时候。** 实测：开盖时如果打印机已经把缓存
//! 打完、正在等新数据，它只会回 `CoverOpened` 并继续应答；只有缓存满、电机在
//! 转的时候开盖，它才会真的沉默。所以：
//!
//! - 发送不节流，缓存由 `0x77` 预算维持满的，时长由打印机消耗速度决定；
//! - 点阵是伪随机的，每行都不一样、压不掉，一页 3000 行约 220KB；
//! - 想看更长的窗口就加 `--rows` 或 `--speed min`，看 `send finished` 的实际
//!   耗时；
//! - 如果最后是 `CoverOpened` 而不是沉默，程序会明确说“这次不算复现”并退出。
//!
//! 主实验是 `late` / `late-skip`：同一个完整页，唯一区别是沉默后发不发
//! `EndPage`。`early` / `early-end-page` 是辅助实验：在同一个中途位置只切换
//! `EndPage`，用来把“剩余页面长度”这个变量也控制住。
//!
//! ```text
//! probe_end_page --mode late      --rows 3000 --prompt-at 30
//! probe_end_page --mode late-skip --rows 3000 --prompt-at 30
//! ```
//!
//! 比较两次的 `after_reopen`：如果 `late` 明显更慢、`late-skip` 稳定在 1~2 秒，
//! 就说明 EndPage 是关键，并且候选修复有效。

mod common;

use std::{
    io::Write,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use dz_print::{
    bitmap::Bitmap,
    command::{req, resp, status::PrinterStatus},
    frontend::{Device, DeviceConfig},
    planner::{
        Assembler,
        print::{MAX_LINES_PER_PAGE, PrintCommand, into_reqs, print_bitmap_page},
    },
};
use mytm::TaskManager;
use tokio::time::sleep;
use tracing::{info, warn};

use common::{PrintArgs, PrintSettings, SHUTDOWN_TIMEOUT};

/// 连接打印机的超时。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// `0x77` 失败时按这个字节数继续发，和 `FlowPlanner` 的默认值一致。
const FALLBACK_BUDGET: usize = 8 * 1024;
/// 发送期间每隔多久打一次进度，让操作员确认程序还活着。
const PROGRESS_LOG_INTERVAL: Duration = Duration::from_secs(1);

/// 缓存满时两次 `0x77` 之间至少间隔多久。
///
/// 打印机跟不上时预算是 0，立刻重问只会把 USB 和固件打满。
const BUDGET_POLL_GAP: Duration = Duration::from_millis(50);

/// 重连后单次 `0x70` 最多等多久。
///
/// 打印机恢复后回包只要几十毫秒（实测 7~105ms），所以这里不需要用
/// `--recv-timeout` 那么长；短超时可以把“多久恢复”的时间分辨率压到
/// 几百毫秒以内。
const POST_PROBE_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Parser, Debug)]
#[command(version, about = "验证 EndPage 是否延长卡住后的恢复时间（真的会打印）")]
struct Cli {
    #[command(flatten)]
    print: PrintArgs,

    /// 实验模式
    #[arg(long, value_enum, default_value_t = Mode::Early)]
    mode: Mode,

    /// 页面行数
    ///
    /// 发送不节流，速度由打印机消耗缓存的速度决定（`0x77` 预算）。行数决定
    /// 整个发送能持续多久，也就是操作员有多长时间可以开盖。日志会打印实际
    /// 发送时长；如果不到 4 秒，加大这个值（或者加 `--speed min`）。
    #[arg(long, default_value_t = 3000)]
    rows: usize,

    /// 每块最多多少字节
    #[arg(long, default_value_t = 2000)]
    chunk: usize,

    /// 打印到多少百分比时提示开盖
    ///
    /// 剩下的部分还要继续灌进已经沉默的打印机，所以不要设得太晚。
    #[arg(long, default_value_t = 30)]
    prompt_at: u8,

    /// 每隔多少字节强制探一次状态；省略时只在预算用完时探
    #[arg(long, value_name = "BYTES")]
    probe_every: Option<usize>,

    /// 重新连接前等待多久
    #[arg(
        long,
        value_name = "DURATION",
        value_parser = humantime::parse_duration,
        default_value = "1.3s"
    )]
    settle: Duration,

    /// 重连后每隔多久探一次状态
    ///
    /// 每次探测自身最多等 200ms，所以实际分辨率是 `200ms + --poll-interval`。
    #[arg(
        long,
        value_name = "DURATION",
        value_parser = humantime::parse_duration,
        default_value = "100ms"
    )]
    poll_interval: Duration,

    /// 重连后最多探测多久，超时就报告仍然沉默
    #[arg(
        long,
        value_name = "DURATION",
        value_parser = humantime::parse_duration,
        default_value = "20s"
    )]
    poll_for: Duration,

    /// 开始前不等待回车
    #[arg(long)]
    no_wait: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// 探测到沉默就停，不发 EndPage
    Early,
    /// 探测到沉默就停，但仍然发一条 EndPage
    EarlyEndPage,
    /// 不看沉默，发完整页 + heat + 0x73 + EndPage
    Late,
    /// 发完整页 + heat；0x73 和 0x70 都失败时跳过 EndPage
    LateSkip,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    common::init_tracing();
    let cli = Cli::parse();
    common::exit_code(run(cli).await)
}

async fn run(cli: Cli) -> Result<()> {
    validate(&cli)?;
    print_intro(&cli);

    if !cli.no_wait {
        info!("按 Enter 开始打印（--no-wait 跳过）...");
        tokio::task::spawn_blocking(wait_for_enter).await?;
    }

    let tm = TaskManager::new();
    let result = run_with_tm(&cli, &tm).await;
    tm.close();
    result
}

fn validate(cli: &Cli) -> Result<()> {
    if cli.rows == 0 || cli.rows > MAX_LINES_PER_PAGE {
        bail!("--rows 必须在 1..={MAX_LINES_PER_PAGE} 之间");
    }
    if cli.chunk == 0 {
        bail!("--chunk 不能为 0");
    }
    if cli.prompt_at > 100 {
        bail!("--prompt-at 必须在 0..=100 之间");
    }
    if cli.poll_for.is_zero() {
        bail!("--poll-for 不能为 0");
    }
    if cli.settle.is_zero() {
        bail!("--settle 不能为 0");
    }
    Ok(())
}

fn print_intro(cli: &Cli) {
    info!("=== probe_end_page ===");
    info!(
        mode = ?cli.mode,
        rows = cli.rows,
        chunk = cli.chunk,
        prompt_at = cli.prompt_at,
        probe_every = ?cli.probe_every,
        settle = ?cli.settle,
        poll_interval = ?cli.poll_interval,
        poll_for = ?cli.poll_for,
        post_probe_timeout = ?POST_PROBE_TIMEOUT,
        "plan"
    );
    info!("1. 关好盖子、装好纸。");
    info!(
        "2. 程序发送到约 {}% 时会打印 >>> OPEN THE COVER NOW <<<。",
        cli.prompt_at
    );
    info!("3. 看到后立刻打开盖子，不要关回去；剩下的数据会继续灌进去。");
    info!(
        "   注意：只有打印机正在打印（缓存满、电机在转）时开盖才会沉默。\
         如果它已经追平、在那儿等数据，就只会回 CoverOpened。"
    );
    info!("4. 程序会记录第一次沉默、是否发 EndPage、重新连接后的第一次成功读状态。");
    info!("5. 结束后把 RESULT 行和完整日志贴回来。");
    info!(
        "提示：发送时长由打印机消耗速度决定。日志里的 send finished 如果不到 4 秒，\
         就把 --rows 调大（例如 6000），或者加 --speed min。"
    );
    match cli.mode {
        Mode::Early => info!("mode=early：探测到沉默就停，不发 EndPage。"),
        Mode::EarlyEndPage => {
            info!("mode=early-end-page：探测到沉默就停，但仍然发一条 EndPage。")
        }
        Mode::Late => {
            info!("mode=late：不看沉默，发完整页 + heat + 0x73 + EndPage（复现失败场景）。")
        }
        Mode::LateSkip => {
            info!("mode=late-skip：发完整页 + heat；0x73 和 0x70 都失败时跳过 EndPage。")
        }
    }
}

fn wait_for_enter() {
    print!("按 Enter 继续... ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}

async fn run_with_tm(cli: &Cli, tm: &TaskManager) -> Result<()> {
    let config = cli.print.device_config();
    let dev = connect(tm, &config).await?;

    let info = common::printer::query_info(&dev, config.recv_timeout).await?;
    info.log_connected();
    let width = usize::from(info.head_dots);
    let page_lines = cli.rows;

    PrintSettings::from(&cli.print).send(&dev).await?;

    let bitmap = build_bitmap(width, page_lines)?;
    let page = print_bitmap_page(&bitmap, 0, cli.print.rle.into(), page_lines)
        .context("生成页面命令失败")?;

    let options = cli.print.job_options();
    dev.try_send_batch(&[
        &req::start_page::StartPage {
            page_idx: 1,
            print_sep_line: false,
            data_pixel_width: u16::try_from(width).context("打印头点数超出 u16")?,
            printer_gap_type: options.gap_type,
            printer_gap_length: options.gap_length,
        },
        &req::total_lines::TotalLines {
            lines: u16::try_from(page.lines).context("页面行数超出 u16")?,
        },
    ])
    .await?;

    let (areas, total_bytes) = encode_page(&page.commands, dev.model_version())?;
    let chunks = total_bytes.div_ceil(cli.chunk).max(1);
    info!(
        total_bytes,
        chunks,
        rows = cli.rows,
        "send plan；发送不节流，时长由打印机消耗速度决定"
    );
    let started = Instant::now();
    let mut sent_bytes = 0usize;
    let mut sent_lines = 0usize;
    let mut budget_left = match probe_budget(&dev, config.recv_timeout).await {
        Probe::Answer { budget, status } => {
            info!(?status, ?budget, "initial budget");
            budget.unwrap_or(FALLBACK_BUDGET).max(1)
        }
        Probe::Silent { .. } => {
            warn!("打印机在 StartPage 之后就没有回包，本次实验无法继续");
            let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
            return Ok(());
        }
    };
    let mut index = 0usize;
    let mut last_probe_bytes = 0usize;
    let mut last_log = Instant::now();
    let mut prompt_time = None;
    let mut first_silence = None;
    let mut first_error: Option<(PrinterStatus, Instant)> = None;

    while index < areas.len() {
        let forced = cli
            .probe_every
            .is_some_and(|every| sent_bytes.saturating_sub(last_probe_bytes) >= every);
        if first_silence.is_none() && (budget_left == 0 || forced) {
            if budget_left == 0 {
                // 缓存满了：等打印机让出空间再问，别把 USB 打满。
                sleep(BUDGET_POLL_GAP).await;
            }
            last_probe_bytes = sent_bytes;
            let probe_started = Instant::now();
            match probe_budget(&dev, config.recv_timeout).await {
                Probe::Answer { status, budget } => {
                    if let Some(status) = status {
                        if status.is_error() && first_error.is_none() {
                            first_error = Some((status, probe_started));
                            warn!(?status, sent_bytes, sent_lines, "printer reported an error");
                        } else {
                            info!(?status, ?budget, sent_bytes, sent_lines, "probe ok");
                        }
                    } else {
                        info!(
                            ?budget,
                            sent_bytes, sent_lines, "probe: 0x70 missed, 0x77 answered"
                        );
                    }
                    budget_left = budget.unwrap_or(FALLBACK_BUDGET).max(1);

                    if first_error.is_some() && matches!(cli.mode, Mode::Early | Mode::EarlyEndPage)
                    {
                        break;
                    }
                }
                Probe::Silent { status, budget } => {
                    if first_silence.is_none() {
                        first_silence = Some(probe_started);
                        warn!(
                            sent_bytes,
                            sent_lines,
                            ?status,
                            ?budget,
                            "probe failed: printer stopped answering"
                        );
                    }
                    if matches!(cli.mode, Mode::Early | Mode::EarlyEndPage) {
                        break;
                    }
                    // 已经沉默了，就别再为预算探测：把剩下的数据全速写完。
                    budget_left = usize::MAX;
                }
            }
        }

        let mut chunk = Vec::new();
        let mut chunk_lines = 0usize;
        while index < areas.len() {
            let area = &areas[index];
            let want = chunk.len() + area.bytes.len();
            if !chunk.is_empty() && (want > cli.chunk || want > budget_left) {
                break;
            }
            chunk.extend_from_slice(&area.bytes);
            chunk_lines += area.lines;
            index += 1;
            if chunk.len() >= cli.chunk {
                break;
            }
        }

        let chunk_len = chunk.len();
        if let Err(error) = dev.send_cmd_area(chunk).await {
            warn!(%error, sent_bytes, sent_lines, "write failed during send");
            if first_silence.is_none() {
                first_silence = Some(Instant::now());
            }
            break;
        }
        sent_bytes += chunk_len;
        sent_lines += chunk_lines;
        budget_left = budget_left.saturating_sub(chunk_len);

        if prompt_time.is_none() {
            let progress = sent_bytes
                .saturating_mul(100)
                .checked_div(total_bytes)
                .unwrap_or(100);
            if progress >= usize::from(cli.prompt_at) {
                prompt_time = Some(Instant::now());
                info!("============================================================");
                info!(
                    sent_bytes,
                    total_bytes, sent_lines, page_lines, ">>> OPEN THE COVER NOW <<<"
                );
                info!(
                    elapsed = ?started.elapsed(),
                    remaining = ?remaining_estimate(total_bytes - sent_bytes, sent_bytes, started),
                    "现在开盖：打印机缓存是满的、电机在转"
                );
                info!("============================================================");
            }
        }

        if last_log.elapsed() >= PROGRESS_LOG_INTERVAL {
            last_log = Instant::now();
            info!(
                sent_bytes,
                total_bytes, sent_lines, elapsed = ?started.elapsed(), "sending"
            );
        }
    }

    let send_elapsed = started.elapsed();
    info!(
        sent_bytes,
        total_bytes, send_elapsed = ?send_elapsed, "send finished"
    );
    if first_silence.is_none() {
        match first_error {
            Some((status, at)) => warn!(
                ?status,
                at = ?at.duration_since(started),
                "打印机回的是错误状态、不是沉默：开盖那一刻它已经空闲（缓存空了、电机停了）\
                 就不会沉默。请早点开盖，或者加大 --rows / 加 --speed min 让发送更久。"
            ),
            None => warn!(
                sent_bytes,
                total_bytes,
                send_elapsed = ?send_elapsed,
                "整页发完都没观察到沉默：开盖太晚，或者发送时间太短"
            ),
        }
        send_end_page(&dev).await.ok();
        let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
        return Ok(());
    }

    let mut endpage_at = None;
    let mut stats_ok = None;
    match cli.mode {
        Mode::Early => {}
        Mode::EarlyEndPage => {
            // 和 early 的唯一区别：沉默之后仍然把 EndPage 写进去。
            match send_end_page(&dev).await {
                Ok(()) => {
                    endpage_at = Some(Instant::now());
                    info!("EndPage sent into the silent printer");
                }
                Err(error) => warn!(%error, "EndPage write failed"),
            }
        }
        Mode::Late | Mode::LateSkip => {
            if let Err(error) = send_heat(&dev, &bitmap).await {
                warn!(%error, "heat write failed");
            }
            let stats = query_statistics(&dev, config.recv_timeout).await;
            stats_ok = Some(stats.is_some());
            match &stats {
                Some(stats) => info!(
                    print_pages = stats.print_pages,
                    print_lines = stats.print_lines,
                    "0x73 ok"
                ),
                None => warn!("0x73 没有回包"),
            }

            let send_end = match cli.mode {
                Mode::Late => true,
                Mode::LateSkip => {
                    if stats.is_some() {
                        true
                    } else {
                        let status = read_status(&dev, config.recv_timeout).await;
                        info!(?status, "0x73 失败后补探 0x70");
                        if status.is_some() {
                            true
                        } else {
                            warn!("0x73 和 0x70 都没有回包，跳过 EndPage");
                            false
                        }
                    }
                }
                Mode::Early | Mode::EarlyEndPage => unreachable!(),
            };
            if send_end {
                match send_end_page(&dev).await {
                    Ok(()) => {
                        endpage_at = Some(Instant::now());
                        info!("EndPage sent");
                    }
                    Err(error) => warn!(%error, "EndPage write failed"),
                }
            }
        }
    }

    info!(
        settle = ?cli.settle,
        "closing the connection; waiting before reopening"
    );
    let reset_started = Instant::now();
    if let Err(error) = dev.shutdown(SHUTDOWN_TIMEOUT).await {
        warn!(%error, "shutdown failed; the USB handle may still be held");
    }
    drop(dev);
    let drop_elapsed = reset_started.elapsed();
    sleep(cli.settle).await;

    let dev = connect(tm, &config).await?;
    let reopen_at = Instant::now();
    let reopen_elapsed = reopen_at.duration_since(reset_started);
    info!(elapsed = ?reopen_elapsed, "device reopened");

    let mut answer = None;
    let mut attempts = 0usize;
    let poll_deadline = Instant::now() + cli.poll_for;
    loop {
        attempts += 1;
        let probe_at = Instant::now();
        let timeout = config.recv_timeout.min(POST_PROBE_TIMEOUT);
        match read_status(&dev, timeout).await {
            Some(status) => {
                answer = Some((status, probe_at));
                info!(
                    attempt = attempts,
                    ?status,
                    after_reopen = ?probe_at.duration_since(reopen_at),
                    round_trip = ?probe_at.elapsed(),
                    "printer answered after reset"
                );
                break;
            }
            None => {
                if attempts % 10 == 0 {
                    warn!(
                        attempt = attempts,
                        after_reopen = ?probe_at.duration_since(reopen_at),
                        "status still silent"
                    );
                }
            }
        }
        if Instant::now() >= poll_deadline {
            break;
        }
        sleep(cli.poll_interval).await;
    }

    let answered_after_reopen = answer.map(|(_, at)| at.duration_since(reopen_at));
    let answer_elapsed = answer.map(|(status, at)| (status, at.duration_since(started)));
    let from_silence = match (first_silence, answer) {
        (Some(silence), Some((_, at))) => Some(at.duration_since(silence)),
        _ => None,
    };
    let from_endpage = match (endpage_at, answer) {
        (Some(endpage), Some((_, at))) => Some(at.duration_since(endpage)),
        _ => None,
    };

    info!(
        "RESULT mode={:?} rows={} sent_bytes={} sent_lines={} drop_elapsed={:?} \
         reopen_elapsed={:?} prompt={:?} first_silence={:?} first_error={:?} \
         endpage={:?} stats_ok={:?} attempts={} after_reopen={:?} answer={:?} \
         from_silence={:?} from_endpage={:?}",
        cli.mode,
        page.lines,
        sent_bytes,
        sent_lines,
        drop_elapsed,
        reopen_elapsed,
        prompt_time.map(|at| at.duration_since(started)),
        first_silence.map(|at| at.duration_since(started)),
        first_error.map(|(status, at)| (status, at.duration_since(started))),
        endpage_at.map(|at| at.duration_since(started)),
        stats_ok,
        attempts,
        answered_after_reopen,
        answer_elapsed,
        from_silence,
        from_endpage,
    );

    let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
    Ok(())
}

async fn connect(tm: &TaskManager, config: &DeviceConfig) -> Result<Device> {
    let dev = Device::new(tm, config.clone());
    match tokio::time::timeout(CONNECT_TIMEOUT, dev.ready()).await {
        Ok(Ok(())) => Ok(dev),
        Ok(Err(error)) => {
            let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
            Err(error.into())
        }
        Err(_) => {
            let _ = dev.shutdown(SHUTDOWN_TIMEOUT).await;
            bail!("连接打印机超时（{CONNECT_TIMEOUT:?}）")
        }
    }
}

fn build_bitmap(width: usize, rows: usize) -> Result<Bitmap> {
    let mut bitmap = Bitmap::new(width, rows).context("创建位图失败")?;
    // 用伪随机点阵：每一行都不一样，planner 就没法把相邻行压成 repeats。
    // 否则 1418 行只剩几 KB，几百毫秒就发完，操作员根本来不及开盖。
    let mut rng: u64 = 0x9e37_79b9_7f4a_7c15;
    for y in 0..rows {
        let mut line = bitmap.line_mut(y)?;
        if y % 128 == 0 {
            // 每 128 行一条实线，方便在纸上看进度。
            line.fill(true);
            continue;
        }
        for x in 0..width {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            if rng & 0b11 == 0 {
                line.set(x, true);
            }
        }
    }
    Ok(bitmap)
}

/// 按当前发送速率估算剩余时间。
fn remaining_estimate(remaining: usize, sent: usize, started: Instant) -> Option<Duration> {
    let elapsed = started.elapsed();
    if sent == 0 || elapsed.is_zero() {
        return None;
    }
    Duration::try_from_secs_f64(elapsed.as_secs_f64() * remaining as f64 / sent as f64).ok()
}

struct Area {
    bytes: Vec<u8>,
    lines: usize,
}

fn encode_page(
    commands: &[PrintCommand],
    model_version: dz_print::command::ModelVersion,
) -> Result<(Vec<Area>, usize)> {
    let assembler = Assembler::new(model_version);
    let reqs = into_reqs(commands);
    let mut areas = Vec::with_capacity(reqs.len());
    let mut total_bytes = 0usize;
    for (command, request) in commands.iter().zip(reqs.iter()) {
        let bytes = assembler
            .encode(&[request.as_ref()])
            .context("编码打印命令失败")?;
        total_bytes += bytes.len();
        areas.push(Area {
            bytes,
            lines: command_lines(command),
        });
    }
    Ok((areas, total_bytes))
}

fn command_lines(command: &PrintCommand) -> usize {
    match command {
        PrintCommand::Line { repeats, .. }
        | PrintCommand::RleByte { repeats, .. }
        | PrintCommand::Rle4x { repeats, .. }
        | PrintCommand::Rle4d { repeats, .. }
        | PrintCommand::Rle5x { repeats, .. }
        | PrintCommand::Rle5d { repeats, .. }
        | PrintCommand::Rle6x { repeats, .. }
        | PrintCommand::Rle6d { repeats, .. } => usize::from(*repeats) + 1,
        PrintCommand::Feed { lines } => *lines as usize,
    }
}

enum Probe {
    Answer {
        status: Option<PrinterStatus>,
        budget: Option<usize>,
    },
    Silent {
        status: Option<PrinterStatus>,
        budget: Option<usize>,
    },
}

async fn probe_budget(dev: &Device, timeout: Duration) -> Probe {
    if let Err(error) = dev
        .try_send_batch(&[&req::status::Status {}, &req::buffer_size::BufferSize {}])
        .await
    {
        warn!(%error, "probe write failed");
        return Probe::Silent {
            status: None,
            budget: None,
        };
    }

    let deadline = Instant::now() + timeout;
    let status = dev
        .recv_cmd_timeout::<resp::status::Status>(resp::status::CMD, timeout)
        .await
        .ok()
        .flatten()
        .map(|status| status.status);
    let remaining = deadline.saturating_duration_since(Instant::now());
    let budget = dev
        .recv_cmd_timeout::<resp::buffer_size::BufferSize>(resp::buffer_size::CMD, remaining)
        .await
        .ok()
        .flatten()
        .map(|buffer| buffer.budget());

    if status.is_none() && budget.is_none() {
        Probe::Silent { status, budget }
    } else {
        Probe::Answer { status, budget }
    }
}

async fn read_status(dev: &Device, timeout: Duration) -> Option<PrinterStatus> {
    dev.try_send_batch(&[&req::status::Status {}]).await.ok()?;
    dev.recv_cmd_timeout::<resp::status::Status>(resp::status::CMD, timeout)
        .await
        .ok()
        .flatten()
        .map(|status| status.status)
}

async fn query_statistics(dev: &Device, timeout: Duration) -> Option<resp::statistics::Statistics> {
    dev.try_send_batch(&[&req::statistics::Statistics {}])
        .await
        .ok()?;
    dev.recv_cmd_timeout::<resp::statistics::Statistics>(resp::statistics::CMD, timeout)
        .await
        .ok()
        .flatten()
}

async fn send_end_page(dev: &Device) -> Result<()> {
    dev.try_send_batch(&[&req::end_page::EndPage {}]).await?;
    Ok(())
}

async fn send_heat(dev: &Device, bitmap: &Bitmap) -> Result<()> {
    let mut max_dots = 0u16;
    let mut sum = 0usize;
    for y in 0..bitmap.height() {
        let ones = bitmap.line(y)?.bits().count_ones();
        max_dots = max_dots.max(u16::try_from(ones).unwrap_or(u16::MAX));
        sum += ones;
    }
    let avg_dots = (sum / bitmap.height().max(1)) as u16;
    dev.try_send_batch(&[&req::heat_control::HeatControl { max_dots, avg_dots }])
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_lines_per_command() {
        let line = PrintCommand::Line {
            skips: 0,
            repeats: 2,
            data: Vec::new(),
        };
        assert_eq!(command_lines(&line), 3);

        let feed = PrintCommand::Feed { lines: 7 };
        assert_eq!(command_lines(&feed), 7);

        let rle = PrintCommand::Rle5x {
            repeats: 0,
            rle_runs_count: 1,
            data: Vec::new(),
        };
        assert_eq!(command_lines(&rle), 1);
    }
}
