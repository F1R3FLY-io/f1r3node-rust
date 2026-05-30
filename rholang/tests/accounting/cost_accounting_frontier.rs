use std::{env, fs};

use models::rust::casper::protocol::casper_message::DeployData;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{
    BillableKind, BillableTokenEvent, RedexId, RuntimeBudget, SourcePath,
};
use rholang::rust::interpreter::metering::{ContinuationKey, MeteredFrame, MeteredMachine};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Classification {
    ConfirmedSafe,
    Bisimilar,
    ProjectionRisk,
    ProofOrModelStrengthening,
    NeedsSourceAudit,
    ConfirmedCurrentBug,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Record,
    Guard,
    StrengthenFormal,
    Audit,
    FixSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PromotionTarget {
    Record,
    RustGuard,
    Rocq,
    Tla,
    Sage,
    Audit,
    SourceFix,
}

#[derive(Clone, Copy, Debug)]
struct FrontierFixture {
    name: &'static str,
    threat_family: &'static str,
    reproduced_in_rust: bool,
    violates_production_invariant: bool,
    guarded_by_production: bool,
    theorem_gap: bool,
    classification: Classification,
    action: Action,
    promotion_target: PromotionTarget,
}

#[derive(Clone, Debug, Deserialize)]
struct GeneratedFixtureSet {
    fixtures: Vec<GeneratedFixture>,
}

#[derive(Clone, Debug, Deserialize)]
struct GeneratedFixture {
    id: String,
    classification: String,
    threat_family: String,
    promotion_target: String,
    initial_budget: i64,
    events: Vec<GeneratedEvent>,
    expected_total_cost: i64,
    expected_event_count: u64,
    expects_invalid_admission: bool,
    expects_oop: bool,
    #[serde(default)]
    settlement: serde_json::Value,
    #[serde(default)]
    replay_mutations: Vec<String>,
    #[serde(default)]
    coverage_features: Vec<String>,
    #[serde(default)]
    source_seed: serde_json::Value,
    #[serde(default)]
    attack_campaign: String,
    #[serde(default)]
    oracle_kind: String,
    #[serde(default)]
    production_path: String,
    #[serde(default)]
    campaign_steps: Vec<String>,
    #[serde(default)]
    minimized_input_digest: String,
    #[serde(default)]
    reproducer_command: String,
    #[serde(default)]
    trace_digest_fields: Vec<String>,
    #[serde(default)]
    source_file: String,
    #[serde(default)]
    source_line: u64,
    #[serde(default)]
    source_symbol: String,
    #[serde(default)]
    cost_surface: String,
    #[serde(default)]
    source_risk: String,
    #[serde(default)]
    reachable_from_user_deploy: bool,
    #[serde(default)]
    source_surface_status: String,
    #[serde(default)]
    oracle_surface: String,
    #[serde(default)]
    mutation_axis: String,
    #[serde(default)]
    expected_disposition: String,
    #[serde(default)]
    source_facets: Vec<String>,
    #[serde(default)]
    source_anchor_digest: String,
    #[serde(default)]
    cross_surface_role: String,
    #[serde(default)]
    semantic_oracle: String,
    #[serde(default)]
    security_surface: String,
    #[serde(default)]
    external_input_kind: String,
    #[serde(default)]
    auth_boundary: String,
    #[serde(default)]
    replay_boundary: String,
    #[serde(default)]
    slashing_authorization: serde_json::Value,
    #[serde(default)]
    secret_material_touched: bool,
    #[serde(default)]
    source_anchor_status: String,
    #[serde(default)]
    dependency_advisory_id: String,
}

#[derive(Clone, Debug, Deserialize)]
struct GeneratedEvent {
    kind: String,
    weight: u64,
    #[serde(default)]
    descriptor: String,
    #[serde(default)]
    primitive_descriptor: Option<String>,
    #[serde(default)]
    deploy: u8,
    #[serde(default)]
    path: Vec<u32>,
    #[serde(default)]
    redex_id: Option<u64>,
    #[serde(default)]
    local_index: Option<u64>,
}

fn fixtures() -> Vec<FrontierFixture> {
    vec![
        FrontierFixture {
            name: "zero_weight_rejected",
            threat_family: "producer_routing",
            reproduced_in_rust: false,
            violates_production_invariant: false,
            guarded_by_production: false,
            theorem_gap: false,
            classification: Classification::ConfirmedSafe,
            action: Action::Record,
            promotion_target: PromotionTarget::Record,
        },
        FrontierFixture {
            name: "trace_cap_boundary",
            threat_family: "concurrency_schedule",
            reproduced_in_rust: false,
            violates_production_invariant: false,
            guarded_by_production: false,
            theorem_gap: true,
            classification: Classification::ProofOrModelStrengthening,
            action: Action::StrengthenFormal,
            promotion_target: PromotionTarget::Rocq,
        },
        FrontierFixture {
            name: "repeated_oop_boundary",
            threat_family: "concurrency_schedule",
            reproduced_in_rust: true,
            violates_production_invariant: false,
            guarded_by_production: false,
            theorem_gap: true,
            classification: Classification::ProofOrModelStrengthening,
            action: Action::StrengthenFormal,
            promotion_target: PromotionTarget::Tla,
        },
        FrontierFixture {
            name: "producer_routing_guard",
            threat_family: "producer_routing",
            reproduced_in_rust: false,
            violates_production_invariant: false,
            guarded_by_production: true,
            theorem_gap: false,
            classification: Classification::ProjectionRisk,
            action: Action::Guard,
            promotion_target: PromotionTarget::RustGuard,
        },
        FrontierFixture {
            name: "replay_field_mutation",
            threat_family: "replay_authentication",
            reproduced_in_rust: false,
            violates_production_invariant: false,
            guarded_by_production: false,
            theorem_gap: false,
            classification: Classification::Bisimilar,
            action: Action::Record,
            promotion_target: PromotionTarget::Record,
        },
        FrontierFixture {
            name: "multi_deploy_settlement",
            threat_family: "settlement",
            reproduced_in_rust: true,
            violates_production_invariant: false,
            guarded_by_production: false,
            theorem_gap: true,
            classification: Classification::ProofOrModelStrengthening,
            action: Action::StrengthenFormal,
            promotion_target: PromotionTarget::Rocq,
        },
        FrontierFixture {
            name: "slashing_composition",
            threat_family: "slashing_composition",
            reproduced_in_rust: true,
            violates_production_invariant: false,
            guarded_by_production: false,
            theorem_gap: false,
            classification: Classification::ConfirmedSafe,
            action: Action::Record,
            promotion_target: PromotionTarget::Record,
        },
        FrontierFixture {
            name: "resource_exhaustion",
            threat_family: "resource_exhaustion",
            reproduced_in_rust: true,
            violates_production_invariant: false,
            guarded_by_production: false,
            theorem_gap: true,
            classification: Classification::ProofOrModelStrengthening,
            action: Action::StrengthenFormal,
            promotion_target: PromotionTarget::Sage,
        },
        FrontierFixture {
            name: "requires_source_audit",
            threat_family: "search_governance",
            reproduced_in_rust: false,
            violates_production_invariant: false,
            guarded_by_production: false,
            theorem_gap: false,
            classification: Classification::NeedsSourceAudit,
            action: Action::Audit,
            promotion_target: PromotionTarget::Audit,
        },
        FrontierFixture {
            name: "production_invariant_violation",
            threat_family: "search_governance",
            reproduced_in_rust: false,
            violates_production_invariant: true,
            guarded_by_production: false,
            theorem_gap: false,
            classification: Classification::ConfirmedCurrentBug,
            action: Action::FixSource,
            promotion_target: PromotionTarget::SourceFix,
        },
    ]
}

#[test]
fn cost_accounting_frontier_generated_fixtures_are_classified() {
    for fixture in fixtures() {
        assert!(
            !fixture.threat_family.is_empty(),
            "fixture {} must name its threat family",
            fixture.name
        );

        if fixture.action == Action::FixSource {
            assert!(
                fixture.reproduced_in_rust || fixture.violates_production_invariant,
                "fixture {} violates witness-to-source rule",
                fixture.name
            );
        }

        match fixture.classification {
            Classification::ProjectionRisk => {
                assert!(fixture.guarded_by_production);
                assert_eq!(fixture.action, Action::Guard);
                assert_eq!(fixture.promotion_target, PromotionTarget::RustGuard);
            }
            Classification::ProofOrModelStrengthening => {
                assert!(fixture.theorem_gap);
                assert_eq!(fixture.action, Action::StrengthenFormal);
                assert!(matches!(
                    fixture.promotion_target,
                    PromotionTarget::Rocq | PromotionTarget::Tla | PromotionTarget::Sage
                ));
            }
            Classification::NeedsSourceAudit => {
                assert_eq!(fixture.action, Action::Audit);
                assert_eq!(fixture.promotion_target, PromotionTarget::Audit);
            }
            Classification::ConfirmedCurrentBug => {
                assert!(fixture.reproduced_in_rust || fixture.violates_production_invariant);
                assert_eq!(fixture.action, Action::FixSource);
                assert_eq!(fixture.promotion_target, PromotionTarget::SourceFix);
            }
            Classification::ConfirmedSafe | Classification::Bisimilar => {
                assert_eq!(fixture.action, Action::Record);
                assert_eq!(fixture.promotion_target, PromotionTarget::Record);
            }
        }
    }
}

fn generated_event(index: u64, event: &GeneratedEvent) -> BillableTokenEvent {
    let stable_index = event.path.last().copied().map(u64::from).unwrap_or(index);
    let redex_id = event.redex_id.unwrap_or(stable_index);
    let local_index = event.local_index.unwrap_or(stable_index);
    let kind = match event.kind.as_str() {
        "source" => BillableKind::SourceStep,
        "substitution" => BillableKind::Substitution,
        _ => BillableKind::Primitive(
            event
                .primitive_descriptor
                .clone()
                .unwrap_or_else(|| event.descriptor.clone()),
        ),
    };
    BillableTokenEvent {
        deploy_id: [event.deploy; 32],
        // D0: per-deploy lane key (constant within a deploy), keyed off the
        // generated deploy tag so distinct deploys get distinct lane keys.
        sig_hash: [event.deploy; 32],
        source_path: SourcePath(event.path.clone()),
        redex_id: RedexId(redex_id),
        local_index,
        kind,
        weight: event.weight,
    }
}

fn trace_digest_field_names() -> Vec<String> {
    [
        "deploy_id",
        "source_path",
        "redex_id",
        "local_index",
        "billable_kind",
        "primitive_descriptor",
        "weight",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn embedded_generated_fixtures() -> Vec<GeneratedFixture> {
    vec![
        GeneratedFixture {
            id: "embedded_valid_parallel_trace".to_string(),
            classification: "confirmed_safe".to_string(),
            threat_family: "concurrency_schedule".to_string(),
            promotion_target: "rust:test".to_string(),
            initial_budget: 8,
            events: vec![
                GeneratedEvent {
                    kind: "source".to_string(),
                    weight: 1,
                    descriptor: "parallel".to_string(),
                    primitive_descriptor: None,
                    deploy: 0,
                    path: vec![0],
                    redex_id: None,
                    local_index: None,
                },
                GeneratedEvent {
                    kind: "primitive".to_string(),
                    weight: 2,
                    descriptor: "parallel-primitive".to_string(),
                    primitive_descriptor: Some("parallel-primitive".to_string()),
                    deploy: 0,
                    path: vec![1],
                    redex_id: None,
                    local_index: None,
                },
            ],
            expected_total_cost: 3,
            expected_event_count: 2,
            expects_invalid_admission: false,
            expects_oop: false,
            settlement: serde_json::Value::Null,
            replay_mutations: Vec::new(),
            coverage_features: Vec::new(),
            source_seed: serde_json::Value::Null,
            attack_campaign: String::new(),
            oracle_kind: String::new(),
            production_path: String::new(),
            campaign_steps: Vec::new(),
            minimized_input_digest: String::new(),
            reproducer_command: String::new(),
            trace_digest_fields: trace_digest_field_names(),
            source_file: String::new(),
            source_line: 0,
            source_symbol: String::new(),
            cost_surface: String::new(),
            source_risk: String::new(),
            reachable_from_user_deploy: false,
            source_surface_status: String::new(),
            oracle_surface: String::new(),
            mutation_axis: String::new(),
            expected_disposition: String::new(),
            source_facets: Vec::new(),
            source_anchor_digest: String::new(),
            cross_surface_role: String::new(),
            semantic_oracle: String::new(),
            security_surface: String::new(),
            external_input_kind: String::new(),
            auth_boundary: String::new(),
            replay_boundary: String::new(),
            slashing_authorization: serde_json::Value::Null,
            secret_material_touched: false,
            source_anchor_status: String::new(),
            dependency_advisory_id: String::new(),
        },
        GeneratedFixture {
            id: "embedded_zero_weight_rejected".to_string(),
            classification: "confirmed_safe".to_string(),
            threat_family: "producer_routing".to_string(),
            promotion_target: "rust:fuzz".to_string(),
            initial_budget: 4,
            events: vec![GeneratedEvent {
                kind: "primitive".to_string(),
                weight: 0,
                descriptor: "empty-variable-work".to_string(),
                primitive_descriptor: Some("empty-variable-work".to_string()),
                deploy: 0,
                path: vec![],
                redex_id: None,
                local_index: None,
            }],
            expected_total_cost: 0,
            expected_event_count: 0,
            expects_invalid_admission: true,
            expects_oop: false,
            settlement: serde_json::Value::Null,
            replay_mutations: Vec::new(),
            coverage_features: Vec::new(),
            source_seed: serde_json::Value::Null,
            attack_campaign: String::new(),
            oracle_kind: String::new(),
            production_path: String::new(),
            campaign_steps: Vec::new(),
            minimized_input_digest: String::new(),
            reproducer_command: String::new(),
            trace_digest_fields: Vec::new(),
            source_file: String::new(),
            source_line: 0,
            source_symbol: String::new(),
            cost_surface: String::new(),
            source_risk: String::new(),
            reachable_from_user_deploy: false,
            source_surface_status: String::new(),
            oracle_surface: String::new(),
            mutation_axis: String::new(),
            expected_disposition: String::new(),
            source_facets: Vec::new(),
            source_anchor_digest: String::new(),
            cross_surface_role: String::new(),
            semantic_oracle: String::new(),
            security_surface: String::new(),
            external_input_kind: String::new(),
            auth_boundary: String::new(),
            replay_boundary: String::new(),
            slashing_authorization: serde_json::Value::Null,
            secret_material_touched: false,
            source_anchor_status: String::new(),
            dependency_advisory_id: String::new(),
        },
        GeneratedFixture {
            id: "embedded_oop_boundary".to_string(),
            classification: "proof_or_model_strengthening".to_string(),
            threat_family: "concurrency_schedule".to_string(),
            promotion_target: "tla:RuntimeBudgetReplay".to_string(),
            initial_budget: 3,
            events: vec![GeneratedEvent {
                kind: "source".to_string(),
                weight: 5,
                descriptor: "oop".to_string(),
                primitive_descriptor: None,
                deploy: 0,
                path: vec![0],
                redex_id: None,
                local_index: None,
            }],
            expected_total_cost: 3,
            expected_event_count: 1,
            expects_invalid_admission: false,
            expects_oop: true,
            settlement: serde_json::Value::Null,
            replay_mutations: Vec::new(),
            coverage_features: Vec::new(),
            source_seed: serde_json::Value::Null,
            attack_campaign: String::new(),
            oracle_kind: String::new(),
            production_path: String::new(),
            campaign_steps: Vec::new(),
            minimized_input_digest: String::new(),
            reproducer_command: String::new(),
            trace_digest_fields: trace_digest_field_names(),
            source_file: String::new(),
            source_line: 0,
            source_symbol: String::new(),
            cost_surface: String::new(),
            source_risk: String::new(),
            reachable_from_user_deploy: false,
            source_surface_status: String::new(),
            oracle_surface: String::new(),
            mutation_axis: String::new(),
            expected_disposition: String::new(),
            source_facets: Vec::new(),
            source_anchor_digest: String::new(),
            cross_surface_role: String::new(),
            semantic_oracle: String::new(),
            security_surface: String::new(),
            external_input_kind: String::new(),
            auth_boundary: String::new(),
            replay_boundary: String::new(),
            slashing_authorization: serde_json::Value::Null,
            secret_material_touched: false,
            source_anchor_status: String::new(),
            dependency_advisory_id: String::new(),
        },
        GeneratedFixture {
            id: "embedded_descriptor_bound".to_string(),
            classification: "confirmed_safe".to_string(),
            threat_family: "resource_exhaustion".to_string(),
            promotion_target: "rust:fuzz".to_string(),
            initial_budget: 4,
            events: vec![GeneratedEvent {
                kind: "primitive".to_string(),
                weight: 1,
                descriptor: "x".repeat(513),
                primitive_descriptor: Some("x".repeat(513)),
                deploy: 0,
                path: vec![],
                redex_id: None,
                local_index: None,
            }],
            expected_total_cost: 0,
            expected_event_count: 0,
            expects_invalid_admission: true,
            expects_oop: false,
            settlement: serde_json::Value::Null,
            replay_mutations: Vec::new(),
            coverage_features: Vec::new(),
            source_seed: serde_json::Value::Null,
            attack_campaign: String::new(),
            oracle_kind: String::new(),
            production_path: String::new(),
            campaign_steps: Vec::new(),
            minimized_input_digest: String::new(),
            reproducer_command: String::new(),
            trace_digest_fields: Vec::new(),
            source_file: String::new(),
            source_line: 0,
            source_symbol: String::new(),
            cost_surface: String::new(),
            source_risk: String::new(),
            reachable_from_user_deploy: false,
            source_surface_status: String::new(),
            oracle_surface: String::new(),
            mutation_axis: String::new(),
            expected_disposition: String::new(),
            source_facets: Vec::new(),
            source_anchor_digest: String::new(),
            cross_surface_role: String::new(),
            semantic_oracle: String::new(),
            security_surface: String::new(),
            external_input_kind: String::new(),
            auth_boundary: String::new(),
            replay_boundary: String::new(),
            slashing_authorization: serde_json::Value::Null,
            secret_material_touched: false,
            source_anchor_status: String::new(),
            dependency_advisory_id: String::new(),
        },
        GeneratedFixture {
            id: "embedded_source_path_bound".to_string(),
            classification: "confirmed_safe".to_string(),
            threat_family: "resource_exhaustion".to_string(),
            promotion_target: "rust:fuzz".to_string(),
            initial_budget: 4,
            events: vec![GeneratedEvent {
                kind: "source".to_string(),
                weight: 1,
                descriptor: "source-path-bound".to_string(),
                primitive_descriptor: None,
                deploy: 0,
                path: vec![0; 1025],
                redex_id: None,
                local_index: None,
            }],
            expected_total_cost: 0,
            expected_event_count: 0,
            expects_invalid_admission: true,
            expects_oop: false,
            settlement: serde_json::Value::Null,
            replay_mutations: Vec::new(),
            coverage_features: Vec::new(),
            source_seed: serde_json::Value::Null,
            attack_campaign: String::new(),
            oracle_kind: String::new(),
            production_path: String::new(),
            campaign_steps: Vec::new(),
            minimized_input_digest: String::new(),
            reproducer_command: String::new(),
            trace_digest_fields: Vec::new(),
            source_file: String::new(),
            source_line: 0,
            source_symbol: String::new(),
            cost_surface: String::new(),
            source_risk: String::new(),
            reachable_from_user_deploy: false,
            source_surface_status: String::new(),
            oracle_surface: String::new(),
            mutation_axis: String::new(),
            expected_disposition: String::new(),
            source_facets: Vec::new(),
            source_anchor_digest: String::new(),
            cross_surface_role: String::new(),
            semantic_oracle: String::new(),
            security_surface: String::new(),
            external_input_kind: String::new(),
            auth_boundary: String::new(),
            replay_boundary: String::new(),
            slashing_authorization: serde_json::Value::Null,
            secret_material_touched: false,
            source_anchor_status: String::new(),
            dependency_advisory_id: String::new(),
        },
        GeneratedFixture {
            id: "embedded_v3_stateful_campaign".to_string(),
            classification: "proof_or_model_strengthening".to_string(),
            threat_family: "stateful_campaign".to_string(),
            promotion_target: "rust:test".to_string(),
            initial_budget: 6,
            events: vec![
                GeneratedEvent {
                    kind: "source".to_string(),
                    weight: 2,
                    descriptor: "stateful/source".to_string(),
                    primitive_descriptor: None,
                    deploy: 0,
                    path: vec![0],
                    redex_id: None,
                    local_index: None,
                },
                GeneratedEvent {
                    kind: "primitive".to_string(),
                    weight: 1,
                    descriptor: "stateful/primitive".to_string(),
                    primitive_descriptor: Some("stateful/primitive".to_string()),
                    deploy: 0,
                    path: vec![1],
                    redex_id: None,
                    local_index: None,
                },
            ],
            expected_total_cost: 3,
            expected_event_count: 2,
            expects_invalid_admission: false,
            expects_oop: false,
            settlement: serde_json::json!({
                "escrow": 12,
                "token_cost": 6,
                "refund": 6,
                "authority": "casper"
            }),
            replay_mutations: Vec::new(),
            coverage_features: vec![
                "stateful".to_string(),
                "production_path".to_string(),
                "oracle".to_string(),
                "campaign_steps".to_string(),
                "attack_campaign".to_string(),
            ],
            source_seed: serde_json::Value::Null,
            attack_campaign: "stateful_budget_lifecycle".to_string(),
            oracle_kind: "stateful_runtime_budget_campaign".to_string(),
            production_path: "rholang::RuntimeBudget::reserve_canonical".to_string(),
            campaign_steps: vec![
                "precharge".to_string(),
                "reserve".to_string(),
                "finalize".to_string(),
                "replay".to_string(),
                "settle".to_string(),
            ],
            minimized_input_digest: "embedded-v3-stateful".to_string(),
            reproducer_command:
                "cargo nextest run -p rholang generated_frontier_stateful_campaign_fixtures_hold"
                    .to_string(),
            trace_digest_fields: trace_digest_field_names(),
            source_file: String::new(),
            source_line: 0,
            source_symbol: String::new(),
            cost_surface: String::new(),
            source_risk: String::new(),
            reachable_from_user_deploy: false,
            source_surface_status: String::new(),
            oracle_surface: String::new(),
            mutation_axis: String::new(),
            expected_disposition: String::new(),
            source_facets: Vec::new(),
            source_anchor_digest: String::new(),
            cross_surface_role: String::new(),
            semantic_oracle: String::new(),
            security_surface: String::new(),
            external_input_kind: String::new(),
            auth_boundary: String::new(),
            replay_boundary: String::new(),
            slashing_authorization: serde_json::Value::Null,
            secret_material_touched: false,
            source_anchor_status: String::new(),
            dependency_advisory_id: String::new(),
        },
    ]
}

fn generated_fixtures_from_env() -> Vec<GeneratedFixture> {
    let Ok(path) = env::var("COST_ACCOUNTING_FRONTIER_FIXTURES_JSON") else {
        return Vec::new();
    };
    let content = fs::read_to_string(&path).expect("generated cost-accounting fixture json");
    serde_json::from_str::<GeneratedFixtureSet>(&content)
        .expect("generated cost-accounting fixture schema")
        .fixtures
}

fn assert_terminal_classification(fixture: &GeneratedFixture) {
    assert!(
        matches!(
            fixture.classification.as_str(),
            "confirmed_safe"
                | "bisimilar"
                | "projection_risk"
                | "assumption_counterexample"
                | "proof_or_model_strengthening"
                | "needs_source_audit"
                | "confirmed_current_bug"
        ),
        "fixture {} has unknown classification {}",
        fixture.id,
        fixture.classification
    );
    assert_ne!(
        fixture.classification, "unexpected",
        "fixture {} must be classified before replay",
        fixture.id
    );
    assert!(
        !fixture.threat_family.is_empty(),
        "fixture {} must name its threat family",
        fixture.id
    );
    assert!(
        !fixture.promotion_target.is_empty() && fixture.promotion_target != "none",
        "fixture {} must name its promotion target",
        fixture.id
    );
}

fn replay_generated_fixture(fixture: &GeneratedFixture) {
    assert_terminal_classification(fixture);
    let budget = RuntimeBudget::new(Cost::create(
        fixture.initial_budget,
        format!("generated fixture {}", fixture.id),
    ));
    let mut saw_error = false;
    for (index, event) in fixture.events.iter().enumerate() {
        let result = budget.reserve_canonical(generated_event(index as u64, event));
        if result.is_err() {
            saw_error = true;
        }
    }
    assert_eq!(
        budget.total_cost().value,
        fixture.expected_total_cost,
        "fixture {} total cost",
        fixture.id
    );
    assert_eq!(
        budget.cost_trace_event_count(),
        fixture.expected_event_count,
        "fixture {} event count",
        fixture.id
    );
    if fixture.expects_invalid_admission || fixture.expects_oop {
        assert!(
            saw_error,
            "fixture {} expected a rejected reservation",
            fixture.id
        );
    }
    if fixture.expects_oop {
        assert!(
            budget.last_oop_event().is_some(),
            "fixture {} expected OOP boundary evidence",
            fixture.id
        );
    }
}

#[test]
fn generated_frontier_replay_fixtures_hold() {
    let mut fixtures = embedded_generated_fixtures();
    fixtures.extend(generated_fixtures_from_env());
    assert!(!fixtures.is_empty());
    for fixture in &fixtures {
        replay_generated_fixture(fixture);
    }
}

fn trace_digest_for(
    events: &[GeneratedEvent],
) -> rholang::rust::interpreter::accounting::CostTraceDigest {
    trace_digest_with_budget(events, 16).expect("metamorphic fixture event")
}

fn trace_digest_with_budget(
    events: &[GeneratedEvent],
    initial_budget: i64,
) -> Option<rholang::rust::interpreter::accounting::CostTraceDigest> {
    let budget = RuntimeBudget::new(Cost::create(initial_budget, "metamorphic trace fixture"));
    for (index, event) in events.iter().enumerate() {
        if budget
            .reserve_canonical(generated_event(index as u64, event))
            .is_err()
        {
            return None;
        }
    }
    Some(budget.cost_trace_digest())
}

fn settlement_i64(settlement: &serde_json::Value, field: &str) -> Option<i64> {
    settlement.get(field).and_then(serde_json::Value::as_i64)
}

fn assert_settlement_projection(fixture: &GeneratedFixture) {
    if !fixture.settlement.is_object()
        || fixture.settlement.as_object().is_some_and(|v| v.is_empty())
    {
        return;
    }
    let Some(escrow) = settlement_i64(&fixture.settlement, "escrow") else {
        return;
    };
    let Some(token_cost) = settlement_i64(&fixture.settlement, "token_cost") else {
        return;
    };
    let Some(refund) = settlement_i64(&fixture.settlement, "refund") else {
        return;
    };
    assert!(escrow >= 0, "fixture {} settlement escrow", fixture.id);
    assert!(
        token_cost >= 0,
        "fixture {} settlement token cost",
        fixture.id
    );
    assert!(refund >= 0, "fixture {} settlement refund", fixture.id);
    assert!(
        refund <= escrow,
        "fixture {} refund must stay bounded by escrow",
        fixture.id
    );
    assert_eq!(
        refund,
        if token_cost >= escrow {
            0
        } else {
            escrow - token_cost
        },
        "fixture {} settlement refund projection",
        fixture.id
    );
}

fn valid_success_fixture(fixture: &GeneratedFixture) -> bool {
    !fixture.expects_invalid_admission
        && !fixture.expects_oop
        && !fixture.events.is_empty()
        && fixture.expected_event_count == fixture.events.len() as u64
}

fn event_weight_sum(events: &[GeneratedEvent]) -> i64 {
    events.iter().map(|event| event.weight as i64).sum()
}

fn mutate_replay_identity(events: &[GeneratedEvent]) -> Vec<GeneratedEvent> {
    let mut mutated = events.to_vec();
    if let Some(index) = mutated.iter().position(|event| event.kind == "primitive") {
        if let Some(primitive_descriptor) = &mut mutated[index].primitive_descriptor {
            primitive_descriptor.push_str("-mutated");
        } else {
            mutated[index].descriptor.push_str("-mutated");
        }
    } else if let Some(first) = mutated.first_mut() {
        first.path.push(255);
    }
    mutated
}

fn replay_identity_field_mutations(
    events: &[GeneratedEvent],
) -> Vec<(&'static str, Vec<GeneratedEvent>)> {
    let mut variants = Vec::new();
    if events.is_empty() {
        return variants;
    }

    let mut deploy = events.to_vec();
    deploy[0].deploy = deploy[0].deploy.wrapping_add(1);
    variants.push(("deploy_id", deploy));

    let mut source_path = events.to_vec();
    source_path[0].path.push(255);
    variants.push(("source_path", source_path));

    let mut redex_id = events.to_vec();
    let next_redex_id = redex_id[0]
        .redex_id
        .unwrap_or_else(|| redex_id[0].path.last().copied().map(u64::from).unwrap_or(0))
        .wrapping_add(17);
    redex_id[0].redex_id = Some(next_redex_id);
    variants.push(("redex_id", redex_id));

    let mut local_index = events.to_vec();
    let next_local_index = local_index[0]
        .local_index
        .unwrap_or_else(|| {
            local_index[0]
                .path
                .last()
                .copied()
                .map(u64::from)
                .unwrap_or(0)
        })
        .wrapping_add(19);
    local_index[0].local_index = Some(next_local_index);
    variants.push(("local_index", local_index));

    let mut kind = events.to_vec();
    kind[0].kind = if kind[0].kind == "source" {
        "substitution".to_string()
    } else {
        "source".to_string()
    };
    variants.push(("billable_kind", kind));

    if let Some(index) = events.iter().position(|event| event.kind == "primitive") {
        let mut descriptor = events.to_vec();
        if let Some(primitive_descriptor) = &mut descriptor[index].primitive_descriptor {
            primitive_descriptor.push_str("-mutated");
        } else {
            descriptor[index].descriptor.push_str("-mutated");
        }
        variants.push(("primitive_descriptor", descriptor));
    }

    let mut weight = events.to_vec();
    weight[0].weight += 1;
    variants.push(("weight", weight));

    variants
}

fn assert_stateful_campaign_metadata(fixture: &GeneratedFixture) {
    let is_v3_fixture = fixture.coverage_features.iter().any(|feature| {
        matches!(
            feature.as_str(),
            "stateful"
                | "production_path"
                | "oracle"
                | "source_corpus"
                | "exploit_cross_product"
                | "campaign_steps"
        )
    }) || matches!(
        fixture.threat_family.as_str(),
        "stateful_campaign" | "production_path_diff" | "source_corpus" | "exploit_cross_product"
    );

    if !is_v3_fixture {
        return;
    }

    assert!(
        !fixture.oracle_kind.is_empty(),
        "fixture {} must name its oracle kind",
        fixture.id
    );
    assert!(
        !fixture.production_path.is_empty(),
        "fixture {} must name the production path it checks",
        fixture.id
    );
    assert!(
        !fixture.campaign_steps.is_empty(),
        "fixture {} must carry minimized campaign steps",
        fixture.id
    );
    assert!(
        !fixture.minimized_input_digest.is_empty(),
        "fixture {} must carry a minimized-input digest",
        fixture.id
    );
    assert!(
        !fixture.reproducer_command.is_empty(),
        "fixture {} must carry a reproducer command",
        fixture.id
    );
    assert!(
        fixture.reproducer_command.contains("cargo")
            || fixture.reproducer_command.contains("sage")
            || fixture.reproducer_command.contains("nextest"),
        "fixture {} reproducer command must name an executable replay path",
        fixture.id
    );

    if fixture.threat_family == "stateful_campaign" {
        assert!(
            fixture.campaign_steps.iter().any(|step| step == "reserve"),
            "fixture {} stateful campaign must include reservation",
            fixture.id
        );
        assert!(
            fixture.campaign_steps.iter().any(|step| step == "finalize"),
            "fixture {} stateful campaign must include finalization",
            fixture.id
        );
    }

    if fixture.threat_family == "production_path_diff" {
        assert!(
            fixture.production_path.contains("ProcessedDeploy")
                || fixture.production_path.contains("RuntimeBudget"),
            "fixture {} production diff must name the production boundary",
            fixture.id
        );
    }

    if fixture.threat_family == "exploit_cross_product" {
        assert!(
            fixture.campaign_steps.iter().any(|step| step == "slash")
                && fixture.campaign_steps.iter().any(|step| step == "settle"),
            "fixture {} exploit cross-product must include slash and settlement phases",
            fixture.id
        );
    }
}

#[test]
fn generated_frontier_differential_fixtures_hold() {
    let mut fixtures = embedded_generated_fixtures();
    fixtures.extend(generated_fixtures_from_env());
    let mut checked_success_projection = false;
    let mut checked_primitive_descriptor_projection = false;
    for fixture in &fixtures {
        replay_generated_fixture(fixture);
        assert_settlement_projection(fixture);
        assert!(
            fixture.attack_campaign.is_empty()
                || fixture
                    .coverage_features
                    .iter()
                    .any(|feature| feature == "attack_campaign"),
            "fixture {} campaign must be reflected in coverage features",
            fixture.id
        );
        let has_source_seed = !fixture.source_seed.is_null()
            && !fixture
                .source_seed
                .as_object()
                .is_some_and(serde_json::Map::is_empty);
        if has_source_seed {
            assert!(
                fixture
                    .source_seed
                    .get("seeds")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|seeds| !seeds.is_empty()),
                "fixture {} source seed projection",
                fixture.id
            );
        }
        if valid_success_fixture(fixture) {
            checked_success_projection = true;
            checked_primitive_descriptor_projection |=
                fixture.events.iter().any(|event| event.kind == "primitive");
            for field in trace_digest_field_names() {
                assert!(
                    fixture.trace_digest_fields.is_empty()
                        || fixture.trace_digest_fields.contains(&field),
                    "fixture {} trace digest coverage missing field {}",
                    fixture.id,
                    field
                );
            }
            let budget = event_weight_sum(&fixture.events) + 16;
            let forward = trace_digest_with_budget(&fixture.events, budget)
                .expect("valid fixture forward trace");
            let mut reversed = fixture.events.clone();
            reversed.reverse();
            let reverse =
                trace_digest_with_budget(&reversed, budget).expect("valid fixture reversed trace");
            assert_eq!(forward, reverse, "fixture {} canonical order", fixture.id);

            let mut duplicated = fixture.events.clone();
            duplicated.push(fixture.events[0].clone());
            let duplicated_digest = trace_digest_with_budget(&duplicated, budget + 16)
                .expect("valid fixture duplicated trace");
            assert_ne!(
                forward, duplicated_digest,
                "fixture {} duplicate event must affect trace evidence",
                fixture.id
            );

            let mutated = mutate_replay_identity(&fixture.events);
            let mutated_digest =
                trace_digest_with_budget(&mutated, budget).expect("valid fixture mutated trace");
            assert_ne!(
                forward, mutated_digest,
                "fixture {} replay identity mutation must affect trace evidence",
                fixture.id
            );

            for (field, mutated) in replay_identity_field_mutations(&fixture.events) {
                let mutated_budget = event_weight_sum(&mutated) + 16;
                let mutated_digest = trace_digest_with_budget(&mutated, mutated_budget)
                    .expect("valid fixture field-mutated trace");
                assert_ne!(
                    forward, mutated_digest,
                    "fixture {} {} mutation must affect trace evidence",
                    fixture.id, field
                );
            }
        }
    }
    assert!(checked_success_projection);
    assert!(checked_primitive_descriptor_projection);
}

#[test]
fn generated_frontier_stateful_campaign_fixtures_hold() {
    let mut fixtures = embedded_generated_fixtures();
    fixtures.extend(generated_fixtures_from_env());
    let mut checked_v3_fixture = false;
    for fixture in &fixtures {
        replay_generated_fixture(fixture);
        assert_settlement_projection(fixture);
        assert_stateful_campaign_metadata(fixture);
        checked_v3_fixture |= !fixture.oracle_kind.is_empty()
            || !fixture.production_path.is_empty()
            || !fixture.campaign_steps.is_empty();
    }
    assert!(checked_v3_fixture);
}

#[test]
fn generated_frontier_metamorphic_fixtures_hold() {
    let events = vec![
        GeneratedEvent {
            kind: "source".to_string(),
            weight: 1,
            descriptor: "parallel".to_string(),
            primitive_descriptor: None,
            deploy: 1,
            path: vec![0],
            redex_id: None,
            local_index: None,
        },
        GeneratedEvent {
            kind: "primitive".to_string(),
            weight: 2,
            descriptor: "parallel-primitive".to_string(),
            primitive_descriptor: Some("parallel-primitive".to_string()),
            deploy: 1,
            path: vec![1],
            redex_id: None,
            local_index: None,
        },
        GeneratedEvent {
            kind: "substitution".to_string(),
            weight: 1,
            descriptor: "parallel-substitution".to_string(),
            primitive_descriptor: None,
            deploy: 1,
            path: vec![2],
            redex_id: None,
            local_index: None,
        },
    ];
    let reversed = events.iter().cloned().rev().collect::<Vec<_>>();
    let forward_digest = trace_digest_for(&events);
    let reversed_digest = trace_digest_for(&reversed);
    assert_eq!(forward_digest, reversed_digest);

    let mut duplicated = events.clone();
    duplicated.push(events[0].clone());
    let duplicated_digest = trace_digest_for(&duplicated);
    assert_ne!(forward_digest, duplicated_digest);

    let mut renamed = events.clone();
    renamed[1].primitive_descriptor = Some("parallel-primitive-renamed".to_string());
    let renamed_digest = trace_digest_for(&renamed);
    assert_ne!(forward_digest, renamed_digest);

    for (field, mutated) in replay_identity_field_mutations(&events) {
        let mutated_budget = event_weight_sum(&mutated) + 16;
        let mutated_digest = trace_digest_with_budget(&mutated, mutated_budget)
            .expect("field-mutated metamorphic event");
        assert_ne!(
            forward_digest, mutated_digest,
            "{} mutation must affect trace evidence",
            field
        );
    }
}

fn embedded_horizon_v10_fixtures() -> Vec<GeneratedFixture> {
    serde_json::from_str::<GeneratedFixtureSet>(include_str!("horizon_v10_fixtures.json"))
        .expect("embedded horizon v10 cost-accounting fixture schema")
        .fixtures
}

fn embedded_horizon_v11_fixtures() -> Vec<GeneratedFixture> {
    serde_json::from_str::<GeneratedFixtureSet>(include_str!("horizon_v11_fixtures.json"))
        .expect("embedded horizon v11 cost-accounting fixture schema")
        .fixtures
}

fn embedded_horizon_v12_fixtures() -> Vec<GeneratedFixture> {
    serde_json::from_str::<GeneratedFixtureSet>(include_str!("horizon_v12_fixtures.json"))
        .expect("embedded horizon v12 cost-accounting fixture schema")
        .fixtures
}

fn embedded_horizon_v13_fixtures() -> Vec<GeneratedFixture> {
    serde_json::from_str::<GeneratedFixtureSet>(include_str!("horizon_v13_fixtures.json"))
        .expect("embedded horizon v13 cost-accounting fixture schema")
        .fixtures
}

fn embedded_horizon_v14_fixtures() -> Vec<GeneratedFixture> {
    serde_json::from_str::<GeneratedFixtureSet>(include_str!("horizon_v14_fixtures.json"))
        .expect("embedded horizon v14 cost-accounting fixture schema")
        .fixtures
}

fn generated_fixture_corpus() -> Vec<GeneratedFixture> {
    let mut fixtures = embedded_generated_fixtures();
    fixtures.extend(embedded_horizon_v10_fixtures());
    fixtures.extend(embedded_horizon_v11_fixtures());
    fixtures.extend(embedded_horizon_v12_fixtures());
    fixtures.extend(embedded_horizon_v13_fixtures());
    fixtures.extend(embedded_horizon_v14_fixtures());
    fixtures.extend(generated_fixtures_from_env());
    fixtures
}

fn has_feature(fixture: &GeneratedFixture, feature: &str) -> bool {
    fixture
        .coverage_features
        .iter()
        .any(|candidate| candidate == feature)
}

fn is_v11_source_anchored(fixture: &GeneratedFixture) -> bool {
    has_feature(fixture, "source_anchored")
        || !fixture.cost_surface.is_empty()
        || fixture.threat_family.starts_with("source_anchored_")
}

fn is_v12_production_oracle(fixture: &GeneratedFixture) -> bool {
    fixture.semantic_oracle.is_empty()
        && (has_feature(fixture, "production_oracle_surface")
            || !fixture.oracle_surface.is_empty()
            || fixture.threat_family.starts_with("production_oracle_"))
}

fn is_v13_source_semantic_oracle(fixture: &GeneratedFixture) -> bool {
    has_feature(fixture, "source_semantic_oracle")
        || !fixture.semantic_oracle.is_empty()
        || fixture.threat_family.starts_with("source_semantic_")
}

fn is_v14_source_graph_security(fixture: &GeneratedFixture) -> bool {
    has_feature(fixture, "source_graph_security")
        || !fixture.security_surface.is_empty()
        || fixture.threat_family.starts_with("source_graph_")
}

fn assert_v11_source_surface_metadata(fixture: &GeneratedFixture) {
    assert!(
        !fixture.source_file.is_empty(),
        "fixture {} must name the source file it anchors",
        fixture.id
    );
    assert!(
        !fixture.source_symbol.is_empty(),
        "fixture {} must name the source symbol it anchors",
        fixture.id
    );
    assert!(
        !fixture.cost_surface.is_empty(),
        "fixture {} must name the cost surface it covers",
        fixture.id
    );
    assert!(
        !fixture.source_risk.is_empty(),
        "fixture {} must name the source risk it covers",
        fixture.id
    );
    assert!(
        matches!(fixture.source_surface_status.as_str(), "present" | "absent"),
        "fixture {} must classify source-surface presence",
        fixture.id
    );
    if fixture.source_surface_status == "present" {
        assert!(
            fixture.source_line > 0,
            "fixture {} present source surface must include a line anchor",
            fixture.id
        );
    }
    assert!(
        fixture.production_path.contains(&fixture.source_file),
        "fixture {} production path must include source file",
        fixture.id
    );
    assert!(
        fixture.production_path.contains(&fixture.source_symbol),
        "fixture {} production path must include source symbol",
        fixture.id
    );
    if fixture.classification == "confirmed_current_bug" {
        assert!(
            fixture.promotion_target.starts_with("source_fix"),
            "fixture {} confirmed source bug must target source repair",
            fixture.id
        );
    }
    if fixture.classification == "needs_source_audit" {
        assert!(
            fixture.promotion_target.starts_with("audit"),
            "fixture {} source-audit classification must target audit",
            fixture.id
        );
    }
}

fn assert_v12_production_oracle_metadata(fixture: &GeneratedFixture) {
    assert_v11_source_surface_metadata(fixture);
    assert!(
        !fixture.oracle_surface.is_empty(),
        "fixture {} must name the native production oracle surface",
        fixture.id
    );
    assert!(
        !fixture.oracle_kind.is_empty(),
        "fixture {} must name the native production oracle kind",
        fixture.id
    );
    assert!(
        !fixture.mutation_axis.is_empty(),
        "fixture {} must name the searched mutation axis",
        fixture.id
    );
    assert!(
        matches!(
            fixture.expected_disposition.as_str(),
            "accepted"
                | "rejected_before_mutation"
                | "oop_boundary"
                | "replay_invalid"
                | "settlement_bounded"
                | "source_absent"
                | "coverage_adequacy"
        ),
        "fixture {} must have a terminal native-oracle disposition",
        fixture.id
    );
    assert!(
        fixture
            .coverage_features
            .iter()
            .any(|feature| feature == "production_oracle_surface"),
        "fixture {} must expose production-oracle coverage",
        fixture.id
    );
}

fn replay_matching_fixtures(
    name: &str,
    predicate: impl Fn(&GeneratedFixture) -> bool,
) -> Vec<GeneratedFixture> {
    let fixtures = generated_fixture_corpus()
        .into_iter()
        .filter(predicate)
        .collect::<Vec<_>>();
    assert!(
        !fixtures.is_empty(),
        "{name} must have at least one fixture"
    );
    for fixture in &fixtures {
        replay_generated_fixture(fixture);
        assert_settlement_projection(fixture);
    }
    fixtures
}

fn deploy_data_from_fixture(fixture: &GeneratedFixture) -> DeployData {
    let phlo_limit = settlement_i64(&fixture.settlement, "phlo_limit")
        .or_else(|| settlement_i64(&fixture.settlement, "escrow"))
        .unwrap_or(0);
    let phlo_price = settlement_i64(&fixture.settlement, "phlo_price").unwrap_or(1);
    DeployData {
        term: "v12-production-oracle".to_string(),
        time_stamp: 0,
        phlo_price,
        phlo_limit,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
    }
}

fn assert_v12_runtime_budget_oracle(fixture: &GeneratedFixture) {
    let budget = RuntimeBudget::new(Cost::create(
        fixture.initial_budget,
        format!("v12 oracle {}", fixture.id),
    ));
    let mut saw_error = false;
    for (index, event) in fixture.events.iter().enumerate() {
        saw_error |= budget
            .reserve_canonical(generated_event(index as u64, event))
            .is_err();
    }
    match fixture.expected_disposition.as_str() {
        "accepted" => {
            assert!(!saw_error, "fixture {} must be accepted", fixture.id);
            assert_eq!(budget.total_cost().value, fixture.expected_total_cost);
            assert_eq!(
                budget.cost_trace_event_count(),
                fixture.expected_event_count
            );
        }
        "rejected_before_mutation" => {
            assert!(saw_error, "fixture {} must reject", fixture.id);
            assert_eq!(budget.total_cost().value, 0);
            assert_eq!(budget.cost_trace_event_count(), 0);
        }
        "oop_boundary" => {
            assert!(saw_error, "fixture {} must cross OOP", fixture.id);
            assert_eq!(budget.total_cost().value, fixture.initial_budget);
            assert_eq!(budget.cost_trace_event_count(), 1);
            assert!(budget.last_oop_event().is_some());
        }
        other => panic!(
            "fixture {} has unsupported runtime-budget disposition {}",
            fixture.id, other
        ),
    }
}

fn assert_v12_metering_oracle(fixture: &GeneratedFixture) {
    match fixture.oracle_kind.as_str() {
        "metering_canonical_drain" => {
            let budget = RuntimeBudget::new(Cost::create(10, "v12 metering drain"));
            let machine = MeteredMachine::new(budget.clone());
            machine.enqueue_billable(SourcePath(vec![2]), BillableKind::SourceStep, 1);
            machine.enqueue_billable(SourcePath(vec![1]), BillableKind::Substitution, 2);
            machine.drain_canonical().unwrap();
            let event_log = budget.get_event_log();
            assert_eq!(event_log.len(), 2);
            assert_eq!(event_log[0].source_path, SourcePath(vec![1]));
            assert_eq!(event_log[1].source_path, SourcePath(vec![2]));
            assert_eq!(budget.total_cost().value, 3);
        }
        "metering_nonbillable_trace_exclusion" => {
            let budget = RuntimeBudget::new(Cost::create(10, "v12 nonbillable"));
            let machine = MeteredMachine::new(budget.clone());
            let before = budget.cost_trace_digest();
            let key = ContinuationKey {
                deploy_id: [0; 32],
                source_path: SourcePath(vec![0]),
                redex_id: RedexId(0),
            };
            machine.enqueue_frame(MeteredFrame::InstallGate(key));
            machine.drain_canonical().unwrap();
            machine.drain_canonical().unwrap();
            machine.drain_canonical().unwrap();
            assert_eq!(budget.total_cost().value, 0);
            assert_eq!(budget.cost_trace_event_count(), 0);
            assert_eq!(budget.cost_trace_digest(), before);
        }
        other => panic!(
            "fixture {} has unsupported metering oracle {}",
            fixture.id, other
        ),
    }
}

fn assert_v12_parallel_oracle(fixture: &GeneratedFixture) {
    let budget = event_weight_sum(&fixture.events) + 16;
    let forward =
        trace_digest_with_budget(&fixture.events, budget).expect("v12 parallel forward trace");
    let mut reversed = fixture.events.clone();
    reversed.reverse();
    let reverse = trace_digest_with_budget(&reversed, budget).expect("v12 parallel reversed trace");
    assert_eq!(forward, reverse, "fixture {} canonical trace", fixture.id);
}

fn assert_v12_settlement_oracle(fixture: &GeneratedFixture) {
    let deploy = deploy_data_from_fixture(fixture);
    match fixture.oracle_kind.as_str() {
        "settlement_refund_projection" => {
            let token_cost = settlement_i64(&fixture.settlement, "token_cost").unwrap_or(0);
            let refund = deploy
                .refund_amount_for_token_cost(token_cost)
                .expect("v12 settlement refund");
            let escrow = deploy.checked_total_phlo_charge().unwrap();
            assert_eq!(
                refund,
                settlement_i64(&fixture.settlement, "refund").unwrap()
            );
            assert!(refund <= escrow);
            assert_eq!(deploy.phlo_limit.saturating_sub(token_cost), refund);
        }
        "settlement_overflow_rejected" => {
            assert!(deploy.checked_total_phlo_charge().is_err());
        }
        other => panic!(
            "fixture {} has unsupported settlement oracle {}",
            fixture.id, other
        ),
    }
}

fn assert_v12_casper_or_slashing_oracle_shape(fixture: &GeneratedFixture) {
    assert!(
        !fixture.replay_mutations.is_empty(),
        "fixture {} must carry replay mutation axes",
        fixture.id
    );
    assert!(
        fixture
            .replay_mutations
            .iter()
            .any(|field| field == &fixture.mutation_axis),
        "fixture {} replay mutations must include the searched mutation axis",
        fixture.id
    );
    assert!(
        fixture.expected_disposition == "replay_invalid"
            || fixture.expected_disposition == "accepted",
        "fixture {} must classify Casper/slashing replay disposition",
        fixture.id
    );
}

fn assert_v12_legacy_oracle(fixture: &GeneratedFixture) {
    assert_eq!(fixture.source_surface_status, "absent");
    assert_eq!(fixture.source_line, 0);
    assert_eq!(fixture.expected_disposition, "source_absent");
}

fn run_v12_native_oracle(fixture: &GeneratedFixture) {
    assert_v12_production_oracle_metadata(fixture);
    match fixture.oracle_surface.as_str() {
        "runtime_budget" => assert_v12_runtime_budget_oracle(fixture),
        "metering" => assert_v12_metering_oracle(fixture),
        "parallel_eval" => assert_v12_parallel_oracle(fixture),
        "settlement" => assert_v12_settlement_oracle(fixture),
        "casper_replay" | "slashing" => assert_v12_casper_or_slashing_oracle_shape(fixture),
        "legacy_quarantine" => assert_v12_legacy_oracle(fixture),
        "coverage_adequacy" => {}
        other => panic!(
            "fixture {} has unsupported oracle surface {}",
            fixture.id, other
        ),
    }
}

fn assert_v13_source_semantic_metadata(fixture: &GeneratedFixture) {
    assert_v12_production_oracle_metadata(fixture);
    assert!(
        !fixture.semantic_oracle.is_empty(),
        "fixture {} must name the source-semantic oracle",
        fixture.id
    );
    assert!(
        !fixture.source_anchor_digest.is_empty() && fixture.source_anchor_digest.len() >= 16,
        "fixture {} must carry a stable source-anchor digest",
        fixture.id
    );
    assert!(
        !fixture.cross_surface_role.is_empty(),
        "fixture {} must name its cross-surface role",
        fixture.id
    );
    assert!(
        !fixture.source_facets.is_empty(),
        "fixture {} must carry source facets",
        fixture.id
    );
    assert!(
        has_feature(fixture, "source_semantic_oracle"),
        "fixture {} must expose source-semantic coverage",
        fixture.id
    );
    assert!(
        has_feature(fixture, "source_facet"),
        "fixture {} must expose source facet coverage",
        fixture.id
    );
    assert!(
        has_feature(fixture, "source_anchor_digest"),
        "fixture {} must expose source-anchor digest coverage",
        fixture.id
    );
    assert!(
        has_feature(fixture, "cross_surface_role"),
        "fixture {} must expose cross-surface role coverage",
        fixture.id
    );
}

fn assert_v13_runtime_metering_parallel_oracle(fixture: &GeneratedFixture) {
    match fixture.semantic_oracle.as_str() {
        "runtime_to_settlement_fuel_isolation" => {
            let budget = RuntimeBudget::new(Cost::create(
                fixture.initial_budget,
                format!("v13 settlement isolation {}", fixture.id),
            ));
            let event = fixture
                .events
                .first()
                .expect("v13 settlement isolation event");
            assert!(budget.reserve_canonical(generated_event(0, event)).is_err());
            assert_eq!(budget.total_cost().value, fixture.initial_budget);
            assert_eq!(budget.cost_trace_event_count(), 1);
            assert!(budget.last_oop_event().is_some());
            assert_settlement_projection(fixture);
        }
        "metering_to_parallel_digest_stability" => {
            let budget = RuntimeBudget::new(Cost::create(10, "v13 metering parallel"));
            let machine = MeteredMachine::new(budget.clone());
            machine.enqueue_billable(SourcePath(vec![3]), BillableKind::SourceStep, 2);
            machine.enqueue_billable(SourcePath(vec![1]), BillableKind::Substitution, 1);
            machine.enqueue_billable(
                SourcePath(vec![2]),
                BillableKind::Primitive("v13/parallel/c".to_string()),
                1,
            );
            machine.drain_canonical().unwrap();
            let event_log = budget.get_event_log();
            assert_eq!(event_log.len(), 3);
            assert_eq!(event_log[0].source_path, SourcePath(vec![1]));
            assert_eq!(event_log[1].source_path, SourcePath(vec![2]));
            assert_eq!(event_log[2].source_path, SourcePath(vec![3]));

            let trace_budget = event_weight_sum(&fixture.events) + 16;
            let forward = trace_digest_with_budget(&fixture.events, trace_budget)
                .expect("v13 metering-to-parallel forward trace");
            let mut reversed = fixture.events.clone();
            reversed.reverse();
            let reverse = trace_digest_with_budget(&reversed, trace_budget)
                .expect("v13 metering-to-parallel reversed trace");
            assert_eq!(forward, reverse, "fixture {} canonical trace", fixture.id);
        }
        other => panic!(
            "fixture {} has unsupported V13 runtime/metering/parallel oracle {}",
            fixture.id, other
        ),
    }
}

fn assert_v13_casper_settlement_slashing_oracle(fixture: &GeneratedFixture) {
    match fixture.semantic_oracle.as_str() {
        "replay_to_slashing_authentication" => {
            for field in ["slash_fields", "block_hash", "signature"] {
                assert!(
                    fixture
                        .replay_mutations
                        .iter()
                        .any(|mutation| mutation == field),
                    "fixture {} must authenticate replay/slashing field {}",
                    fixture.id,
                    field
                );
            }
            assert_eq!(fixture.expected_disposition, "replay_invalid");
            assert!(fixture
                .source_facets
                .iter()
                .any(|facet| facet == "slashing"));
        }
        "legacy_to_runtime_quarantine" => {
            assert_eq!(fixture.source_surface_status, "absent");
            assert_eq!(fixture.source_line, 0);
            assert_eq!(fixture.expected_disposition, "source_absent");
            assert!(fixture
                .source_facets
                .iter()
                .any(|facet| facet == "legacy_quarantine"));
        }
        other => panic!(
            "fixture {} has unsupported V13 Casper/slashing oracle {}",
            fixture.id, other
        ),
    }
}

fn run_v13_source_semantic_oracle(fixture: &GeneratedFixture) {
    assert_v13_source_semantic_metadata(fixture);
    match fixture.semantic_oracle.as_str() {
        "runtime_to_settlement_fuel_isolation" | "metering_to_parallel_digest_stability" => {
            assert_v13_runtime_metering_parallel_oracle(fixture)
        }
        "replay_to_slashing_authentication" | "legacy_to_runtime_quarantine" => {
            assert_v13_casper_settlement_slashing_oracle(fixture)
        }
        "coverage_adequacy" => {}
        other => panic!(
            "fixture {} has unsupported source-semantic oracle {}",
            fixture.id, other
        ),
    }
}

fn assert_v14_source_graph_metadata(fixture: &GeneratedFixture) {
    assert_v11_source_surface_metadata(fixture);
    assert!(
        !fixture.security_surface.is_empty(),
        "fixture {} must name its security surface",
        fixture.id
    );
    assert!(
        !fixture.external_input_kind.is_empty(),
        "fixture {} must name its external input kind",
        fixture.id
    );
    assert!(
        !fixture.auth_boundary.is_empty(),
        "fixture {} must name its auth boundary",
        fixture.id
    );
    assert!(
        !fixture.replay_boundary.is_empty(),
        "fixture {} must name its replay boundary",
        fixture.id
    );
    assert!(
        !fixture.source_anchor_status.is_empty(),
        "fixture {} must classify its source anchor status",
        fixture.id
    );
    assert_eq!(
        fixture.source_anchor_status, fixture.source_surface_status,
        "fixture {} source anchor status must mirror source surface status",
        fixture.id
    );
    assert!(
        has_feature(fixture, "source_graph_security"),
        "fixture {} must expose source-graph security coverage",
        fixture.id
    );
    assert!(
        has_feature(fixture, "security_surface"),
        "fixture {} must expose security-surface coverage",
        fixture.id
    );
    assert!(
        has_feature(fixture, "auth_boundary"),
        "fixture {} must expose auth-boundary coverage",
        fixture.id
    );
    assert!(
        has_feature(fixture, "replay_boundary"),
        "fixture {} must expose replay-boundary coverage",
        fixture.id
    );
    assert!(
        has_feature(fixture, "source_anchor_status"),
        "fixture {} must expose source-anchor-status coverage",
        fixture.id
    );
    if fixture.secret_material_touched {
        assert_eq!(fixture.cost_surface, "crypto_key_material");
        assert_eq!(fixture.classification, "needs_source_audit");
    }
    if !fixture.dependency_advisory_id.is_empty() {
        assert_eq!(fixture.cost_surface, "dependency_advisory");
        assert!(fixture.dependency_advisory_id.starts_with("RUSTSEC-"));
        assert_eq!(fixture.classification, "needs_source_audit");
    }
}

fn assert_v14_slashing_oracle(fixture: &GeneratedFixture) {
    assert_eq!(fixture.security_surface, "slashing_authorization");
    assert_eq!(fixture.expected_disposition, "replay_invalid");
    assert!(fixture
        .source_facets
        .iter()
        .any(|facet| facet == "slashing"));
    for field in [
        "slash_epoch",
        "slash_fields",
        "target_activation_epoch",
        "evidence_epoch",
        "parent_pre_state_bond",
        "block_hash",
        "signature",
    ] {
        assert!(
            fixture
                .replay_mutations
                .iter()
                .any(|mutation| mutation == field),
            "fixture {} must authenticate slashing field {}",
            fixture.id,
            field
        );
    }
    for facet in ["parent_pre_state", "current_evidence"] {
        assert!(
            fixture.source_facets.iter().any(|value| value == facet),
            "fixture {} must expose slashing source facet {}",
            fixture.id,
            facet
        );
    }
    let auth = &fixture.slashing_authorization;
    let current_epoch = auth
        .get("current_epoch")
        .and_then(serde_json::Value::as_i64);
    assert_eq!(
        auth.get("evidence_epoch")
            .and_then(serde_json::Value::as_i64),
        current_epoch,
        "fixture {} must bind slash evidence to the current epoch",
        fixture.id
    );
    assert_eq!(
        auth.get("target_activation_epoch")
            .and_then(serde_json::Value::as_i64),
        current_epoch,
        "fixture {} must bind target activation to the current epoch",
        fixture.id
    );
    assert!(
        auth.get("parent_pre_state_bond")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
            > 0,
        "fixture {} must carry parent pre-state bond evidence",
        fixture.id
    );
    assert_settlement_projection(fixture);
}

fn assert_v14_mergeable_channel_oracle(fixture: &GeneratedFixture) {
    assert_eq!(fixture.security_surface, "typed_mergeable_channel");
    assert_eq!(fixture.cost_surface, "mergeable_channels");
    assert_eq!(fixture.expected_disposition, "accepted");
    assert!(fixture
        .source_facets
        .iter()
        .any(|facet| facet == "mergeable_channels"));
    assert!(fixture
        .replay_mutations
        .iter()
        .any(|mutation| mutation == "merge_type"));
    match fixture.source_risk.as_str() {
        "typed_bitmask_diff_roundtrip" => {
            for field in ["mergeable_diff", "bitmask_bits"] {
                assert!(
                    fixture
                        .replay_mutations
                        .iter()
                        .any(|mutation| mutation == field),
                    "fixture {} must carry mergeable mutation axis {}",
                    fixture.id,
                    field
                );
            }
        }
        "non_numeric_mergeable_fallback" => {
            for field in ["payload_kind", "conflict_path"] {
                assert!(
                    fixture
                        .replay_mutations
                        .iter()
                        .any(|mutation| mutation == field),
                    "fixture {} must carry non-numeric mergeable axis {}",
                    fixture.id,
                    field
                );
            }
        }
        "merge_type_persistence" => {
            for field in ["serialized_mergeable_entry", "post_state_hash"] {
                assert!(
                    fixture
                        .replay_mutations
                        .iter()
                        .any(|mutation| mutation == field),
                    "fixture {} must carry mergeable persistence axis {}",
                    fixture.id,
                    field
                );
            }
        }
        other => panic!(
            "fixture {} has unsupported V14 mergeable source risk {}",
            fixture.id, other
        ),
    }
}

fn assert_v14_node_security_oracle(fixture: &GeneratedFixture) {
    match fixture.security_surface.as_str() {
        "transport_tls" => {
            assert_eq!(fixture.auth_boundary, "tls_peer_certificate");
            assert!(fixture
                .source_facets
                .iter()
                .any(|facet| facet == "transport_tls"));
            assert!(fixture
                .source_facets
                .iter()
                .any(|facet| facet == "certificate_boundary"));
        }
        "crypto_key_material" => {
            assert!(fixture.secret_material_touched);
            assert_eq!(fixture.classification, "needs_source_audit");
            assert!(fixture
                .replay_mutations
                .iter()
                .any(|mutation| mutation == "secret_material"));
        }
        "dependency_advisory" => {
            assert!(fixture.dependency_advisory_id.starts_with("RUSTSEC-"));
            assert_eq!(fixture.classification, "needs_source_audit");
            assert!(fixture
                .source_facets
                .iter()
                .any(|facet| facet == "dependency_advisory"));
        }
        other => panic!(
            "fixture {} has unsupported V14 node-security surface {}",
            fixture.id, other
        ),
    }
}

fn run_v14_source_graph_oracle(fixture: &GeneratedFixture) {
    assert_v14_source_graph_metadata(fixture);
    match fixture.security_surface.as_str() {
        "slashing_authorization" => assert_v14_slashing_oracle(fixture),
        "typed_mergeable_channel" => assert_v14_mergeable_channel_oracle(fixture),
        "transport_tls" | "crypto_key_material" | "dependency_advisory" => {
            assert_v14_node_security_oracle(fixture)
        }
        "coverage_adequacy" => {}
        other => panic!(
            "fixture {} has unsupported V14 source-graph surface {}",
            fixture.id, other
        ),
    }
}

#[test]
fn generated_frontier_adversarial_fixtures_hold() {
    replay_matching_fixtures("adversarial", |fixture| {
        fixture.threat_family.contains("hybrid_fuzz")
            || has_feature(fixture, "invalid_admission")
            || has_feature(fixture, "negative_mutation")
    });
}

#[test]
fn generated_frontier_property_fixtures_hold() {
    replay_matching_fixtures("property", |fixture| {
        has_feature(fixture, "bounded_depth") || has_feature(fixture, "coverage_adequacy")
    });
}

#[test]
fn generated_frontier_negative_auth_fixtures_hold() {
    let fixtures = replay_matching_fixtures("negative auth", |fixture| {
        has_feature(fixture, "negative_auth")
    });
    assert!(fixtures
        .iter()
        .any(|fixture| has_feature(fixture, "negative_auth")));
}

#[test]
fn generated_frontier_source_shape_fixtures_hold() {
    replay_matching_fixtures("source shape", |fixture| {
        has_feature(fixture, "rho_source")
            || !fixture.source_seed.is_null()
            || fixture.threat_family.contains("corpus")
    });
}

#[test]
fn generated_frontier_production_fixtures_hold() {
    replay_matching_fixtures("production", |fixture| {
        has_feature(fixture, "production_replay_target")
            || fixture.production_path.contains("RuntimeBudget")
            || fixture.promotion_target.starts_with("rust:")
    });
}

#[test]
fn generated_frontier_rholang_eval_fixtures_hold() {
    replay_matching_fixtures("rholang eval", |fixture| {
        has_feature(fixture, "rho_source") || fixture.threat_family.contains("corpus")
    });
}

#[test]
fn generated_frontier_casper_boundary_fixtures_hold() {
    replay_matching_fixtures("casper boundary", |fixture| {
        fixture.threat_family.contains("casper")
            || has_feature(fixture, "settlement")
            || has_feature(fixture, "slashing")
    });
}

#[test]
fn generated_frontier_semantic_eval_fixtures_hold() {
    replay_matching_fixtures("semantic eval", |fixture| {
        has_feature(fixture, "rho_source") && fixture.promotion_target.starts_with("rust:")
    });
}

#[test]
fn generated_frontier_play_replay_fixtures_hold() {
    replay_matching_fixtures("play replay", |fixture| {
        has_feature(fixture, "replay") || has_feature(fixture, "production_replay_target")
    });
}

#[test]
fn generated_frontier_phlo_boundary_fixtures_hold() {
    replay_matching_fixtures("phlo boundary", |fixture| {
        fixture.expects_oop
            || fixture.expects_invalid_admission
            || has_feature(fixture, "invalid_admission")
    });
}

#[test]
fn generated_frontier_state_root_fixtures_hold() {
    replay_matching_fixtures("state root", |fixture| {
        has_feature(fixture, "lifecycle") || has_feature(fixture, "campaign_steps")
    });
}

#[test]
fn generated_frontier_auth_composition_fixtures_hold() {
    replay_matching_fixtures("auth composition", |fixture| {
        has_feature(fixture, "negative_auth")
    });
}

#[test]
fn generated_frontier_generative_semantic_fixtures_hold() {
    replay_matching_fixtures("generative semantic", |fixture| {
        has_feature(fixture, "fuzz_seed_kind") || has_feature(fixture, "mutator_family")
    });
}

#[test]
fn generated_frontier_semantic_metamorphic_fixtures_hold() {
    let fixtures = replay_matching_fixtures("semantic metamorphic", |fixture| {
        has_feature(fixture, "parallel_schedule_stress")
            || fixture.threat_family.contains("parallel")
            || fixture.threat_family.contains("concurrency")
    });
    for fixture in fixtures
        .iter()
        .filter(|fixture| valid_success_fixture(fixture))
    {
        let budget = event_weight_sum(&fixture.events) + 16;
        let forward =
            trace_digest_with_budget(&fixture.events, budget).expect("valid fixture forward trace");
        let mut reversed = fixture.events.clone();
        reversed.reverse();
        let reverse =
            trace_digest_with_budget(&reversed, budget).expect("valid fixture reversed trace");
        assert_eq!(forward, reverse, "fixture {} canonical order", fixture.id);
    }
}

#[test]
fn generated_frontier_external_service_replay_fixtures_hold() {
    replay_matching_fixtures("external service", |fixture| {
        fixture.threat_family.contains("external")
            || has_feature(fixture, "external_service_replay")
    });
}

#[test]
fn generated_frontier_coverage_adequacy_holds() {
    let fixtures = generated_fixture_corpus();
    for feature in [
        "coverage_adequacy",
        "production_replay_target",
        "promotion_gate",
        "bounded_depth",
        "events",
    ] {
        assert!(
            fixtures.iter().any(|fixture| has_feature(fixture, feature)),
            "frontier corpus must cover {feature}"
        );
    }
}

#[test]
fn generated_frontier_corpus_semantic_fixtures_hold() {
    replay_matching_fixtures("corpus semantic", |fixture| {
        has_feature(fixture, "source_seed")
            || has_feature(fixture, "source_corpus_case")
            || fixture.threat_family.contains("corpus")
    });
}

#[test]
fn generated_frontier_grammar_mutation_fixtures_hold() {
    replay_matching_fixtures("grammar mutation", |fixture| {
        has_feature(fixture, "mutator_family") || fixture.threat_family.contains("corpus")
    });
}

#[test]
fn generated_frontier_differential_oracle_fixtures_hold() {
    replay_matching_fixtures("differential oracle", |fixture| {
        !fixture.oracle_kind.is_empty()
            || has_feature(fixture, "production_replay_target")
            || fixture.threat_family.contains("replay")
    });
}

#[test]
fn generated_frontier_external_service_matrix_fixtures_hold() {
    replay_matching_fixtures("external service matrix", |fixture| {
        fixture.threat_family.contains("external") || has_feature(fixture, "mock_external_service")
    });
}

#[test]
fn generated_frontier_casper_security_matrix_fixtures_hold() {
    replay_matching_fixtures("casper security", |fixture| {
        fixture.threat_family.contains("casper")
            || has_feature(fixture, "settlement")
            || has_feature(fixture, "slashing")
            || has_feature(fixture, "negative_auth")
    });
}

#[test]
fn generated_frontier_runtime_trace_interleaving_properties_hold() {
    let fixtures = replay_matching_fixtures("runtime trace interleaving", |fixture| {
        has_feature(fixture, "parallel_schedule_stress")
            || has_feature(fixture, "multi_deploy")
            || fixture.threat_family.contains("parallel")
    });
    assert!(fixtures
        .iter()
        .any(|fixture| has_feature(fixture, "multi_deploy")));
}

#[test]
fn generated_frontier_v9_coverage_adequacy_holds() {
    generated_frontier_coverage_adequacy_holds();
}

#[test]
fn generated_frontier_v10_fuzz_seed_fixtures_hold() {
    let fixtures = replay_matching_fixtures("v10 fuzz seed", |fixture| {
        has_feature(fixture, "fuzz_target") || has_feature(fixture, "kani_harness")
    });
    assert!(fixtures
        .iter()
        .any(|fixture| has_feature(fixture, "fuzz_target")));
}

#[test]
fn generated_frontier_v10_lifecycle_trace_fixtures_hold() {
    let fixtures = replay_matching_fixtures("v10 lifecycle", |fixture| {
        has_feature(fixture, "lifecycle") || fixture.threat_family.contains("lifecycle")
    });
    assert!(fixtures
        .iter()
        .any(|fixture| has_feature(fixture, "campaign_steps")));
}

#[test]
fn generated_frontier_v10_replay_payload_matrix_fixtures_hold() {
    let fixtures = replay_matching_fixtures("v10 replay payload", |fixture| {
        has_feature(fixture, "replay") || fixture.threat_family.contains("replay")
    });
    assert!(fixtures
        .iter()
        .any(|fixture| !fixture.replay_mutations.is_empty()));
}

#[test]
fn generated_frontier_v10_casper_block_auth_fixtures_hold() {
    let fixtures = replay_matching_fixtures("v10 casper block auth", |fixture| {
        fixture.threat_family.contains("casper")
            || has_feature(fixture, "settlement")
            || has_feature(fixture, "slashing")
    });
    assert!(fixtures
        .iter()
        .any(|fixture| fixture.settlement.is_object()));
}

#[test]
fn generated_frontier_v10_parallel_schedule_stress_fixtures_hold() {
    generated_frontier_runtime_trace_interleaving_properties_hold();
}

#[test]
fn generated_frontier_v10_semantic_corpus_mutation_fixtures_hold() {
    generated_frontier_corpus_semantic_fixtures_hold();
}

#[test]
fn generated_frontier_v10_coverage_adequacy_holds() {
    let fixtures = generated_fixture_corpus();
    for feature in [
        "fuzz_target",
        "kani_harness",
        "parallel_schedule_stress",
        "settlement",
        "slashing",
        "legacy_quarantine",
        "negative_auth",
        "coverage_adequacy",
    ] {
        assert!(
            fixtures.iter().any(|fixture| has_feature(fixture, feature)),
            "v10 frontier corpus must cover {feature}"
        );
    }
}

#[test]
fn generated_frontier_v11_source_anchored_fixtures_hold() {
    let fixtures = replay_matching_fixtures("v11 source anchored", is_v11_source_anchored);
    assert!(fixtures
        .iter()
        .any(|fixture| fixture.source_surface_status == "present"));
    assert!(fixtures
        .iter()
        .any(|fixture| fixture.source_surface_status == "absent"));
    for fixture in &fixtures {
        assert_v11_source_surface_metadata(fixture);
    }
}

#[test]
fn generated_frontier_v11_runtime_budget_source_risks_hold() {
    let fixtures = replay_matching_fixtures("v11 runtime budget source risks", |fixture| {
        matches!(
            fixture.cost_surface.as_str(),
            "runtime_budget" | "metering" | "parallel_eval"
        )
    });
    for surface in ["runtime_budget", "metering", "parallel_eval"] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.cost_surface == surface),
            "v11 frontier corpus must cover cost surface {surface}"
        );
    }
    for fixture in &fixtures {
        assert_v11_source_surface_metadata(fixture);
        assert!(
            fixture.reachable_from_user_deploy || fixture.cost_surface == "runtime_budget",
            "fixture {} must justify non-user-reachable runtime anchors explicitly",
            fixture.id
        );
    }
}

