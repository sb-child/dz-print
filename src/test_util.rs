// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 测试共用的工具函数。
//!
//! 只在 `cfg(test)` 下编译。

/// 把 `"1f 70 00 8f"` 这类十六进制字符串转成 `Vec<u8>`。
///
/// 只接受两位的 token，空格、制表符和逗号都算分隔符；非法 token 直接 panic，
/// 避免测试里写错字节却静默漏掉。
pub(crate) fn hexdec(s: &str) -> Vec<u8> {
    s.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .map(|part| {
            u8::from_str_radix(part, 16)
                .unwrap_or_else(|e| panic!("hexdec: invalid hex token {part:?}: {e}"))
        })
        .collect()
}
