// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

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

pub struct Envelope {
    payload: Vec<u8>,
    kind: EnvelopeKind,
}

impl Envelope {
    pub fn new(payload: Vec<u8>, kind: EnvelopeKind) -> Self {
        Envelope { payload, kind }
    }
}
