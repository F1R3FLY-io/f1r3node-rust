use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use eyre::{ensure, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

mod bindings;
mod models;
#[cfg(test)]
mod tests;

const AREA: &str = "formal/tlaplus/node_observation";
const TOOL_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "src/main.rs",
    "src/models.rs",
    "src/bindings.rs",
    "src/tests.rs",
];

#[derive(Parser)]
struct Args {
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    Models {
        #[arg(long)]
        jar: PathBuf,
        #[arg(long, default_value = "java")]
        java: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Bindings {
        #[arg(long)]
        isolated_manifest: PathBuf,
        #[arg(long)]
        rust_log: PathBuf,
        #[arg(long)]
        models: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}

fn digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        hash.update(&buffer[..length]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn tool_sources(root: &Path) -> Result<BTreeMap<String, String>> {
    TOOL_FILES
        .iter()
        .map(|file| {
            let name = format!("scripts/node-observation/{file}");
            Ok((name.clone(), digest(&root.join(name))?))
        })
        .collect()
}

fn check_hashes(root: &Path, sources: &BTreeMap<String, String>) -> Result<()> {
    ensure!(!sources.is_empty(), "The source inventory is empty.");
    let root = root.canonicalize()?;
    for (name, expected) in sources {
        let path = root.join(name).canonicalize()?;
        ensure!(path.starts_with(&root), "Source outside root: {name}");
        ensure!(digest(&path)? == *expected, "Source mismatch: {name}");
    }
    Ok(())
}

fn run() -> Result<()> {
    let args = Args::parse();
    let root = args.root.canonicalize()?;
    match args.command {
        Action::Models { jar, java, output } => models::run(&root, &jar, &java, &output),
        Action::Bindings {
            isolated_manifest,
            rust_log,
            models,
            output,
        } => bindings::run(&root, &isolated_manifest, &rust_log, &models, &output),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("FAIL: {error:#}");
        std::process::exit(1);
    }
}