#[test]
fn generated_frontier_v11_casper_settlement_slashing_source_risks_hold() {
    let fixtures =
        replay_matching_fixtures("v11 casper settlement slashing source risks", |fixture| {
            matches!(
                fixture.cost_surface.as_str(),
                "casper_replay" | "settlement" | "slashing" | "legacy_quarantine"
            )
        });
    for surface in [
        "casper_replay",
        "settlement",
        "slashing",
        "legacy_quarantine",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.cost_surface == surface),
            "v11 frontier corpus must cover cost surface {surface}"
        );
    }
    assert!(fixtures
        .iter()
        .filter(|fixture| fixture.cost_surface != "legacy_quarantine")
        .all(|fixture| fixture.source_surface_status == "present"));
    assert!(fixtures
        .iter()
        .any(|fixture| fixture.cost_surface == "legacy_quarantine"
            && fixture.source_surface_status == "absent"));
    for fixture in &fixtures {
        assert_v11_source_surface_metadata(fixture);
    }
}

#[test]
fn generated_frontier_v11_coverage_adequacy_holds() {
    let fixtures = generated_fixture_corpus()
        .into_iter()
        .filter(is_v11_source_anchored)
        .collect::<Vec<_>>();
    assert!(
        !fixtures.is_empty(),
        "v11 source-anchored fixtures must be embedded"
    );
    for surface in [
        "runtime_budget",
        "metering",
        "parallel_eval",
        "casper_replay",
        "settlement",
        "slashing",
        "legacy_quarantine",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.cost_surface == surface),
            "v11 frontier corpus must cover cost surface {surface}"
        );
    }
    for feature in [
        "source_anchored",
        "source_file",
        "source_symbol",
        "cost_surface",
        "source_risk",
        "production_replay_target",
        "promotion_gate",
        "coverage_adequacy",
    ] {
        assert!(
            fixtures.iter().any(|fixture| has_feature(fixture, feature)),
            "v11 frontier corpus must cover {feature}"
        );
    }
    assert!(fixtures.iter().any(|fixture| {
        fixture.cost_surface == "coverage_adequacy"
            || has_feature(fixture, "term_family:v11_source_anchored_coverage_adequacy")
    }));
}

