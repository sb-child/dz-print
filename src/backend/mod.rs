// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    thread,
    time::{self, Duration},
};

use rusb::UsbContext;
use thiserror::Error;

use crate::command::{self, packager};

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
    #[error("tokio join error: `{0:?}`")]
    TokioJoinError(#[from] tokio::task::JoinError),
}

struct Endpoint {
    config: u8,
    iface: u8,
    setting: u8,
    address: u8,
}

pub type CommandPayload = Vec<u8>;
pub type CommandResponse = command::Command<command::Device>;
/// Created -> Sent/Errored -> Received(CommandPayload)
pub type CommandResultWithResponse =
    tokio::sync::oneshot::Receiver<Option<tokio::sync::oneshot::Receiver<CommandResponse>>>;
/// Created -> Sent/Errored
pub type CommandResultWithoutResponse = tokio::sync::oneshot::Receiver<bool>;
/// Created -> Sent/Errored -> Received(CommandPayload)
pub type CommandResultSenderWithResponse =
    tokio::sync::oneshot::Sender<Option<tokio::sync::oneshot::Receiver<CommandResponse>>>;
/// Created -> Sent/Errored
pub type CommandResultSenderWithoutResponse = tokio::sync::oneshot::Sender<bool>;

pub enum Command {
    WithResponse(CommandPayload, CommandResultSenderWithResponse),
    WithoutResponse(CommandPayload, CommandResultSenderWithoutResponse),
}

impl Command {
    pub fn with_response(payload: CommandPayload) -> (Command, CommandResultWithResponse) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (Command::WithResponse(payload, tx), rx)
    }

    pub fn without_response(payload: CommandPayload) -> (Command, CommandResultWithoutResponse) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (Command::WithoutResponse(payload, tx), rx)
    }
}

pub struct USBBackend {
    close_chan: tokio::sync::broadcast::Sender<()>,
    command_tx: tokio::sync::mpsc::Sender<Command>,
}

