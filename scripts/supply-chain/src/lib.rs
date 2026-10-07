pub mod install;
pub mod policy;
pub mod report;
pub mod scan;

use std::path::Path;
use std::process::{Command, Output};

use eyre::{ensure, Result};
use sha2::{Digest, Sha256};

pub const CHECKS: [&str; 4] = ["advisories", "bans", "licenses", "sources"];

pub fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

pub fn file_digest(path: &Path) -> Result<String> { Ok(digest(&std::fs::read(path)?)) }

pub trait Runner {
    fn output(&self, root: &Path, program: &str, args: &[&str]) -> Result<Output>;
}

pub struct SystemRunner;

impl Runner for SystemRunner {
    fn output(&self, root: &Path, program: &str, args: &[&str]) -> Result<Output> {
        Ok(Command::new(program)
            .args(args)
            .current_dir(root)
            .output()?)
    }
}

pub fn successful(output: Output) -> Result<String> {
    ensure!(
        output.status.success(),
        "Command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}
