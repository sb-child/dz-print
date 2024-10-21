// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#[derive(Clone)]
pub struct PrinterParam {
    pub device_type: i32,
    pub device_name: String,
    pub device_version: String,
    pub software_version: String,
    pub device_address: String,
    pub device_addr_type: i32,
    pub printer_dpi: i32,
    pub printer_width: i32,
    pub paper_width: i32,
    pub printer_locate_area: i32,
    pub print_darkness: i32,
    pub darkness_count: i32,
    pub darkness_min_suggested: i32,
    pub print_speed: i32,
    pub speed_count: i32,
    pub gap_type: i32,
    pub gap_length: i32,
    pub motor_mode: i32,
    pub auto_power_off_mins: i32,
    pub language: i32,
    pub supported_gap_types: Vec<i32>,
    pub supported_motor_modes: Vec<i32>,
    pub supported_languages: Vec<i32>,
    pub manufacturer: String,
    pub series_name: String,
    pub dev_int_name: String,
    pub peripheral_flags: i32,
    pub hardware_flags: i32,
    pub software_flags: i32,
    pub attribute_flags: i32,
    pub upgrade_crc: i32,
    pub open_hint_voice: bool,
    pub auto_out_page: bool,
    pub can_set_gen_flags: bool,
    pub printer_head_tem: f64,
    pub battery_voltage: f64,
    pub manu_ship_time: String,
    pub mcu_id: String,
    pub stack_head: String,
    pub stack_tail: String,
    pub heap_head: String,
    pub heap_tail: String,
    pub max_stack: i32,
    pub heap_unused: i32,
    pub heap_min_unused: i32,
    pub debug1: String,
    pub debug2: String,
    pub debug3: String,
    pub debug4: String,
    pub work_lines: i32,
    pub print_lines: i32,
    pub null_lines: i32,
    pub print_pages: i32,
    pub charge_status: i32,
    pub printer_status: i32,
    pub battery_count: i32,
}

pub enum PageGap {
    /// 小票纸
    Ticket,
    /// 透明贴
    LocatorHole,
    /// 不干胶
    Adhesive,
    /// 卡纸
    CardPaper,
}

pub enum PrintSpeed {
    Min,
    Speed1,
    Default,
    Speed3,
    Max,
}

pub enum PrintDarkness {
    Min,
    Darkness1,
    Darkness2,
    Darkness3,
    Darkness4,
    Default,
    Darkness6,
    Darkness7,
    Darkness8,
    Darkness9,
    Darkness10,
    Darkness11,
    Darkness12,
    Darkness13,
    Max,
}

pub enum PaperType {
    Default,
    Hole,
    Gap,
    Black,
}
