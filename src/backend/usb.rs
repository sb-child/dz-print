// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! USB HID 中断端点传输。
//!
//! 选设备 → 找同一接口上的 IN/OUT 中断端点对 → 声明接口。写入时用
//! [`crate::backend::usb_frame::encode_reports`] 按 64 字节报告切片，读取时用
//! [`crate::backend::usb_frame::decode_report`] 去掉 `1e <长度>` 包头和补零。
//!
//! # 用法
//!
//! ```no_run
//! use dz_print::backend::{ReadStatus, Transport, usb::{UsbSelector, UsbTransport}};
//!
//! # fn run() -> Result<(), Box<dyn std::error::Error>> {
//! // 按设备名（型号-序列号）选，或 UsbSelector::Auto / VidPid(0x3533, 0x5c15)
//! let mut transport = UsbTransport::open(&UsbSelector::DeviceName("DP27P-Y4094C023".into()))?;
//!
//! // 命令区已经由 Assembler 编好，这里只管发字节
//! transport.write(&[0x1f, 0x70, 0x00, 0x8f])?;
//!
//! let mut received = Vec::new();
//! if let ReadStatus::Data = transport.read_chunk(&mut received)? {
//!     println!("response: {}", pretty_hex::simple_hex(&received));
//! }
//! # Ok(())
//! # }
//! ```
//!
//! 直接调用 [`Transport::write`] 时传入的是**命令区**（`Assembler::encode` 的输出），
//! 不含 USB 的 `1e <长度>` 外层；`UsbTransport` 会自己切报告。
//! 大多数场景用 [`crate::frontend::Device`] 即可，它会把 `UsbTransport` 放进
//! worker 线程并处理收发队列。
//!
//! 退出时 `Drop` 会释放接口并把手动 detach 过的内核驱动接回去；
//! [`Transport::reset`] 之后会重新 claim 一次。

use crate::{
    backend::usb_frame,
    backend::{ReadStatus, Transport, TransportError, WriteOutcome},
};
use rusb::{Context, Device, DeviceHandle, Direction, TransferType, UsbContext};
use std::time::Duration;
use tracing::{debug, info, warn};

/// 德佟的 USB VID。
pub const DETONGER_VID: u16 = 0x3533;
/// 读中断端点的超时。
pub const READ_TIMEOUT: Duration = Duration::from_millis(100);
/// 写中断端点的超时。
pub const WRITE_TIMEOUT: Duration = Duration::from_millis(500);
/// 读 USB 字符串描述符的超时。
const NAME_TIMEOUT: Duration = Duration::from_secs(1);
/// `reset` 之后等设备重新枚举。
const RESET_DELAY: Duration = Duration::from_secs(1);

/// USB 设备选择方式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsbSelector {
    /// 按 USB 设备名（`型号-序列号`）匹配，例如 `DP27P-Y4094C023`。
    DeviceName(String),
    /// 按 VID/PID 匹配，取第一台。
    VidPid(u16, u16),
    /// 取第一台带中断端点对的德佟设备。
    Auto,
}

/// 一对中断端点及其所在接口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Endpoints {
    in_iface: u8,
    in_setting: u8,
    ep_in: u8,
    out_iface: u8,
    out_setting: u8,
    ep_out: u8,
}

/// USB 传输实现。
pub struct UsbTransport {
    handle: DeviceHandle<Context>,
    endpoints: Endpoints,
    /// 打开时从内核驱动手里 detach 过的接口，Drop 时要还回去。
    detached: Vec<u8>,
    read_timeout: Duration,
    write_timeout: Duration,
    /// 读写已经报过 Disconnected 时为 false。
    ///
    /// 设备拔掉后 libusb 的 release / attach 可能继续阻塞，Drop 里跳过这些调用，
    /// 避免 worker 线程卡在 USB 清理上不退出。
    connected: bool,
}

