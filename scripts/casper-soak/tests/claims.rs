use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn source() -> PathBuf {
    std::env::var_os("SOAK_SOURCE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf()
        })
}

fn binary() -> PathBuf {
    std::env::var_os("SOAK_CLAIM_CHECKER_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_check-casper-claims")))
}

fn check(root: &Path, output: &Path, strict: bool) -> i32 {
    let mut command = Command::new(binary());
    command.arg("--root").arg(root).arg("--output").arg(output);
    if strict {
        command.arg("--strict");
    }
    command.output().unwrap().status.code().unwrap_or(128)
}

#[test]
fn pending_claims_are_visible_and_strict_discharge_refuses() {
    let root = fixture();
    let root = root.path();
    let output = tempfile::tempdir().unwrap();
    assert_eq!(
        check(&source(), &output.path().join("source-audit.json"), false),
        0
    );
    assert_eq!(check(root, &output.path().join("audit.json"), false), 0);
    let report = casper_soak::record(&output.path().join("audit.json")).unwrap();
    assert_eq!(report["claims"].as_array().unwrap().len(), 8);
    assert_eq!(report["claim_discharge"], "pending");
    assert_eq!(check(root, &output.path().join("strict.json"), true), 4);
}

fn fixture() -> tempfile::TempDir {
    let source = source();
    let root = tempfile::tempdir().unwrap();
    let plan_path = root.path().join(casper_soak::models::PLAN);
    fs::create_dir_all(plan_path.parent().unwrap()).unwrap();
    let plan = casper_soak::record(&source.join(casper_soak::models::PLAN)).unwrap();
    let mut claims =
        vec![serde_json::json!({"id":plan["claim"],"specification":plan["specification"]})];
    claims.extend(
        plan["profile_verification"]["claims"]
            .as_array()
            .unwrap()
            .iter()
            .cloned(),
    );
    for claim in claims {
        let path = root.path().join(claim["specification"].as_str().unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("```yaml\nclaim_id: {}\nstatus: pending\nconstruction: not-applicable\nrefutation: pending\nbinding: pending\nsoak: pending\n```\nSynthetic audit input.\n", claim["id"].as_str().unwrap())).unwrap();
    }
    fs::write(&plan_path, casper_soak::encoded(&plan).unwrap()).unwrap();
    root
}

#[test]
fn missing_claims_and_unsupported_discharge_cannot_pass() {
    let root = fixture();
    let plan_path = root.path().join(casper_soak::models::PLAN);
    let mut plan = casper_soak::record(&plan_path).unwrap();
    let spec = root.path().join(plan["specification"].as_str().unwrap());
    let original = fs::read_to_string(&spec).unwrap();
    fs::write(
        &spec,
        original.replacen("status: pending", "status: discharged", 1),
    )
    .unwrap();
    assert_eq!(
        check(
            root.path(),
            &root.path().join("false-discharge.json"),
            false
        ),
        2
    );
    fs::write(&spec, original).unwrap();
    plan["profile_verification"]["claims"]
        .as_array_mut()
        .unwrap()
        .pop();
    fs::write(&plan_path, casper_soak::encoded(&plan).unwrap()).unwrap();
    assert_eq!(
        check(root.path(), &root.path().join("missing.json"), false),
        2
    );
}

