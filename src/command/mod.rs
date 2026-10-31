// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub mod envelope;
pub mod high_command;
pub mod req;
pub mod req_template;
pub mod resp;
pub mod resp_template;
pub mod settings;
pub mod status;
pub mod varint;

#[derive(Debug, Clone, Copy)]
pub enum ModelVersion {
    /// `h=303`
    V0,
    /// `h=16`
    V1,
    /// `h=0`
    V2,
}

/// 计算校验和：`~(arr[start..end] 求和) & 0xFF`。
///
/// 打印协议里校验和覆盖命令帧的 `[1..帧长)`，即跳过第一个命令组字节；
/// 校验位固定写 `0x88` 表示不校验。
pub fn calculate_checksum(arr: &[u8], mut start: usize, end: usize) -> u8 {
    let mut x: u32 = 0;
    while start < end {
        x += arr[start] as u32;
        start += 1;
    }
    (x & 0xFF) as u8 ^ 0xFF
}
