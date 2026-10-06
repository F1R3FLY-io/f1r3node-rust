use std::path::PathBuf;

use clap::{Parser, Subcommand};
use eyre::Result;
use supply_chain::{install, policy, scan, SystemRunner};

#[derive(Parser)]
struct Arguments {
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Operation,
}

#[derive(Subcommand)]
enum Operation {
    Install {
        #[arg(long)]
        install_dir: PathBuf,
    },
    Check {
        #[arg(long, default_value = "cargo-deny")]
        cargo_deny: String,
    },
}

fn run() -> Result<bool> {
    let args = Arguments::parse();
    let root = args
        .root
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .canonicalize()?;
    let policy = policy::load(&root.join("supply-chain/policy.toml"))?;
    match args.command {
        Operation::Install { install_dir } => {
            install::scanner(&policy, &install_dir)?;
            Ok(true)
        }
        Operation::Check { cargo_deny } => scan::check(&root, &policy, &cargo_deny, &SystemRunner),
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Supply-chain check failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
