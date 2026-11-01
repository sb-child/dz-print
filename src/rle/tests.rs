// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use super::*;

use deku::bitvec::{BitVec, Msb0, bits};

use crate::bitmap::BitmapLine;

fn make_bits(bits: &[bool]) -> BitVec<u8, Msb0> {
    let mut bv = BitVec::<u8, Msb0>::new();
    for &b in bits {
        bv.push(b);
    }
    bv
}

#[test]
fn test_rle_bool_starts_with_one() {
    let bv = make_bits(&[true, true, true, false, false, true]);
    let res: Result<Vec<Run<bool>>, _> = BitmapLine::new(&bv).runs_bits();
    let expected = vec![
        Run {
            value: true,
            count: 3,
        },
        Run {
            value: false,
            count: 2,
        },
        Run {
            value: true,
            count: 1,
        },
    ];
    assert_eq!(res.unwrap(), expected);
}

#[test]
fn test_rle_bool_starts_with_zero() {
    let bv = make_bits(&[false, false, true, true, true, false]);
    let res: Result<Vec<Run<bool>>, _> = BitmapLine::new(&bv).runs_bits();
    let expected = vec![
        Run {
            value: false,
            count: 2,
        },
        Run {
            value: true,
            count: 3,
        },
        Run {
            value: false,
            count: 1,
        },
    ];
    assert_eq!(res.unwrap(), expected);
}

#[test]
fn test_rle_bool_all_same() {
    let ones = make_bits(&[true, true, true, true]);
    let res_ones: Vec<Run<bool>> = BitmapLine::new(&ones).runs_bits().unwrap();
    assert_eq!(
        res_ones,
        vec![Run {
            value: true,
            count: 4
        }]
    );

    let zeros = make_bits(&[false, false, false]);
    let res_zeros: Vec<Run<bool>> = BitmapLine::new(&zeros).runs_bits().unwrap();
    assert_eq!(
        res_zeros,
        vec![Run {
            value: false,
            count: 3
        }]
    );
}

#[test]
fn test_rle_bool_single_bit() {
    let bv = make_bits(&[true]);
    let res: Vec<Run<bool>> = BitmapLine::new(&bv).runs_bits().unwrap();
    assert_eq!(
        res,
        vec![Run {
            value: true,
            count: 1
        }]
    );
}

#[test]
fn test_rle_bool_empty_err() {
    let bv = make_bits(&[]);
    let res: Result<Vec<Run<bool>>, RleTransformError> = BitmapLine::new(&bv).runs_bits();
    assert!(matches!(res, Err(RleTransformError::EmptyData)));
}

#[test]
fn test_rle_u8_basic() {
    let bytes = vec![0xAA, 0xAA, 0xFF, 0x00, 0x00, 0x00];
    let bv = BitVec::<u8, Msb0>::from_slice(&bytes);
    let res: Vec<Run<u8>> = BitmapLine::new(&bv).runs_bytes().unwrap();
    let expected = vec![
        Run {
            value: 0xAA,
            count: 2,
        },
        Run {
            value: 0xFF,
            count: 1,
        },
        Run {
            value: 0x00,
            count: 3,
        },
    ];
    assert_eq!(res, expected);
}

#[test]
fn test_rle_u8_all_distinct() {
    let bytes = vec![0x01, 0x02, 0x03, 0x04];
    let bv = BitVec::<u8, Msb0>::from_slice(&bytes);
    let res: Vec<Run<u8>> = BitmapLine::new(&bv).runs_bytes().unwrap();
    let expected = vec![
        Run {
            value: 0x01,
            count: 1,
        },
        Run {
            value: 0x02,
            count: 1,
        },
        Run {
            value: 0x03,
            count: 1,
        },
        Run {
            value: 0x04,
            count: 1,
        },
    ];
    assert_eq!(res, expected);
}

#[test]
fn test_rle_u8_empty_err() {
    let bv = BitVec::<u8, Msb0>::new();
    let res: Result<Vec<Run<u8>>, RleTransformError> = BitmapLine::new(&bv).runs_bytes();
    assert!(matches!(res, Err(RleTransformError::EmptyData)));
}

