#[path = "../src/campaign_control/models.rs"]
mod models;

use std::fs;
use std::path::Path;

use casper_soak::models::classify;
use casper_soak::{encoded, record};

fn fixture(root: &Path) {
    let directory = root.join(models::DIRECTORY);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("verification-plan.jsonc"),
        encoded(&models::registration()).unwrap(),
    )
    .unwrap();
    fs::write(directory.join("CampaignControl.tla"), "MODULE Fixture").unwrap();
    for (name, _, knob) in models::CONTROLS {
        fs::write(directory.join(name), models::configuration(knob)).unwrap();
    }
}

#[test]
fn registration_rejects_missing_controls_and_disabled_invariants() {
    for (name, _, _) in models::CONTROLS {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        assert!(models::validate(root.path()).is_ok());
        let path = root.path().join(models::DIRECTORY).join(name);
        let original = fs::read_to_string(&path).unwrap();
        fs::write(&path, original.replace("ScheduledLaunch ", "")).unwrap();
        assert!(models::validate(root.path()).is_err());
        fs::remove_file(path).unwrap();
        assert!(models::validate(root.path()).is_err());
    }
}

#[test]
fn controls_require_the_exact_violation_and_a_counterexample() {
    for (_, property, _) in models::CONTROLS.into_iter().skip(1) {
        let property = property.unwrap();
        let log = format!("Error: Invariant {property} is violated.\nError: The behavior up to this point is:\nState 1: <Initial predicate>\n");
        assert!(classify(12, &log, Some(property)));
        for code in [0, 1, 124, 137] {
            assert!(!classify(code, &log, Some(property)));
        }
        assert!(!classify(
            12,
            &log.replace(property, "TypeOK"),
            Some(property)
        ));
        assert!(!classify(
            12,
            &log.replace("State 1: <Initial predicate>\n", ""),
            Some(property)
        ));
        assert!(!classify(
            12,
            &(log.clone() + "Error: A tool failed.\n"),
            Some(property)
        ));
        assert!(!classify(
            12,
            &(log + casper_soak::models::CLEAN),
            Some(property)
        ));
    }
    assert!(classify(0, casper_soak::models::CLEAN, None));
    assert!(!classify(0, "The search is incomplete.", None));
}

#[test]
fn setup_failure_retains_a_nonpassing_report_without_execution() {
    let root = tempfile::tempdir().unwrap();
    let jar = root.path().join("wrong.jar");
    fs::write(&jar, "incorrect verifier").unwrap();
    let output = root.path().join("evidence");
    assert_eq!(
        models::run(
            root.path(),
            &output,
            "missing-java",
            &jar,
            "missing-timeout",
            1
        )
        .unwrap(),
        1
    );
    let report = record(&output.join("report.json")).unwrap();
    assert_eq!(report["status"], "failed");
    assert_eq!(report["claim_discharge"], "pending");
    assert_eq!(report["results"].as_array().unwrap().len(), 0);
    assert!(models::run(
        root.path(),
        &output,
        "missing-java",
        &jar,
        "missing-timeout",
        1
    )
    .is_err());
}
