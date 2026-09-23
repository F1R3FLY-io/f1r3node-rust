use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::json;

use crate::{bindings, check_hashes, digest, models, AREA};

fn case(exit: i32, invariant: Option<&str>) -> models::Case {
    models::Case {
        module: "MC_Example".into(),
        configuration: "MC_Example.cfg".into(),
        expected_exit: exit,
        invariant: invariant.map(str::to_owned),
    }
}

#[test]
fn counterexamples_require_the_exact_error_exit_and_transition_trace() {
    let case = case(12, Some("BoundDeadline"));
    let log = "Error: Invariant BoundDeadline is violated.\nError: The behavior up to this point is:\nState 1:\nState 2:\n";
    assert!(models::classify(12, log, &case));
    for (code, text) in [
        (0, log.to_owned()),
        (1, log.to_owned()),
        (124, log.to_owned()),
        (12, log.replace("BoundDeadline", "TypeOK")),
        (12, log.replace("State 2:", "")),
        (12, format!("{log}Error: Unexpected exception\n")),
        (
            12,
            format!("{log}Error: Invariant BoundDeadline is violated.\n"),
        ),
    ] {
        assert!(!models::classify(code, &text, &case), "{code}: {text}");
    }
}

#[test]
fn positives_require_a_complete_search_without_errors() {
    let case = case(0, None);
    let log = "Model checking completed. No error has been found.";
    assert!(models::classify(0, log, &case));
    for (code, text) in [
        (12, log),
        (0, "Finished in 0s."),
        (
            0,
            "Model checking completed. No error has been found.\nError: Failure",
        ),
    ] {
        assert!(!models::classify(code, text, &case));
    }
}

#[test]
fn model_inventory_rejects_missing_duplicate_and_inverted_controls() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let area = root.join(AREA);
    let plan: models::Plan =
        serde_json::from_slice(&fs::read(area.join("verification-plan.json")).unwrap()).unwrap();
    models::validate_plan(&area, &plan).unwrap();
    for mutation in ["empty", "missing", "duplicate", "outcome"] {
        let mut changed = plan.clone();
        match mutation {
            "empty" => changed.models.clear(),
            "missing" => {
                changed.models.pop();
            }
            "duplicate" => changed.models.push(changed.models[0].clone()),
            _ => changed.models[0].expected_exit = 12,
        }
        assert!(
            models::validate_plan(&area, &changed).is_err(),
            "{mutation}"
        );
    }
}

#[test]
fn incomplete_failed_or_duplicate_suite_executions_are_rejected() {
    let mut log = String::new();
    for (target, binary, count) in [
        ("tests/soak_snapshot.rs", "soak_snapshot", 19),
        ("tests/soak_snapshot.rs", "soak_snapshot", 26),
        ("unittests src/lib.rs", "block_storage", 2),
        ("tests/soak_observer.rs", "soak_observer", 18),
    ] {
        log.push_str(&format!(
            "     Running {target} (target/debug/deps/{binary}-abcdef)\n"
        ));
        for n in 0..count {
            log.push_str(&format!("test case_{n} ... ok\n"));
        }
        log.push_str(&format!("test result: ok. {count} passed; 0 failed;\n"));
    }
    assert_eq!(bindings::read_suites(&log).unwrap().len(), 4);
    for invalid in [
        String::new(),
        log.replacen("test case_0 ... ok\n", "", 1),
        log.replacen("test result: ok.", "test result: FAILED.", 1),
        format!("{log}{log}"),
        format!("{log}test result: FAILED.\n"),
    ] {
        assert!(bindings::read_suites(&invalid).is_err());
    }
}

#[test]
fn changed_missing_and_escaped_source_inputs_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    fs::create_dir(&root).unwrap();
    let source = root.join("test.rs");
    fs::write(&source, "fn regression() {}").unwrap();
    let inventory = BTreeMap::from([("test.rs".into(), digest(&source).unwrap())]);
    check_hashes(&root, &inventory).unwrap();
    fs::write(&source, "fn regression() { panic!(); }").unwrap();
    assert!(check_hashes(&root, &inventory).is_err());
    fs::remove_file(&source).unwrap();
    assert!(check_hashes(&root, &inventory).is_err());
    assert!(check_hashes(&root, &BTreeMap::new()).is_err());
    let outside = tmp.path().join("outside.rs");
    fs::write(&outside, "outside").unwrap();
    assert!(check_hashes(
        &root,
        &BTreeMap::from([("../outside.rs".into(), digest(&outside).unwrap())])
    )
    .is_err());
}

#[test]
fn missing_tests_properties_and_invariants_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let area = root.join(AREA);
    fs::create_dir_all(&area).unwrap();
    fs::write(
        root.join("claim.md"),
        "## Required properties\n\n1. A property.\n\n## Scope\n",
    )
    .unwrap();
    fs::write(root.join("test.rs"), "fn regression() {}").unwrap();
    fs::write(area.join("Model.tla"), "Safe == TRUE\n").unwrap();
    fs::write(area.join("MC_Model.cfg"), "INVARIANT Safe\n").unwrap();
    let mut bindings: bindings::Bindings = serde_json::from_value(json!({"claims": [{
        "claim_id": "example", "claim": "claim.md", "model": "Model", "properties": [{
            "property": 1, "invariants": ["Safe"], "coverage": "bounded-model-and-rust-tests",
            "tests": [{"source": "test.rs", "name": "regression"}]
        }]
    }]}))
    .unwrap();
    let suites = BTreeMap::from([("test.rs".into(), bindings::Suite {
        executable: "test".into(),
        passed: BTreeSet::from(["regression".into()]),
    })]);
    assert_eq!(
        bindings::check_tests(root, &bindings, &suites)
            .unwrap()
            .len(),
        1
    );
    assert!(bindings::check_tests(root, &bindings, &BTreeMap::new()).is_err());
    fs::write(root.join("test.rs"), "fn unrelated() {}").unwrap();
    assert!(bindings::check_tests(root, &bindings, &suites).is_err());
    fs::write(root.join("test.rs"), "fn regression() {}").unwrap();
    fs::write(area.join("MC_Model.cfg"), "INVARIANT Other\n").unwrap();
    assert!(bindings::check_tests(root, &bindings, &suites).is_err());
    bindings.claims[0].properties.clear();
    assert!(bindings::check_tests(root, &bindings, &suites).is_err());
    fs::write(
        root.join("claim.md"),
        "## Required properties\n\n## Scope\n",
    )
    .unwrap();
    assert!(bindings::check_tests(root, &bindings, &suites).is_err());
}

#[test]
fn manifests_reject_empty_malformed_and_duplicate_entries() {
    let line = format!("{}  source.rs\n", "a".repeat(64));
    assert_eq!(bindings::read_manifest(&line).unwrap().len(), 1);
    for invalid in [
        String::new(),
        "bad  source.rs\n".into(),
        format!("{line}{line}"),
    ] {
        assert!(bindings::read_manifest(&invalid).is_err());
    }
}

#[test]
fn verifier_timeouts_fail_and_existing_logs_are_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    let log = tmp.path().join("timeout.log");
    let mut command = Command::new("sleep");
    command.arg("5");
    assert_eq!(
        models::run_process(&mut command, &log, Duration::from_millis(20)).unwrap(),
        124
    );
    fs::write(&log, "retained failure").unwrap();
    assert!(models::run_process(&mut command, &log, Duration::from_secs(1)).is_err());
    assert_eq!(fs::read_to_string(log).unwrap(), "retained failure");
}
