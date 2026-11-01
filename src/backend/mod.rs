// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 传输层：只搬字节，不解析命令。
//!
//! [`Transport`] 是设备 worker 唯一的 IO 接口。命令编码在
//! [`crate::planner::Assembler`]，发送节奏在 [`crate::planner::flow`]，
//! 这里只负责把一段命令区按链路要求发出去，再把收到的字节交回 worker。
//!
//! # 用法
//!
//! 正常业务用 [`frontend::Device`](crate::frontend::Device) 就够了；只有接新链路
//! （BLE、串口、测试替身）时才需要自己实现 [`Transport`]：
//!
//! ```
//! use dz_print::backend::{ReadStatus, Transport, TransportError, WriteOutcome};
//!
//! /// 只回超时的空实现。
//! struct NullTransport;
//!
//! impl Transport for NullTransport {
//!     fn write(&mut self, _cmd_area: &[u8]) -> Result<WriteOutcome, TransportError> {
//!         Ok(WriteOutcome::Written)
//!     }
//!
//!     fn read_chunk(&mut self, _out: &mut Vec<u8>) -> Result<ReadStatus, TransportError> {
//!         Ok(ReadStatus::Timeout)
//!     }
//!
//!     fn reset(&mut self) -> Result<(), TransportError> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! 真机 USB 用 [`usb::UsbTransport`]，见该模块文档。

pub mod usb;
pub mod usb_frame;

use snafu::Snafu;

/// 传输层错误。
#[derive(Debug, Snafu, PartialEq, Eq)]
pub enum TransportError {
    /// 没有找到匹配的设备。
    #[snafu(display("no matching device"))]
    NotFound,

    /// 设备已经断开。
    #[snafu(display("device disconnected: {message}"))]
    Disconnected { message: String },

    /// 底层读写失败。
    #[snafu(display("transport io error: {message}"))]
    Io { message: String },

    /// 发送前的封包失败。
    #[snafu(display("transport encode error: {message}"))]
    Encode { message: String },

    /// 收到的数据不符合传输层格式。
    #[snafu(display("transport protocol error: {message}"))]
    Protocol { message: String },
}

/// 一次写入的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// 这段命令区已经全部交给链路。
    Written,
    /// 链路暂时不接受（例如 BLE 信用窗口满）。调用方应该先读一轮再试。
    Blocked,
}

/// 一次读取的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadStatus {
    /// `out` 里追加了新字节。
    Data,
    /// 本次没有数据，链路仍然正常。
    Timeout,
    /// 设备已经断开。
    Closed,
}

/// 字节运输层。
///
/// 约定：
///
/// - 不知道命令语义，不解析 `Envelope` / `RespTrait`。
/// - [`Transport::write`] 不得阻塞等待只能从读通道拿到的数据：BLE 的
///   `ff03` 信用就属于这种，做不到时返回 [`WriteOutcome::Blocked`]，
///   让调用方先去读。
/// - [`Transport::read_chunk`] 追加的字节可能不对应任何请求（例如 `0x7b`
///   的主动上报），切帧和分发由 worker 负责。
pub trait Transport: Send + 'static {
    /// 写一段已编码的命令区（不含传输层封包）。
    fn write(&mut self, cmd_area: &[u8]) -> Result<WriteOutcome, TransportError>;

    /// 读一段，把命令区字节追加到 `out`。超时由实现自己配置。
    fn read_chunk(&mut self, out: &mut Vec<u8>) -> Result<ReadStatus, TransportError>;

    /// 复位设备并清空内部缓冲。
    fn reset(&mut self) -> Result<(), TransportError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemoryTransport {
        written: Vec<u8>,
        incoming: Vec<u8>,
        reads: Vec<ReadStatus>,
    }

    impl Transport for MemoryTransport {
        fn write(&mut self, cmd_area: &[u8]) -> Result<WriteOutcome, TransportError> {
            self.written.extend_from_slice(cmd_area);
            Ok(WriteOutcome::Written)
        }

        fn read_chunk(&mut self, out: &mut Vec<u8>) -> Result<ReadStatus, TransportError> {
            if let Some(status) = self.reads.pop() {
                return Ok(status);
            }
            out.extend_from_slice(&self.incoming);
            self.incoming.clear();
            Ok(ReadStatus::Data)
        }

        fn reset(&mut self) -> Result<(), TransportError> {
            self.incoming.clear();
            Ok(())
        }
    }

    /// worker 会把它放进 `Box<dyn Transport>`，所以 trait 必须是对象安全的。
    #[test]
    fn transport_is_object_safe() {
        let mut t: Box<dyn Transport> = Box::new(MemoryTransport::default());
        assert_eq!(t.write(&[0x1f, 0x70]).unwrap(), WriteOutcome::Written);

        let mut out = Vec::new();
        assert_eq!(t.read_chunk(&mut out).unwrap(), ReadStatus::Data);
        assert!(out.is_empty());
        assert!(t.reset().is_ok());
    }
}