#[test]
fn claim_scope_source_and_evidence_are_independent_of_soak() {
    use serde_json::json;
    let root = fixture();
    let spec = "docs/claims/casper-soak-harness.md";
    let header = "```yaml\nclaim_id: CLAIM-CASPER-SOAK-001\nstatus: discharged\nartifacts:\n  - source_file.txt\nconstruction: not-applicable\nrefutation: bounded-safety-pass\nbinding: passed\nsoak: pending\n```\nSynthetic audit input, not verification evidence.\n";
    fs::write(root.path().join(spec), header).unwrap();
    fs::write(root.path().join("source_file.txt"), "source").unwrap();
    let report = json!({"claim_discharge":"discharged","claim_ids":["CLAIM-CASPER-SOAK-001"],"phase":"pre_pr216_merge","purpose":"synthetic-audit-input","source_digests":{"source_file.txt":casper_soak::hash(b"source")},"claim_digests":{spec:casper_soak::hash(header.as_bytes())},"tiers":{"refutation":"bounded-safety-pass","binding":"passed","construction":"not-applicable"}});
    fs::write(
        root.path().join("verification.json"),
        casper_soak::encoded(&report).unwrap(),
    )
    .unwrap();
    let ledger = json!({"artifact":{"path":"source_file.txt","sha256":casper_soak::hash(b"source"),"commit":"0".repeat(40)},"verified_at":"2026-09-18T00:00:00Z","claim_digests":{spec:casper_soak::hash(header.as_bytes())},"claim_ids":["CLAIM-CASPER-SOAK-001"],"status":"discharged","phase_status":{"pre_pr216_merge":"discharged"},"evidence":{"ref":"verification.json","sha256":casper_soak::file_hash(&root.path().join("verification.json")).unwrap()},"waiver":null});
    let path = root
        .path()
        .join("docs/casper/cbc-evidence/source-file-txt.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let save =
        |value: &serde_json::Value| fs::write(&path, format!("```json\n{value}\n```\n")).unwrap();
    save(&ledger);
    let selected = |name: &str| {
        Command::new(binary())
            .args(["--claim", "CLAIM-CASPER-SOAK-001", "--strict", "--root"])
            .arg(root.path())
            .arg("--output")
            .arg(root.path().join(name))
            .output()
            .unwrap()
            .status
            .code()
            .unwrap()
    };
    assert_eq!(selected("selected.json"), 0);
    let result = casper_soak::record(&root.path().join("selected.json")).unwrap();
    assert_eq!(result["claims"].as_array().unwrap().len(), 1);
    assert_eq!(result["claims"][0]["soak"], "pending");
    assert_eq!(result["proof_execution"], false);
    assert_eq!(check(root.path(), &root.path().join("all.json"), true), 4);
    let spaced_header = header.replace("construction:", "\n  - missing.txt\nconstruction:");
    fs::write(root.path().join(spec), &spaced_header).unwrap();
    let mut spaced_report = report.clone();
    spaced_report["claim_digests"][spec] = casper_soak::hash(spaced_header.as_bytes()).into();
    fs::write(
        root.path().join("verification.json"),
        casper_soak::encoded(&spaced_report).unwrap(),
    )
    .unwrap();
    let mut spaced_ledger = ledger.clone();
    spaced_ledger["claim_digests"][spec] = spaced_report["claim_digests"][spec].clone();
    spaced_ledger["evidence"]["sha256"] =
        casper_soak::file_hash(&root.path().join("verification.json"))
            .unwrap()
            .into();
    save(&spaced_ledger);
    assert_eq!(selected("incomplete-artifact-list.json"), 2);
    fs::write(root.path().join(spec), header).unwrap();
    fs::write(
        root.path().join("verification.json"),
        casper_soak::encoded(&report).unwrap(),
    )
    .unwrap();
    save(&ledger);
    fs::write(root.path().join("source_file.txt"), "changed").unwrap();
    assert_eq!(selected("stale-source.json"), 2);
    let mut relabeled = ledger.clone();
    relabeled["artifact"]["sha256"] = casper_soak::hash(b"changed").into();
    save(&relabeled);
    assert_eq!(selected("relabeled-old-evidence.json"), 2);
    fs::write(root.path().join("source_file.txt"), "source").unwrap();
    let mut bad = ledger.clone();
    bad["claim_ids"] = json!([]);
    save(&bad);
    assert_eq!(selected("wrong-claim.json"), 2);
    bad = ledger.clone();
    bad["phase_status"]["pre_pr216_merge"] = "pending".into();
    save(&bad);
    assert_eq!(selected("wrong-phase.json"), 2);
    save(&ledger);
    fs::write(root.path().join("verification.json"), "{}").unwrap();
    assert_eq!(selected("stale-evidence.json"), 2);
    fs::write(
        root.path().join("verification.json"),
        casper_soak::encoded(&report).unwrap(),
    )
    .unwrap();
    let outside = root.path().join("outside-ledgers");
    fs::rename(path.parent().unwrap(), &outside).unwrap();
    std::os::unix::fs::symlink(&outside, path.parent().unwrap()).unwrap();
    assert_eq!(selected("symlink-ledger-parent.json"), 2);
}