impl UsbTransport {
    /// 按选择器打开并声明设备。
    pub fn open(selector: &UsbSelector) -> Result<Self, TransportError> {
        let ctx = Context::new().map_err(|e| TransportError::Io {
            message: e.to_string(),
        })?;
        let devices = ctx.devices().map_err(|e| TransportError::Io {
            message: e.to_string(),
        })?;

        for device in devices.iter() {
            let Ok(desc) = device.device_descriptor() else {
                continue;
            };
            if !selector_matches(selector, &desc) {
                continue;
            }

            let Ok(handle) = device.open() else {
                continue;
            };

            if let UsbSelector::DeviceName(expected) = selector {
                let names = read_device_names(&handle, &device, &desc);
                if !names.iter().any(|name| usb_name_matches(name, expected)) {
                    debug!(names = ?names, expected, "skip USB device: name mismatch");
                    continue;
                }
            }

            // 连上先复位一次，清掉上一轮残留的半条命令 / 打印状态。
            // 失败不致命：有些机型复位后不会重新枚举，直接用也能工作。
            if let Err(e) = handle.reset() {
                warn!("reset USB device failed: {e:?}");
            }

            let Some(endpoints) = find_endpoints(&device)? else {
                debug!("skip USB device: no interrupt endpoint pair");
                continue;
            };

            let detached = claim_interfaces(&handle, endpoints)?;

            info!(
                vid = format!("{:#06x}", desc.vendor_id()),
                pid = format!("{:#06x}", desc.product_id()),
                ep_in = format!("{:#04x}", endpoints.ep_in),
                ep_out = format!("{:#04x}", endpoints.ep_out),
                "opened USB printer"
            );

            return Ok(Self {
                handle,
                endpoints,
                detached,
                read_timeout: READ_TIMEOUT,
                write_timeout: WRITE_TIMEOUT,
                connected: true,
            });
        }

        Err(TransportError::NotFound)
    }

    pub fn open_by_name(name: impl Into<String>) -> Result<Self, TransportError> {
        Self::open(&UsbSelector::DeviceName(name.into()))
    }

    /// 覆盖默认的读写超时。
    pub fn with_timeouts(mut self, read: Duration, write: Duration) -> Self {
        self.read_timeout = read;
        self.write_timeout = write;
        self
    }

    pub fn read_timeout(&self) -> Duration {
        self.read_timeout
    }

    pub fn write_timeout(&self) -> Duration {
        self.write_timeout
    }

    /// 记下 USB 错误；设备已经断开时不再在 Drop 里碰接口。
    fn mark_error(&mut self, err: &TransportError) {
        if matches!(err, TransportError::Disconnected { .. }) {
            self.connected = false;
        }
    }
}

impl Transport for UsbTransport {
    fn write(&mut self, cmd_area: &[u8]) -> Result<WriteOutcome, TransportError> {
        if cmd_area.is_empty() {
            return Ok(WriteOutcome::Written);
        }

        let reports = usb_frame::encode_reports(cmd_area).map_err(|e| TransportError::Encode {
            message: e.to_string(),
        })?;
        for report in reports {
            let written = match self.handle.write_interrupt(
                self.endpoints.ep_out,
                &report,
                self.write_timeout,
            ) {
                Ok(written) => written,
                Err(e) => {
                    let err = map_usb_error(e);
                    self.mark_error(&err);
                    return Err(err);
                }
            };
            if written != report.len() {
                return Err(TransportError::Io {
                    message: format!("short write: {written} of {} byte(s)", report.len()),
                });
            }
        }
        Ok(WriteOutcome::Written)
    }

    fn read_chunk(&mut self, out: &mut Vec<u8>) -> Result<ReadStatus, TransportError> {
        let mut report = [0u8; usb_frame::REPORT_SIZE];
        match self
            .handle
            .read_interrupt(self.endpoints.ep_in, &mut report, self.read_timeout)
        {
            Ok(0) => Ok(ReadStatus::Timeout),
            Ok(n) => {
                let chunk = usb_frame::decode_report(&report[..n]).map_err(|e| {
                    TransportError::Protocol {
                        message: e.to_string(),
                    }
                })?;
                out.extend_from_slice(chunk);
                Ok(ReadStatus::Data)
            }
            Err(rusb::Error::Timeout) => Ok(ReadStatus::Timeout),
            Err(e) => {
                let err = map_usb_error(e);
                self.mark_error(&err);
                Err(err)
            }
        }
    }

    fn reset(&mut self) -> Result<(), TransportError> {
        info!("resetting USB device");
        if let Err(e) = self.handle.reset() {
            let err = map_usb_error(e);
            self.mark_error(&err);
            return Err(err);
        }
        std::thread::sleep(RESET_DELAY);
        // reset 之后接口声明可能丢失，重新 claim 一次；失败只记录，
        // 保持和以前一样的“reset 本身成功就返回”语义。
        match claim_interfaces(&self.handle, self.endpoints) {
            Ok(detached) => self.detached = detached,
            Err(e) => warn!("re-claim interfaces after reset failed: {e}"),
        }
        info!("USB device reset completed");
        Ok(())
    }
}

