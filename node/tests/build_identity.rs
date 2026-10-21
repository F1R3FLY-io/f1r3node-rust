#[path = "../build_identity.rs"]
mod build_identity;

use std::io;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

fn git_output(code: i32, stdout: &[u8]) -> io::Result<Output> {
    Ok(Output {
        status: ExitStatus::from_raw((code << 8) as _),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    })
}

#[test]
fn explicit_identity_takes_precedence_without_running_git() {
    let identity = build_identity::resolve_build_id(Some("  v0.4.46-42-g4d8d9d79  "), || {
        panic!("Git must not run when the build identity is explicit")
    })
    .unwrap();
    assert_eq!(identity, "v0.4.46-42-g4d8d9d79");
}

#[test]
fn missing_or_blank_identity_uses_git() {
    for explicit in [None, Some(""), Some("   ")] {
        let identity =
            build_identity::resolve_build_id(explicit, || git_output(0, b"v0.4.46-42-g4d8d9d79\n"))
                .unwrap();
        assert_eq!(identity, "v0.4.46-42-g4d8d9d79");
    }
}

#[test]
fn failed_git_cannot_supply_an_identity() {
    for stdout in [b"".as_slice(), b"misleading-identity\n".as_slice()] {
        let identity = build_identity::resolve_build_id(None, || git_output(128, stdout)).unwrap();
        assert_eq!(identity, "unknown");
    }
}

#[test]
fn missing_git_uses_unknown() {
    let identity = build_identity::resolve_build_id(None, || {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "git is unavailable",
        ))
    })
    .unwrap();
    assert_eq!(identity, "unknown");
}

#[test]
fn empty_or_invalid_git_output_uses_unknown() {
    for stdout in [b"".as_slice(), b" \n".as_slice(), b"\xff".as_slice()] {
        let identity = build_identity::resolve_build_id(None, || git_output(0, stdout)).unwrap();
        assert_eq!(identity, "unknown");
    }
}

#[test]
fn explicit_control_characters_cannot_emit_cargo_directives() {
    for explicit in [
        "build\ncargo:rustc-env=INJECTED=value",
        "build\rvalue",
        "build\tvalue",
        "build\0value",
    ] {
        assert!(build_identity::resolve_build_id(Some(explicit), || {
            panic!("An invalid explicit identity must fail before Git runs")
        })
        .is_err());
    }
}

#[test]
fn git_control_characters_cannot_emit_cargo_directives() {
    let identity = build_identity::resolve_build_id(None, || {
        git_output(0, b"build\ncargo:rustc-env=INJECTED=value\n")
    })
    .unwrap();
    assert_eq!(identity, "unknown");
}
