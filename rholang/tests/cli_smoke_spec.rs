use std::path::Path;
use std::process::Command;

/// The CLI prints evaluation errors but still exits with 0, so the test also
/// checks the output: the cost line proves the evaluation finished, and the
/// error header must be absent.
#[test]
fn the_cli_runs_the_registry_contract_without_errors() {
    let data_dir = tempfile::tempdir().unwrap();
    let registry =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../casper/src/main/resources/Registry.rho");
    assert!(registry.is_file(), "{} not found", registry.display());

    let output = Command::new(env!("CARGO_BIN_EXE_rholang-cli"))
        .arg("--quiet")
        .arg("--data-dir")
        .arg(data_dir.path().join("rspace"))
        .arg(&registry)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("Estimated deploy cost"),
        "stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("Errors received during evaluation"),
        "stdout:\n{stdout}"
    );
}
