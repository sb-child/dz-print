// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印机命令层。
//!
//! 分为四部分：
//!
//! - [`req`]：主机 → 设备的命令。每条命令实现 [`req::ReqTrait`]，
//!   打包成一到多条 [`envelope::Envelope`]。
//! - [`resp`]：设备 → 主机的响应，按机型归一化成 domain 类型。
//! - [`req_template`] / [`resp_template`]：只用 deku 描述线上字节，保留原始
//!   magic；正常业务不需要直接碰。
//! - [`varint`] / [`settings`] / [`status`]：变长整数、设置值和状态码。
//!
//! [`ModelVersion`] 决定同一条命令用哪组字段（`v0` / `v1` / `v2`）。
//!
//! # 用法
//!
//! 只是打包 / 解析 payload 时直接调用 `pack` / `parse`：
//!
//! ```
//! use dz_print::command::{
//!     ModelVersion,
//!     req::{self, ReqTrait},
//!     resp::{self, RespTrait},
//!     status::PrinterStatus,
//! };
//!
//! // 请求 → payload（不含长度字段和校验和）
//! let envelopes = req::status::Status {}.pack(ModelVersion::V2)?;
//! assert_eq!(envelopes[0].payload(), &[0x1f, 0x70]);
//!
//! // payload → 响应
//! let status =
//!     resp::status::Status::parse(&[0x1f, 0x70, 0x00, 0x01, 0x01], ModelVersion::V2)?;
//! assert_eq!(status.status, PrinterStatus::Printable);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! 加上长度字段和校验和（命令区）用 [`crate::planner::Assembler`]；
//! 加上 USB 外层用 [`crate::planner::usb`]。

pub mod envelope;
pub mod high_command;
pub mod req;
pub mod req_template;
pub mod resp;
pub mod resp_template;
pub mod settings;
pub mod status;
pub mod varint;

#[derive(Debug, Clone, Copy)]
pub enum ModelVersion {
    /// `h=303`
    V0,
    /// `h=16`
    V1,
    /// `h=0`
    V2,
}

/// 计算校验和：`~(arr[start..end] 求和) & 0xFF`。
///
/// 打印协议里校验和覆盖命令帧的 `[1..帧长)`，即跳过第一个命令组字节；
/// 校验位固定写 `0x88` 表示不校验。
pub fn calculate_checksum(arr: &[u8], mut start: usize, end: usize) -> u8 {
    let mut x: u32 = 0;
    while start < end {
        x += arr[start] as u32;
        start += 1;
    }
    (x & 0xFF) as u8 ^ 0xFF
}
