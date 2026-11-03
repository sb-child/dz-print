// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! `dzprint_image` 和 `dzprint_typst` 共用的命令行参数与打印流程。
//!
//! 两个 bin 只负责“怎么得到一张位图”，连接设备、发送设置、打印和退出顺序
//! 都在这里，避免改一边漏一边。

pub mod image;
pub mod printer;
#[cfg(any(feature = "bin-typst", feature = "bin-dzserver"))]
pub mod typst;

use std::time::Duration;

use anyhow::Context as _;
use clap::{Args, ValueEnum};
use dz_print::{
    backend::usb::UsbSelector,
    command::{
        req::{self, ReqTrait},
        settings::{DarknessSetting, GapTypeSetting, SpeedSetting},
    },
    frontend::{Device, DeviceConfig, FrontendError, PrintJobOptions},
    planner::print::RleMode,
    raster::DitherMode,
};
use mytm::TaskManager;
use tracing::{error, info};

/// 退出前等 worker 线程真正停下（释放 USB 句柄）的超时。
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// `--gap-length` 省略时用的间隔长度，单位 `0.01mm`（0.5mm）。
pub const DEFAULT_GAP_LENGTH: u32 = 50;

/// `--print-timeout` 的默认值。
const DEFAULT_PRINT_TIMEOUT: &str = "600s";

/// `--recv-timeout` 的默认值。
const DEFAULT_RECV_TIMEOUT: &str = "3s";

/// 抖动方式。
#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RasterArg {
    /// Floyd-Steinberg 误差扩散
    #[default]
    #[value(name = "floyd-steinberg")]
    FloydSteinberg,
    /// 亮度截断
    #[value(name = "threshold")]
    Threshold,
}

impl From<RasterArg> for DitherMode {
    fn from(value: RasterArg) -> Self {
        match value {
            RasterArg::FloydSteinberg => DitherMode::FloydSteinberg,
            RasterArg::Threshold => DitherMode::Threshold,
        }
    }
}

impl From<DitherMode> for RasterArg {
    fn from(value: DitherMode) -> Self {
        match value {
            DitherMode::FloydSteinberg => Self::FloydSteinberg,
            DitherMode::Threshold => Self::Threshold,
        }
    }
}

/// RLE 编码方案。
///
/// 默认 `off`：DP27P 上 RLE 对真实照片会打出白带，页计数也会异常；普通
/// `0x21` 行命令是当前唯一在真机上稳定验证过的路径。简单线稿仍可显式选
/// `byte` / `bits5x` / `bits6x` 做实验。
#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RleArg {
    /// 不压缩
    #[default]
    #[value(name = "off")]
    Off,
    /// 字节级 RLE
    #[value(name = "byte")]
    Byte,
    /// 4 位符号，自动 X / D
    #[value(name = "bits4")]
    Bits4,
    /// 5 位符号，自动 X / D
    #[value(name = "bits5")]
    Bits5,
    /// 6 位符号，自动 X / D
    #[value(name = "bits6")]
    Bits6,
    /// 4 位符号，只用 X
    #[value(name = "bits4x")]
    Bits4x,
    /// 5 位符号，只用 X
    #[value(name = "bits5x")]
    Bits5x,
    /// 6 位符号，只用 X
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

impl From<RleMode> for RleArg {
    fn from(value: RleMode) -> Self {
        match value {
            RleMode::Off => Self::Off,
            RleMode::Byte => Self::Byte,
            RleMode::Bits4 => Self::Bits4,
            RleMode::Bits5 => Self::Bits5,
            RleMode::Bits6 => Self::Bits6,
            RleMode::Bits4XOnly => Self::Bits4x,
            RleMode::Bits5XOnly => Self::Bits5x,
            RleMode::Bits6XOnly => Self::Bits6x,
        }
    }
}

/// 所有打印类 bin 共用的参数。
#[derive(Args, Debug, Clone)]
pub struct PrintArgs {
    /// 按 USB 设备名（`型号-序列号`）选择；省略时取第一台德佟设备
    #[arg(long, value_name = "NAME")]
    pub device: Option<String>,

