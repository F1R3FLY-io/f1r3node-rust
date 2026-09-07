use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use eyre::{ensure, eyre, Result};
use flate2::read::GzDecoder;

use crate::digest;
use crate::policy::Policy;

pub fn target(architecture: &str, system: &str) -> Result<String> {
    let arch = match architecture {
        "aarch64" | "arm64" => "aarch64",
        "x86_64" | "AMD64" => "x86_64",
        _ => return Err(eyre!("Unsupported scanner architecture: {architecture}")),
    };
    let os = match system {
        "macos" => "apple-darwin",
        "linux" => "unknown-linux-musl",
        _ => return Err(eyre!("Unsupported scanner operating system: {system}")),
    };
    Ok(format!("{arch}-{os}"))
}

pub fn archive(bytes: &[u8], expected: &str, destination: &Path) -> Result<()> {
    ensure!(
        digest(bytes) == expected,
        "The cargo-deny archive checksum does not match the policy."
    );
    let mut tar = tar::Archive::new(GzDecoder::new(bytes));
    let mut executable = None;
    for entry in tar.entries()? {
        let mut entry = entry?;
        if entry.header().entry_type().is_file()
            && entry
                .path()?
                .file_name()
                .is_some_and(|name| name == "cargo-deny")
        {
            ensure!(
                executable.is_none(),
                "The cargo-deny archive must contain exactly one executable."
            );
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            executable = Some(data);
        }
    }
    let executable = executable
        .ok_or_else(|| eyre!("The cargo-deny archive must contain exactly one executable."))?;
    fs::create_dir_all(destination)?;
    let mut temporary = tempfile::NamedTempFile::new_in(destination)?;
    temporary.write_all(&executable)?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o755))?;
    temporary.persist(destination.join("cargo-deny"))?;
    Ok(())
}

pub fn scanner(policy: &Policy, destination: &Path) -> Result<()> {
    let target = target(std::env::consts::ARCH, std::env::consts::OS)?;
    let version = &policy.scanner.version;
    semver::Version::parse(version)?;
    let expected = policy
        .scanner
        .sha256
        .get(&target)
        .ok_or_else(|| eyre!("No scanner checksum for {target}."))?;
    let url = format!("https://github.com/EmbarkStudios/cargo-deny/releases/download/{version}/cargo-deny-{version}-{target}.tar.gz");
    let response = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "120",
            &url,
        ])
        .output()?;
    ensure!(
        response.status.success(),
        "Scanner download failed: {}",
        String::from_utf8_lossy(&response.stderr)
    );
    archive(&response.stdout, expected, destination)
}