#[test]
fn generated_frontier_v12_production_oracle_fixtures_hold() {
    let fixtures = replay_matching_fixtures("v12 production oracle", is_v12_production_oracle);
    assert!(fixtures
        .iter()
        .any(|fixture| fixture.expected_disposition == "accepted"));
    assert!(fixtures
        .iter()
        .any(|fixture| fixture.expected_disposition == "replay_invalid"));
    for fixture in &fixtures {
        run_v12_native_oracle(fixture);
    }
}

#[test]
fn generated_frontier_v12_runtime_metering_parallel_oracles_hold() {
    let fixtures = replay_matching_fixtures("v12 runtime metering parallel", |fixture| {
        fixture.semantic_oracle.is_empty()
            && matches!(
                fixture.oracle_surface.as_str(),
                "runtime_budget" | "metering" | "parallel_eval"
            )
    });
    for surface in ["runtime_budget", "metering", "parallel_eval"] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.oracle_surface == surface),
            "v12 frontier corpus must cover oracle surface {surface}"
        );
    }
    for fixture in &fixtures {
        run_v12_native_oracle(fixture);
    }
}

#[test]
fn generated_frontier_v12_casper_settlement_slashing_oracles_hold() {
    let fixtures = replay_matching_fixtures("v12 casper settlement slashing", |fixture| {
        fixture.semantic_oracle.is_empty()
            && matches!(
                fixture.oracle_surface.as_str(),
                "casper_replay" | "settlement" | "slashing" | "legacy_quarantine"
            )
    });
    for surface in [
        "casper_replay",
        "settlement",
        "slashing",
        "legacy_quarantine",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.oracle_surface == surface),
            "v12 frontier corpus must cover oracle surface {surface}"
        );
    }
    for fixture in &fixtures {
        run_v12_native_oracle(fixture);
    }
}