#[test]
fn test_rle_u8_unaligned_5bits_padding() {
    let mut bv = BitVec::<u8, Msb0>::new();
    bv.extend_from_bitslice(bits![u8, Msb0;
        // 0xFF
        1, 1, 1, 1, 1, 1, 1, 1,
        // 0x00
        0, 0, 0, 0, 0, 0, 0, 0,
        // 0xAA
        1, 0, 1, 0, 1, 0, 1, 0,
        // 11111 -> 11111000 (0xF8)
        1, 1, 1, 1, 1
    ]);

    let res: Vec<Run<u8>> = BitmapLine::new(&bv).runs_bytes().unwrap();
    let expected = vec![
        Run {
            value: 0xFF,
            count: 1,
        },
        Run {
            value: 0x00,
            count: 1,
        },
        Run {
            value: 0xAA,
            count: 1,
        },
        Run {
            value: 0xF8,
            count: 1,
        },
    ];
    assert_eq!(res, expected);
}

#[test]
fn test_rle_u8_unaligned_single_bit_padding() {
    let mut bv = BitVec::<u8, Msb0>::new();
    bv.push(true);
    let res: Vec<Run<u8>> = BitmapLine::new(&bv).runs_bytes().unwrap();
    let expected = vec![Run {
        value: 0x80,
        count: 1,
    }];
    assert_eq!(res, expected);
}

#[test]
fn test_rle_u8_unaligned_zero_bit_padding() {
    let mut bv = BitVec::<u8, Msb0>::new();
    bv.extend_from_bitslice(bits![u8, Msb0; 0; 3]);
    let res: Vec<Run<u8>> = BitmapLine::new(&bv).runs_bytes().unwrap();
    let expected = vec![Run {
        value: 0x00,
        count: 1,
    }];
    assert_eq!(res, expected);
}

#[test]
fn test_rle_u8_padded_byte_merges_with_previous() {
    let mut bv = BitVec::<u8, Msb0>::new();
    bv.extend_from_bitslice(bits![u8, Msb0;
        1, 1, 1, 1, 1, 0, 0, 0, // 0xF8
        1, 1, 1, 1, 1          // 11111 -> 0xF8
    ]);
    let res: Vec<Run<u8>> = BitmapLine::new(&bv).runs_bytes().unwrap();
    let expected = vec![Run {
        value: 0xF8,
        count: 2,
    }];
    assert_eq!(res, expected);
}

fn line_of(bits: &[bool]) -> BitVec<u8, Msb0> {
    let mut bv = BitVec::<u8, Msb0>::new();
    for &b in bits {
        bv.push(b);
    }
    bv
}

fn line_of_bytes(bytes: &[u8]) -> BitVec<u8, Msb0> {
    BitVec::<u8, Msb0>::from_slice(bytes)
}

/// 文档里的例子：`111100111011`，RLE4，尾部是黑色所以不裁剪。
#[test]
fn bits_x4_matches_doc_example() {
    let bv = line_of(&[
        true, true, true, true, false, false, true, true, true, false, true, true,
    ]);
    let res = RleBitStream::encode(BitmapLine::new(&bv), None, RleBits::Bits4).unwrap();
    assert_eq!(res.symbol_count, 5);
    // 1011 0001 1010 0000 1001 + 补 0
    assert_eq!(res.data, vec![0xB1, 0xA0, 0x90]);
}

#[test]
fn bits_drops_trailing_white_run() {
    let bv = line_of(
        &[true, true, true, true]
            .iter()
            .chain(&[false; 8])
            .copied()
            .collect::<Vec<_>>(),
    );
    let res = RleBitStream::encode(BitmapLine::new(&bv), None, RleBits::Bits4).unwrap();
    assert_eq!(res.symbol_count, 1);
    assert_eq!(res.data, vec![0xB0]);
}

#[test]
fn bits_all_white_is_empty() {
    let bv = line_of(&[false; 12]);
    let res = RleBitStream::encode(BitmapLine::new(&bv), None, RleBits::Bits4).unwrap();
    assert_eq!(res.symbol_count, 0);
    assert!(res.data.is_empty());
}

