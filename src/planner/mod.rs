// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 命令组装和传输层封包。
//!
//! - [`assembler`]：命令区的编码 / 切帧解码（BLE 帧格式）。
//! - [`flow`]：预算感知的分块发送计划（`0x77` 预算、9000 字节分块、超时退化）。
//! - `print`：位图到打印命令参数的翻译，按 `0x26` 的行数上限切页。
//! - [`crate::backend::usb_frame`]：USB 外层 `1e <长度>` 的打包 / 解包和 HID 报告切片。
//!
//! BLE 是串口透传，没有外层包头，直接使用 [`assembler`] 的输出（流控在链路层）。
//!
//! # 用法
//!
//! 一条命令区由 [`Assembler`] 生成；USB 链路再切成 64 字节报告：
//!
//! ```
//! use dz_print::command::{ModelVersion, req};
//! use dz_print::{backend::usb_frame, planner::Assembler};
//!
//! // 1. 编码命令区（含长度字段和校验和）
//! let area = Assembler::new(ModelVersion::V2).encode(&[&req::status::Status {}])?;
//! assert_eq!(area, vec![0x1f, 0x70, 0x00, 0x8f]);
//!
//! // 2. USB 链路：切成 HID 报告
//! let reports = usb_frame::encode_reports(&area)?;
//! assert_eq!(reports.len(), 1);
//!
//! // 3. 收到响应后切帧并解析
//! let mut buffer = vec![0x1f, 0x70, 0x08, 0x00, 0x01, 0x01, 0x00, 0x00, 0x35, 0x05, 0x10, 0x3b];
//! let (payload, used) = Assembler::new(ModelVersion::V2)
//!     .decode_frame(&buffer)?
//!     .expect("complete frame");
//! buffer.drain(..used);
//! assert_eq!(payload[1], 0x70);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! 打印场景通常不直接用 `Assembler`，而是让 [`flow::FlowPlanner`] 决定
//! 每次发多少，见 [`flow`] 和 [`mod@print`]。

pub mod assembler;
pub mod flow;
pub mod print;

pub use assembler::{Assembler, DecodeError};