#[test]
fn generated_frontier_v12_coverage_adequacy_holds() {
    let fixtures = generated_fixture_corpus()
        .into_iter()
        .filter(is_v12_production_oracle)
        .collect::<Vec<_>>();
    assert!(
        !fixtures.is_empty(),
        "v12 production-oracle fixtures must be embedded"
    );
    for surface in [
        "runtime_budget",
        "metering",
        "parallel_eval",
        "casper_replay",
        "settlement",
        "slashing",
        "legacy_quarantine",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.oracle_surface == surface),
            "v12 frontier corpus must cover oracle surface {surface}"
        );
    }
    for disposition in [
        "accepted",
        "rejected_before_mutation",
        "oop_boundary",
        "replay_invalid",
        "settlement_bounded",
        "source_absent",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.expected_disposition == disposition),
            "v12 frontier corpus must cover disposition {disposition}"
        );
    }
    for feature in [
        "production_oracle_surface",
        "mutation_axis",
        "expected_disposition",
        "production_replay_target",
        "promotion_gate",
        "coverage_adequacy",
    ] {
        assert!(
            fixtures.iter().any(|fixture| has_feature(fixture, feature)),
            "v12 frontier corpus must cover {feature}"
        );
    }
}

#[test]
fn generated_frontier_v13_source_semantic_oracles_hold() {
    let fixtures = replay_matching_fixtures("v13 source semantic", is_v13_source_semantic_oracle);
    assert!(fixtures
        .iter()
        .any(|fixture| fixture.semantic_oracle == "replay_to_slashing_authentication"));
    for fixture in &fixtures {
        run_v13_source_semantic_oracle(fixture);
    }
}

