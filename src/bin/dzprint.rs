// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::{thread, time::Duration};

use dz_print::command::{packager, variable_bytes::VariableBytesI32, Command, HostCommand};

#[derive(Debug)]
struct Endpoint {
    config: u8,
    iface: u8,
    setting: u8,
    address: u8,
}

fn main() {
    match rusb::Context::new() {
        Ok(mut ctx) => match open_device(&mut ctx, 0x3533, 0x5c15) {
            Some((mut device, device_desc, mut handle)) => {
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
                for n in 0..device_desc.num_configurations() {
                    let config_desc = match device.config_descriptor(n) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };

                    for interface in config_desc.interfaces() {
                        for interface_desc in interface.descriptors() {
                            for endpoint_desc in interface_desc.endpoint_descriptors() {
                                if endpoint_desc.direction() == rusb::Direction::In
                                    && endpoint_desc.transfer_type()
                                        == rusb::TransferType::Interrupt
                                {
                                    println!(
                                        "IN: config={} iface={} setting={}, address={} transfer={:?}",
                                        config_desc.number(),
                                        interface_desc.interface_number(),
                                        interface_desc.setting_number(),
                                        endpoint_desc.address(),
                                        endpoint_desc.transfer_type(),
                                    );

                                    in_ep = Endpoint {
                                        config: config_desc.number(),
                                        iface: interface_desc.interface_number(),
                                        setting: interface_desc.setting_number(),
                                        address: endpoint_desc.address(),
                                    };

                                    // return Some(Endpoint {
                                    //     config: config_desc.number(),
                                    //     iface: interface_desc.interface_number(),
                                    //     setting: interface_desc.setting_number(),
                                    //     address: endpoint_desc.address(),
                                    // });
                                } else if endpoint_desc.direction() == rusb::Direction::Out
                                    && endpoint_desc.transfer_type()
                                        == rusb::TransferType::Interrupt
                                {
                                    println!(
                                        "OUT: config={} iface={} setting={}, address={} transfer={:?}",
                                        config_desc.number(),
                                        interface_desc.interface_number(),
                                        interface_desc.setting_number(),
                                        endpoint_desc.address(),
                                        endpoint_desc.transfer_type(),
                                    );

                                    out_ep = Endpoint {
                                        config: config_desc.number(),
                                        iface: interface_desc.interface_number(),
                                        setting: interface_desc.setting_number(),
                                        address: endpoint_desc.address(),
                                    };

                                    // return Some(Endpoint {
                                    //     config: config_desc.number(),
                                    //     iface: interface_desc.interface_number(),
                                    //     setting: interface_desc.setting_number(),
                                    //     address: endpoint_desc.address(),
                                    // });
                                }
                            }
                        }
                    }
                }

                let has_kernel_driver = match handle.kernel_driver_active(out_ep.iface) {
                    Ok(true) => {
                        handle.detach_kernel_driver(out_ep.iface).ok();
                        true
                    }
                    _ => false,
                };

                println!("out - kernel driver? {}", has_kernel_driver);

                let has_kernel_driver = match handle.kernel_driver_active(in_ep.iface) {
                    Ok(true) => {
                        handle.detach_kernel_driver(in_ep.iface).ok();
                        true
                    }
                    _ => false,
                };

                println!("in - kernel driver? {}", has_kernel_driver);

                // thread::sleep(Duration::from_secs(1));

                // handle.set_active_configuration(out_ep.config).unwrap();
                // handle.reset().unwrap();
                handle.claim_interface(out_ep.iface).unwrap();
                handle
                    .set_alternate_setting(out_ep.iface, out_ep.setting)
                    .unwrap();
                handle.claim_interface(in_ep.iface).unwrap();
                handle
                    .set_alternate_setting(in_ep.iface, in_ep.setting)
                    .unwrap();

                let timeout = Duration::from_secs(1);

                let mut cmd_buf: Vec<u8> = Vec::new();
                // let cmd = Command::new_host(HostCommand::GetSetPrintDarkness);
                // let cmd = cmd.package(0x01.to_variable_bytes(), false);
                // cmd_buf.extend(&cmd);
                // let cmd = Command::new_host(HostCommand::GetSetPrintSpeed);
                // let cmd = cmd.package(0x00.to_variable_bytes(), false);
                // cmd_buf.extend(&cmd);
                let cmd = Command::new_host(HostCommand::Test);
                let cmd = cmd.package(vec![], false);
                cmd_buf.extend(&cmd);

                // cmd_buf.extend(&[0x1b, 0x40]);
                // // cmd_buf.extend(&[0x1b, 0x4a, 0xbf]);
                // cmd_buf.extend(&[0x1f, 0x2a, 0x08, 0x00, 0b00001111]);
                // cmd_buf.extend(&[0x1f, 0x2e, 0x55]);
                // cmd_buf.extend(&[0x1f, 0x2b, 0x01, 0x01, 0b11110000]);
                // cmd_buf.extend(&[0x1f, 0x2e, 0x55]);
                // cmd_buf.extend(&[0x0c]);

                // let cmd = Command::new_host(HostCommand::GetSetPrintDarkness);
                // let cmd = cmd.package(vec![], false);
                // cmd_buf.extend(&cmd);
                // let cmd = Command::new_host(HostCommand::ReadSoftwareVersion);
                // let cmd = cmd.package(vec![], false);
                // cmd_buf.extend(&cmd);
                let mut packed = packager::package_usb(cmd_buf);
                packed.resize(512, 0);

                handle
                    .write_interrupt(out_ep.address, &packed, timeout)
                    .unwrap();
                // ---
                // handle.set_active_configuration(in_ep.config).unwrap();
                // handle.claim_interface(in_ep.iface).unwrap();
                // handle
                //     .set_alternate_setting(in_ep.iface, in_ep.setting)
                //     .unwrap();
                let mut buf = [0; 64];
                let timeout = Duration::from_secs(1);

                handle
                    .read_interrupt(in_ep.address, &mut buf, timeout)
                    .unwrap();
                let buf = packager::unpackage_usb(buf.to_vec()).unwrap();

                println!("{:02x?}", buf);

                // ---

                // thread::sleep(Duration::from_secs(1));

                // handle.claim_interface(out_ep.iface).unwrap();
                // handle
                //     .set_alternate_setting(out_ep.iface, out_ep.setting)
                //     .unwrap();
                let timeout = Duration::from_secs(1);

                let mut cmd_buf: Vec<u8> = Vec::new();
                // let cmd = Command::new_host(HostCommand::Init);
                // let cmd = cmd.package(vec![], false);
                // cmd_buf.extend(&cmd);
                let cmd = Command::new_host(HostCommand::GetSetPrintDarkness);
                let cmd = cmd.package(vec![], false);
                cmd_buf.extend(&cmd);
                let mut packed = packager::package_usb(cmd_buf);
                packed.resize(512, 0);

                handle
                    .write_interrupt(out_ep.address, &packed, timeout)
                    .unwrap();
                // ---
                // handle.set_active_configuration(in_ep.config).unwrap();
                // handle.claim_interface(in_ep.iface).unwrap();
                // handle
                //     .set_alternate_setting(in_ep.iface, in_ep.setting)
                //     .unwrap();
                let mut buf = [0; 64];
                let timeout = Duration::from_secs(1);

                handle
                    .read_interrupt(in_ep.address, &mut buf, timeout)
                    .unwrap();
                let buf = packager::unpackage_usb(buf.to_vec()).unwrap();
                println!("{:02x?}", buf);
            }
            None => println!("could not find device"),
        },
        Err(e) => {
            panic!("{:?}", e)
        }
    }
}

fn open_device<T: rusb::UsbContext>(
    context: &mut T,
    vid: u16,
    pid: u16,
) -> Option<(
    rusb::Device<T>,
    rusb::DeviceDescriptor,
    rusb::DeviceHandle<T>,
)> {
    let devices = match context.devices() {
        Ok(d) => d,
        Err(_) => return None,
    };

    for device in devices.iter() {
        let device_desc = match device.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };

        if device_desc.vendor_id() == vid && device_desc.product_id() == pid {
            match device.open() {
                Ok(handle) => return Some((device, device_desc, handle)),
                Err(e) => panic!("Device found but failed to open: {}", e),
            }
        }
    }

    None
}
