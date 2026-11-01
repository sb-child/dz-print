// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印机协议变长编码
//!
//! 先区分「协议读法」和「编码 API」：
//!
//! - 协议里没有能自动识别 3 字节的 Auto。`Auto` 读侧只认 1/2 字节；
//!   3 字节必须按命令上下文用 `Fixed3`，或按数据长度判断（如 `0x45` 响应）。
//! - 编码 API 的 `Auto` 为了通用性也能编 3 字节（字节形态与 `Fixed3` 相同）；
//!   但 3 字节形态只用于 `0x45` 这类写侧已经知道取值范围、手动选宽度的字段。
//!
//! 协议里存在 4 种编码格式：
//! - 自动变长（读侧 1/2 字节，写侧可到 3 字节）：
//!   - 编码: `<192` = 1 字节，`<16384` = `0xC0|高位 低位`，否则 `0xC0|高位 中位 低位`，上限 `4194303`
//!   - 解码: `<192` = 1 字节，`<16384` = `0xC0|高位 低位`，上限 `16383`
//!   - 解码侧无自动 3 字节识别，须按命令上下文用 Fixed3
//! - 2 字节大端：`0xC0|高位 低位`（0xC0 与高位拼在首字节），0x25 命令专用，上限 `16383`
//! - 3 字节大端：`0xC0|高位 中位 低位`，0x45 命令专用，上限 `4194303`
//! - 0xC0 标志 + 2 字节大端：`0xC0 高位 低位`（0xC0 是独立标志字节），0x26 命令 + `v1` 机型专用，上限 `65535`
//!
//! `0xC0|xxx` 指的是两个字节取或。
//!
//! # 用法
//!
//! ```
//! use dz_print::command::varint::{VarAuto, VarintMode};
//!
//! // 直接编码 / 解码
//! assert_eq!(VarintMode::Auto.encode(191), vec![0xBF]);
//! assert_eq!(VarintMode::Auto.encode(192), vec![0xC0, 0xC0]);
//! assert_eq!(VarintMode::Auto.decode(&[0xC0, 0xC0]), Some((192, 2)));
//!
//! // 越界时 encode 会 panic，先检查时可以走 encode_safe
//! assert_eq!(VarintMode::Fixed2.encode_safe(16_384), None);
//!
//! // deku 模板里的字段用 VarAuto / VarFixed2 / VarFixed3 / VarC0Be16
//! let field = VarAuto::new(300);
//! assert_eq!(field.get(), 300);
//! ```

use deku::{DekuError, DekuReader, DekuWriter, ctx::Order, no_std_io, reader::Reader};
use pretty_hex::simple_hex;

