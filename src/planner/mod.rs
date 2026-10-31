// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 命令组装和传输层封包。
//!
//! - [`assembler`]：命令区的编码 / 切帧解码（BLE 帧格式）。
//! - [`usb`]：USB 外层 `1e <长度>` 的打包 / 解包和 HID 报告切片。
//!
//! BLE 是串口透传，没有外层包头，直接使用 [`assembler`] 的输出（流控在链路层）。

pub mod assembler;
pub mod usb;

pub use assembler::{Assembler, DecodeError};
