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
