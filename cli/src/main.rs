#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! `appd` command line entry point.

mod build;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use target_pack_format::{Platform, Target};

#[derive(Debug, Parser)]
#[command(name = "appd", version, about = "appd native app tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Build native app bundles for one or more platforms.
    Build {
        /// Comma-separated platform families to build.
        platforms: String,
        /// App project directory.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Path to target-pack.json for the runtime artifacts.
        #[arg(long = "target-pack")]
        target_pack: Option<PathBuf>,
        /// Path to the Wrangler configuration file.
        #[arg(short = 'c', long = "config")]
        config: Option<PathBuf>,
        /// Reuse an existing dist/ directory instead of running the web build.
        #[arg(long)]
        skip_web_build: bool,
    },
    /// List runtime targets supported by this CLI.
    Targets,
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
        Command::Build {
            platforms,
            project,
            target_pack,
            config,
            skip_web_build,
        } => {
            let platforms = parse_platforms(&platforms)?;
            let summaries = build::run(&build::BuildRequest {
                platforms,
                project_dir: project,
                target_pack_manifest: target_pack,
                config_path: config,
                skip_web_build,
            })?;
            for summary in summaries {
                println!(
                    "Built {} bundle: {}",
                    summary.platform.display_name(),
                    summary.bundle_dir.display()
                );
            }
            Ok(())
        }
        Command::Targets => {
            list_targets();
            Ok(())
        }
    }
}

fn parse_platforms(value: &str) -> Result<Vec<Platform>> {
    value
        .split(',')
        .map(|platform| platform.parse().map_err(anyhow::Error::from))
        .collect()
}

fn list_targets() {
    for target in Target::ALL {
        println!("{target}");
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command, Platform, parse_platforms};

    #[test]
    fn parses_comma_separated_platforms() {
        assert!(matches!(
            parse_platforms("macos,android"),
            Ok(platforms) if platforms == vec![Platform::Macos, Platform::Android]
        ));
    }

    #[test]
    fn accepts_one_comma_separated_platform_argument() {
        assert!(matches!(
            Cli::try_parse_from(["appd", "build", "macos,android"]),
            Ok(Cli { command: Command::Build { platforms, .. } })
                if platforms == "macos,android"
        ));
    }

    #[test]
    fn rejects_multiple_positional_platform_arguments() {
        assert!(Cli::try_parse_from(["appd", "build", "macos", "android"]).is_err());
    }

    #[test]
    fn rejects_the_removed_platforms_flag() {
        assert!(Cli::try_parse_from(["appd", "build", "--platforms=macos"]).is_err());
    }
}
