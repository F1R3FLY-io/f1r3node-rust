use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn timed_out_container_is_captured_before_removal() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let worker = bin.join("worker");
    fs::write(&worker, "fixture").unwrap();
    fs::set_permissions(&worker, fs::Permissions::from_mode(0o755)).unwrap();
    let mut build = String::new();
    for (name, test) in [
        ("casper-soak", false),
        ("check-casper-bindings", false),
        ("check-casper-claims", false),
        ("manifest", true),
        ("models", true),
        ("bindings", true),
        ("interruption", true),
        ("claims", true),
        ("driver", true),
    ] {
        build.push_str(&format!("{}\n", serde_json::json!({"reason":"compiler-artifact","target":{"name":name},"profile":{"test":test},"executable":worker})));
    }
    fs::write(root.path().join("build.jsonl"), build).unwrap();
    let docker = r#"#!/usr/bin/env bash
set -eu
printf '%s\n' "$*" >>"$FAKE_OPERATIONS"
case "$1" in
 create) printf 'fixture-container\n' ;;
 inspect) printf '[{"Mounts":[],"HostConfig":{"NetworkMode":"none","Privileged":false,"PidMode":""},"Config":{"User":"65534:65534"},"State":{"Running":true,"OOMKilled":false,"ExitCode":0}}]\n' ;;
 cp)
  if [[ "$2" == - ]]; then cat >/dev/null
  elif [[ "$2" == *:/case/evidence ]]; then mkdir -p "$3"; cp "$FAKE_CAPTURE_SOURCE" "$3/retained.txt"; fi ;;
 start) printf 'retained observation\n' >"$FAKE_CAPTURE_SOURCE"; exit 124 ;;
 stop) exit 0 ;;
 rm) rm -f "$FAKE_CAPTURE_SOURCE" ;;
 *) exit 2 ;;
esac
"#;
    for (name, body) in [
        ("rustup", "#!/bin/sh\nexit 0\n"),
        ("cargo", "#!/bin/sh\ncat \"$FAKE_BUILD_JSON\"\n"),
        ("docker", docker),
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
        .env("FAKE_BUILD_JSON", root.path().join("build.jsonl"))
        .env("FAKE_OPERATIONS", root.path().join("operations.txt"))
        .env("FAKE_CAPTURE_SOURCE", root.path().join("capture.txt"))
        .output()
        .unwrap();
    assert!(!response.status.success());
    assert_eq!(
        casper_soak::record(&output.join("report.json")).unwrap()["status"],
        "failed"
    );
    assert_eq!(
        fs::read_to_string(output.join("interrupted-evidence/retained.txt")).unwrap(),
        "retained observation\n"
    );
    let operations = fs::read_to_string(root.path().join("operations.txt")).unwrap();
    let stop = operations.find("stop -t 5 fixture-container").unwrap();
    let capture = operations.find("/interrupted-evidence").unwrap();
    let remove = operations.find("rm -f fixture-container").unwrap();
    assert!(stop < capture && capture < remove);
}

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
