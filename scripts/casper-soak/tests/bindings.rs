use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn source() -> PathBuf {
    std::env::var_os("SOAK_SOURCE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf()
        })
}

fn check(evidence: &Path, output: &Path) -> bool {
    let mut command = Command::new("bash");
    command
        .arg(source().join("scripts/ci/check-casper-soak-bindings.sh"))
        .arg("--verify-evidence")
        .arg(evidence)
        .arg("--output")
        .arg(output);
    if std::env::var_os("SOAK_BINDING_CHECKER_BIN").is_none() {
        if let Some(binary) = option_env!("CARGO_BIN_EXE_check-casper-bindings") {
            command.env("SOAK_BINDING_CHECKER_BIN", binary);
        }
    }
    command.output().unwrap().status.success()
}

#[test]
fn required_case_cannot_be_replaced_by_an_unrelated_case() {
    let root = tempfile::tempdir().unwrap();
    assert!(Command::new("tar")
        .arg("-xzf")
        .arg(source().join(
            "docs/casper/cbc-evidence/runs/casper-rust-migration-20260917-01/bindings.tar.gz"
        ))
        .arg("-C")
        .arg(root.path())
        .status()
        .unwrap()
        .success());
    let historical = root.path().join("evidence");
    let evidence = root.path().join("synthetic-inventory");
    for path in casper_soak::walk(&historical).unwrap() {
        if !path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("invocation-")
        {
            continue;
        }
        let source = casper_soak::record(&path).unwrap();
        let destination = evidence.join(path.strip_prefix(&historical).unwrap());
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, casper_soak::encoded(&serde_json::json!({"command":source["command"],"expected_exit":source["expected_exit"],"actual_exit":source["actual_exit"],"evidence_kind":"synthetic_fixture","purpose":"inventory-validator-input"})).unwrap()).unwrap();
    }
    fs::create_dir(evidence.join("terminal-before-exec")).unwrap();
    fs::write(evidence.join("terminal-before-exec/invocation-01.json"), casper_soak::encoded(&serde_json::json!({"command":["bash","scripts/run-merge-recovery-soak.sh"],"expected_exit":1,"actual_exit":1,"evidence_kind":"synthetic_fixture","purpose":"inventory-validator-input"})).unwrap()).unwrap();
    for (suite, count) in [("manifest", 3), ("models", 4), ("driver", 7)] {
        fs::write(evidence.join(format!("{suite}.txt")), format!("Synthetic inventory-validator input.\ntest result: ok. {count} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n")).unwrap();
    }
    assert!(check(&evidence, &root.path().join("valid")));
    fs::rename(
        evidence.join("capability"),
        evidence.join("unregistered-capability"),
    )
    .unwrap();
    assert!(
        !check(&evidence, &root.path().join("invalid")),
        "The gate accepted an unrelated case in place of a required case."
    );
}
