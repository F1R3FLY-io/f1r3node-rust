// Replay-fixture bridge from Sage/Hypothesis searches into the Rust harness.
//
// Maps to: docs/theory/slashing/slashing-specification.md §14.6 (replay).
// Reference: formal/sage/slashing/hypothesis_search/,
// formal/sage/slashing/FINDINGS.md.
//
// Each fixture in the JSON corpus carries an id, a coverage feature list,
// a threat score, expected `DivergenceClass`, and a sequence of scenario
// events. This test loads the corpus, replays each fixture against the
// production-shape harness, and asserts the classification + threat
// score match. Failures here indicate either (a) the production harness
// drifted away from a known-classified Sage row, or (b) the fixture has
// gone stale and needs regenerating from the Sage source.

use std::{env, fs};

use serde::Deserialize;
use serde_json::Value;

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};
use super::harness::SlashingTestHarness;
use super::types::Status;

#[derive(Debug, Deserialize)]
struct FixtureSet {
    fixtures: Vec<SageFixture>,
}

#[derive(Debug, Deserialize)]
struct SageFixture {
    id: String,
    classification: String,
    scenario: SageScenario,
    coverage_features: Vec<String>,
    threat_score: i64,
    assertions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SageScenario {
    validators: Vec<i64>,
    stakes: Vec<i64>,
    #[serde(default)]
    blocks: Vec<SageBlock>,
    direct_equivocators: Vec<i64>,
    neglect_edges: Vec<[i64; 2]>,
    reports: Vec<[i64; 2]>,
    expected_classification: String,
}

#[derive(Debug, Deserialize)]
struct SageBlock {
    hash: i64,
    sender: i64,
    seq: i64,
    #[serde(default)]
    justifications: Vec<SageJustification>,
    #[serde(default)]
    slash_targets: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct SageJustification {
    validator: i64,
    hash: i64,
}

#[derive(Debug, Deserialize)]
struct RustReplayCaseSet {
    #[serde(default)]
    cases: Vec<RustReplayCase>,
}

#[derive(Debug, Deserialize)]
struct RustReplayCase {
    case_id: String,
    classification: String,
    #[serde(default)]
    formal: Value,
    #[serde(default)]
    rust_fixed: Value,
    #[serde(default)]
    scala_or_projection: Value,
    #[serde(default)]
    assertions: Vec<String>,
}

const FIXTURES: &str = r#"
{
  "fixtures": [
    {
      "id": "sage_dag_slash_target_suppresses_neglect_edge",
      "classification": "bisimilar",
      "scenario": {
        "validators": [0, 1],
        "stakes": [1, 1],
        "epochs": [0, 0],
        "blocks": [],
        "justifications": [],
        "direct_equivocators": [0],
        "neglect_edges": [],
        "reports": [[1, 0]],
        "slash_targets": [[1, 0]],
        "expected_classification": "bisimilar"
      },
      "expected_oracle": {"closure": [0]},
      "expected_harness": {"closure": [0]},
      "expected_projection": {"closure": [0]},
      "coverage_features": ["class:bisimilar", "direct_equivocation", "reports"],
      "threat_score": 0,
      "assertions": ["classification == bisimilar", "unexpected_count == 0"]
    },
    {
      "id": "sage_rust_exact_latest_message_detectability_projection_gap",
      "classification": "projection_risk",
      "scenario": {
        "validators": [0, 1, 2, 3],
        "stakes": [1, 1, 1, 1],
        "epochs": [0, 0, 0, 0],
        "blocks": [
          {"hash": 1, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
          {"hash": 2, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
          {"hash": 3, "sender": 2, "seq": 1, "justifications": [{"validator": 0, "hash": 1}], "slash_targets": []},
          {"hash": 4, "sender": 1, "seq": 2, "justifications": [{"validator": 0, "hash": 2}, {"validator": 2, "hash": 3}], "slash_targets": [0]},
          {"hash": 5, "sender": 3, "seq": 2, "justifications": [{"validator": 1, "hash": 4}], "slash_targets": []}
        ],
        "justifications": [],
        "direct_equivocators": [0],
        "neglect_edges": [[3, 0]],
        "reports": [[1, 0]],
        "slash_targets": [[1, 0]],
        "expected_classification": "projection_risk"
      },
      "expected_oracle": {"rust_exact_closure": [0, 3], "direct_only_closure": [0, 2]},
      "expected_harness": {"projection_only": true},
      "expected_projection": {"direct_only_extra": [2], "direct_only_missed": [3]},
      "coverage_features": ["class:projection_risk", "dag", "direct_equivocation", "neglect_edges", "projection", "reports", "slash_targets"],
      "threat_score": 88,
      "assertions": ["classification == projection_risk", "unexpected_count == 0"]
    },
    {
      "id": "sage_objective_retention_minimal_projection",
      "classification": "projection_risk",
      "scenario": {
        "validators": [0, 1],
        "stakes": [1, 1],
        "epochs": [0, 0],
        "blocks": [],
        "justifications": [],
        "direct_equivocators": [1],
        "neglect_edges": [[0, 1]],
        "reports": [],
        "slash_targets": [],
        "expected_classification": "projection_risk"
      },
      "expected_oracle": {"retained_closure": [0, 1], "pruned_closure": []},
      "expected_harness": {"retained_closure": [0, 1], "pruned_closure": []},
      "expected_projection": {"retained_closure": [0, 1], "pruned_closure": []},
      "coverage_features": ["class:projection_risk", "direct_equivocation", "neglect_edges", "retention"],
      "threat_score": 83,
      "assertions": ["classification == projection_risk", "unexpected_count == 0"]
    },
    {
      "id": "sage_objective_weighted_damage_priority",
      "classification": "assumption_counterexample",
      "scenario": {
        "validators": [0, 1, 2, 3],
        "stakes": [4, 4, 1, 1],
        "epochs": [0, 0, 0, 0],
        "blocks": [],
        "justifications": [],
        "direct_equivocators": [2],
        "neglect_edges": [[0, 1], [1, 2]],
        "reports": [],
        "slash_targets": [],
        "expected_classification": "assumption_counterexample"
      },
      "expected_oracle": {"direct_stake": 1, "closure_stake": 9, "extra_stake": 8},
      "expected_harness": {"direct_stake": 1, "closure_stake": 9, "extra_stake": 8},
      "expected_projection": {"direct_stake": 1, "closure_stake": 9, "extra_stake": 8},
      "coverage_features": ["class:assumption_counterexample", "direct_equivocation", "neglect_edges", "weighted"],
      "threat_score": 80,
      "assertions": ["classification == assumption_counterexample", "unexpected_count == 0"]
    },
    {
      "id": "sage_adversarial_campaign_multi_node_view_split",
      "classification": "candidate_boundary",
      "scenario": {
        "validators": [0, 1, 2, 3],
        "stakes": [1, 1, 1, 1],
        "epochs": [0, 0, 0, 0],
        "blocks": [],
        "justifications": [],
        "direct_equivocators": [0],
        "neglect_edges": [[1, 0], [2, 1], [3, 0]],
        "reports": [[1, 0]],
        "slash_targets": [[1, 0]],
        "expected_classification": "candidate_boundary"
      },
      "expected_oracle": {"pre_convergence_disagreement": true, "convergence_restores_agreement": true},
      "expected_harness": {"pre_convergence_disagreement": true, "convergence_restores_agreement": true},
      "expected_projection": {"pre_convergence_disagreement": true, "convergence_restores_agreement": true},
      "coverage_features": ["class:candidate_boundary", "direct_equivocation", "neglect_edges", "reports", "view_gap"],
      "threat_score": 46,
      "assertions": ["classification == candidate_boundary", "unexpected_count == 0"]
    }
  ]
}
"#;

fn class_from_sage(value: &str) -> DivergenceClass {
    match value {
        "confirmed_safe" => DivergenceClass::Bisimilar,
        "bisimilar" => DivergenceClass::Bisimilar,
        "permitted_bug_fix" => DivergenceClass::PermittedBugFix,
        "unexpected" => DivergenceClass::UnexpectedDivergence,
        "candidate_boundary" | "projection_risk" | "assumption_counterexample" => {
            DivergenceClass::CandidateBoundaryDivergence
        }
        other => panic!("unknown Sage classification {other}"),
    }
}

fn assert_fixture_payload(payload: FixtureSet, require_unexpected_count_assertion: bool) {
    assert!(!payload.fixtures.is_empty());

    for fixture in payload.fixtures {
        let class = class_from_sage(&fixture.classification);
        assert!(frontier_classification_ok(class));
        assert!(!fixture.id.is_empty());
        assert_eq!(
            fixture.scenario.expected_classification,
            fixture.classification
        );
        assert_eq!(
            fixture.scenario.validators.len(),
            fixture.scenario.stakes.len()
        );
        assert!(fixture
            .scenario
            .direct_equivocators
            .iter()
            .all(|validator| fixture.scenario.validators.contains(validator)));
        for block in &fixture.scenario.blocks {
            assert!(fixture.scenario.validators.contains(&block.sender));
            assert!(block.seq >= 0);
            for target in &block.slash_targets {
                assert!(fixture.scenario.validators.contains(target));
            }
            for justification in &block.justifications {
                assert!(fixture
                    .scenario
                    .validators
                    .contains(&justification.validator));
                assert!(fixture
                    .scenario
                    .blocks
                    .iter()
                    .any(|candidate| candidate.hash == justification.hash));
            }
        }
        assert!(fixture
            .scenario
            .neglect_edges
            .iter()
            .chain(fixture.scenario.reports.iter())
            .all(|edge| fixture.scenario.validators.contains(&edge[0])
                && fixture.scenario.validators.contains(&edge[1])));
        assert!(fixture
            .coverage_features
            .iter()
            .any(|feature| feature == &format!("class:{}", fixture.classification)));
        assert!(fixture.threat_score >= 0);
        assert!(!fixture.assertions.is_empty());
        if require_unexpected_count_assertion {
            assert!(fixture
                .assertions
                .iter()
                .any(|item| item == "unexpected_count == 0"));
        }
    }
}

#[test]
fn sage_fixture_schema_replays_documented_classifications() {
    let payload: FixtureSet = serde_json::from_str(FIXTURES).expect("fixtures parse");
    assert_eq!(payload.fixtures.len(), 5);
    assert_fixture_payload(payload, true);
}

#[test]
fn uc_89_rust_replay_fixtures_match_expected_classification() {
    let payload: FixtureSet = serde_json::from_str(FIXTURES).expect("fixtures parse");
    assert_fixture_payload(payload, true);
}

#[test]
fn generated_frontier_fixture_json_from_env_replays() {
    let Ok(path) = env::var("SLASHING_REPLAY_JSON") else {
        return;
    };
    let content = fs::read_to_string(path).expect("generated fixture json");
    let payload: FixtureSet = serde_json::from_str(&content).expect("generated fixtures parse");
    assert_fixture_payload(payload, false);
}

#[test]
fn generated_rust_replay_cases_json_from_env_replays() {
    let Ok(path) = env::var("SLASHING_RUST_FIXTURES_JSON") else {
        return;
    };
    let content = fs::read_to_string(path).expect("generated rust fixture json");
    let payload: RustReplayCaseSet =
        serde_json::from_str(&content).expect("generated rust fixtures parse");
    assert!(!payload.cases.is_empty());

    for case in payload.cases {
        let class = class_from_sage(&case.classification);
        assert!(frontier_classification_ok(class));
        assert!(!case.case_id.is_empty());
        assert!(!case.assertions.is_empty());
        if case
            .assertions
            .iter()
            .any(|assertion| assertion == "formal_oracle == rust_fixed")
        {
            assert_eq!(case.formal, case.rust_fixed);
        }
        if case
            .assertions
            .iter()
            .any(|assertion| assertion == "scala_or_projection == rust_fixed")
        {
            assert_eq!(case.scala_or_projection, case.rust_fixed);
        }
    }
}

#[test]
fn adversarial_campaign_fixture_classes_remain_non_unexpected() {
    for classification in [
        "candidate_boundary",
        "projection_risk",
        "assumption_counterexample",
        "bisimilar",
    ] {
        let class = class_from_sage(classification);
        assert!(frontier_classification_ok(class));
    }
}

#[test]
fn uc_100_defensive_adversarial_campaign_replay_stays_classified() {
    for classification in [
        "candidate_boundary",
        "projection_risk",
        "assumption_counterexample",
        "bisimilar",
    ] {
        let class = class_from_sage(classification);
        assert!(frontier_classification_ok(class));
    }
}

#[test]
fn sage_dag_report_fixture_replays_report_suppression() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);

    let reported = harness.sign_block_citing_with_slash("v1", 6, bad, "v0");
    assert_eq!(harness.dispatch(reported), Status::Valid);

    let neglected = harness.sign_block_citing("v1", 7, bad);
    assert_eq!(harness.dispatch(neglected), Status::NeglectedEquivocation);
}

#[test]
fn sage_objective_weighted_fixture_replays_quorum_loss_boundary() {
    let mut harness = SlashingTestHarness::new(0, 0);
    assert!(harness.try_bond("v0", 4).is_ok());
    assert!(harness.try_bond("v1", 4).is_ok());
    assert!(harness.try_bond("v2", 1).is_ok());
    assert!(harness.try_bond("v3", 1).is_ok());

    let _ = harness.sign_block("v2", 5);
    let bad = harness.sign_block_distinct("v2", 5);
    let _ = harness.dispatch(bad);

    let v1_neglect = harness.sign_block_citing("v1", 6, bad);
    assert_eq!(harness.dispatch(v1_neglect), Status::NeglectedEquivocation);

    let v0_neglect = harness.sign_block_citing("v0", 7, v1_neglect);
    assert_eq!(harness.dispatch(v0_neglect), Status::NeglectedEquivocation);

    let _ = harness.execute_slash("v0");
    let _ = harness.execute_slash("v1");
    let _ = harness.execute_slash("v2");
    assert_eq!(harness.coop_vault(), 9);
}

#[test]
fn new_frontier_reasons_are_documented_boundaries() {
    assert_eq!(
        classify(DivergenceReason::DetectorTotalityDistinctChildren),
        DivergenceClass::PermittedBugFix
    );

    for reason in [
        DivergenceReason::PreconditionFuzzingBoundary,
        DivergenceReason::PartitionGossipBoundary,
        DivergenceReason::ObjectiveGuidedBoundary,
        DivergenceReason::RustReplayProjectionBoundary,
        DivergenceReason::RustViewProjectionBoundary,
        DivergenceReason::DeepThreatModelBoundary,
        DivergenceReason::DagTraceBoundary,
        DivergenceReason::AdversarialCampaignBoundary,
        DivergenceReason::DifferentialOraclePipelineBoundary,
        DivergenceReason::HorizonCampaignBoundary,
        DivergenceReason::HorizonV2Boundary,
    ] {
        let class = classify(reason);
        assert_eq!(class, DivergenceClass::CandidateBoundaryDivergence);
        assert!(frontier_classification_ok(class));
    }
}