macro_rules! impl_var_int {
    (
        $(
            $(#[$meta:meta])*
            $vis:vis struct $name:ident {
                mode: $mode:expr $(,)?
            }
        );* $(;)?
    ) => {
        $(
            $(#[$meta])*
            #[derive(Debug, PartialEq, Clone, Copy, Default)]
            $vis struct $name {
                v: i32,
            }

            impl $name {
                pub const fn new(v: i32) -> Self {
                    Self { v }
                }

                pub const fn get(&self) -> i32 {
                    self.v
                }

                pub fn set(&mut self, val: i32) {
                    self.v = val;
                }
            }

            impl<'a> DekuReader<'a> for $name {
                fn from_reader_with_ctx<R: no_std_io::Read + no_std_io::Seek>(
                    reader: &mut Reader<R>,
                    _ctx: (),
                ) -> Result<Self, DekuError>
                where
                    Self: Sized,
                {
                    let mode = $mode;
                    let mut buf = [0u8; 3];

                    reader.read_bytes(1, &mut buf[..1], Order::Msb0)?;
                    // 按模式决定整段编码的长度；Auto 的首字节没有 0xC0 标志时是单字节。
                    let len = match mode {
                        VarintMode::Auto if buf[0] & 0xC0 != 0xC0 => 1,
                        VarintMode::Auto | VarintMode::Fixed2 => 2,
                        VarintMode::Fixed3 | VarintMode::C0Be16 => 3,
                    };
                    if len > 1 {
                        reader.read_bytes(len - 1, &mut buf[1..len], Order::Msb0)?;
                    }

                    let bytes = &buf[..len];
                    let (v, used) = mode.decode(bytes).ok_or_else(|| {
                        DekuError::Parse(
                            format!(
                                "Failed to decode {mode:?} from {len} byte(s): {}",
                                simple_hex(&bytes)
                            )
                            .into(),
                        )
                    })?;
                    debug_assert_eq!(used, len);

                    Ok(Self { v })
                }
            }

            impl DekuWriter for $name {
                fn to_writer<W: no_std_io::Write + no_std_io::Seek>(
                    &self,
                    writer: &mut deku::prelude::Writer<W>,
                    _ctx: (),
                ) -> Result<(), DekuError> {
                    let v = $mode.encode_safe(self.v).ok_or_else(|| {
                        DekuError::Parse(
                            format!("Failed to encode {:?} in {:?} mode.", self.v, $mode).into(),
                        )
                    })?;
                    writer.write_bytes(&v)?;
                    Ok(())
                }
            }
        )*
    };
}

impl_var_int! {
    /// 自动变长；细节见 [VarintMode::Auto]。
    pub struct VarAuto {
        mode: VarintMode::Auto,
    };
    /// 固定 2 字节；细节见 [VarintMode::Fixed2]。
    pub struct VarFixed2 {
        mode: VarintMode::Fixed2,
    };
    /// 固定 3 字节；细节见 [VarintMode::Fixed3]。
    pub struct VarFixed3 {
        mode: VarintMode::Fixed3,
    };
    /// `0xC0` 标志 + 2 字节大端；细节见 [VarintMode::C0Be16]。
    pub struct VarC0Be16 {
        mode: VarintMode::C0Be16,
    };
}

/// 编解码模式。
///
/// `Auto` 编码最多 3 字节、解码只认 1/2 字节；写侧要明确 3 字节时用 `Fixed3`，
/// 包长度字段必须保证 `<= 16383`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarintMode {
    /// 自动变长；编码上限 `4194303`，解码上限 `16383`。
    Auto,
    /// 2 字节大端，`0x25` 命令专用，上限 `16383`。
    Fixed2,
    /// 3 字节大端，`0x45` 命令专用，上限 `4194303`。
    Fixed3,
    /// `0xC0` 标志 + 2 字节大端，`0x26` + `v1` 专用，上限 `65535`。
    C0Be16,
}

impl VarintMode {
    /// 该模式可编码的最大值
    ///
    /// `Auto` 的 3 字节只是编码 API 的旁路，不代表协议读侧能自动识别。
    pub const fn max_value(self) -> i32 {
        match self {
            VarintMode::Auto | VarintMode::Fixed3 => 4_194_303,
            VarintMode::Fixed2 => 16_383,
            VarintMode::C0Be16 => 65_535,
        }
    }

    /// 包长度字段允许的最大值
    pub const fn max_packet_len() -> i32 {
        16_383
    }

    /// 该模式下 v 的编码长度
    pub const fn encoded_len(self, v: i32) -> usize {
        match self {
            VarintMode::Auto if v < 192 => 1,
            VarintMode::Auto if v < 16_384 => 2,
            VarintMode::Auto => 3,
            VarintMode::Fixed2 => 2,
            VarintMode::Fixed3 | VarintMode::C0Be16 => 3,
        }
    }

    /// 编码。v 必须 >= 0 且 <= max_value()，否则返回 None。
    pub fn encode_safe(self, v: i32) -> Option<Vec<u8>> {
        if v < 0 || v > self.max_value() {
            return None;
        }
        let mut out = Vec::with_capacity(self.encoded_len(v));
        self.encode_into(v, &mut out);
        Some(out)
    }

    /// 编码到已有缓冲尾部。v 必须 >= 0 且 <= max_value()，否则返回 None。
    pub fn encode_into_safe(self, v: i32, out: &mut Vec<u8>) -> Option<()> {
        if v < 0 || v > self.max_value() {
            return None;
        }
        match self {
            VarintMode::Auto if v < 192 => {
                out.push(v as u8);
            }
            VarintMode::Auto if v < 16_384 => {
                out.push(0xC0 | ((v >> 8) as u8));
                out.push((v & 0xFF) as u8);
            }
            VarintMode::Auto => {
                out.push(0xC0 | ((v >> 16) as u8));
                out.push(((v >> 8) & 0xFF) as u8);
                out.push((v & 0xFF) as u8);
            }
            VarintMode::Fixed2 => {
                out.push(0xC0 | ((v >> 8) as u8));
                out.push((v & 0xFF) as u8);
            }
            VarintMode::Fixed3 => {
                out.push(0xC0 | ((v >> 16) as u8));
                out.push(((v >> 8) & 0xFF) as u8);
                out.push((v & 0xFF) as u8);
            }
            VarintMode::C0Be16 => {
                out.push(0xC0);
                out.push(((v >> 8) & 0xFF) as u8);
                out.push((v & 0xFF) as u8);
            }
        }
        Some(())
    }

    /// 编码。v 必须 >= 0 且 <= max_value()，否则 panic。
    pub fn encode(self, v: i32) -> Vec<u8> {
        assert!(
            v >= 0 && v <= self.max_value(),
            "varint: value {v} out of range for {self:?} (0..={})",
            self.max_value()
        );
        let mut out = Vec::with_capacity(self.encoded_len(v));
        self.encode_into(v, &mut out);
        out
    }

    /// 编码到已有缓冲尾部
    pub fn encode_into(self, v: i32, out: &mut Vec<u8>) {
        assert!(
            v >= 0 && v <= self.max_value(),
            "varint: value {v} out of range for {self:?} (0..={})",
            self.max_value()
        );
        match self {
            VarintMode::Auto if v < 192 => {
                out.push(v as u8);
            }
            VarintMode::Auto if v < 16_384 => {
                out.push(0xC0 | ((v >> 8) as u8));
                out.push((v & 0xFF) as u8);
            }
            VarintMode::Auto => {
                out.push(0xC0 | ((v >> 16) as u8));
                out.push(((v >> 8) & 0xFF) as u8);
                out.push((v & 0xFF) as u8);
            }
            VarintMode::Fixed2 => {
                out.push(0xC0 | ((v >> 8) as u8));
                out.push((v & 0xFF) as u8);
            }
            VarintMode::Fixed3 => {
                out.push(0xC0 | ((v >> 16) as u8));
                out.push(((v >> 8) & 0xFF) as u8);
                out.push((v & 0xFF) as u8);
            }
            VarintMode::C0Be16 => {
                out.push(0xC0);
                out.push(((v >> 8) & 0xFF) as u8);
                out.push((v & 0xFF) as u8);
            }
        }
    }

    /// 解码，返回 (值, 字节数)；数据不足或格式非法返回 None
    pub fn decode(self, buf: &[u8]) -> Option<(i32, usize)> {
        match self {
            VarintMode::Auto => {
                let &b0 = buf.first()?;
                if b0 & 0xC0 != 0xC0 {
                    Some((b0 as i32, 1))
                } else {
                    let &b1 = buf.get(1)?;
                    Some(((((b0 & 0x3F) as i32) << 8) | b1 as i32, 2))
                }
            }
            VarintMode::Fixed2 => {
                let (&b0, &b1) = (buf.first()?, buf.get(1)?);
                if b0 & 0xC0 != 0xC0 {
                    return None; // 缺少 0xC0 标志
                }
                Some(((((b0 & 0x3F) as i32) << 8) | b1 as i32, 2))
            }
            VarintMode::Fixed3 => {
                let (&b0, &b1, &b2) = (buf.first()?, buf.get(1)?, buf.get(2)?);
                if b0 & 0xC0 != 0xC0 {
                    return None;
                }
                Some((
                    (((b0 & 0x3F) as i32) << 16) | ((b1 as i32) << 8) | b2 as i32,
                    3,
                ))
            }
            VarintMode::C0Be16 => {
                let (&b0, &b1, &b2) = (buf.first()?, buf.get(1)?, buf.get(2)?);
                if b0 != 0xC0 {
                    return None;
                }
                Some((((b1 as i32) << 8) | b2 as i32, 3))
            }
        }
    }
}

#[cfg(test)]
mod tests;