#[test]
fn generated_frontier_v13_runtime_metering_parallel_oracles_hold() {
    let fixtures = replay_matching_fixtures("v13 runtime metering parallel", |fixture| {
        matches!(
            fixture.semantic_oracle.as_str(),
            "runtime_to_settlement_fuel_isolation" | "metering_to_parallel_digest_stability"
        )
    });
    for semantic_oracle in [
        "runtime_to_settlement_fuel_isolation",
        "metering_to_parallel_digest_stability",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.semantic_oracle == semantic_oracle),
            "v13 frontier corpus must cover semantic oracle {semantic_oracle}"
        );
    }
    for fixture in &fixtures {
        assert_v13_source_semantic_metadata(fixture);
        assert_v13_runtime_metering_parallel_oracle(fixture);
    }
}

#[test]
fn generated_frontier_v13_casper_settlement_slashing_oracles_hold() {
    let fixtures = replay_matching_fixtures("v13 casper settlement slashing", |fixture| {
        matches!(
            fixture.semantic_oracle.as_str(),
            "replay_to_slashing_authentication" | "legacy_to_runtime_quarantine"
        )
    });
    for semantic_oracle in [
        "replay_to_slashing_authentication",
        "legacy_to_runtime_quarantine",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.semantic_oracle == semantic_oracle),
            "v13 frontier corpus must cover semantic oracle {semantic_oracle}"
        );
    }
    for fixture in &fixtures {
        assert_v13_source_semantic_metadata(fixture);
        assert_v13_casper_settlement_slashing_oracle(fixture);
    }
}

