// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

/// RLE encode processing function
pub fn m304a(arr: &mut Vec<i8>, dzint: &mut i32, b: i8, mut i: i32, line_bytes: i32) -> bool {
    while i >= 63 {
        if *dzint + 2 > line_bytes {
            return false;
        }

        arr[{
            let x = *dzint;
            *dzint = x + 1;
            x as usize
        }] = -1;

        arr[{
            let x = *dzint;
            *dzint = x + 1;
            x as usize
        }] = b;

        i -= 63;
    }

    match i {
        1 => {
            if (b as u8) > 192 {
                if *dzint + 2 <= line_bytes {
                    arr[{
                        let x = *dzint;
                        *dzint = x + 1;
                        x as usize
                    }] = -63;

                    arr[{
                        let x = *dzint;
                        *dzint = x + 1;
                        x as usize
                    }] = b;
                } else {
                    return false;
                }
            } else {
                if *dzint + 1 <= line_bytes {
                    arr[{
                        let x = *dzint;
                        *dzint = x + 1;
                        x as usize
                    }] = b;
                    return true;
                }
                return false;
            }
        }
        2 => {
            if *dzint + 2 <= line_bytes {
                if (b as u8) > 192 {
                    arr[{
                        let x = *dzint;
                        *dzint = x + 1;
                        x as usize
                    }] = -62;

                    arr[{
                        let x = *dzint;
                        *dzint = x + 1;
                        x as usize
                    }] = b;
                } else {
                    arr[{
                        let x = *dzint;
                        *dzint = x + 1;
                        x as usize
                    }] = b;

                    arr[{
                        let x = *dzint;
                        *dzint = x + 1;
                        x as usize
                    }] = b;
                }
                return false;
            }
        }
        _ => {
            if i > 0 {
                if *dzint + 2 <= line_bytes {
                    arr[{
                        let x = *dzint;
                        *dzint = x + 1;
                        x as usize
                    }] = (i | 192) as i8;

                    arr[{
                        let x = *dzint;
                        *dzint = x + 1;
                        x as usize
                    }] = b;
                } else {
                    return false;
                }
            }
            return true;
        }
    }
    false
}

/// RLE encode
pub fn m305a(arr: Vec<i8>, i: i32, arr2: &mut Vec<i8>, line_bytes: i32) -> i32 {
    if i > 0 {
        let mut dzint = 0;
        let mut b = arr[0];
        let mut i2 = 1;
        for i3 in 1..i {
            if arr[i3 as usize] == b {
                i2 += 1;
            } else {
                if !m304a(arr2, &mut dzint, b, i2, line_bytes) {
                    return 0;
                }
                b = arr[i3 as usize];
                i2 = 1;
            }
        }
        if !m304a(arr2, &mut dzint, b, i2, line_bytes) {
            return 0;
        }
        return dzint;
    } else {
        return 0;
    }
}

/// RLE5 encode
pub fn m307a() {}

#[cfg(test)]
mod test {
    use super::m305a;

    #[test]
    fn test_rle_m305a() {
        let arr1: Vec<i8> = vec![0, 111, 1, 2, 2, 2, 2, 3, 4, 4, 5, 5, 5, 6];
        let arr1_len = arr1.len();
        let mut arr2: Vec<i8> = Vec::new();
        arr2.resize(arr1.len(), 0);
        m305a(arr1, arr1_len as i32, &mut arr2, 32);
        println!("{arr2:?}");
    }
}