    /// 纸张间隔类型：continuous / hole / gap / black-mark / transparent，或 0..=4
    #[arg(long, value_name = "TYPE", value_parser = parse_gap_type)]
    pub gap_type: Option<GapTypeSetting>,

    /// 纸张间隔长度，单位 mm，可带小数（例如 2 / 2.5 / 2.5mm）
    #[arg(long, value_name = "MM", value_parser = parse_gap_length)]
    pub gap_length: Option<u32>,

    /// 打印浓度：min / normal / max，或用户面值 1..=15
    #[arg(long, value_name = "VALUE", value_parser = parse_darkness)]
    pub darkness: Option<DarknessSetting>,

    /// 打印速度：min / normal / max，或用户面值 1..=5
    #[arg(long, value_name = "VALUE", value_parser = parse_speed)]
    pub speed: Option<SpeedSetting>,

    /// 抖动方式
    #[arg(long, value_enum, default_value = "floyd-steinberg")]
    pub raster: RasterArg,

    /// 行数据的 RLE 编码方案；默认不压缩
    #[arg(long, value_enum, default_value = "off")]
    pub rle: RleArg,

    /// 等打印结束的总超时，例如 `600s` / `10m`
    #[arg(
        long,
        value_name = "DURATION",
        value_parser = humantime::parse_duration,
        default_value = DEFAULT_PRINT_TIMEOUT
    )]
    pub print_timeout: Duration,

    /// 单条响应的等待超时，例如 `3s` / `500ms`
    ///
    /// 固件卡住时 `0x70` 不会回包，这个值决定每次状态查询等多久；
    /// 连续 5 次没有任何回应就会判定打印机失联并报错退出。
    #[arg(
        long,
        value_name = "DURATION",
        value_parser = humantime::parse_duration,
        default_value = DEFAULT_RECV_TIMEOUT
    )]
    pub recv_timeout: Duration,
}

/// 真正需要发给打印机的设置；用来判断这一页要不要重发。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrintSettings {
    pub gap_type: Option<GapTypeSetting>,
    pub gap_length: Option<u32>,
    pub darkness: Option<DarknessSetting>,
    pub speed: Option<SpeedSetting>,
}

impl From<&PrintArgs> for PrintSettings {
    fn from(args: &PrintArgs) -> Self {
        Self {
            gap_type: args.gap_type,
            gap_length: args.gap_length,
            darkness: args.darkness,
            speed: args.speed,
        }
    }
}

impl PrintSettings {
    /// 只发送设置过的字段，顺序固定为纸张类型 / 间隔 / 浓度 / 速度。
    pub async fn send(&self, dev: &Device) -> Result<(), FrontendError> {
        let mut reqs: Vec<Box<dyn ReqTrait>> = Vec::new();
        if let Some(gap_type) = self.gap_type {
            reqs.push(Box::new(req::gap_type::GapType { gap_type }));
        }
        if let Some(gap) = self.gap_length {
            reqs.push(Box::new(req::gap_length::GapLength { gap }));
        }
        if let Some(darkness) = self.darkness {
            reqs.push(Box::new(req::darkness::Darkness { darkness }));
        }
        if let Some(speed) = self.speed {
            reqs.push(Box::new(req::speed::Speed { speed }));
        }
        if reqs.is_empty() {
            return Ok(());
        }

        info!(count = reqs.len(), "send print settings");
        let refs: Vec<&dyn ReqTrait> = reqs.iter().map(|req| req.as_ref()).collect();
        dev.try_send_batch(&refs).await?;
        Ok(())
    }
}

impl PrintArgs {
    pub fn selector(&self) -> UsbSelector {
        match self.device.as_deref() {
            Some(name) => {
                info!("select device by name: {name}");
                UsbSelector::DeviceName(name.to_owned())
            }
            None => UsbSelector::Auto,
        }
    }

    pub fn device_config(&self) -> DeviceConfig {
        DeviceConfig {
            selector: self.selector(),
            // 0 会让每次查询立刻超时，没有意义；允许毫秒级，但不接受 0。
            recv_timeout: self.recv_timeout.max(Duration::from_millis(1)),
            print_done_timeout: self.print_timeout.max(Duration::from_millis(1)),
            ..Default::default()
        }
    }

