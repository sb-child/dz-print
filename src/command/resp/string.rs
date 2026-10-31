// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 响应里的字符串解码。
//!
//! 设备返回的字符串可能是 GBK/GB2312，并且带 `0x00` 结尾。

use encoding_rs::GBK;

use super::{InvalidStringSnafu, ParseRespError};

/// 解码 `0x00` 结尾的 GBK/GB2312 字符串，返回去掉结尾 `0x00` 的 `Box<str>`。
pub fn decode_gbk_nul(bytes: &[u8]) -> Result<Box<str>, ParseRespError> {
    let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    let (s, had_errors) = GBK.decode_without_bom_handling(&bytes[..end]);
    snafu::ensure!(!had_errors, InvalidStringSnafu);
    Ok(s.into_owned().into_boxed_str())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn ascii_with_nul() {
        assert_eq!(&*decode_gbk_nul(b"DP27P\0").unwrap(), "DP27P");
    }

    #[test]
    fn gbk_with_nul() {
        assert_eq!(
            &*decode_gbk_nul(&[0xb5, 0xc2, 0xd9, 0xa1, 0x00]).unwrap(),
            "德佟"
        );
    }

    #[test]
    fn without_nul() {
        assert_eq!(&*decode_gbk_nul(&[0xb5, 0xc2, 0xd9, 0xa1]).unwrap(), "德佟");
    }

    #[test]
    fn strips_all_trailing_nul() {
        assert_eq!(&*decode_gbk_nul(b"DP27P\0\0\0").unwrap(), "DP27P");
    }

    #[test]
    fn empty() {
        assert_eq!(&*decode_gbk_nul(b"\0").unwrap(), "");
        assert_eq!(&*decode_gbk_nul(b"").unwrap(), "");
    }

    #[test]
    fn invalid_gbk_is_error() {
        let err = decode_gbk_nul(&[0xff, 0x00]).unwrap_err();
        assert!(matches!(err, ParseRespError::InvalidString), "{err:?}");
    }
}
