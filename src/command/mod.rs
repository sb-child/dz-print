// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub mod checksum;
pub mod packager;
pub mod variable_bytes;
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::{FromPrimitive, ToPrimitive};
use std::marker::PhantomData;
use variable_bytes::{FromVariableBytes, VariableBytesI32};
pub struct DefaultState;
pub struct Host;
pub struct Device;

pub enum Commands {
    Host(HostCommand),
    Device(DeviceCommand),
}

#[derive(Debug, Clone, Copy, FromPrimitive, ToPrimitive, PartialEq)]
pub enum HostCommand {
    Init = 0x1f78,
    ReadSoftwareVersion = 0x1f7c,
    ReadDeviceName = 0x1f79,
    GetSetPrintPaperType = 0x1f42,
    GetSetPrintSpeed = 0x1f44,
    GetSetPrintPaperGap = 0x1f45,
    GetSetPrintDarkness = 0x1f43,
    ReadManufacturer = 0x1f75,
    GetPrinterStatus = 0x1f70,
    // Test = 0x1f70,
    EnableHighCommand = 0x1f80,
    GetSensorStatus = 0x1f88,
}

#[derive(Debug, Clone, Copy, FromPrimitive, ToPrimitive, PartialEq)]
pub enum DeviceCommand {
    InitResult = 0x1f78,
    SoftwareVersion = 0x1f7c,
    DeviceName = 0x1f79,
    Manufacturer = 0x1f75,
    PrintSpeed = 0x1f44,
    PaperType = 0x1f42,
    PaperGap = 0x1f45,
    PrintDarkness = 0x1f43,
    PrinterStatus = 0x1f70,
    HighCommand = 0x1f80,
    SensorStatus = 0x1f88,
}

pub struct Command<Direction = DefaultState> {
    cmd: Commands,
    payload: Vec<u8>,
    direction: PhantomData<Direction>,
}

impl Command<Host> {
    pub fn package(&self, p: Vec<u8>, fixed_checksum: bool) -> Vec<u8> {
        let payload_len_buf = (p.len() as i32).to_variable_bytes();
        // 命令组 + 命令类型 + 数据长度 + 数据... + 校验和
        let packet_len = 2 + payload_len_buf.len() + p.len() + 1;
        let mut buf = Vec::<u8>::with_capacity(packet_len);
        for _ in 0..packet_len {
            buf.push(0);
        }
        (buf[0], buf[1]) = self.get_header();
        for i in 0..payload_len_buf.len() {
            buf[2 + i] = payload_len_buf[i];
        }
        for i in 0..p.len() {
            buf[2 + payload_len_buf.len() + i] = p[i];
        }
        let checksum = if fixed_checksum {
            0x88
        } else {
            checksum::calculate_checksum(&buf, 1, packet_len)
        };
        buf[packet_len - 1] = checksum;
        buf
    }
}
impl Command<Device> {
    pub fn get_payload(&self) -> Vec<u8> {
        self.payload.clone()
    }

    pub fn get_command(&self) -> DeviceCommand {
        match &self.cmd {
            Commands::Host(_) => unreachable!(),
            Commands::Device(cmd) => *cmd,
        }
    }
}

impl<Direction> Command<Direction> {
    /// returns a tuple `(命令组, 命令类型)`
    pub fn get_header(&self) -> (u8, u8) {
        match &self.cmd {
            Commands::Host(cmd) => {
                let x = cmd.to_u16().unwrap().to_be_bytes();
                (x[0], x[1])
            }
            Commands::Device(cmd) => {
                let x = cmd.to_u16().unwrap().to_be_bytes();
                (x[0], x[1])
            }
        }
    }
}

impl Command {
    pub fn new_host(cmd: HostCommand) -> Command<Host> {
        Command {
            cmd: Commands::Host(cmd),
            payload: vec![],
            direction: PhantomData::default(),
        }
    }

    pub fn parse_device_command(cmd: impl AsRef<Vec<u8>>) -> Option<(Command<Device>, usize)> {
        let cmd = cmd.as_ref();
        if cmd.len() < 4 {
            return None;
        }
        let (cg, ct) = (cmd[0], cmd[1]);
        let c = u16::from_be_bytes([cg, ct]);
        let c = if let Some(c) = DeviceCommand::from_u16(c) {
            c
        } else {
            return None;
        };
        let packet_len_bytes = vec![cmd[2], cmd[3]].from_variable_bytes();
        let (packet_len, packet_len_offset) = if let Some(x) = packet_len_bytes {
            (x.0, x.1)
        } else {
            return None;
        };
        let command_len = 2 + packet_len_offset + packet_len as usize + 1;
        if cmd.len() < command_len {
            return None;
        }
        let mut payload = Vec::<u8>::with_capacity(packet_len as usize);
        payload.resize(packet_len as usize, 0);
        payload.copy_from_slice(
            &cmd[2 + packet_len_offset..2 + packet_len_offset + packet_len as usize],
        );
        let checksum = cmd[command_len - 1];
        let mut command = Vec::<u8>::with_capacity(command_len);
        command.extend_from_slice(&[cg, ct]);
        command.extend_from_slice(&packet_len.to_variable_bytes());
        command.extend_from_slice(&payload);
        command.push(0);
        let calculated_checksum = checksum::calculate_checksum(&command, 1, command.len());
        if !(checksum == 0x88 || checksum == calculated_checksum) {
            return None;
        }
        Some((
            Command {
                cmd: Commands::Device(c),
                payload,
                direction: PhantomData::default(),
            },
            command.len(),
        ))
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_host_command() {
        use crate::command::{Command, HostCommand};
        let c = Command::new_host(HostCommand::ReadSoftwareVersion);
        let r = c.package(vec![], false);
        assert_eq!(
            r,
            vec![0x1f, 0x7c, 0x00, 0x83],
            "unexcepted result: {:02x?}",
            r
        );
    }

    #[test]
    fn test_device_command() {
        use crate::command::{Command, DeviceCommand};
        let c = vec![
            0x1f, 0x7c, 0x0d, 0x33, 0x2e, 0x31, 0x2e, 0x32, 0x30, 0x32, 0x33, 0x30, 0x36, 0x32,
            0x30, 0x00, 0x27, 0x1f, 0x7d,
        ];
        let c = Command::parse_device_command(c);
        if let Some((cmd, b)) = c {
            assert_eq!(b, 17);
            assert_eq!(
                cmd.get_payload(),
                vec![0x33, 0x2e, 0x31, 0x2e, 0x32, 0x30, 0x32, 0x33, 0x30, 0x36, 0x32, 0x30, 0x00]
            );
            assert_eq!(cmd.get_command(), DeviceCommand::SoftwareVersion);
        } else {
            panic!("failed")
        }
    }
}
