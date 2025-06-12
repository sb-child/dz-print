// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(version, about = "dz-print Command-Line Interface")]
struct CommandArgs {
    #[command(flatten)]
    selector: SelectorArgs,

    #[command(subcommand)]
    command: Subcommands,
}

#[derive(Args, Debug)]
struct SelectorArgs {
    /// (selector) Serial Number
    #[arg(long)]
    sn: Option<String>,

    /// (selector) USB VID
    #[arg(long)]
    vid: Option<String>,

    /// (selector) USB PID
    #[arg(long)]
    pid: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Subcommands {
    /// Get/Set printer settings
    Config,

    /// Print a document
    Print,

    /// List all printers avaliable
    List,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = CommandArgs::parse();
    main_fn().await
}

async fn main_fn() -> anyhow::Result<()> {
    Ok(())
}
