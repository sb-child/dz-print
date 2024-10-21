// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub struct PrinterInfo {
    pub device_type: u8,
    pub device_name: String,
    pub device_version: String,
    pub software_version: String,
    pub device_address: String,
    pub device_addr_type: u8,
    pub device_dpi: u16,
    pub device_width: u32,
    pub manufacturer: String,
    pub series_name: String,
    pub dev_int_name: String,
    pub peripheral_flags: u16,
    pub hardware_flags: u32,
    pub software_flags: u32,
    pub mcu_id: String,
}

#[derive(Debug)]
pub enum ProgressInfo {
    AdapterEnabling,
    AdapterEnabled,
    AdapterDisabled,
    DeviceBonding,
    DeviceBonded,
    DeviceUnbonded,
    DeviceLocateWrong,
}

#[derive(Debug)]
pub enum PrintProgress {
    Connected,
    StartCopy,
    DataEnded,
    Success,
    Failed,
}