#[test]
fn generated_frontier_v13_coverage_adequacy_holds() {
    let fixtures = generated_fixture_corpus()
        .into_iter()
        .filter(is_v13_source_semantic_oracle)
        .collect::<Vec<_>>();
    assert!(
        !fixtures.is_empty(),
        "v13 source-semantic fixtures must be embedded"
    );
    for semantic_oracle in [
        "runtime_to_settlement_fuel_isolation",
        "metering_to_parallel_digest_stability",
        "replay_to_slashing_authentication",
        "legacy_to_runtime_quarantine",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.semantic_oracle == semantic_oracle),
            "v13 frontier corpus must cover semantic oracle {semantic_oracle}"
        );
    }
    for facet in [
        "runtime_budget",
        "casper_replay",
        "settlement",
        "metering",
        "parallel_eval",
        "slashing",
        "legacy_quarantine",
    ] {
        assert!(
            fixtures.iter().any(|fixture| fixture
                .source_facets
                .iter()
                .any(|candidate| candidate == facet)),
            "v13 frontier corpus must cover source facet {facet}"
        );
    }
    for feature in [
        "source_semantic_oracle",
        "source_facet",
        "source_anchor_digest",
        "cross_surface_role",
        "production_replay_target",
        "promotion_gate",
        "coverage_adequacy",
    ] {
        assert!(
            fixtures.iter().any(|fixture| has_feature(fixture, feature)),
            "v13 frontier corpus must cover {feature}"
        );
    }
}

