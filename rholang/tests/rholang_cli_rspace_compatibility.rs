//! Black-box compatibility gates for the standalone Rholang CLI.
//!
//! These exercise the compiled binary so internal RSpace representation changes
//! cannot accidentally alter the default, `--quiet`, or
//! `--unmatched-sends-only` user-facing contracts.

use std::fs;
use std::process::{Command, Output};

fn run_cli(source: &str, flags: &[&str]) -> Output {
    let fixture = tempfile::tempdir().expect("temporary CLI fixture");
    let source_path = fixture.path().join("input.rho");
    let data_path = fixture.path().join("rspace");
    fs::write(&source_path, source).expect("write Rholang fixture");

    Command::new(env!("CARGO_BIN_EXE_rholang-cli"))
        .args(flags)
        .arg("--data-dir")
        .arg(data_path)
        .arg(source_path)
        .output()
        .expect("run rholang-cli")
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "rholang-cli failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).expect("CLI stdout must be UTF-8")
}

#[test]
fn cli_prints_rspace_by_default_and_quiet_is_the_explicit_opt_out() {
    let default_output = stdout(&run_cli(r#"@"cli-visible"!(42)"#, &[]));
    assert!(
        default_output.contains("Storage Contents:"),
        "{default_output}"
    );
    assert!(default_output.contains("cli-visible"), "{default_output}");
    assert!(default_output.contains("42"), "{default_output}");

    let quiet_output = stdout(&run_cli(r#"@"cli-hidden"!(43)"#, &["--quiet"]));
    assert!(
        !quiet_output.contains("Storage Contents:"),
        "{quiet_output}"
    );
    assert!(
        quiet_output.contains("Estimated deploy cost:"),
        "{quiet_output}"
    );
}

#[test]
fn cli_unmatched_sends_flag_preserves_the_reduced_rspace_view() {
    let output = stdout(&run_cli(r#"@"cli-unmatched"!(44)"#, &[
        "--unmatched-sends-only",
    ]));
    assert!(output.contains("Storage Contents:"), "{output}");
    assert!(output.contains("cli-unmatched"), "{output}");
    assert!(output.contains("44"), "{output}");
    assert!(output.contains("Resting diagnostics:"), "{output}");
}
