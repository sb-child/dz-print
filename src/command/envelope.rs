// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 请求载荷和它的打包方式。
//!
//! [`Envelope::payload`] 是 `命令组 + 命令类型 + 数据`，不含长度字段和校验和；
//! [`EnvelopeKind`] 决定 [`crate::planner::Assembler`] 怎么补外层：
//!
//! - `AutoPackWithChecksum`：`v2`，补 1/2 字节长度和校验和；
//! - `AutoPackWithFixedChecksum`：`v1`，校验和固定 `0x88`；
//! - `ManualPack`：`v2` 的 RLE 命令，长度和校验和由命令自己拼；
//! - `Raw`：`v0`，不补。
//!
//! # 用法
//!
//! ```
//! use dz_print::command::envelope::{Envelope, EnvelopeKind};
//!
//! let envelope = Envelope::new(
//!     vec![0x1f, 0x70, 0x00], // 1f 70 + 空数据
//!     EnvelopeKind::AutoPackWithChecksum,
//! );
//! assert_eq!(envelope.payload(), &[0x1f, 0x70, 0x00]);
//! assert!(matches!(envelope.kind(), EnvelopeKind::AutoPackWithChecksum));
//! ```
//!
//! 业务代码一般不直接构造 `Envelope`：实现 [`crate::command::req::ReqTrait`]
//! 后由 `ReqTrait::pack` 返回，再交给 `Assembler`。

#[derive(Debug, Clone, Copy)]
pub enum EnvelopeKind {
    // for `v2`
    AutoPackWithChecksum,
    // for `v1`, checksum = `0x88`
    AutoPackWithFixedChecksum,
    // for `v2`, RLE
    ManualPack,
    // for `v0`
    Raw,
}

#[derive(Debug, Clone)]
pub struct Envelope {
    payload: Vec<u8>,
    kind: EnvelopeKind,
}

impl Envelope {
    pub fn new(payload: Vec<u8>, kind: EnvelopeKind) -> Self {
        Envelope { payload, kind }
    }

    /// `payload` = 命令组 + 命令类型 + 数据，不含长度字段和校验和
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// 打包方式
    pub fn kind(&self) -> EnvelopeKind {
        self.kind
    }
}

impl From<Envelope> for Vec<Envelope> {
    fn from(val: Envelope) -> Self {
        vec![val]
    }
}
