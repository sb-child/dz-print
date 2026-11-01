// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use super::*;
use deku::bitvec::bits;

fn make_bits(bits: &[bool]) -> BitVec<u8, Msb0> {
    let mut bv = BitVec::<u8, Msb0>::new();
    for &b in bits {
        bv.push(b);
    }
    bv
}

#[test]
fn new_rejects_empty_size() {
    assert_eq!(
        Bitmap::new(0, 4).unwrap_err(),
        BitmapError::EmptySize {
            width: 0,
            height: 4
        }
    );
    assert_eq!(
        Bitmap::new(4, 0).unwrap_err(),
        BitmapError::EmptySize {
            width: 4,
            height: 0
        }
    );
}

#[test]
fn new_rejects_overflow() {
    assert_eq!(
        Bitmap::new(usize::MAX, 2).unwrap_err(),
        BitmapError::SizeOverflow {
            width: usize::MAX,
            height: 2
        }
    );
}

#[test]
fn from_bits_checks_length() {
    let bv = make_bits(&[true, false, true]);
    assert_eq!(
        Bitmap::from_bits(2, 2, bv.clone()).unwrap_err(),
        BitmapError::LengthMismatch {
            expected: 4,
            actual: 3
        }
    );
    assert_eq!(Bitmap::from_bits(3, 1, bv).unwrap().width(), 3);
}

#[test]
fn line_out_of_range() {
    let bmp = Bitmap::new(4, 2).unwrap();
    assert_eq!(
        bmp.line(2).unwrap_err(),
        BitmapError::LineOutOfRange {
            index: 2,
            height: 2
        }
    );
    assert_eq!(
        bmp.line(usize::MAX).unwrap_err(),
        BitmapError::SizeOverflow {
            width: 4,
            height: 2
        }
    );
}

#[test]
fn write_and_read_pixels() {
    let mut bmp = Bitmap::new(5, 2).unwrap();
    {
        let mut line = bmp.line_mut(0).unwrap();
        line.copy_from_bytes(&[0b1010_1000]).unwrap();
    }

    let line = bmp.line(0).unwrap();
    assert_eq!(line.len(), 5);
    assert_eq!(line.get(0), Some(true));
    assert_eq!(line.get(1), Some(false));
    assert_eq!(line.get(4), Some(true));
    assert_eq!(line.first_one(), Some(0));
    assert_eq!(line.last_one(), Some(4));
    assert_eq!(line.to_bytes(), vec![0b1010_1000]);
    assert!(bmp.line(1).unwrap().is_blank());
    assert_eq!(bmp.line(1).unwrap().first_one(), None);
}

#[test]
fn try_set_rejects_out_of_range() {
    let mut bmp = Bitmap::new(3, 1).unwrap();
    let mut line = bmp.line_mut(0).unwrap();
    line.try_set(2, true).unwrap();
    assert_eq!(
        line.try_set(3, true).unwrap_err(),
        BitmapError::PixelOutOfRange { x: 3, width: 3 }
    );
    assert_eq!(line.as_line().to_bytes(), vec![0b0010_0000]);
}

#[test]
fn copy_from_bytes_checks_length() {
    let mut bmp = Bitmap::new(8, 1).unwrap();
    let mut line = bmp.line_mut(0).unwrap();
    assert_eq!(
        line.copy_from_bytes(&[0x00, 0x00]).unwrap_err(),
        BitmapError::LengthMismatch {
            expected: 1,
            actual: 2
        }
    );
}

#[test]
fn copy_ignores_padding_bits() {
    let mut bmp = Bitmap::new(3, 1).unwrap();
    bmp.line_mut(0).unwrap().copy_from_bytes(&[0xFF]).unwrap();
    assert_eq!(bmp.line(0).unwrap().to_bytes(), vec![0b1110_0000]);
}

#[test]
fn trim_end_cuts_trailing_zeros() {
    let bv = make_bits(&[true, true, false, false]);
    let line = BitmapLine::new(&bv);
    assert_eq!(line.trim_end().len(), 2);
    assert_eq!(line.trim_end().to_bytes(), vec![0b1100_0000]);

    let empty = make_bits(&[false, false]);
    assert_eq!(BitmapLine::new(&empty).trim_end().len(), 0);
}

#[test]
fn rows_stay_aligned_after_writing() {
    let mut bmp = Bitmap::new(3, 3).unwrap();
    for y in 0..bmp.height() {
        bmp.line_mut(y).unwrap().set(y, true);
    }
    for y in 0..bmp.height() {
        let line = bmp.line(y).unwrap();
        assert_eq!(line.first_one(), Some(y));
        assert_eq!(line.last_one(), Some(y));
    }
}

#[test]
fn lines_are_equal_when_bitwise_equal() {
    let bv = make_bits(&[true, false, true, false]);
    assert_eq!(BitmapLine::new(&bv), BitmapLine::new(&bv));
    let other = make_bits(&[true, false, true, true]);
    assert_ne!(BitmapLine::new(&bv), BitmapLine::new(&other));
}

#[test]
fn lines_iterates_all_rows() {
    let mut bmp = Bitmap::new(4, 3).unwrap();
    bmp.line_mut(1).unwrap().copy_from_bytes(&[0xF0]).unwrap();
    let rows: Vec<Vec<u8>> = bmp.lines().map(|line| line.to_bytes()).collect();
    assert_eq!(rows, vec![vec![0x00], vec![0xF0], vec![0x00]]);
}

#[test]
fn unaligned_to_bytes_matches_manual_bits() {
    let bv = BitVec::<u8, Msb0>::from_bitslice(bits![u8, Msb0;
        1, 0, 1, 0, 1, 0, 1, 0,
        1, 1, 1
    ]);
    assert_eq!(BitmapLine::new(&bv).to_bytes(), vec![0xAA, 0b1110_0000]);
}

#[test]
fn fill_and_into_bits() {
    let mut bmp = Bitmap::new(2, 2).unwrap();
    bmp.fill(true);
    assert_eq!(bmp.into_bits(), BitVec::<u8, Msb0>::repeat(true, 4));
}
