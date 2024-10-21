// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub trait VariableBytesI32 {
    fn to_variable_bytes(self) -> Vec<u8>;
}

impl VariableBytesI32 for i32 {
    // #[allow(unreachable_code)]
    fn to_variable_bytes(self) -> Vec<u8> {
        if self < 0 {
            unimplemented!()
        }
        if self < 192 {
            // 0b11000000
            Vec::from([self as u8])
        } else if self < 16384 {
            // 0b01000000_00000000
            let mut x = self.to_be_bytes();
            x[2] |= 0b11000000;
            let x = &x[2..4];
            Vec::from(x)
        } else if self < 4194304 {
            // 0b01000000_00000000_00000000
            // 仅用于特定命令
            let mut x = self.to_be_bytes();
            x[1] |= 0b11000000;
            let x = &x[1..4];
            Vec::from(x)
        } else {
            unimplemented!()
        }
    }
}

pub trait FromVariableBytes {
    fn from_variable_bytes(&self) -> Option<(i32, usize)>;
    fn from_variable_bytes_fixed(&self, x: usize) -> Option<i32>;
}

impl FromVariableBytes for Vec<u8> {
    fn from_variable_bytes(&self) -> Option<(i32, usize)> {
        if self.len() >= 1 && self[0] & 0b11000000 != 0b11000000 {
            // 1 byte
            Some((self[0] as i32, 1))
        } else if self.len() >= 2 && self[0] & 0b11000000 == 0b11000000 {
            // 2 bytes
            let b0 = self[0] & 0b00111111;
            let b1 = self[1];
            let mut x = [0u8; 4];
            x[2] = b0;
            x[3] = b1;
            Some((i32::from_be_bytes(x), 2))
        } else {
            None
        }
    }

    fn from_variable_bytes_fixed(&self, x: usize) -> Option<i32> {
        if self.len() < x {
            return None;
        }
        match x {
            1 => Some(self[0] as i32),
            2 => {
                if self[0] & 0b11000000 != 0b11000000 {
                    None
                } else {
                    let b0 = self[0] & 0b00111111;
                    let b1 = self[1];
                    let mut x = [0u8; 4];
                    x[2] = b0;
                    x[3] = b1;
                    Some(i32::from_be_bytes(x))
                }
            }
            3 => {
                // 仅用于特定命令
                if self[0] & 0b11000000 != 0b11000000 {
                    None
                } else {
                    let b0 = self[0] & 0b00111111;
                    let b1 = self[1];
                    let b2 = self[2];
                    let mut x = [0u8; 4];
                    x[1] = b0;
                    x[2] = b1;
                    x[3] = b2;
                    Some(i32::from_be_bytes(x))
                }
            }
            _ => unimplemented!(),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::command::variable_bytes::FromVariableBytes;

    use super::VariableBytesI32;

    #[test]
    fn test_variable_bytes() {
        for i in 0..16384 {
            let a: i32 = i;
            let v = a.to_variable_bytes();
            let b = v.from_variable_bytes();
            if let Some((b, s)) = b {
                if a != b {
                    panic!("failure: {} {:02x?} {} {}", a, v, b, s)
                }
            } else {
                panic!("b is None: {} {:02x?} {:?}", a, v, b)
            }
        }
    }

    #[test]
    fn test_variable_bytes_padding() {
        for i in 0..16384 {
            let a: i32 = i;
            let mut v = a.to_variable_bytes();
            v.push(19);
            v.push(89);
            v.push(06);
            v.push(04);
            let b = v.from_variable_bytes();
            if let Some((b, s)) = b {
                if a != b {
                    panic!("failure: {} {:02x?} {} {}", a, v, b, s)
                }
            } else {
                panic!("b is None: {} {:02x?} {:?}", a, v, b)
            }
        }
    }
}