impl Drop for UsbTransport {
    fn drop(&mut self) {
        if !self.connected {
            debug!("device disconnected, skip USB release/attach");
            return;
        }
        // 先释放接口，再把内核驱动接回去，避免下一次打开时设备被占用。
        let mut seen = Vec::new();
        for iface in [self.endpoints.out_iface, self.endpoints.in_iface] {
            if seen.contains(&iface) {
                continue;
            }
            seen.push(iface);
            if let Err(e) = self.handle.release_interface(iface) {
                debug!("release USB interface {iface} failed: {e:?}");
            }
        }
        for iface in &self.detached {
            if let Err(e) = self.handle.attach_kernel_driver(*iface) {
                debug!("attach kernel driver to interface {iface} failed: {e:?}");
            }
        }
    }
}

fn selector_matches(selector: &UsbSelector, desc: &rusb::DeviceDescriptor) -> bool {
    match selector {
        // 按名称选择时不预先按 VID 过滤，最终以字符串匹配为准。
        UsbSelector::DeviceName(_) => true,
        UsbSelector::VidPid(vid, pid) => desc.vendor_id() == *vid && desc.product_id() == *pid,
        UsbSelector::Auto => desc.vendor_id() == DETONGER_VID,
    }
}

/// 在设备上找一对中断端点。
///
/// 优先选同一个接口上同时具备 IN/OUT 的接口；没有这种接口时，退回到
/// 「任意接口的 IN + 任意接口的 OUT」。
fn find_endpoints(device: &Device<Context>) -> Result<Option<Endpoints>, TransportError> {
    let Ok(config) = device.config_descriptor(0) else {
        return Ok(None);
    };

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
                return Ok(Some(Endpoints {
                    in_iface: desc.interface_number(),
                    in_setting: desc.setting_number(),
                    ep_in,
                    out_iface: desc.interface_number(),
                    out_setting: desc.setting_number(),
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
            Some(Endpoints {
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

fn claim_interfaces(
    handle: &DeviceHandle<Context>,
    endpoints: Endpoints,
) -> Result<Vec<u8>, TransportError> {
    // 优先让 libusb 在释放接口时自动接回内核驱动；不支持时 Drop 里手动 attach。
    if let Err(e) = handle.set_auto_detach_kernel_driver(true) {
        debug!("set_auto_detach_kernel_driver failed: {e:?}");
    }

    let mut detached: Vec<u8> = Vec::new();
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
                .map_err(|e| TransportError::Io {
                    message: e.to_string(),
                })?;
            if !detached.contains(&iface) {
                detached.push(iface);
            }
        }
        handle
            .claim_interface(iface)
            .map_err(|e| TransportError::Io {
                message: e.to_string(),
            })?;
        handle
            .set_alternate_setting(iface, setting)
            .map_err(|e| TransportError::Io {
                message: e.to_string(),
            })?;
        claimed.push((iface, setting));
    }
    Ok(detached)
}

/// 读取设备上所有可用的 USB 字符串：产品名 + 各接口字符串。
fn read_device_names(
    handle: &DeviceHandle<Context>,
    device: &Device<Context>,
    desc: &rusb::DeviceDescriptor,
) -> Vec<String> {
    let mut names = Vec::new();

    let Ok(langs) = handle.read_languages(NAME_TIMEOUT) else {
        return names;
    };
    let Some(lang) = langs.into_iter().next() else {
        return names;
    };

    if let Ok(name) = handle.read_product_string(lang, desc, NAME_TIMEOUT) {
        names.push(name);
    }

    if let Ok(config) = device.config_descriptor(0) {
        for iface in config.interfaces() {
            for idesc in iface.descriptors() {
                if let Ok(name) = handle.read_interface_string(lang, &idesc, NAME_TIMEOUT) {
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

fn map_usb_error(e: rusb::Error) -> TransportError {
    match e {
        rusb::Error::NoDevice | rusb::Error::Pipe | rusb::Error::Io => {
            TransportError::Disconnected {
                message: e.to_string(),
            }
        }
        other => TransportError::Io {
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_matching_accepts_exact_and_suffix() {
        assert!(usb_name_matches("DP27P-Y4094C023", "DP27P-Y4094C023"));
        assert!(usb_name_matches(
            "printer interface @ DP27P-Y4094C023",
            "DP27P-Y4094C023"
        ));
        assert!(usb_name_matches("  DP27P-Y4094C023  ", "DP27P-Y4094C023"));
        assert!(!usb_name_matches("DP305P-Y60870279", "DP27P-Y4094C023"));
    }
}