impl USBBackend {
    fn new_usb_backend_blocking(selector: USBSelector) -> Result<Self, BackendError> {
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
        let mut in_ep = Endpoint {
            config: 0,
            iface: 0,
            setting: 0,
            address: 0,
        };
        let mut out_ep = Endpoint {
            config: 0,
            iface: 0,
            setting: 0,
            address: 0,
        };
        let cd = device.config_descriptor(0)?;
        for iface in cd.interfaces() {
            for desc in iface.descriptors() {
                for ep in desc.endpoint_descriptors() {
                    if ep.direction() == rusb::Direction::In
                        && ep.transfer_type() == rusb::TransferType::Interrupt
                    {
                        in_ep = Endpoint {
                            config: cd.number(),
                            iface: desc.interface_number(),
                            setting: desc.setting_number(),
                            address: ep.address(),
                        };
                    }
                    if ep.direction() == rusb::Direction::Out
                        && ep.transfer_type() == rusb::TransferType::Interrupt
                    {
                        out_ep = Endpoint {
                            config: cd.number(),
                            iface: desc.interface_number(),
                            setting: desc.setting_number(),
                            address: ep.address(),
                        };
                    }
                }
            }
        }
        let (close_chan, mut close_sig_1) = tokio::sync::broadcast::channel(1);
        let mut close_sig_2 = close_chan.subscribe();
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(1);
        let (recv_tx, mut recv_rx) = tokio::sync::mpsc::channel(1);
        let max_in_size = 64;
        let max_out_size = 62;
        let in_timeout = Duration::from_millis(500);
        let out_timeout = Duration::from_millis(100);
        // open device
        let h = Arc::new(device.open()?);
        h.reset().unwrap();
        match h.kernel_driver_active(out_ep.iface) {
            Ok(true) => {
                h.detach_kernel_driver(out_ep.iface).ok();
            }
            _ => {}
        };
        match h.kernel_driver_active(in_ep.iface) {
            Ok(true) => {
                h.detach_kernel_driver(in_ep.iface).ok();
            }
            _ => {}
        };
        h.claim_interface(out_ep.iface)?;
        h.set_alternate_setting(out_ep.iface, out_ep.setting)?;
        h.claim_interface(in_ep.iface)?;
        h.set_alternate_setting(in_ep.iface, in_ep.setting)?;
        let h1 = h.clone();
        let h2 = h.clone();
        // OUT thread, send data to device
        tokio::task::spawn_blocking(move || {
            let mut packet_buf = VecDeque::new();
            let mut raw_packet_len = 0;
            let mut tim = time::Instant::now();
            loop {
                if !close_sig_1.is_empty() {
                    close_sig_1.blocking_recv().ok();
                    println!("OUT thread: closed");
                    return;
                }
                if raw_packet_len >= max_out_size
                    || tim.elapsed() > time::Duration::from_millis(100)
                {
                    tim = time::Instant::now();
                    let mut buf = Vec::with_capacity(max_out_size);
                    let mut committed_cmds = Vec::new();
                    loop {
                        if buf.len() >= max_out_size {
                            break;
                        }
                        let p = if let Some(p) = packet_buf.pop_front() {
                            p
                        } else {
                            break;
                        };
                        match p {
                            Command::WithResponse(p, sender) => {
                                if p.len() + buf.len() > max_out_size {
                                    let next: Vec<u8> = p
                                        .iter()
                                        .skip(max_out_size - buf.len())
                                        .map(|x| x.to_owned())
                                        .collect();
                                    let curr: Vec<u8> = p
                                        .iter()
                                        .take(max_out_size - buf.len())
                                        .map(|x| x.to_owned())
                                        .collect();
                                    packet_buf.push_front(Command::WithResponse(next, sender));
                                    buf.extend(curr);
                                    break;
                                }
                                buf.extend(p);
                                committed_cmds.push(Command::WithResponse(vec![], sender));
                            }
                            Command::WithoutResponse(p, sender) => {
                                if p.len() + buf.len() > max_out_size {
                                    let next: Vec<u8> = p
                                        .iter()
                                        .skip(max_out_size - buf.len())
                                        .map(|x| x.to_owned())
                                        .collect();
                                    let curr: Vec<u8> = p
                                        .iter()
                                        .take(max_out_size - buf.len())
                                        .map(|x| x.to_owned())
                                        .collect();
                                    packet_buf.push_front(Command::WithoutResponse(next, sender));
                                    buf.extend(curr);
                                    break;
                                }
                                buf.extend(p);
                                committed_cmds.push(Command::WithoutResponse(vec![], sender));
                            }
                        }
                    }
                    if buf.len() != 0 {
                        // println!("OUT thread: writing {} bytes...", buf.len());
                        raw_packet_len -= buf.len();
                        buf.resize(max_out_size, 0);
                        let buf = packager::package_usb(buf); // + 2 bytes
                        let res = h1.write_interrupt(out_ep.address, &buf, in_timeout);
                        match res {
                            Ok(_) => {
                                for c in committed_cmds {
                                    match c {
                                        Command::WithResponse(_, sender) => {
                                            let (tx, rx) = tokio::sync::oneshot::channel();
                                            sender.send(Some(rx)).ok();
                                            recv_tx.blocking_send(tx).ok();
                                        }
                                        Command::WithoutResponse(_, sender) => {
                                            sender.send(true).ok();
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                println!("OUT thread: USB error: {:?}", e);
                                for c in committed_cmds {
                                    match c {
                                        Command::WithResponse(_, sender) => {
                                            sender.send(None).ok();
                                        }
                                        Command::WithoutResponse(_, sender) => {
                                            sender.send(false).ok();
                                        }
                                    }
                                }
                            }
                        }
                        // thread::sleep(Duration::from_millis(5));
                    }
                }

                let cmd = cmd_rx.try_recv();
                let cmd = match cmd {
                    Ok(x) => x,
                    Err(e) => match e {
                        tokio::sync::mpsc::error::TryRecvError::Empty => {
                            thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        tokio::sync::mpsc::error::TryRecvError::Disconnected => {
                            println!("OUT thread: command channel closed");
                            return;
                        }
                    },
                };
                match cmd {
                    Command::WithResponse(p, sender) => {
                        let packet_len = p.len();
                        raw_packet_len += packet_len;
                        packet_buf.push_back(Command::WithResponse(p, sender));
                    }
                    Command::WithoutResponse(p, sender) => {
                        let packet_len = p.len();
                        raw_packet_len += packet_len;
                        packet_buf.push_back(Command::WithoutResponse(p, sender));
                    }
                }
            }
        });
        // IN thread, receive data from device
        tokio::task::spawn_blocking(move || {
            let mut response_buf: VecDeque<
                tokio::sync::oneshot::Sender<command::Command<command::Device>>,
            > = VecDeque::new();
            let mut received = Vec::new();
            let mut parsed = VecDeque::new();
            loop {
                if !close_sig_2.is_empty() {
                    close_sig_2.blocking_recv().ok();
                    println!("IN thread: closed");
                    return;
                }
                if received.len() > 0 {
                    loop {
                        let r = command::Command::parse_device_command(&received);
                        let (cmd, len) = if let Some((cmd, len)) = r {
                            (cmd, len)
                        } else {
                            break;
                        };
                        received = received.into_iter().skip(len).collect();
                        parsed.push_back(cmd);
                    }
                }
                if response_buf.len() > 0 {
                    if let Some(r) = response_buf.pop_front() {
                        if let Some(p) = parsed.pop_front() {
                            r.send(p).ok();
                        } else {
                            response_buf.push_front(r);
                        }
                    }
                    let mut buf = Vec::with_capacity(max_in_size);
                    buf.resize(max_in_size, 0);
                    let res = h2.read_interrupt(in_ep.address, &mut buf, out_timeout);
                    match res {
                        Ok(_) => {}
                        Err(e) => match e {
                            rusb::Error::Timeout => {}
                            e @ _ => {
                                println!("IN thread: USB error: {e:?}");
                                return;
                            }
                        },
                    }
                    let unpacked = packager::unpackage_usb(buf);
                    let unpacked = match unpacked {
                        Some(x) => x,
                        None => {
                            continue;
                        }
                    };
                    received.extend(unpacked);
                }
                let resp = recv_rx.try_recv();
                let resp = match resp {
                    Ok(x) => x,
                    Err(e) => match e {
                        tokio::sync::mpsc::error::TryRecvError::Empty => {
                            thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        tokio::sync::mpsc::error::TryRecvError::Disconnected => {
                            println!("IN thread: response channel closed");
                            return;
                        }
                    },
                };
                response_buf.push_back(resp);
            }
        });
        Ok(USBBackend {
            close_chan,
            command_tx: cmd_tx,
        })
    }
    pub async fn new(selector: USBSelector) -> Result<Self, BackendError> {
        let x = tokio::task::spawn_blocking(|| Self::new_usb_backend_blocking(selector)).await?;
        x
    }

    pub async fn push(
        &self,
        cmd: Command,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<Command>> {
        self.command_tx.send(cmd).await
    }
}
impl Drop for USBBackend {
    fn drop(&mut self) {
        self.close_chan.send(()).ok();
    }
}