    /// 把命令行里的纸张设置带进每页的 `StartPage`。
    pub fn job_options(&self) -> PrintJobOptions {
        let gap_type = self.gap_type.unwrap_or_default();
        let gap_length = match self.gap_length {
            Some(gap) => gap,
            // 只给了 `--gap-type` 时用 0.5mm，否则 v2 的 `printer_gap_length` 会校验失败。
            None if gap_type != GapTypeSetting::Continuous => DEFAULT_GAP_LENGTH,
            None => 0,
        };
        PrintJobOptions {
            gap_type,
            gap_length,
        }
    }
}

/// 连接打印机、查询设备信息、执行 `body`，然后按固定顺序退出。
///
/// 无论打印成功与否都会 `shutdown` + `tm.close()`，保证 worker 线程和 USB
/// 句柄都释放掉再让进程结束；`body` 的错误优先返回，其次才是 shutdown 的错误。
// dzserver 也会编译 common，但连接 / 退出由它自己的 printer worker 管理。
#[allow(dead_code)]
pub async fn with_device<F>(args: &PrintArgs, body: F) -> anyhow::Result<()>
where
    F: AsyncFnOnce(&Device, &printer::PrinterInfo) -> anyhow::Result<()>,
{
    let tm = TaskManager::new();
    let config = args.device_config();
    let recv_timeout = config.recv_timeout;
    let dev = Device::new(&tm, config);

    let result = async {
        dev.ready().await.context("设备没有就绪")?;
        let info = printer::query_info(&dev, recv_timeout).await?;
        info.log_connected();
        body(&dev, &info).await
    }
    .await;

    let stopped = dev.shutdown(SHUTDOWN_TIMEOUT).await;
    tm.close();
    result?;
    stopped?;
    Ok(())
}

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .init();
}

