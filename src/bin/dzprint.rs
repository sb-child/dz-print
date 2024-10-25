// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use dz_print::{
    backend,
    command::{self, HostCommand},
    image_proc::{
        cmd_parser::{BitmapParser, PrintCommand},
        Bitmap,
    },
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    main_fn().await
}

async fn main_fn() -> anyhow::Result<()> {
    let b = backend::USBBackend::new(backend::USBSelector::DeviceSerial(
        "DP27P-Y4094C023".to_string(),
    ))
    .await?;

    let png_img = image::ImageReader::open("/home/sbchild/test.png").unwrap();
    let png_img = png_img.decode().unwrap();
    let png_img = png_img.into_luma8();
    let bitmap = Bitmap::from_gray_image(&png_img);
    // 断点:
    // 最慢 50
    // 较慢 75
    // 正常 100
    // 较快 110
    // 最快 120
    let parser = BitmapParser::new(bitmap, 120);

    let (cmd, chan) = backend::Command::without_response(
        command::Command::new_host(HostCommand::GetSetPrintDarkness).package(vec![0x00], false),
    );
    b.push(cmd).await.ok();
    chan.await.ok();

    let (cmd, chan) = backend::Command::without_response(
        command::Command::new_host(HostCommand::GetSetPrintSpeed).package(vec![0x04], false),
    );
    b.push(cmd).await.ok();
    chan.await.ok();

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

    let (cmd, chan) = backend::Command::with_response(
        command::Command::new_host(HostCommand::EnableHighCommand).package(vec![0x7f], false),
    );
    b.push(cmd).await.ok();
    println!("Enable high command");
    let chan = chan.await?;
    if let Some(chan) = chan {
        let resp = chan.await?;
        println!("Received: {:?}", resp.get_command());
    } else {
        println!("Failed");
    }

    let (cmd, _) = backend::Command::without_response(
        PrintCommand::ResetPrinter
            .parse()
            .unwrap()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
    );
    b.push(cmd).await.ok();

    for c in parser {
        if let Some(c) = c.parse() {
            for c in c {
                let (cmd, ch) = backend::Command::without_response(c);
                b.push(cmd).await.ok();
                // ch.await.ok();
            }
        } else {
            // let (cmd1, chan1) = backend::Command::with_response(
            //     command::Command::new_host(HostCommand::GetPrinterStatus).package(vec![], false),
            // );
            let (cmd2, chan2) = backend::Command::with_response(
                command::Command::new_host(HostCommand::GetSensorStatus).package(vec![0x01], false),
            );
            // b.push(cmd1).await.ok();
            b.push(cmd2).await.ok();
            // let chan = chan1.await?;
            // if let Some(chan) = chan {
            //     let resp = chan.await?;
            //     println!("Received: {:?}", resp.get_command());
            // } else {
            //     println!("Failed");
            // }
            let chan = chan2.await?;
            if let Some(chan) = chan {
                let resp = chan.await?;
                let payload = resp.get_payload();
                let temp1 = u16::from_be_bytes([payload[1], payload[2]]);
                let temp2 = u16::from_be_bytes([payload[3], payload[4]]);
                let temp3 = u16::from_be_bytes([payload[5], payload[6]]);
                let temp4 = u16::from_be_bytes([payload[7], payload[8]]);
                println!(
                    "Received: {:?} 1={} 2={} 3={} 4={}",
                    resp.get_command(),
                    temp1,
                    temp2,
                    temp3,
                    temp4
                );
            } else {
                println!("Failed");
            }
        }
    }

    let (cmd, _) = backend::Command::without_response(
        PrintCommand::FeedLines(2)
            .parse()
            .unwrap()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
    );
    b.push(cmd).await.ok();

    let (cmd, _) = backend::Command::without_response(
        PrintCommand::NextPaper
            .parse()
            .unwrap()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
    );
    b.push(cmd).await.ok();

    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    return Ok(());
}