#[test]
fn generated_frontier_v14_slashing_security_oracles_hold() {
    let fixtures = replay_matching_fixtures("v14 slashing security", |fixture| {
        fixture.security_surface == "slashing_authorization"
    });
    for fixture in &fixtures {
        assert_v14_source_graph_metadata(fixture);
        assert_v14_slashing_oracle(fixture);
    }
}

#[test]
fn generated_frontier_v14_mergeable_channel_oracles_hold() {
    let fixtures = replay_matching_fixtures("v14 mergeable channel security", |fixture| {
        fixture.security_surface == "typed_mergeable_channel"
    });
    for source_risk in [
        "typed_bitmask_diff_roundtrip",
        "non_numeric_mergeable_fallback",
        "merge_type_persistence",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.source_risk == source_risk),
            "v14 frontier corpus must cover mergeable source risk {source_risk}"
        );
    }
    for fixture in &fixtures {
        assert_v14_source_graph_metadata(fixture);
        assert_v14_mergeable_channel_oracle(fixture);
    }
}

#[test]
fn generated_frontier_v14_node_security_oracles_hold() {
    let fixtures = replay_matching_fixtures("v14 node security", |fixture| {
        matches!(
            fixture.security_surface.as_str(),
            "transport_tls" | "crypto_key_material" | "dependency_advisory"
        )
    });
    for surface in [
        "transport_tls",
        "crypto_key_material",
        "dependency_advisory",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.security_surface == surface),
            "v14 frontier corpus must cover node security surface {surface}"
        );
    }
    for fixture in &fixtures {
        assert_v14_source_graph_metadata(fixture);
        assert_v14_node_security_oracle(fixture);
    }
}

