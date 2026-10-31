// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

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
