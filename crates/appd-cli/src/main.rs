#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! `appd` command line entry point.

mod build;
mod target_packs;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use appd_target_pack::{Target, load_manifest};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "appd", version, about = "appd cross-platform app tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Build a native app bundle for a platform.
    Build {
        /// Platform family to build.
        platform: Option<BuildPlatform>,
        /// Comma-separated platforms to build.
        #[arg(long, value_delimiter = ',')]
        platforms: Vec<BuildPlatform>,
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
    /// Inspect and validate target-pack metadata.
    Pack {
        #[command(subcommand)]
        command: PackCommand,
    },
}

#[derive(Debug, Subcommand)]
enum PackCommand {
    /// Inspect a target-pack manifest.
    Inspect {
        /// Path to target-pack.json.
        manifest: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BuildPlatform {
    Ios,
    IosSimulator,
    Android,
    Macos,
    Windows,
    Linux,
}

impl BuildPlatform {
    fn display_name(self) -> &'static str {
        match self {
            Self::Ios => "iOS",
            Self::IosSimulator => "iOS Simulator",
            Self::Android => "Android",
            Self::Macos => "macOS",
            Self::Windows => "Windows",
            Self::Linux => "Linux",
        }
    }

    fn build_dir_name(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::IosSimulator => "ios-simulator",
            Self::Android => "android",
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }
}

impl FromStr for BuildPlatform {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ios" => Ok(Self::Ios),
            "ios-simulator" => Ok(Self::IosSimulator),
            "android" => Ok(Self::Android),
            "macos" => Ok(Self::Macos),
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            _ => {
                bail!(
                    "unknown platform '{value}'; expected ios, ios-simulator, android, macos, windows, or linux"
                )
            }
        }
    }
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
        Command::Pack {
            command: PackCommand::Inspect { manifest },
        } => inspect_pack(&manifest),
    }
}

fn selected_build_platforms(
    platform: Option<BuildPlatform>,
    platforms: Vec<BuildPlatform>,
) -> Result<Vec<BuildPlatform>> {
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

fn inspect_pack(manifest_path: &Path) -> Result<()> {
    let manifest = load_manifest(manifest_path)
        .with_context(|| format!("invalid target pack: {}", manifest_path.display()))?;

    println!("target: {}", manifest.target);
    println!("appd version: {}", manifest.appd_version);
    println!("artifacts: {}", manifest.artifacts.len());

    if manifest.required_tools.is_empty() {
        println!("required tools: none");
    } else {
        println!("required tools: {}", manifest.required_tools.join(", "));
    }

    Ok(())
}
