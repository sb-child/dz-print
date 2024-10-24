// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::{collections::VecDeque, time::Duration};

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
/// Created -> Sent/Errored -> Received(CommandPayload)
pub type CommandResultWithResponse =
    tokio::sync::oneshot::Receiver<Option<tokio::sync::oneshot::Receiver<CommandPayload>>>;
/// Created -> Sent/Errored
pub type CommandResultWithoutResponse = tokio::sync::oneshot::Receiver<bool>;
/// Created -> Sent/Errored -> Received(CommandPayload)
pub type CommandResultSenderWithResponse =
    tokio::sync::oneshot::Sender<Option<tokio::sync::oneshot::Receiver<CommandPayload>>>;
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
        let ctx1 = ctx.clone();
        let ctx2 = ctx.clone();
        let device1 = device.clone();
        let device2 = device.clone();
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(1);
        let max_in_size = 61;
        let max_out_size = 61;
        // OUT thread, send data to device
        tokio::task::spawn_blocking(move || {
            let mut packet_buf = VecDeque::new();
            let mut response_sender_buf = VecDeque::new();
            let mut sender_buf = VecDeque::new();
            let mut raw_packet_len = 0;

            // let mut pending
            loop {
                if !close_sig_1.is_empty() {
                    close_sig_1.blocking_recv().ok();
                    return;
                }
                assert_eq!(
                    response_sender_buf.len() + sender_buf.len(),
                    packet_buf.len()
                );

                if raw_packet_len >= max_out_size {
                    // let buf = Vec::with_capacity(max_out_size);
                }

                let cmd = cmd_rx.try_recv();
                let cmd = match cmd {
                    Ok(x) => x,
                    Err(e) => match e {
                        tokio::sync::mpsc::error::TryRecvError::Empty => {
                            continue;
                        }
                        tokio::sync::mpsc::error::TryRecvError::Disconnected => {
                            return;
                        }
                    },
                };
                match cmd {
                    Command::WithResponse(p, sender) => {
                        let packet_len = p.len();
                        raw_packet_len += packet_len;
                        packet_buf.push_back(p);
                        response_sender_buf.push_back(sender);
                    }
                    Command::WithoutResponse(p, sender) => {
                        let packet_len = p.len();
                        raw_packet_len += packet_len;
                        packet_buf.push_back(p);
                        sender_buf.push_back(sender);
                    }
                }
            }
            close_sig_1.is_empty();
        });
        // IN thread, receive data from device
        tokio::task::spawn_blocking(move || {
            close_sig_2.is_empty();
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
}
impl Drop for USBBackend {
    fn drop(&mut self) {
        self.close_chan.send(()).ok();
    }
}