#[test]
fn generated_frontier_v14_coverage_adequacy_holds() {
    let fixtures = generated_fixture_corpus()
        .into_iter()
        .filter(is_v14_source_graph_security)
        .collect::<Vec<_>>();
    assert!(
        !fixtures.is_empty(),
        "v14 source-graph fixtures must be embedded"
    );
    for surface in [
        "slashing_authorization",
        "typed_mergeable_channel",
        "transport_tls",
        "crypto_key_material",
        "dependency_advisory",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.security_surface == surface),
            "v14 frontier corpus must cover security surface {surface}"
        );
    }
    for cost_surface in [
        "slashing",
        "mergeable_channels",
        "transport_tls",
        "crypto_key_material",
        "dependency_advisory",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.cost_surface == cost_surface),
            "v14 frontier corpus must cover cost surface {cost_surface}"
        );
    }
    for feature in [
        "source_graph_security",
        "security_surface",
        "external_input_kind",
        "auth_boundary",
        "replay_boundary",
        "source_anchor_status",
        "production_replay_target",
        "promotion_gate",
        "coverage_adequacy",
    ] {
        assert!(
            fixtures.iter().any(|fixture| has_feature(fixture, feature)),
            "v14 frontier corpus must cover {feature}"
        );
    }
    for fixture in &fixtures {
        run_v14_source_graph_oracle(fixture);
    }
}

#[test]
fn runtime_budget_event_sequence_properties_hold() {
    for fixture in generated_fixture_corpus()
        .iter()
        .filter(|fixture| !fixture.events.is_empty())
    {
        let budget = RuntimeBudget::new(Cost::create(
            fixture.initial_budget,
            format!("property fixture {}", fixture.id),
        ));
        let mut saw_error = false;
        for (index, event) in fixture.events.iter().enumerate() {
            saw_error |= budget
                .reserve_canonical(generated_event(index as u64, event))
                .is_err();
            assert!(budget.total_cost().value >= 0);
            assert!(budget.remaining().value >= 0);
            assert!(budget.total_cost().value <= fixture.initial_budget.max(0));
        }
        if fixture.expects_invalid_admission || fixture.expects_oop {
            assert!(
                saw_error,
                "fixture {} expected a rejected event",
                fixture.id
            );
        }
    }
}

#[test]
fn projection_risk_witnesses_have_guarded_safe_disposition() {
    let generated = generated_fixture_corpus();
    for fixture in generated
        .iter()
        .filter(|fixture| fixture.classification == "projection_risk")
    {
        assert!(
            fixture
                .coverage_features
                .iter()
                .any(|feature| feature == "projection")
                || fixture
                    .coverage_features
                    .iter()
                    .any(|feature| feature == "production_replay_target")
                || fixture.promotion_target.starts_with("rust:"),
            "projection-risk fixture {} must have a concrete Rust guard path",
            fixture.id
        );
        replay_generated_fixture(fixture);
    }

    assert!(fixtures()
        .iter()
        .any(|fixture| fixture.classification == Classification::ProjectionRisk));
}
