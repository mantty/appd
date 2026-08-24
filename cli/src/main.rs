#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! `appd` command line entry point.

mod build;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
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
    /// Build a native app bundle for a platform.
    Build {
        /// Platform family to build.
        platform: Option<Platform>,
        /// Comma-separated platforms to build.
        #[arg(long, value_delimiter = ',')]
        platforms: Vec<Platform>,
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
            platform,
            platforms,
            project,
            target_pack,
            config,
            skip_web_build,
        } => {
            let platforms = selected_build_platforms(platform, platforms)?;
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

fn selected_build_platforms(
    platform: Option<Platform>,
    platforms: Vec<Platform>,
) -> Result<Vec<Platform>> {
    match (platform, platforms.is_empty()) {
        (Some(platform), true) => Ok(vec![platform]),
        (None, false) => Ok(platforms),
        (Some(_), false) => bail!("pass either a positional platform or --platforms, not both"),
        (None, true) => {
            bail!("build requires a platform; use appd build macos or --platforms=macos")
        }
    }
}

fn list_targets() {
    for target in Target::ALL {
        println!("{target}");
    }
}

#[cfg(test)]
mod tests {
    use super::{Platform, selected_build_platforms};

    #[test]
    fn selects_a_positional_platform() {
        assert!(matches!(
            selected_build_platforms(Some(Platform::Macos), Vec::new()),
            Ok(platforms) if platforms == vec![Platform::Macos]
        ));
    }

    #[test]
    fn rejects_mixed_platform_argument_forms() {
        assert!(selected_build_platforms(Some(Platform::Macos), vec![Platform::Ios]).is_err());
    }
}
