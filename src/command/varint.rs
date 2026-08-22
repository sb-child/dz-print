// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印机协议变长编码
//!
//! 协议里存在 4 种编码格式：
//! - 自动变长大端（1/2/3 字节）：
//!   - 编码: `<192` = 1 字节，`<16384` = `0xC0|高位 低位`，否则 `0xC0|高位 中位 低位`，上限 `4194303`
//!   - 解码: `<192` = 1 字节，`<16384` = `0xC0|高位 低位`，上限 `16383`
//!   - 解码侧无自动 3 字节识别，须按命令上下文用 Fixed3
//! - 2 字节大端：`0xC0|高位 低位`（0xC0 与高位拼在首字节），0x25 命令专用，上限 `16383`
//! - 3 字节大端：`0xC0|高位 中位 低位`，0x45 命令专用，上限 `4194303`
//! - 0xC0 标志 + 2 字节大端：`0xC0 高位 低位`（0xC0 是独立标志字节），0x26 命令 + `v1` 机型专用，上限 `65535`
//!
//! `0xC0|xxx` 指的是两个字节取或。

/// 编解码模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarintMode {
    /// 自动变长：编码 1/2/3 字节，解码自动识别 1/2 字节
    /// - 编码上限 `4194303`，解码上限 `16383`
    /// - 作为包长度字段时调用方必须保证 `<= 16383`
    Auto,
    /// 2 字节大端：`0xC0|高位 低位`
    /// - 上限 `16383`
    /// - 0x25 命令专用
    Fixed2,
    /// 3 字节大端：`0xC0|高位 中位 低位`
    /// - 上限 `4194303`
    /// - 0x45 命令专用
    Fixed3,
    /// 0xC0 标志 + 2 字节大端：`0xC0 高位 低位`
    /// - 上限 `65535`
    /// - 0x26 命令 + `v1` 机型专用
    C0Be16,
}

impl VarintMode {
    /// 该模式可编码的最大值
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
mod test {
    use super::VarintMode;

    /// Auto 的 3 字节形态无法自动解码，roundtrip 只对 1/2 字节范围做往返；
    /// 3 字节形态单独断言编码字节 + 用 Fixed3 解码验证。
    #[test]
    fn roundtrip_all_modes() {
        for mode in [
            VarintMode::Auto,
            VarintMode::Fixed2,
            VarintMode::Fixed3,
            VarintMode::C0Be16,
        ] {
            let max = mode.max_value();
            for v in [0, 1, 50, 191, 192, 16383, 16384, 65_535, 4_194_303] {
                if v > max || (mode == VarintMode::Auto && v >= 16_384) {
                    continue;
                }
                let enc = mode.encode(v);
                let (dec, n) = mode.decode(&enc).unwrap();
                assert_eq!((dec, n), (v, enc.len()), "mode={mode:?} v={v}");
            }
        }
    }

    #[test]
    fn auto_three_byte_form() {
        // 3 字节形态（Rust 旁路选项；SDK 从未对命令参数用 3 字节变长，
        // v2 0x26 超 16383 是 SDK short 错值路径，固件支持与否未实锤）
        assert_eq!(VarintMode::Auto.encode(16384), vec![0xC0, 0x40, 0x00]);
        assert_eq!(VarintMode::Auto.encode(32767), vec![0xC0, 0x7F, 0xFF]);
        assert_eq!(VarintMode::Auto.encode(4_194_303), vec![0xFF, 0xFF, 0xFF]);
        // 解码必须按上下文用 Fixed3（Auto 会误读成 2 字节）
        assert_eq!(
            VarintMode::Fixed3.decode(&[0xC0, 0x40, 0x00]),
            Some((16384, 3))
        );
        assert_eq!(
            VarintMode::Auto.decode(&[0xC0, 0x40, 0x00]),
            Some((0x40, 2))
        );
    }

    #[test]
    fn auto_boundaries() {
        assert_eq!(VarintMode::Auto.encode(192), vec![0xC0, 0xC0]);
        assert_eq!(VarintMode::Auto.encode(16383), vec![0xFF, 0xFF]);
    }

    #[test]
    fn fixed2_always_two_bytes() {
        assert_eq!(VarintMode::Fixed2.encode(50), vec![0xC0, 50]);
        assert_eq!(VarintMode::Fixed2.decode(&[0xC0, 50]), Some((50, 2)));
        assert_eq!(VarintMode::Fixed2.decode(&[50]), None);
        // 0x25 两个字段拼起来正好 4 字节（值 <192 也必须 2 字节，Auto 会编成 1 字节导致错位）
        let mut b = vec![0x1f, 0x25];
        VarintMode::Fixed2.encode_into(100, &mut b);
        VarintMode::Fixed2.encode_into(200, &mut b);
        assert_eq!(b, vec![0x1f, 0x25, 0xC0, 100, 0xC0, 200]);
    }

    #[test]
    fn fixed3_matches_auto_three_byte() {
        assert_eq!(VarintMode::Fixed3.encode(16_384), vec![0xC0, 0x40, 0x00]);
        assert_eq!(VarintMode::Fixed3.encode(4_194_303), vec![0xFF, 0xFF, 0xFF]);
        assert_eq!(
            VarintMode::Fixed3.decode(&[0xC0, 0x40, 0x00]),
            Some((16384, 3))
        );
        // 已知限制：Auto 会把 3 字节误读成 2 字节（需要按命令上下文用 Fixed3）
        assert_eq!(
            VarintMode::Auto.decode(&[0xC0, 0x40, 0x00]),
            Some((0x40, 2))
        );
    }

    #[test]
    fn c0_be16() {
        assert_eq!(VarintMode::C0Be16.encode(50), vec![0xC0, 0x00, 0x32]);
        assert_eq!(VarintMode::C0Be16.encode(0x1234), vec![0xC0, 0x12, 0x34]);
        assert_eq!(VarintMode::C0Be16.encode(65_535), vec![0xC0, 0xFF, 0xFF]);
        assert_eq!(
            VarintMode::C0Be16.decode(&[0xC0, 0x12, 0x34]),
            Some((0x1234, 3))
        );
        assert_eq!(
            VarintMode::C0Be16.decode(&[0xC0, 0xFF, 0xFF]),
            Some((65_535, 3))
        );
        assert_eq!(VarintMode::C0Be16.decode(&[0x00, 0x34, 0x12]), None);
    }

    #[test]
    fn truncated_returns_none() {
        assert_eq!(VarintMode::Auto.decode(&[0xC0]), None);
        assert_eq!(VarintMode::Fixed3.decode(&[0xC0, 0x40]), None);
        assert_eq!(VarintMode::C0Be16.decode(&[0xC0]), None);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn overflow_panics() {
        VarintMode::Auto.encode(4_194_304);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn c0be16_overflow_panics() {
        VarintMode::C0Be16.encode(65_536);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn negative_panics() {
        VarintMode::Fixed2.encode(-1);
    }
}
