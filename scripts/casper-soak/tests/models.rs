use casper_soak::{models, *};
use serde_json::json;

#[test]
fn exact_positive_and_negative_verdicts() {
    let good = "Error: Invariant IdentityPinned is violated.\nError: The behavior up to this point is:\nState 1: initial\n";
    assert!(models::classify(0, models::CLEAN, None));
    assert!(models::classify(12, good, Some("IdentityPinned")));
    for status in [0, 1, 124, -9] {
        assert!(!models::classify(status, good, Some("IdentityPinned")));
    }
    for output in [
        format!("prefix {}", models::CLEAN),
        format!("{}\nError: broken", models::CLEAN),
        "Starting TLC".into(),
    ] {
        assert!(!models::classify(0, &output, None));
    }
    for output in [
        good.replace("IdentityPinned", "Other"),
        good.replace("State 1:", "not State 1:"),
        good.replace("Error: Invariant", "Invariant"),
        format!("{good}{good}"),
        format!("{good}Error: verifier failure\n"),
        format!("{good}{}", models::CLEAN),
        "Error: Invariant IdentityPinned is violated.\n".into(),
    ] {
        assert!(!models::classify(12, &output, Some("IdentityPinned")));
    }
}
#[test]
fn registry_and_defect_knobs_are_exact() {
    let control = json!({"configuration":"clean.cfg","expected_exit":0,"properties":["Safe"]});
    let body = "INVARIANT TypeOK\nINVARIANT Safe\nAllowBug = FALSE\n";
    let knobs = vec!["AllowBug".into()];
    models::validate_configuration(&control, body, &knobs).unwrap();
    for bad in [
        body.replace("INVARIANT Safe\n", ""),
        body.replace("FALSE", "TRUE"),
        format!("{body}INVARIANT Safe\n"),
    ] {
        assert!(models::validate_configuration(&control, &bad, &knobs).is_err());
    }
    let mut plan = json!({"positive_control":control,"negative_controls":[{"configuration":"unsafe.cfg","expected_exit":12,"property":"Safe","knob":"AllowBug"}]});
    models::controls(&plan).unwrap();
    plan["negative_controls"][0]["configuration"] = "../escape.cfg".into();
    assert!(models::controls(&plan).is_err());
}
#[test]
fn execution_retains_failures_and_detects_source_changes() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    for mode in [
        "passed",
        "timeout",
        "mutation",
        "missing-config",
        "missing-java",
    ] {
        let root = tempfile::tempdir().unwrap();
        let area = root.path().join("formal/tlaplus/casper_soak");
        fs::create_dir_all(&area).unwrap();
        fs::create_dir_all(root.path().join("scripts/casper-soak/src")).unwrap();
        fs::write(
            root.path().join("scripts/casper-soak/src/models.rs"),
            include_bytes!("../src/models.rs"),
        )
        .unwrap();
        let plan = json!({"model":"formal/tlaplus/casper_soak/Model.tla","positive_control":{"configuration":"clean.cfg","expected_exit":0,"properties":["Safe"]},"negative_controls":[{"configuration":"unsafe.cfg","expected_exit":12,"property":"Safe","knob":"AllowBug"}]});
        fs::write(root.path().join(models::PLAN), encoded(&plan).unwrap()).unwrap();
        fs::write(area.join("Model.tla"), "synthetic model").unwrap();
        fs::write(
            area.join("clean.cfg"),
            "INVARIANT TypeOK\nINVARIANT Safe\nAllowBug = FALSE\n",
        )
        .unwrap();
        fs::write(
            area.join("unsafe.cfg"),
            "INVARIANT TypeOK\nINVARIANT Safe\nAllowBug = TRUE\n",
        )
        .unwrap();
        let jar = root.path().join("tools.jar");
        fs::write(&jar, "synthetic jar").unwrap();
        let java = root.path().join("java");
        let setup = match mode {
            "timeout" => "printf 'partial trace\\n'; sleep 8\n",
            "mutation" => "root=\"$(dirname \"$0\")\"; printf changed >\"$root/formal/tlaplus/casper_soak/Model.tla\"\n",
            _ => "",
        };
        fs::write(&java, format!("#!/bin/sh\n{setup}for arg in \"$@\"; do\ncase \"$arg\" in *unsafe.cfg) printf 'Error: Invariant Safe is violated.\\nThe behavior up to this point is:\\nState 1: initial\\n'; exit 12;; esac\ndone\nprintf '{}\\n'\n", models::CLEAN)).unwrap();
        fs::set_permissions(&java, fs::Permissions::from_mode(0o755)).unwrap();
        if mode == "missing-config" {
            fs::remove_file(area.join("clean.cfg")).unwrap();
        }
        if mode == "missing-java" {
            fs::remove_file(&java).unwrap();
        }
        let output = root.path().join("results");
        let code = models::run(root.path(), &output, java.to_str().unwrap(), &jar, 1).unwrap();
        assert_eq!(code, if mode == "passed" { 0 } else { 1 });
        let report = record(&output.join("report.json")).unwrap();
        assert_eq!(report["results"].as_array().unwrap().len(), 2);
        if mode == "timeout" {
            assert_eq!(report["results"][0]["outcome"], "timeout");
            assert!(fs::read_to_string(output.join("clean.log"))
                .unwrap()
                .contains("partial trace"));
        }
        if mode == "mutation" {
            assert_eq!(report["results"][0]["outcome"], "input_changed");
        }
    }
}

#[test]
fn existing_results_are_not_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("results");
    std::fs::create_dir(&output).unwrap();
    std::fs::write(output.join("report.json"), "retained").unwrap();
    assert!(models::run(
        root.path(),
        &output,
        "missing-java",
        &root.path().join("missing.jar"),
        1
    )
    .is_err());
    assert_eq!(
        std::fs::read_to_string(output.join("report.json")).unwrap(),
        "retained"
    );
}
