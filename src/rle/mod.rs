// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use deku::bitvec::BitVec;
use deku::bitvec::LocalBits;

pub struct BitmapLine {
    data: BitVec,
}

impl BitmapLine {
    pub fn new(data: BitVec) -> Self {
        Self { data }
    }
}

#[derive(Debug, PartialEq)]
pub struct Run {
    pub value: bool,
    pub count: usize,
}

#[cfg(test)]
mod tests {
    use deku::bitvec::bits;
    use typst::foundations::Fold;

    use super::*;

    #[test]
    fn test() {
        let b = bits![u16, LocalBits;
        1, 1, 1, 1, 1, 0, 0, 0,
        0, 0, 1, 0, 1, 0, 1, 0,
        1, 1, 1, 1, 1, 1, 0, 0,
        0, 0, 0, 0, 1,];
        let bm = BitVec::from_bitslice(b);
        let len = bm.len();
        let first: bool = (*bm.first().unwrap()).into();
        let mut bm2 = bm.clone();
        bm2.shift_end(1);
        let bm3 = bm.clone() ^ bm2.clone();
        println!("{}", bm);
        println!("{}", bm2);
        println!("{}", bm ^ bm2);
        let prepend_zero = if !first { Some(0) } else { None };
        let v: Vec<Run> = prepend_zero
            .into_iter()
            .chain(bm3.iter_ones())
            .chain(std::iter::once(len)) // 补充末尾边界 bm.len()
            .map_windows(|[a, b]| b - a)
            .enumerate()
            .map(|(idx, num)| {
                // println!("{}, {}", idx, num);
                let bitval = (idx % 2 != 0) ^ first;
                Run {
                    value: bitval,
                    count: num,
                }
            })
            .collect();
        println!("{:?}", v);
        let exp = &[
            Run {
                value: true,
                count: 5,
            },
            Run {
                value: false,
                count: 5,
            },
            Run {
                value: true,
                count: 1,
            },
            Run {
                value: false,
                count: 1,
            },
            Run {
                value: true,
                count: 1,
            },
            Run {
                value: false,
                count: 1,
            },
            Run {
                value: true,
                count: 1,
            },
            Run {
                value: false,
                count: 1,
            },
            Run {
                value: true,
                count: 6,
            },
            Run {
                value: false,
                count: 6,
            },
            Run {
                value: true,
                count: 1,
            },
        ];
        assert_eq!(v, exp);
    }
}
