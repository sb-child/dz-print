// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 德佟（DeTong）标签打印机的第三方驱动。
//!
//! # 分层
//!
//! 从下到上：
//!
//! - [`command`]：协议本身。`req` / `resp` 是按机型归一化后的命令和响应，
//!   `*_template` 只描述线上字节，[`command::varint`] 是变长整数，
//!   [`command::settings`] 是速度 / 浓度 / 纸张类型等设置值。
//! - [`planner`]：把命令编码成字节。`assembler` 补长度和校验和，
//!   [`backend::usb_frame`] 再切成 64 字节 HID 报告，[`planner::flow`] 决定什么时候发、
//!   每次发多少。
//! - [`bitmap`] / [`rle`] / [`raster`]：位图、RLE 编码和图像抖动。
//! - [`backend`]：真正读写设备的 [`Transport`](backend::Transport)（目前是 USB）。
//! - [`frontend`]：把上面的东西组装成一台可用的 [`Device`](frontend::Device)，
//!   负责 worker 线程、收发队列和打印任务。
//!
//! # 只打印一张位图
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use dz_print::{
//!     backend::usb::UsbSelector,
//!     bitmap::Bitmap,
//!     frontend::{Device, DeviceConfig},
//!     planner::print::RleMode,
//! };
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let tm = mytm::TaskManager::new(); // 整个程序共用一个，退出前 close
//! let dev = Device::new(
//!     &tm,
//!     DeviceConfig {
//!         selector: UsbSelector::DeviceName("DP27P-Y4094C023".into()),
//!         ..Default::default()
//!     },
//! );
//! dev.ready().await?;
//!
//! // 384 x 32 的位图，第 0 行整行黑。
//! let mut bitmap = Bitmap::new(384, 32)?;
//! bitmap.line_mut(0)?.fill(true);
//!
//! let report = dev.print_bitmap_job(&bitmap, RleMode::Bits5XOnly).await?;
//! println!("{} chunks, {} bytes", report.chunks, report.bytes);
//!
//! dev.shutdown(Duration::from_secs(3)).await?; // 等 worker 真正退出
//! tm.close();
//! # Ok(())
//! # }
//! ```
//!
//! 只想拿字节、不碰设备时，从 [`planner::print::print_bitmap`] 开始：
//!
//! ```
//! use dz_print::{
//!     bitmap::Bitmap,
//!     command::ModelVersion,
//!     planner::{Assembler, print::{into_reqs, print_bitmap, RleMode}},
//! };
//!
//! let mut bitmap = Bitmap::new(32, 2)?;
//! bitmap.line_mut(0)?.fill(true);
//!
//! let commands = print_bitmap(&bitmap, RleMode::Bits4XOnly)?;
//! let reqs = into_reqs(&commands);
//! let refs: Vec<&dyn dz_print::command::req::ReqTrait> =
//!     reqs.iter().map(|req| req.as_ref()).collect();
//! let command_area = Assembler::new(ModelVersion::V2).encode(&refs)?;
//! assert!(!command_area.is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod backend;
pub mod bitmap;
pub mod command;
pub mod derive_alias;
pub mod frontend;
pub mod planner;
pub mod raster;
pub mod rle;

#[cfg(test)]
mod test_util;
