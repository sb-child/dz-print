// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use deku::bitvec::BitField;

use crate::{bitmap::BitmapLine, rle::RleTransformError};

#[derive(Debug, PartialEq, Clone)]
pub struct Run<V> {
    pub value: V,
    pub count: usize,
}

impl TryInto<Vec<Run<bool>>> for &BitmapLine {
    type Error = RleTransformError;

    fn try_into(self) -> Result<Vec<Run<bool>>, Self::Error> {
        let len = self.data.len();
        let first_bit: bool = (*(self.data.first().ok_or(Self::Error::EmptyData)?)).into();
        let mut shifted = self.data.clone();
        shifted.shift_end(1);
        let xored = self.data.clone() ^ shifted;
        let prepend_zero = if !first_bit { Some(0) } else { None };
        let res: Vec<Run<bool>> = prepend_zero
            .into_iter()
            .chain(xored.iter_ones())
            .chain(std::iter::once(len))
            .map_windows(|[a, b]| b - a)
            .enumerate()
            .map(|(idx, num)| {
                let bitval = (idx % 2 != 0) ^ first_bit;
                Run {
                    value: bitval,
                    count: num,
                }
            })
            .collect();
        Ok(res)
    }
}

impl TryInto<Vec<Run<u8>>> for &BitmapLine {
    type Error = RleTransformError;

    fn try_into(self) -> Result<Vec<Run<u8>>, Self::Error> {
        if self.data.is_empty() {
            return Err(Self::Error::EmptyData);
        }
        let bytes: Vec<u8> = self
            .data
            .chunks(8)
            .map(|chunk| {
                let val = chunk.load_be::<u8>();
                if chunk.len() >= 8 {
                    return val;
                }
                val << (8 - chunk.len())
            })
            .collect();
        let res: Vec<Run<u8>> = bytes
            .chunk_by(|a, b| a == b)
            .map(|chunk| Run {
                value: chunk[0],
                count: chunk.len(),
            })
            .collect();
        Ok(res)
    }
}

#[cfg(test)]
mod rle_tests {
    use super::*;
    use deku::bitvec::{BitVec, Msb0, bits};

    fn make_bitmap(bits: &[bool]) -> BitmapLine {
        let mut bv = BitVec::<u8, Msb0>::new();
        for &b in bits {
            bv.push(b);
        }
        BitmapLine { data: bv }
    }

    #[test]
    fn test_rle_bool_starts_with_one() {
        let line = make_bitmap(&[true, true, true, false, false, true]);
        let res: Result<Vec<Run<bool>>, _> = (&line).try_into();
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
        let line = make_bitmap(&[false, false, true, true, true, false]);
        let res: Result<Vec<Run<bool>>, _> = (&line).try_into();
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
        let line_ones = make_bitmap(&[true, true, true, true]);
        let res_ones: Vec<Run<bool>> = (&line_ones).try_into().unwrap();
        assert_eq!(
            res_ones,
            vec![Run {
                value: true,
                count: 4
            }]
        );

        let line_zeros = make_bitmap(&[false, false, false]);
        let res_zeros: Vec<Run<bool>> = (&line_zeros).try_into().unwrap();
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
        let line = make_bitmap(&[true]);
        let res: Vec<Run<bool>> = (&line).try_into().unwrap();
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
        let line = make_bitmap(&[]);
        let res: Result<Vec<Run<bool>>, RleTransformError> = (&line).try_into();
        assert!(matches!(res, Err(RleTransformError::EmptyData)));
    }

    #[test]
    fn test_rle_u8_basic() {
        let bytes = vec![0xAA, 0xAA, 0xFF, 0x00, 0x00, 0x00];
        let bv = BitVec::<u8, Msb0>::from_slice(&bytes);
        let line = BitmapLine { data: bv };
        let res: Vec<Run<u8>> = (&line).try_into().unwrap();
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
        let line = BitmapLine {
            data: BitVec::<u8, Msb0>::from_slice(&bytes),
        };
        let res: Vec<Run<u8>> = (&line).try_into().unwrap();
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
        let line = BitmapLine {
            data: BitVec::<u8, Msb0>::new(),
        };
        let res: Result<Vec<Run<u8>>, RleTransformError> = (&line).try_into();
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
        let line = BitmapLine { data: bv };

        let res: Vec<Run<u8>> = (&line).try_into().unwrap();
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
        let line = BitmapLine { data: bv };
        let res: Vec<Run<u8>> = (&line).try_into().unwrap();
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
        let line = BitmapLine { data: bv };
        let res: Vec<Run<u8>> = (&line).try_into().unwrap();
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
        let line = BitmapLine { data: bv };
        let res: Vec<Run<u8>> = (&line).try_into().unwrap();
        let expected = vec![Run {
            value: 0xF8,
            count: 2,
        }];
        assert_eq!(res, expected);
    }
}
