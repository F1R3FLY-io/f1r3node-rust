use std::{env, fs};

use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{
    BillableKind, BillableTokenEvent, RedexId, RuntimeBudget, SourcePath,
};
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
}

#[derive(Clone, Debug, Deserialize)]
struct GeneratedEvent {
    kind: String,
    weight: u64,
    descriptor: String,
    #[serde(default)]
    deploy: u8,
    #[serde(default)]
    path: Vec<u32>,
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
    let kind = match event.kind.as_str() {
        "source" => BillableKind::SourceStep,
        "substitution" => BillableKind::Substitution,
        _ => BillableKind::Primitive(event.descriptor.clone()),
    };
    BillableTokenEvent {
        deploy_id: [event.deploy; 32],
        source_path: SourcePath(event.path.clone()),
        redex_id: RedexId(stable_index),
        local_index: stable_index,
        kind,
        weight: event.weight,
    }
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
                    deploy: 0,
                    path: vec![0],
                },
                GeneratedEvent {
                    kind: "primitive".to_string(),
                    weight: 2,
                    descriptor: "parallel-primitive".to_string(),
                    deploy: 0,
                    path: vec![1],
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
                deploy: 0,
                path: vec![],
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
                deploy: 0,
                path: vec![0],
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
                deploy: 0,
                path: vec![],
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
                    deploy: 0,
                    path: vec![0],
                },
                GeneratedEvent {
                    kind: "primitive".to_string(),
                    weight: 1,
                    descriptor: "stateful/primitive".to_string(),
                    deploy: 0,
                    path: vec![1],
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
            replay_mutations: vec![
                "cost_trace_digest".to_string(),
                "cost_trace_count".to_string(),
            ],
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
        mutated[index].descriptor.push_str("-mutated");
    } else if let Some(first) = mutated.first_mut() {
        first.path.push(255);
    }
    mutated
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
        assert!(
            fixture.replay_mutations.is_empty()
                || fixture
                    .replay_mutations
                    .iter()
                    .any(|field| field == "cost_trace_digest" || field == "cost_trace_count"),
            "fixture {} replay mutation must touch authenticated cost trace fields",
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
        }
    }
    assert!(checked_success_projection);
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
            deploy: 1,
            path: vec![0],
        },
        GeneratedEvent {
            kind: "primitive".to_string(),
            weight: 2,
            descriptor: "parallel-primitive".to_string(),
            deploy: 1,
            path: vec![1],
        },
        GeneratedEvent {
            kind: "substitution".to_string(),
            weight: 1,
            descriptor: "parallel-substitution".to_string(),
            deploy: 1,
            path: vec![2],
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
    renamed[1].descriptor = "parallel-primitive-renamed".to_string();
    let renamed_digest = trace_digest_for(&renamed);
    assert_ne!(forward_digest, renamed_digest);
}