#[test]
fn bits_d_encodes_xor_with_previous_line() {
    let prev = line_of(&[true, true, true, true, false, false, false, false]);
    let cur = line_of(&[true, true, true, true, false, false, false, true]);
    let res = RleBitStream::encode(
        BitmapLine::new(&cur),
        Some(BitmapLine::new(&prev)),
        RleBits::Bits4,
    )
    .unwrap();
    // diff = 0000 0001 -> (0,7) (1,1)
    assert_eq!(res.symbol_count, 3);
    // 0010(白,3) 0011(白,4) 1000(黑,1)
    assert_eq!(res.data, vec![0x23, 0x80]);
}

#[test]
fn bits_d_requires_same_width() {
    let prev = line_of(&[true; 8]);
    let cur = line_of(&[true; 9]);
    let err = RleBitStream::encode(
        BitmapLine::new(&cur),
        Some(BitmapLine::new(&prev)),
        RleBits::Bits4,
    )
    .unwrap_err();
    assert_eq!(
        err,
        RleTransformError::LineWidthMismatch {
            expected: 9,
            actual: 8
        }
    );
}

#[test]
fn bits_split_long_run_uses_largest_coin() {
    // 200 = 88 + 88 + 14 + 9 + 1
    let bv = line_of(&[true; 200]);
    let res = RleBitStream::encode(BitmapLine::new(&bv), None, RleBits::Bits4).unwrap();
    assert_eq!(res.symbol_count, 5);
}

#[test]
fn bits5_and_bits6_use_their_own_symbol_width() {
    let bv = line_of(&[true; 120]);
    let b5 = RleBitStream::encode(BitmapLine::new(&bv), None, RleBits::Bits5).unwrap();
    // 120 恰好在码本里, 一个符号: E=1, index=15 -> 0b1_1111 (5 位)
    assert_eq!(b5.symbol_count, 1);
    assert_eq!(b5.data, vec![0b1111_1000]);

    let b6 = RleBitStream::encode(BitmapLine::new(&bv), None, RleBits::Bits6).unwrap();
    // 120 = 104 + 16 或者 120 不在 RLE6 码本里(104,125...), 至少 2 个符号
    assert!(b6.symbol_count >= 2);
    assert_eq!(b6.data.len(), (b6.symbol_count * 6).div_ceil(8));
}

#[test]
fn byte_single_low_value_is_literal() {
    let bv = line_of_bytes(&[0x11]);
    let res = RleByteStream::encode(BitmapLine::new(&bv)).unwrap();
    assert_eq!(res.byte_count, 1);
    assert_eq!(res.data, vec![0x11]);
}

#[test]
fn byte_single_high_value_is_escaped() {
    let bv = line_of_bytes(&[0xFF]);
    let res = RleByteStream::encode(BitmapLine::new(&bv)).unwrap();
    assert_eq!(res.data, vec![0xC1, 0xFF]);
}

#[test]
fn byte_run_uses_count_prefix() {
    let bv = line_of_bytes(&[0xAA, 0xAA]);
    let res = RleByteStream::encode(BitmapLine::new(&bv)).unwrap();
    assert_eq!(res.data, vec![0xC2, 0xAA]);
}

#[test]
fn byte_run_of_63_uses_ff() {
    let bv = line_of_bytes(&[0x11; 63]);
    let res = RleByteStream::encode(BitmapLine::new(&bv)).unwrap();
    assert_eq!(res.data, vec![0xFF, 0x11]);
}

#[test]
fn byte_run_of_64_splits() {
    let bv = line_of_bytes(&[0x11; 64]);
    let res = RleByteStream::encode(BitmapLine::new(&bv)).unwrap();
    assert_eq!(res.data, vec![0xFF, 0x11, 0x11]);
}

#[test]
fn byte_drops_trailing_zero_run() {
    let bv = line_of_bytes(&[0xAA, 0xAA, 0xFF, 0x00, 0x00, 0x00]);
    let res = RleByteStream::encode(BitmapLine::new(&bv)).unwrap();
    assert_eq!(res.data, vec![0xC2, 0xAA, 0xC1, 0xFF]);
    assert_eq!(res.byte_count, 4);
}

