#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Maintainer-only appd build tasks.

mod builder;
mod layout;
mod support;

use std::process::ExitCode;

use anyhow::Result;
use appd_cli::Target;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "appd maintainer tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Build a target pack from the current appd workspace.
    TargetPack {
        /// Runtime target to build.
        #[arg(long)]
        target: Target,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::TargetPack { target } => {
            let manifest = builder::build_source_target_pack(target)?;
            println!("Built target pack: {}", manifest.display());
        }
    }
    Ok(())
}
