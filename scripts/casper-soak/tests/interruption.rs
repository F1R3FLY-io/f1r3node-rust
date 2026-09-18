use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn interrupted_runner_cannot_publish_success() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    for (name, body) in [
        ("rustup", "#!/bin/sh\nexit 0\n"),
        ("cargo", "#!/bin/sh\nkill -HUP \"$PPID\"\nsleep 1\nexit 0\n"),
    ] {
        let path = bin.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let source = std::env::var_os("SOAK_SOURCE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf()
        });
    let output = root.path().join("results");
    let response = Command::new("bash")
        .arg(source.join("scripts/ci/check-casper-soak-bindings.sh"))
        .arg("--output")
        .arg(&output)
        .arg("--image")
        .arg(format!("sha256:{}", "0".repeat(64)))
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .output()
        .unwrap();
    let report = casper_soak::record(&output.join("report.json")).unwrap();
    assert_eq!(
        report["status"], "failed",
        "An interrupted runner published success."
    );
    assert_ne!(report["exit_code"], 0);
    assert!(!response.status.success());
}