fn read_bit(data: &[u8], pos: usize) -> bool {
    (data[pos / 8] >> (7 - pos % 8)) & 1 != 0
}

/// 测试用解码器，按 `rle.md` 的符号规则还原一行（X 模式）。
fn decode_bits(data: &[u8], bits: RleBits, symbol_count: usize, width: usize) -> Vec<bool> {
    let lut = bits.lut();
    let index_bits = bits.index_bits();
    let index_mask = (1usize << index_bits) - 1;
    let mut out = Vec::with_capacity(width);
    let mut pos = 0usize;
    for _ in 0..symbol_count {
        let mut symbol = 0usize;
        for _ in 0..bits.symbol_bits() {
            symbol = (symbol << 1) | read_bit(data, pos) as usize;
            pos += 1;
        }
        let element = symbol >> index_bits != 0;
        let index = symbol & index_mask;
        out.extend(std::iter::repeat_n(element, lut[index]));
    }
    assert!(
        out.len() <= width,
        "decoded {} > width {}",
        out.len(),
        width
    );
    out.resize(width, false);
    out
}

fn decode_bytes(data: &[u8], width_bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(width_bytes);
    let mut i = 0usize;
    while i < data.len() {
        let tag = data[i];
        i += 1;
        match tag {
            0xC1 => {
                out.push(data[i]);
                i += 1;
            }
            0x00..=0xC0 => out.push(tag),
            _ => {
                out.extend(std::iter::repeat_n(data[i], (tag & 0x3F) as usize));
                i += 1;
            }
        }
    }
    assert!(out.len() <= width_bytes);
    out.resize(width_bytes, 0x00);
    out
}

fn lcg_bits(width: usize, seed: u32) -> Vec<bool> {
    let mut state = seed;
    (0..width)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state >> 31 != 0
        })
        .collect()
}

#[test]
fn bits_roundtrip_with_decoder() {
    for width in [64usize, 384, 1521] {
        let src = lcg_bits(width, 0x1234_5678);
        let bv = line_of(&src);
        for enc in [RleBits::Bits4, RleBits::Bits5, RleBits::Bits6] {
            let res = RleBitStream::encode(BitmapLine::new(&bv), None, enc).unwrap();
            let decoded = decode_bits(&res.data, enc, res.symbol_count, width);
            assert_eq!(decoded, src, "{enc:?}, width {width}");
        }
    }
}

#[test]
fn bits_d_roundtrip_with_decoder() {
    let width = 384;
    let prev = lcg_bits(width, 0x0BAD_F00D);
    let cur = lcg_bits(width, 0xDEAD_BEEF);
    let prev_bv = line_of(&prev);
    let cur_bv = line_of(&cur);
    for enc in [RleBits::Bits4, RleBits::Bits5, RleBits::Bits6] {
        let res = RleBitStream::encode(
            BitmapLine::new(&cur_bv),
            Some(BitmapLine::new(&prev_bv)),
            enc,
        )
        .unwrap();
        let diff = decode_bits(&res.data, enc, res.symbol_count, width);
        let decoded: Vec<bool> = diff.iter().zip(prev.iter()).map(|(&a, &b)| a ^ b).collect();
        assert_eq!(decoded, cur, "{enc:?}");
    }
}

#[test]
fn byte_roundtrip_with_decoder() {
    let mut state = 0x5EED_1234u32;
    let src: Vec<u8> = (0..191)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect();
    let bv = line_of_bytes(&src);
    let res = RleByteStream::encode(BitmapLine::new(&bv)).unwrap();
    assert_eq!(decode_bytes(&res.data, src.len()), src);
}

#[test]
fn byte_all_zero_is_empty() {
    let bv = line_of_bytes(&[0x00; 8]);
    let res = RleByteStream::encode(BitmapLine::new(&bv)).unwrap();
    assert_eq!(res.byte_count, 0);
    assert!(res.data.is_empty());
}
