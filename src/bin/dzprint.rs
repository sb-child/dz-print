// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::{thread, time::Duration};

use dz_print::{
    backend,
    command::{self, packager, variable_bytes::VariableBytesI32, Command, HostCommand},
    image_proc::{
        cmd_parser::{BitmapParser, PrintCommand},
        Bitmap,
    },
};

#[derive(Debug)]
struct Endpoint {
    config: u8,
    iface: u8,
    setting: u8,
    address: u8,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    main_fn().await
}

async fn main_fn() -> anyhow::Result<()> {
    let b = backend::USBBackend::new(backend::USBSelector::DeviceSerial(
        "DP27P-Y4094C023".to_string(),
    ))
    .await?;

    let png_img = image::ImageReader::open("/home/sbchild/1024.png").unwrap();
    let png_img = png_img.decode().unwrap();
    let png_img = png_img.into_luma8();
    let bitmap = Bitmap::from_gray_image(&png_img);
    let parser = BitmapParser::new(bitmap);

    // let cmds: Vec<u8> = parser.map(|x| x.parse()).flatten().flatten().collect();

    // cmd_buf.extend(PrintCommand::ResetPrinter.parse().iter().flatten());
    // cmd_buf.extend(PrintCommand::FeedLines(2).parse().iter().flatten());
    // cmd_buf.extend(&cmds);
    // // cmd_buf.extend(PrintCommand::NextPaper.parse().iter().flatten());
    // // cmd_buf.extend(PrintCommand::FeedLines(2).parse().iter().flatten());
    // // cmd_buf.extend(&cmds);
    // cmd_buf.extend(PrintCommand::NextPaper.parse().iter().flatten());
    // println!("len={}", cmd_buf.len());

    // let (cmd, chan) = backend::Command::without_response(
    //     PrintCommand::ResetPrinter
    //         .parse()
    //         .into_iter()
    //         .flatten()
    //         .collect::<Vec<_>>(),
    // );
    // b.push(cmd).await.ok();
    // println!("reset: {}", chan.await?);

    let (cmd, chan) = backend::Command::with_response(
        command::Command::new_host(HostCommand::GetPrinterStatus).package(vec![], false),
    );
    b.push(cmd).await.ok();
    println!("pushed");
    let chan = chan.await?;
    if let Some(chan) = chan {
        println!("waiting for response");
        let resp = chan.await?;

        println!("received: {:?}", resp.get_command());
    } else {
        println!("failed");
    }

    let (cmd, _) = backend::Command::without_response(
        PrintCommand::ResetPrinter
            .parse()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
    );
    b.push(cmd).await.ok();

    let mut x = 0;

    for c in parser {
        for c in c.parse() {
            let (cmd, _) = backend::Command::without_response(c);
            b.push(cmd).await.ok();
        }
        x += 1;
        if x % 10 == 0 {
            let (cmd, chan) = backend::Command::with_response(
                command::Command::new_host(HostCommand::GetPrinterStatus).package(vec![], false),
            );
            b.push(cmd).await.ok();
            println!("Request for printer status");
            let chan = chan.await?;
            if let Some(chan) = chan {
                let resp = chan.await?;
                println!("Received: {:?}", resp.get_command());
            } else {
                println!("Failed");
            }
        }
    }

    let (cmd, _) = backend::Command::without_response(
        PrintCommand::FeedLines(2)
            .parse()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
    );
    b.push(cmd).await.ok();

    let (cmd, _) = backend::Command::without_response(
        PrintCommand::NextPaper
            .parse()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
    );
    b.push(cmd).await.ok();

    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    return Ok(());
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
                handle.reset().unwrap();

                handle.claim_interface(out_ep.iface).unwrap();
                handle
                    .set_alternate_setting(out_ep.iface, out_ep.setting)
                    .unwrap();
                handle.claim_interface(in_ep.iface).unwrap();
                handle
                    .set_alternate_setting(in_ep.iface, in_ep.setting)
                    .unwrap();

                // let timeout = Duration::from_secs(1);
                // let mut cmd_buf: Vec<u8> = Vec::new();
                // cmd_buf.extend(&[0x1b, 0x4a]);
                // for p in cmd_buf.chunks(61) {
                //     let packed = packager::package_usb(p.to_vec());
                //     handle
                //         .write_interrupt(out_ep.address, &packed, timeout)
                //         .unwrap();
                //     thread::sleep(Duration::from_millis(1));
                // }
                // thread::sleep(Duration::from_millis(500));

                let timeout = Duration::from_secs(1);

                let mut cmd_buf: Vec<u8> = Vec::new();
                // let cmd = Command::new_host(HostCommand::Init);
                // let cmd = cmd.package(vec![], false);
                // cmd_buf.extend(&cmd);

                // let cmd = Command::new_host(HostCommand::GetSetPrintPaperType);
                // let cmd = cmd.package(vec![0x00], false);
                // cmd_buf.extend(&cmd);
                // let cmd = Command::new_host(HostCommand::GetSetPrintDarkness);
                // let cmd = cmd.package(vec![0x01], false);
                // cmd_buf.extend(&cmd);
                // let cmd = Command::new_host(HostCommand::GetSetPrintSpeed);
                // let cmd = cmd.package(vec![0x00], false);
                // cmd_buf.extend(&cmd);

                let cmd = Command::new_host(HostCommand::Test2);
                let cmd = cmd.package(vec![0x7f], false);
                cmd_buf.extend(&cmd);
                // let cmd = Command::new_host(HostCommand::Test);
                // let cmd = cmd.package(vec![], false);
                // cmd_buf.extend(&cmd);

                let mut packed = packager::package_usb(cmd_buf);
                packed.resize(64, 0);

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

                // return Ok(());

                let timeout = Duration::from_secs(1);

                let mut cmd_buf: Vec<u8> = Vec::new();
                // let cmd = Command::new_host(HostCommand::GetSetPrintDarkness);
                // let cmd = cmd.package(0x01.to_variable_bytes(), false);
                // cmd_buf.extend(&cmd);
                // let cmd = Command::new_host(HostCommand::GetSetPrintSpeed);
                // let cmd = cmd.package(0x00.to_variable_bytes(), false);
                // cmd_buf.extend(&cmd);

                // let cmd = Command::new_host(HostCommand::Test);
                // let cmd = cmd.package(vec![], false);
                // cmd_buf.extend(&cmd);

                let png_img = image::ImageReader::open("/home/sbchild/yndtk.png").unwrap();
                let png_img = png_img.decode().unwrap();
                let png_img = png_img.into_luma8();
                let bitmap = Bitmap::from_gray_image(&png_img);
                let parser = BitmapParser::new(bitmap);
                let cmds: Vec<u8> = parser.map(|x| x.parse()).flatten().flatten().collect();

                cmd_buf.extend(PrintCommand::ResetPrinter.parse().iter().flatten());
                cmd_buf.extend(PrintCommand::FeedLines(2).parse().iter().flatten());
                cmd_buf.extend(&cmds);
                // cmd_buf.extend(PrintCommand::NextPaper.parse().iter().flatten());
                // cmd_buf.extend(PrintCommand::FeedLines(2).parse().iter().flatten());
                // cmd_buf.extend(&cmds);
                cmd_buf.extend(PrintCommand::NextPaper.parse().iter().flatten());
                println!("len={}", cmd_buf.len());
                // println!("{:02x?}", cmd_buf);
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
                for p in cmd_buf.chunks(1024) {
                    for p in p.chunks(62) {
                        let packed = packager::package_usb(p.to_vec());
                        handle
                            .write_interrupt(out_ep.address, &packed, timeout)
                            .unwrap();
                    }
                    thread::sleep(Duration::from_millis(80));
                }

                // packed.resize(64, 0);

                // handle
                //     .write_interrupt(out_ep.address, &packed, timeout)
                //     .unwrap();

                // ---
                // handle.set_active_configuration(in_ep.config).unwrap();
                // handle.claim_interface(in_ep.iface).unwrap();
                // handle
                //     .set_alternate_setting(in_ep.iface, in_ep.setting)
                //     .unwrap();

                // let mut buf = [0; 64];
                // let timeout = Duration::from_secs(1);

                // handle
                //     .read_interrupt(in_ep.address, &mut buf, timeout)
                //     .unwrap();
                // let buf = packager::unpackage_usb(buf.to_vec()).unwrap();

                // println!("{:02x?}", buf);

                // ---

                // thread::sleep(Duration::from_secs(1));

                // handle.claim_interface(out_ep.iface).unwrap();
                // handle
                //     .set_alternate_setting(out_ep.iface, out_ep.setting)
                //     .unwrap();
            }
            None => println!("could not find device"),
        },
        Err(e) => {
            panic!("{:?}", e)
        }
    }
    return Ok(());
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
