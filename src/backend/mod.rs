// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::time::Duration;

use rusb::UsbContext;
use thiserror::Error;

pub enum USBSelector {
    /// by VID and PID, pick the first match
    USBID(u16, u16),
    /// by device serial number "型号-序列号", pick the first match
    DeviceSerial(String),
}

#[derive(Error, Debug)]
pub enum BackendError {
    #[error("rusb error: `{0:?}`")]
    USBError(#[from] rusb::Error),
    #[error("selector no matches")]
    SelectorNoMatches,
}

pub struct USBBackend {}

impl USBBackend {
    pub fn new(selector: USBSelector) -> Result<Self, BackendError> {
        let ctx = rusb::Context::new()?;
        let devices = ctx.devices()?;
        let device = devices.iter().find(|x| {
            let timeout = Duration::from_secs(1);
            let h = if let Ok(h) = x.open() {
                h
            } else {
                return false;
            };
            let lang = if let Ok(lang) = h.read_languages(timeout) {
                if let Some(x) = lang.into_iter().next() {
                    x
                } else {
                    return false;
                }
            } else {
                return false;
            };
            let desc = if let Ok(desc) = x.device_descriptor() {
                desc
            } else {
                return false;
            };
            let cd = if let Ok(cd) = x.config_descriptor(0) {
                cd
            } else {
                return false;
            };
            let iface = if let Some(iface) = cd.interfaces().next() {
                iface
            } else {
                return false;
            };
            let idesc = if let Some(idesc) = iface.descriptors().next() {
                idesc
            } else {
                return false;
            };
            // idesc.description_string_index();
            let iname = if let Ok(x) = h.read_interface_string(lang, &idesc, timeout) {
                x
            } else {
                return false;
            };
            // println!("{}", iname);
            match &selector {
                USBSelector::USBID(v, p) => desc.vendor_id() == *v && desc.product_id() == *p,
                USBSelector::DeviceSerial(n) => iname.ends_with(&format!("@ {n}")),
            }
        });
        let device = if let Some(device) = device {
            device
        } else {
            return Err(BackendError::SelectorNoMatches);
        };
        Ok(USBBackend {})
    }
}
