// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

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

/// `#[deku]` 字段读取必须按类型（模式）走，不能全用 Auto 逻辑。
#[test]
fn deku_reader_uses_mode() {
    use super::{VarAuto, VarC0Be16, VarFixed2, VarFixed3};
    use deku::{DekuReader, reader::Reader};
    use std::io::Cursor;

    // Fixed3: 3 字节，不能只读 2 字节
    let mut cursor = Cursor::new([0xC0, 0x4E, 0x20]);
    let mut reader = Reader::new(&mut cursor);
    let v = VarFixed3::from_reader_with_ctx(&mut reader, ()).unwrap();
    assert_eq!(v.get(), 20_000);
    assert!(reader.end());

    // C0Be16: `0xC0` 是独立标志，后两个字节才是值
    let mut cursor = Cursor::new([0xC0, 0x12, 0x34]);
    let mut reader = Reader::new(&mut cursor);
    let v = VarC0Be16::from_reader_with_ctx(&mut reader, ()).unwrap();
    assert_eq!(v.get(), 0x1234);
    assert!(reader.end());

    // Fixed2: 固定 2 字节
    let mut cursor = Cursor::new([0xC0, 0x4E]);
    let mut reader = Reader::new(&mut cursor);
    let v = VarFixed2::from_reader_with_ctx(&mut reader, ()).unwrap();
    assert_eq!(v.get(), 0x4E);
    assert!(reader.end());

    // Fixed2 不接受 1 字节
    let mut cursor = Cursor::new([0x4E]);
    let mut reader = Reader::new(&mut cursor);
    assert!(VarFixed2::from_reader_with_ctx(&mut reader, ()).is_err());

    // Auto: 保持 1/2 字节自动识别
    let mut cursor = Cursor::new([0x4E]);
    let mut reader = Reader::new(&mut cursor);
    let v = VarAuto::from_reader_with_ctx(&mut reader, ()).unwrap();
    assert_eq!(v.get(), 0x4E);
    assert!(reader.end());

    let mut cursor = Cursor::new([0xC0, 0x4E]);
    let mut reader = Reader::new(&mut cursor);
    let v = VarAuto::from_reader_with_ctx(&mut reader, ()).unwrap();
    assert_eq!(v.get(), 0x4E);
    assert!(reader.end());
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
