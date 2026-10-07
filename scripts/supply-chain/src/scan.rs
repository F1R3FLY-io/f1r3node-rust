use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};

use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

use crate::policy::{self, Policy};
use crate::report::{self, Report};
use crate::{file_digest, successful, Runner, CHECKS};

pub fn validate_manifests(root: &Path, manifests: &[String], runner: &impl Runner) -> Result<()> {
    let tracked = successful(runner.output(root, "git", &[
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "-z",
    ])?)?;
    let root = root.canonicalize()?;
    let mut discovered = BTreeSet::new();
    for name in tracked
        .split('\0')
        .filter(|n| Path::new(n).file_name().is_some_and(|f| f == "Cargo.toml"))
    {
        let located = successful(runner.output(&root, "cargo", &[
            "locate-project",
            "--workspace",
            "--manifest-path",
            name,
            "--message-format",
            "plain",
        ])?)?;
        let path = Path::new(located.trim()).canonicalize()?;
        discovered.insert(
            path.strip_prefix(&root)?
                .to_str()
                .ok_or_else(|| eyre!("The manifest path is not UTF-8."))?
                .to_owned(),
        );
    }
    let expected: BTreeSet<_> = manifests.iter().cloned().collect();
    ensure!(
        expected.len() == manifests.len() && expected == discovered,
        "The scan manifest list must cover every Cargo workspace exactly once."
    );
    for manifest in manifests {
        let path = Path::new(manifest);
        let lock = path.with_file_name("Cargo.lock");
        ensure!(
            !path.is_absolute()
                && !path.components().any(|p| matches!(p, Component::ParentDir))
                && root.join(&lock).is_file(),
            "{manifest}: a repository manifest and lockfile are required."
        );
        let ignored = runner.output(&root, "git", &[
            "check-ignore",
            "-q",
            lock.to_str()
                .ok_or_else(|| eyre!("Invalid lockfile path."))?,
        ])?;
        ensure!(
            ignored.status.code() == Some(1),
            "{}: the lockfile must not be ignored.",
            lock.display()
        );
    }
    Ok(())
}

pub fn validate_metadata(metadata: &Value) -> Result<()> {
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| eyre!("Cargo metadata requires a package list."))?;
    let revision = regex::Regex::new(r"[?&]rev=([0-9a-f]{40})#([0-9a-f]{40})$")?;
    for package in packages {
        ensure!(
            package.is_object(),
            "Cargo metadata contains an invalid package."
        );
        match package.get("source") {
            None | Some(Value::Null) => (),
            Some(Value::String(source)) if source.starts_with("git+") => {
                ensure!(
                    revision.captures(source).is_some_and(|m| m[1] == m[2]),
                    "Git sources require a full, matching commit revision."
                );
            }
            Some(Value::String(_)) => (),
            _ => return Err(eyre!("Cargo metadata contains an invalid package source.")),
        }
    }
    Ok(())
}

fn prefix(manifest: &str) -> String {
    if manifest == "Cargo.toml" {
        "workspace".into()
    } else {
        manifest.trim_end_matches("/Cargo.toml").replace('/', "-")
    }
}

fn scan_manifest(
    root: &Path,
    name: &str,
    tool: &str,
    reports: &Path,
    policy: &Policy,
    runner: &impl Runner,
) -> Result<Report> {
    let lock = root.join(name).with_file_name("Cargo.lock");
    let hash = file_digest(&lock)?;
    let prefix = prefix(name);
    let metadata = runner.output(root, "cargo", &[
        "metadata",
        "--locked",
        "--all-features",
        "--format-version",
        "1",
        "--manifest-path",
        name,
    ])?;
    fs::write(
        reports.join(format!("{prefix}-metadata.json")),
        &metadata.stdout,
    )?;
    fs::write(
        reports.join(format!("{prefix}-metadata.stderr")),
        &metadata.stderr,
    )?;
    let parsed = serde_json::from_str::<Value>(&successful(metadata)?)?;
    validate_metadata(&parsed)?;
    let mut args = vec![
        "--log-level",
        "info",
        "--format",
        "json",
        "--locked",
        "--all-features",
        "--manifest-path",
        name,
        "--config",
        "deny.toml",
        "check",
        "--show-stats",
    ];
    args.extend(CHECKS);
    let output = runner.output(root, tool, &args)?;
    let mut bytes = output.stdout;
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    bytes.extend(output.stderr);
    fs::write(reports.join(format!("{prefix}-deny.jsonl")), &bytes)?;
    let mut report = report::validate(std::str::from_utf8(&bytes)?, &policy.exceptions);
    if !output.status.success() {
        report
            .errors
            .push(format!("cargo-deny exited with status {}.", output.status));
    }
    if file_digest(&lock)? != hash {
        report.errors.push("The scan changed Cargo.lock.".into());
    }
    let evidence = json!({
        "manifest": name,
        "lock_sha256": hash,
        "policy_sha256": file_digest(&root.join("deny.toml"))?,
        "controls_sha256": file_digest(&root.join("supply-chain/policy.toml"))?,
        "metadata_sha256": file_digest(&reports.join(format!("{prefix}-metadata.json")))?,
        "scanner_exit": output.status.code(),
        "errors": report.errors,
    });
    fs::write(
        reports.join(format!("{prefix}-summary.json")),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    Ok(report)
}

pub fn scan_all(root: &Path, policy: &Policy, tool: &str, runner: &impl Runner) -> Result<bool> {
    let reports = root.join("target/supply-chain");
    fs::create_dir_all(&reports)?;
    let mut all = Report::default();
    for manifest in &policy.manifests {
        let report = match scan_manifest(root, manifest, tool, &reports, policy, runner) {
            Ok(report) => report,
            Err(error) => {
                let message = format!("{manifest}: {error}");
                let evidence = json!({"manifest": manifest, "errors": [&message]});
                fs::write(
                    reports.join(format!("{}-summary.json", prefix(manifest))),
                    serde_json::to_vec_pretty(&evidence)?,
                )?;
                Report {
                    errors: vec![message],
                    ..Report::default()
                }
            }
        };
        for error in &report.errors {
            eprintln!("{manifest}: {error}");
        }
        println!(
            "{manifest}: {}",
            if report.errors.is_empty() {
                "PASS"
            } else {
                "FAIL"
            }
        );
        all.errors.extend(report.errors);
        all.encountered.extend(report.encountered);
    }
    let unused: Vec<_> = policy
        .exceptions
        .keys()
        .filter(|id| !all.encountered.contains(*id))
        .cloned()
        .collect();
    if !unused.is_empty() {
        let message = format!("Unused advisory exceptions: {}", unused.join(", "));
        eprintln!("{message}");
        all.errors.push(message);
    }
    Ok(all.errors.is_empty())
}

pub fn check(root: &Path, policy: &Policy, tool: &str, runner: &impl Runner) -> Result<bool> {
    let version = successful(runner.output(root, tool, &["--version"])?)?;
    ensure!(
        version.trim() == format!("cargo-deny {}", policy.scanner.version),
        "Install the cargo-deny version specified in supply-chain/policy.toml."
    );
    let deny = toml::from_str(&fs::read_to_string(root.join("deny.toml"))?)?;
    policy::validate(policy, &deny, chrono::Utc::now().date_naive())?;
    validate_manifests(root, &policy.manifests, runner)?;
    scan_all(root, policy, tool, runner)
}
