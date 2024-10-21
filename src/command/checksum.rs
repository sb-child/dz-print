// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub fn calculate_checksum(arr: &[u8], mut start: usize, end: usize) -> u8 {
    let mut x: u32 = 0;
    while start < end {
        x += arr[start] as u32;
        start += 1;
    }
    (x & 0xFF) as u8 ^ 0xFF
}