/// 把 `run()` 的结果转成进程退出码，并把错误也打进日志。
///
/// 打印中途开盖、缺纸这类设备错误不能只回给 caller，日志里要能看到原因。
pub fn exit_code(result: anyhow::Result<()>) -> std::process::ExitCode {
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            error!("{err:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

pub fn parse_gap_type(value: &str) -> Result<GapTypeSetting, String> {
    match value.to_ascii_lowercase().as_str() {
        "continuous" | "ticket" => Ok(GapTypeSetting::Continuous),
        "hole" => Ok(GapTypeSetting::Hole),
        "gap" | "adhesive" => Ok(GapTypeSetting::Gap),
        "black-mark" | "black_mark" | "cardpaper" => Ok(GapTypeSetting::BlackMark),
        "transparent" => Ok(GapTypeSetting::Transparent),
        other => other.parse::<u8>().map_or_else(
            |_| {
                Err(format!(
                    "未知纸张间隔类型 `{value}`，可选 continuous / hole / gap / \
                     black-mark / transparent 或 0..=4"
                ))
            },
            |raw| GapTypeSetting::try_from(raw).map_err(|e| e.to_string()),
        ),
    }
}

/// 把 `2` / `2.5` / `2.5mm` 转成命令层用的 `0.01mm`。
pub fn parse_gap_length(value: &str) -> Result<u32, String> {
    let trimmed = value.trim();
    let number = trimmed.strip_suffix("mm").map_or(trimmed, str::trim).trim();
    let mm: f64 = number
        .parse()
        .map_err(|_| format!("无法解析间隔长度 `{value}`，请用 mm 数值，例如 2.5"))?;
    if !mm.is_finite() {
        return Err(format!("间隔长度 `{value}` 不是有限数值"));
    }

    let centi_mm = (mm * 100.0).round();
    if !(50.0..=4_194_303.0).contains(&centi_mm) {
        return Err(format!(
            "间隔长度 {mm}mm 超出范围，应在 0.5mm..=41943.03mm 之间"
        ));
    }
    Ok(centi_mm as u32)
}

pub fn parse_darkness(value: &str) -> Result<DarknessSetting, String> {
    value
        .parse()
        .map_err(|e: dz_print::command::settings::PrintSettingError| {
            format!("{e}（浓度可选 min / normal / max 或 1..=15）")
        })
}

pub fn parse_speed(value: &str) -> Result<SpeedSetting, String> {
    value
        .parse()
        .map_err(|e: dz_print::command::settings::PrintSettingError| {
            format!("{e}（速度可选 min / normal / max 或 1..=5）")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        print: PrintArgs,
    }

    fn parse(args: &[&str]) -> TestCli {
        TestCli::try_parse_from(std::iter::once("test").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn cli_defaults() {
        let cli = parse(&[]);
        assert!(cli.print.device.is_none());
        assert!(cli.print.gap_type.is_none());
        assert!(cli.print.gap_length.is_none());
        assert!(cli.print.darkness.is_none());
        assert!(cli.print.speed.is_none());
        assert_eq!(cli.print.raster, RasterArg::FloydSteinberg);
        assert_eq!(cli.print.rle, RleArg::Off);
        assert_eq!(cli.print.print_timeout, Duration::from_secs(600));
        assert_eq!(cli.print.recv_timeout, Duration::from_secs(3));
    }

    #[test]
    fn cli_parses_all_options() {
        let cli = parse(&[
            "--device",
            "DP27P-Y4094C023",
            "--gap-type",
            "gap",
            "--gap-length",
            "2.5mm",
            "--darkness",
            "12",
            "--speed",
            "max",
            "--raster",
            "threshold",
            "--rle",
            "bits6x",
            "--print-timeout",
            "2m",
            "--recv-timeout",
            "500ms",
        ]);

        assert_eq!(cli.print.device.as_deref(), Some("DP27P-Y4094C023"));
        assert_eq!(cli.print.gap_type, Some(GapTypeSetting::Gap));
        assert_eq!(cli.print.gap_length, Some(250));
        assert_eq!(cli.print.darkness, Some(DarknessSetting::Darkness11));
        assert_eq!(cli.print.speed, Some(SpeedSetting::Max));
        assert_eq!(cli.print.raster, RasterArg::Threshold);
        assert_eq!(cli.print.rle, RleArg::Bits6x);
        assert_eq!(cli.print.print_timeout, Duration::from_secs(120));
        assert_eq!(cli.print.recv_timeout, Duration::from_millis(500));
    }

    #[test]
    fn gap_length_accepts_mm_and_decimals() {
        assert_eq!(parse_gap_length("2").unwrap(), 200);
        assert_eq!(parse_gap_length("2.5").unwrap(), 250);
        assert_eq!(parse_gap_length("2.5mm").unwrap(), 250);
        assert_eq!(parse_gap_length(" 0.5 mm ").unwrap(), 50);
        assert!(parse_gap_length("0.4").is_err());
        assert!(parse_gap_length("abc").is_err());
        assert!(parse_gap_length("inf").is_err());
    }

    #[test]
    fn gap_type_accepts_names_and_numbers() {
        assert_eq!(
            parse_gap_type("ticket").unwrap(),
            GapTypeSetting::Continuous
        );
        assert_eq!(parse_gap_type("adhesive").unwrap(), GapTypeSetting::Gap);
        assert_eq!(
            parse_gap_type("black-mark").unwrap(),
            GapTypeSetting::BlackMark
        );
        assert_eq!(parse_gap_type("4").unwrap(), GapTypeSetting::Transparent);
        assert!(parse_gap_type("5").is_err());
        assert!(parse_gap_type("unknown").is_err());
    }

    #[test]
    fn job_options_default_to_continuous() {
        let cli = parse(&[]);
        let options = cli.print.job_options();
        assert_eq!(options.gap_type, GapTypeSetting::Continuous);
        assert_eq!(options.gap_length, 0);
    }

    #[test]
    fn gap_type_without_length_uses_default() {
        let cli = parse(&["--gap-type", "gap"]);
        let options = cli.print.job_options();
        assert_eq!(options.gap_type, GapTypeSetting::Gap);
        assert_eq!(options.gap_length, DEFAULT_GAP_LENGTH);
    }
}
