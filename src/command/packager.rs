// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use super::variable_bytes::{FromVariableBytes, VariableBytesI32};

pub fn package_usb(x: Vec<u8>) -> Vec<u8> {
    let packet_len_buf = (x.len() as i32).to_variable_bytes();
    let packet_len = 1 + packet_len_buf.len() + x.len();
    let mut buf = Vec::<u8>::with_capacity(packet_len);
    buf.push(0x1e);
    buf.extend_from_slice(&packet_len_buf);
    buf.extend_from_slice(&x);
    buf
}
pub fn unpackage_usb(x: Vec<u8>) -> Option<Vec<u8>> {
    if x.len() < 2 {
        return None;
    }
    let packet_type = x[0];
    if packet_type != 0x1e {
        return None;
    }
    let (packet_len, b) = if let Some(x) = vec![x[1], x[2]].from_variable_bytes() {
        x
    } else {
        return None;
    };
    let prefix_len = 1 + b;
    if x.len() < prefix_len + packet_len as usize {
        return None;
    }
    Some(x[prefix_len..prefix_len + packet_len as usize].to_vec())
}

#[cfg(test)]
mod test {
    use super::{package_usb, unpackage_usb};

    #[test]
    fn test_package() {
        let x = vec![0x19, 0x89, 0x06, 0x04];
        let pack = package_usb(x);
        assert_eq!(
            pack,
            vec![0x1e, 0x04, 0x19, 0x89, 0x06, 0x04],
            "unexcepted pack: {:02x?}",
            pack
        );
        let mut pack2 = pack.clone();
        pack2.extend_from_slice(&[b'f', b'u', b'c', b'k', b'C', b'C', b'P']);
        let unpack = unpackage_usb(pack2);
        assert_eq!(
            unpack,
            Some(vec![0x19, 0x89, 0x06, 0x04]),
            "unexcepted unpack: {:02x?}",
            unpack
        );
    }
}
