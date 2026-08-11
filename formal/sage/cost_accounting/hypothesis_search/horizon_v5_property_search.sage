import argparse
import hashlib
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(sys.argv[0]))), "scenario_schema.sage"))

try:
    from hypothesis import HealthCheck, Phase, find, settings
    from hypothesis import strategies as st
    from hypothesis.database import DirectoryBasedExampleDatabase
    from hypothesis.errors import NoSuchExample
except Exception as exc:
    raise SystemExit("Hypothesis is required in the Sage Python environment: {}".format(exc))


OBJECTIVES = [
    "all",
    "security",
    "invariant_mining",
    "negative_auth",
    "source_shape",
    "cross_deploy",
    "scheduler",
    "settlement_slashing",
    "cache",
    "resource",
]

SUPPORTED_MUTATIONS = [
    "cost_trace_digest",
    "cost_trace_event_count",
    "cost",
    "signature",
    "block_hash",
    "failed",
    "system_error",
    "slash_fields",
    "genesis",
    "cost_trace_present",
]


def objective_enabled(selected, objective):
    selected_items = [item.strip() for item in str(selected).split(",") if item.strip()]
    return "all" in selected_items or objective in selected_items


def hypothesis_settings(profile, search_mode):
    if profile == "quick":
        max_examples = 128
    elif profile == "corpus":
        max_examples = 1024
    else:
        max_examples = 4096
    database = None
    if search_mode != "frontier":
        database_dir = os.environ.get("COST_ACCOUNTING_HYPOTHESIS_DB")
        if database_dir:
            os.makedirs(database_dir, exist_ok=True)
            database = DirectoryBasedExampleDatabase(database_dir)
    return settings(
        max_examples=int(max_examples),
        database=database,
        derandomize=database is None,
        deadline=None,
        phases=[Phase.generate, Phase.shrink],
        suppress_health_check=[HealthCheck.too_slow, HealthCheck.filter_too_much],
    )


def find_or_none(strategy, predicate, cfg):
    try:
        return find(strategy, predicate, settings=cfg)
    except NoSuchExample:
        return None


def digest_value(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, default=schema_json_default).encode("utf-8")).hexdigest()[:16]


def expected_fixture_values(scenario):
    consumed = 0
    count = 0
    budget = int(scenario.get("initial_budget", 0))
    for event in scenario.get("events", []):
        weight = int(event.get("weight", 0))
        primitive_descriptor = str(event.get("primitive_descriptor", event.get("descriptor", "")))
        invalid_descriptor = str(event.get("kind", "")) == "primitive" and len(primitive_descriptor) > 512
        invalid_source_path = len(event.get("path", [])) > 1024
        if weight <= 0 or invalid_descriptor or invalid_source_path:
            return (0, 0, True, False)
        if consumed + weight > budget:
            return (budget, count + 1, False, True)
        consumed += weight
        count += 1
    return (consumed, count, False, False)


def discover_source_seeds(roots, limit):
    seeds = []
    for root in roots:
        if not root:
            continue
        root = os.path.abspath(root)
        if not os.path.exists(root):
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [
                name
                for name in dirnames
                if name not in [".git", "target", "fuzz", "node_modules"] and not name.startswith(".")
            ]
            for filename in sorted(filenames):
                if not filename.endswith(".rho"):
                    continue
                path = os.path.join(dirpath, filename)
                try:
                    with open(path, "rb") as handle:
                        content = handle.read(4096)
                    size = os.path.getsize(path)
                except OSError:
                    continue
                seeds.append(
                    {
                        "path": os.path.relpath(path, root),
                        "root": root,
                        "bytes_sampled": int(len(content)),
                        "size_bytes": int(size),
                        "line_count": int(content.count(b"\n") + 1 if content else 0),
                        "sha256_prefix": hashlib.sha256(content).hexdigest()[:16],
                    }
                )
                if len(seeds) >= int(limit):
                    return seeds
    if not seeds:
        seeds.append(
            {
                "path": "synthetic/nil.rho",
                "root": "synthetic",
                "bytes_sampled": 3,
                "size_bytes": 3,
                "line_count": 1,
                "sha256_prefix": hashlib.sha256(b"Nil").hexdigest()[:16],
            }
        )
    return seeds


def event_from_seed(seed, index):
    size = int(seed.get("size_bytes", 1))
    lines = int(seed.get("line_count", 1))
    weight = max(1, min(48, ((size % 43) + (3 * lines)) % 49))
    descriptor = "{}#{}".format(seed.get("path", "seed"), seed.get("sha256_prefix", "digest"))
    return canonical_event("source", weight, descriptor=descriptor[:512], deploy=int(index % 4), path=[int(index), int(lines % 512)])


def command_for_fixture():
    return "COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang generated_frontier_property_fixtures_hold"


def property_invariant_records(profile, search_mode, objectives):
    if not (objective_enabled(objectives, "invariant_mining") or objective_enabled(objectives, "security")):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    weights = find_or_none(
        st.lists(st.integers(min_value=1, max_value=5), min_size=int(3), max_size=int(5)),
        lambda xs: sum(xs) > max(xs) and sum(xs) <= 16,
        cfg,
    ) or [1, 2, 3]
    events = [
        canonical_event(kind, weight, descriptor="v5/property/{}-{}".format(kind, index), deploy=0, path=[index])
        for index, (kind, weight) in enumerate(zip(["source", "primitive", "substitution", "source", "primitive"], weights))
    ]
    monotonic = canonical_scenario(
        "horizon_v5_cost_monotonicity_property",
        events=events,
        initial_budget=sum(int(event["weight"]) for event in events) + 2,
        replay_fields={"fields": ["cost", "cost_trace_event_count", "cost_trace_digest"]},
        replay_mutations=["cost", "cost_trace_event_count", "cost_trace_digest"],
        attack_campaign="v5_property_invariant_mining",
        oracle_kind="runtime_budget_monotonicity",
        production_path="rholang::RuntimeBudget::reserve_canonical",
        campaign_steps=["reserve_sequence", "assert_monotone_cost", "finalize"],
        minimized_input_digest=digest_value(weights),
        reproducer_command=command_for_fixture(),
        candidate_property="cost_monotonicity",
        oracle_strength="formal",
        threat_family="property_invariant",
        expected_invariants=["total_cost_monotone", "event_count_tracks_successful_reservations"],
        rust_reproducer={"test": "generated_frontier_property_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    digest = canonical_scenario(
        "horizon_v5_digest_sensitivity_property",
        events=[
            canonical_event("source", 1, descriptor="v5/digest/a", deploy=0, path=[0]),
            canonical_event("source", 1, descriptor="v5/digest/b", deploy=0, path=[1]),
        ],
        initial_budget=4,
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count"]},
        negative_mutations=["cost_trace_digest", "cost_trace_event_count"],
        attack_campaign="v5_digest_sensitivity",
        oracle_kind="trace_digest_field_sensitivity",
        production_path="rholang::RuntimeBudget::cost_trace_digest",
        campaign_steps=["reserve", "mutate_descriptor", "replay"],
        minimized_input_digest=digest_value("digest-sensitivity"),
        reproducer_command=command_for_fixture(),
        candidate_property="digest_sensitivity",
        oracle_strength="formal",
        threat_family="property_invariant",
        expected_invariants=["trace_entry_id_change_detected", "trace_duplicate_multiplicity_detected"],
        rust_reproducer={"test": "generated_frontier_property_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    reset_isolation = canonical_scenario(
        "horizon_v5_finalize_reset_isolation_property",
        events=[canonical_event("source", 2, descriptor="v5/finalize-reset", deploy=0, path=[0])],
        lifecycle=["reserve", "finalize", "clear_diagnostic", "replay"],
        initial_budget=5,
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "failed"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        attack_campaign="v5_finalize_reset_isolation",
        oracle_kind="retained_trace_window_isolation",
        production_path="RuntimeBudget::cost_trace_digest + RuntimeBudget::reset_from_token",
        campaign_steps=["reserve", "finalize", "assert_no_refund", "replay"],
        minimized_input_digest=digest_value("finalize-reset"),
        reproducer_command=command_for_fixture(),
        candidate_property="reset_isolation",
        oracle_strength="integration",
        threat_family="scheduler_interleaving",
        expected_invariants=["finalization_reads_completed_trace", "reset_clears_retained_trace_without_refund"],
        rust_reproducer={"test": "generated_frontier_property_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v5_property_invariant",
            "confirmed_safe",
            "horizon_v5_cost_monotonicity_property",
            "Generated successful reservations preserve monotone runtime cost and event-count accounting.",
            monotonic,
            {"property_invariant": "cost_monotonicity", "weights": weights, "candidate_property": "cost_monotonicity"},
            ["Rust: generated_frontier_property_fixtures_hold", "Rocq: runtime budget monotonicity lemmas"],
        ),
        record(
            "horizon_v5_property_invariant",
            "confirmed_safe",
            "horizon_v5_digest_sensitivity_property",
            "Digest and event-count mutations remain detectable under generated trace witnesses.",
            digest,
            {"property_invariant": "digest_sensitivity", "negative_auth": ["cost_trace_digest", "cost_trace_event_count"]},
            ["Rust: generated_frontier_property_fixtures_hold", "Rocq: trace identity sensitivity"],
        ),
        record(
            "horizon_v5_property_invariant",
            "confirmed_safe",
            "horizon_v5_finalize_reset_isolation_property",
            "Finalization and diagnostic reset do not refund fuel or erase replayable committed trace evidence.",
            reset_isolation,
            {"property_invariant": "reset_isolation", "scheduler": "finalize_then_replay"},
            ["Rust: generated_frontier_property_fixtures_hold", "TLA+: RuntimeBudgetReplay"],
        ),
    ]


def negative_auth_records(profile, search_mode, objectives):
    if not (objective_enabled(objectives, "negative_auth") or objective_enabled(objectives, "security")):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    mutations = find_or_none(
        st.lists(st.sampled_from(SUPPORTED_MUTATIONS), min_size=int(5), max_size=int(len(SUPPORTED_MUTATIONS)), unique=True),
        lambda xs: "cost_trace_digest" in xs and "cost_trace_event_count" in xs and "signature" in xs and "block_hash" in xs,
        cfg,
    ) or SUPPORTED_MUTATIONS
    scenario = canonical_scenario(
        "horizon_v5_negative_auth_mutation_matrix",
        events=[
            canonical_event("source", 1, descriptor="v5/negative-auth/source", deploy=0, path=[0]),
            canonical_event("primitive", 2, descriptor="v5/negative-auth/primitive", deploy=1, path=[1]),
        ],
        deploy_count=2,
        initial_budget=8,
        replay_fields={"fields": mutations, "mode": "cost_accounted"},
        negative_mutations=mutations,
        rust_replay={"fixture": "generated_frontier_property_fixtures_hold"},
        attack_campaign="v5_negative_auth_mutation_matrix",
        oracle_kind="negative_replay_payload_authentication",
        production_path="ProcessedDeploy replay payload + block hash authentication",
        campaign_steps=["reserve", "finalize", "mutate_authenticated_field", "replay"],
        minimized_input_digest=digest_value(mutations),
        reproducer_command=command_for_fixture(),
        candidate_property="negative_replay_authentication",
        oracle_strength="production_helper",
        threat_family="negative_authentication",
        expected_invariants=["full_replay_payload_authenticates_cost_trace_fields", "block_authenticates_cost_trace_payload"],
        rust_reproducer={"test": "generated_frontier_negative_auth_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v5_negative_auth",
            "confirmed_safe",
            "horizon_v5_negative_auth_mutation_matrix",
            "Negative replay authentication mutates each cost-relevant field through the production-shaped fixture surface.",
            scenario,
            {"negative_auth": mutations, "signed_payload": True, "tamper": True},
            ["Rust: generated_frontier_negative_auth_fixtures_hold", "Casper: replay/block hash field sensitivity tests"],
        )
    ]


def source_shape_records(seed_roots, source_limit, objectives):
    if not (objective_enabled(objectives, "source_shape") or objective_enabled(objectives, "security")):
        return []
    seeds = discover_source_seeds(seed_roots, source_limit)
    events = [event_from_seed(seed, index) for index, seed in enumerate(seeds)]
    total = sum(int(event["weight"]) for event in events)
    source_scenario = canonical_scenario(
        "horizon_v5_source_shape_corpus_property",
        events=events,
        deploy_count=max(1, len(set(int(event["deploy"]) for event in events))),
        initial_budget=total + 16,
        replay_fields={"mode": "cost_accounted", "source_seed_count": len(seeds)},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        source_seed={"seeds": seeds},
        attack_campaign="v5_source_shape_corpus",
        oracle_kind="source_shape_path_stability",
        production_path="rholang source path to RuntimeBudget event identity",
        campaign_steps=["source_seed", "reserve", "mutate_descriptor", "replay"],
        minimized_input_digest=digest_value(seeds),
        reproducer_command=command_for_fixture(),
        candidate_property="source_shape_path_stability",
        oracle_strength="integration",
        threat_family="source_shape",
        expected_invariants=["source_paths_preserve_trace_identity", "primitive_descriptor_mutation_changes_digest"],
        rust_reproducer={"test": "generated_frontier_source_shape_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="bisimilar",
    )
    boundary_descriptor = canonical_scenario(
        "horizon_v5_primitive_descriptor_boundary",
        events=[canonical_event("primitive", 1, descriptor="s" * 512, deploy=0, path=[0, 512])],
        initial_budget=4,
        resource_bounds={"max_descriptor_bytes": 512, "primitive_descriptor_boundary": True},
        source_seed={"seeds": [{"path": "synthetic/descriptor-boundary.rho", "root": "synthetic"}]},
        attack_campaign="v5_primitive_descriptor_boundary",
        oracle_kind="primitive_descriptor_boundary_acceptance",
        production_path="RuntimeBudget::validate_billable_event via reserve_canonical",
        campaign_steps=["source_seed", "admit_boundary_descriptor", "reserve"],
        minimized_input_digest=digest_value("descriptor-boundary-512"),
        reproducer_command=command_for_fixture(),
        candidate_property="primitive_descriptor_boundary_acceptance",
        oracle_strength="integration",
        threat_family="source_shape",
        expected_invariants=["max_length_primitive_descriptor_is_valid", "oversized_primitive_descriptor_is_invalid"],
        rust_reproducer={"test": "generated_frontier_source_shape_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v5_source_shape",
            "bisimilar",
            "horizon_v5_source_shape_corpus_property",
            "Real Rholang source shapes seed path stability checks, while primitive events seed descriptor sensitivity checks.",
            source_scenario,
            {"source_shape": [seed["path"] for seed in seeds], "source_seed": True},
            ["Rust: generated_frontier_source_shape_fixtures_hold", "Sage: source shape corpus"],
        ),
        record(
            "horizon_v5_source_shape",
            "confirmed_safe",
            "horizon_v5_primitive_descriptor_boundary",
            "The maximum accepted primitive descriptor length remains a valid event boundary.",
            boundary_descriptor,
            {"source_shape": "primitive_descriptor_boundary", "bounded_resource": 512},
            ["Rust: generated_frontier_source_shape_fixtures_hold", "Rocq: descriptor bound admission"],
        ),
    ]


def cross_deploy_records(objectives):
    if not (objective_enabled(objectives, "cross_deploy") or objective_enabled(objectives, "security")):
        return []
    trace_separation = canonical_scenario(
        "horizon_v5_cross_deploy_trace_separation",
        events=[
            canonical_event("source", 1, descriptor="v5/cross-deploy/same", deploy=0, path=[0]),
            canonical_event("source", 1, descriptor="v5/cross-deploy/same", deploy=1, path=[0]),
        ],
        deploy_count=2,
        initial_budget=4,
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "deploy"]},
        negative_mutations=["cost_trace_digest", "cost_trace_event_count", "block_hash"],
        attack_campaign="v5_cross_deploy_trace_separation",
        oracle_kind="deploy_domain_separation",
        production_path="rholang::RuntimeBudget::cost_trace_digest",
        campaign_steps=["reserve_deploy_0", "reserve_deploy_1", "finalize", "replay"],
        minimized_input_digest=digest_value("cross-deploy-separation"),
        reproducer_command=command_for_fixture(),
        candidate_property="deploy_domain_separation",
        oracle_strength="formal",
        threat_family="cross_deploy",
        expected_invariants=["deploy_id_participates_in_trace_identity", "same_descriptor_different_deploys_do_not_collapse"],
        rust_reproducer={"test": "generated_frontier_property_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    settlement = canonical_scenario(
        "horizon_v5_cross_deploy_settlement_locality",
        events=[
            canonical_event("source", 2, descriptor="v5/deploy-0/settlement", deploy=0, path=[0]),
            canonical_event("primitive", 3, descriptor="v5/deploy-1/settlement", deploy=1, path=[1]),
        ],
        deploy_count=2,
        initial_budget=8,
        phlo_limit=8,
        phlo_price=2,
        token_cost=5,
        settlement={"authority": "casper", "escrow": 16, "token_cost": 10, "refund": 6, "deploy_local": True},
        replay_fields={"fields": ["cost", "cost_trace_digest", "cost_trace_event_count"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        attack_campaign="v5_cross_deploy_settlement_locality",
        oracle_kind="deploy_local_settlement",
        production_path="DeployData::refund_amount_for_token_cost + RuntimeBudget",
        campaign_steps=["precharge", "reserve_each_deploy", "settle", "replay"],
        minimized_input_digest=digest_value("cross-deploy-settlement"),
        reproducer_command=command_for_fixture(),
        candidate_property="deploy_local_settlement",
        oracle_strength="integration",
        threat_family="cross_deploy",
        expected_invariants=["settlement_sums_deploy_local_costs", "uc_ca_058_refund_cannot_replenish_runtime_fuel"],
        rust_reproducer={"test": "generated_frontier_property_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v5_cross_deploy",
            "confirmed_safe",
            "horizon_v5_cross_deploy_trace_separation",
            "Deploy id remains part of trace identity even when descriptors and paths match.",
            trace_separation,
            {"cross_deploy": "trace_separation", "deploy_separation": True},
            ["Rust: generated_frontier_property_fixtures_hold", "Rocq: trace identity deploy sensitivity"],
        ),
        record(
            "horizon_v5_cross_deploy",
            "confirmed_safe",
            "horizon_v5_cross_deploy_settlement_locality",
            "Settlement remains deploy-local and cannot share runtime fuel across deploys.",
            settlement,
            {"cross_deploy": "settlement_locality", "settlement": True},
            ["Rust: generated_frontier_property_fixtures_hold", "Rocq: multi-deploy settlement locality"],
        ),
    ]


def scheduler_records(objectives):
    if not (objective_enabled(objectives, "scheduler") or objective_enabled(objectives, "security")):
        return []
    scheduler = canonical_scenario(
        "horizon_v5_scheduler_finalize_replay_campaign",
        events=[canonical_event("source", 1, descriptor="v5/scheduler/finalize-replay", deploy=0, path=[0])],
        lifecycle=["reserve", "worker_join", "finalize", "replay", "settle"],
        initial_budget=4,
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "failed"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        concurrency={"worker_join_before_finalize": True, "campaign_steps": ["reserve", "worker_join", "finalize", "replay"]},
        attack_campaign="v5_scheduler_finalize_replay",
        oracle_kind="scheduler_completion_before_finalization",
        production_path="parallel evaluation join + RuntimeBudget finalization",
        campaign_steps=["reserve", "worker_join", "finalize", "replay", "settle"],
        minimized_input_digest=digest_value("scheduler-finalize-replay"),
        reproducer_command=command_for_fixture(),
        candidate_property="scheduler_completion_before_finalization",
        oracle_strength="integration",
        threat_family="scheduler_interleaving",
        expected_invariants=["finalization_after_trace_completion", "parallelism_preserved_before_join"],
        rust_reproducer={"test": "generated_frontier_property_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    rollback = canonical_scenario(
        "horizon_v5_scheduler_rollback_replay_property",
        events=[canonical_event("source", 2, descriptor="v5/scheduler/rollback", deploy=0, path=[0])],
        lifecycle=["reserve", "rollback", "reserve", "worker_join", "finalize", "replay"],
        initial_budget=6,
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        concurrency={"rollback_after_reserve": True},
        attack_campaign="v5_scheduler_rollback_replay",
        oracle_kind="rollback_window_replay_boundary",
        production_path="RuntimeBudget finalization-read/reset boundary",
        campaign_steps=["reserve", "rollback", "reserve", "worker_join", "finalize", "replay"],
        minimized_input_digest=digest_value("scheduler-rollback-replay"),
        reproducer_command=command_for_fixture(),
        candidate_property="rollback_replay_boundary",
        oracle_strength="formal",
        threat_family="scheduler_interleaving",
        expected_invariants=["rollback_preserves_authenticated_trace_boundary", "rollback_does_not_leak_budget"],
        rust_reproducer={"test": "generated_frontier_property_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="proof_or_model_strengthening",
    )
    return [
        record(
            "horizon_v5_scheduler",
            "confirmed_safe",
            "horizon_v5_scheduler_finalize_replay_campaign",
            "Scheduler completion before finalization keeps trace evidence complete without serializing evaluation.",
            scheduler,
            {"scheduler_interleaving": "join_before_finalize", "parallelism": "preserved"},
            ["Rust: generated_frontier_property_fixtures_hold", "TLA+: RuntimeBudgetReplay"],
        ),
        record(
            "horizon_v5_scheduler",
            "proof_or_model_strengthening",
            "horizon_v5_scheduler_rollback_replay_property",
            "Rollback/replay ordering remains a formal property frontier for retained trace windows.",
            rollback,
            {"scheduler_interleaving": "rollback_replay", "rollback": True},
            ["Rust: generated_frontier_property_fixtures_hold", "TLA+: RuntimeBudgetReplay"],
        ),
    ]


def settlement_slashing_records(objectives):
    if not (objective_enabled(objectives, "settlement_slashing") or objective_enabled(objectives, "security")):
        return []
    composed = canonical_scenario(
        "horizon_v5_settlement_slashing_cache_composition",
        events=[
            canonical_event("source", 2, descriptor="v5/settlement/slash-cache", deploy=0, path=[0]),
            canonical_event("substitution", 1, descriptor="v5/settlement/auth", deploy=1, path=[1]),
        ],
        deploy_count=2,
        initial_budget=6,
        phlo_limit=6,
        phlo_price=2,
        token_cost=3,
        settlement={"kind": "slash_after_evaluation", "authority": "casper", "escrow": 12, "token_cost": 6, "refund": 6, "cache_evidence": True},
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "signature", "slash_fields", "genesis"]},
        negative_mutations=["cost_trace_digest", "cost_trace_event_count", "slash_fields", "genesis", "signature"],
        attack_campaign="v5_settlement_slashing_cache_composition",
        oracle_kind="slash_refund_replay_cache_composition",
        production_path="runtime budget + processed deploy payload + settlement projection",
        campaign_steps=["precharge", "reserve", "finalize", "settle", "slash", "replay_cache_lookup"],
        minimized_input_digest=digest_value("settlement-slashing-cache"),
        reproducer_command=command_for_fixture(),
        candidate_property="slash_refund_replay_cache_composition",
        oracle_strength="production_helper",
        threat_family="slashing_composition",
        expected_invariants=[
            "slash_system_effect_is_unmetered_for_user_budget",
            "uc_ca_058_refund_cannot_replenish_runtime_fuel",
            "replay_cache_authenticates_cost_trace_payload",
        ],
        rust_reproducer={"test": "generated_frontier_property_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    stale = canonical_scenario(
        "horizon_v5_stale_evidence_duplicate_boundary",
        events=[canonical_event("source", 1, descriptor="v5/stale-evidence-duplicate", deploy=0, path=[0])],
        initial_budget=4,
        settlement={"kind": "slash_after_evaluation", "authority": "casper", "escrow": 8, "token_cost": 2, "refund": 6, "stale_evidence": True, "duplicate_evidence": True},
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "slash_fields", "genesis", "block_hash"]},
        negative_mutations=["slash_fields", "genesis", "block_hash", "cost_trace_digest"],
        attack_campaign="v5_stale_evidence_duplicate_boundary",
        oracle_kind="stale_duplicate_cost_evidence_boundary",
        production_path="slashing evidence validation + replay payload authentication",
        campaign_steps=["reserve", "finalize", "fork", "submit_duplicate_stale_evidence", "replay"],
        minimized_input_digest=digest_value("stale-evidence-duplicate"),
        reproducer_command=command_for_fixture(),
        candidate_property="stale_duplicate_cost_evidence_rejection",
        oracle_strength="production_helper",
        threat_family="slashing_composition",
        expected_invariants=["stale_cost_evidence_rejected", "duplicate_slash_does_not_mutate_user_cost_trace"],
        rust_reproducer={"test": "generated_frontier_property_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="projection_risk",
    )
    return [
        record(
            "horizon_v5_settlement_slashing",
            "confirmed_safe",
            "horizon_v5_settlement_slashing_cache_composition",
            "Settlement, slashing, and replay-cache evidence compose without changing runtime fuel.",
            composed,
            {"settlement_slashing": "cache_composition", "replay_cache": True, "negative_auth": composed["negative_mutations"]},
            ["Rust: generated_frontier_property_fixtures_hold", "Casper: replay cache and slashing authentication tests"],
        ),
        record(
            "horizon_v5_settlement_slashing",
            "projection_risk",
            "horizon_v5_stale_evidence_duplicate_boundary",
            "Duplicate stale cost-invalid evidence remains a guarded-safe projection risk at the slashing boundary.",
            stale,
            {"settlement_slashing": "stale_duplicate", "stale_evidence": True, "negative_auth": stale["negative_mutations"]},
            ["Rust: generated_frontier_property_fixtures_hold", "Casper: stale evidence guard tests", "Rocq: stale_cost_evidence_sound"],
        ),
    ]


def cache_resource_records(objectives):
    if not (objective_enabled(objectives, "cache") or objective_enabled(objectives, "resource") or objective_enabled(objectives, "security")):
        return []
    cache = canonical_scenario(
        "horizon_v5_cache_resource_churn_property",
        events=[
            canonical_event("source", 1, descriptor="v5/cache/0", deploy=0, path=[0]),
            canonical_event("primitive", 1, descriptor="v5/cache/1", deploy=0, path=[1]),
            canonical_event("substitution", 1, descriptor="v5/cache/2", deploy=0, path=[2]),
            canonical_event("source", 1, descriptor="v5/cache/3", deploy=0, path=[3]),
        ],
        initial_budget=8,
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "cost_trace_present"]},
        negative_mutations=["cost_trace_digest", "cost_trace_event_count", "cost_trace_present"],
        resource_bounds={"max_descriptor_bytes": 512, "max_retained_trace_events": 4, "cache_churn": True},
        attack_campaign="v5_cache_resource_churn",
        oracle_kind="bounded_replay_cache_churn",
        production_path="RuntimeBudget retained trace + replay cache evidence",
        campaign_steps=["reserve_many", "finalize", "clear_diagnostic", "replay_cache_lookup"],
        minimized_input_digest=digest_value("cache-resource-churn"),
        reproducer_command=command_for_fixture(),
        candidate_property="bounded_replay_cache_churn",
        oracle_strength="integration",
        threat_family="cache_resource",
        expected_invariants=["trace_retention_bound_preserved", "cache_lookup_does_not_mutate_runtime_fuel"],
        rust_reproducer={"test": "generated_frontier_property_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    oversized_descriptor = canonical_scenario(
        "horizon_v5_resource_descriptor_overflow_rejection",
        events=[canonical_event("primitive", 1, descriptor="z" * 513, deploy=0, path=[0])],
        initial_budget=4,
        resource_bounds={"max_descriptor_bytes": 512},
        attack_campaign="v5_resource_descriptor_overflow",
        oracle_kind="descriptor_overflow_rejection",
        production_path="RuntimeBudget::validate_billable_event via reserve_canonical",
        campaign_steps=["admit", "reject_before_trace_mutation"],
        minimized_input_digest=digest_value("resource-descriptor-overflow"),
        reproducer_command=command_for_fixture(),
        candidate_property="descriptor_overflow_rejection",
        oracle_strength="formal",
        threat_family="cache_resource",
        expected_invariants=["oversized_billable_event_rejected", "rejection_preserves_budget_and_trace"],
        rust_reproducer={"test": "generated_frontier_property_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v5_cache_resource",
            "confirmed_safe",
            "horizon_v5_cache_resource_churn_property",
            "Replay-cache churn remains bounded and cannot mutate runtime budget state.",
            cache,
            {"cache_resource": "bounded_churn", "replay_cache": True, "bounded_resource": True},
            ["Rust: generated_frontier_property_fixtures_hold", "TLA+: RuntimeBudgetReplay"],
        ),
        record(
            "horizon_v5_cache_resource",
            "confirmed_safe",
            "horizon_v5_resource_descriptor_overflow_rejection",
            "Oversized descriptor witnesses remain rejected before trace or budget mutation.",
            oversized_descriptor,
            {"cache_resource": "descriptor_overflow", "descriptor": 513, "bounded_resource": True},
            ["Rust: generated_frontier_property_fixtures_hold", "Rocq: rb_oversized_weight_admission_rejection_preserves_trace"],
        ),
    ]


def fixture_from_record(item):
    scenario = item["scenario"]
    return scenario_fixture(
        item["name"],
        item["classification"],
        scenario,
        item["deterministic_witness"],
        {
            "classification": item["classification"],
            "promotion_target": scenario.get("promotion_target", "record"),
            "threat_family": scenario.get("threat_family", "search_governance"),
        },
        assertions=[
            "classification != unexpected",
            "promotion_target != none",
            "threat_family != empty",
            "candidate_property != empty",
            "oracle_strength in accepted_set",
            "negative_mutations_replay_authenticated",
        ],
    )


def rust_fixture_from_record(item):
    scenario = item["scenario"]
    total, count, invalid, oop = expected_fixture_values(scenario)
    return {
        "id": item["name"],
        "classification": item["classification"],
        "threat_family": scenario.get("threat_family", "search_governance"),
        "promotion_target": scenario.get("promotion_target", "record"),
        "initial_budget": int(scenario.get("initial_budget", 0)),
        "events": scenario.get("events", []),
        "expected_total_cost": int(total),
        "expected_event_count": int(count),
        "expects_invalid_admission": bool(invalid),
        "expects_oop": bool(oop),
        "settlement": scenario.get("settlement", {}),
        "replay_mutations": scenario.get("replay_mutations", []),
        "negative_mutations": scenario.get("negative_mutations", []),
        "coverage_features": item.get("coverage_features", []),
        "source_seed": scenario.get("source_seed", {}),
        "attack_campaign": scenario.get("attack_campaign", ""),
        "oracle_kind": scenario.get("oracle_kind", ""),
        "production_path": scenario.get("production_path", ""),
        "campaign_steps": scenario.get("campaign_steps", []),
        "minimized_input_digest": scenario.get("minimized_input_digest", ""),
        "reproducer_command": scenario.get("reproducer_command", ""),
        "candidate_property": scenario.get("candidate_property", ""),
        "oracle_strength": scenario.get("oracle_strength", ""),
        "production_disposition": production_disposition(item["classification"]),
        "production_guards": production_guards(item["name"], item["classification"], scenario),
        "trace_digest_fields": scenario.get("trace_digest_fields", []),
    }


def frontier_records(args):
    records = []
    records.extend(property_invariant_records(args.profile, args.search_mode, args.objectives))
    records.extend(negative_auth_records(args.profile, args.search_mode, args.objectives))
    records.extend(source_shape_records(args.source_root, args.source_limit, args.objectives))
    records.extend(cross_deploy_records(args.objectives))
    records.extend(scheduler_records(args.objectives))
    records.extend(settlement_slashing_records(args.objectives))
    records.extend(cache_resource_records(args.objectives))
    return records


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", choices=["quick", "corpus", "deep"], default="quick")
    parser.add_argument("--search-mode", choices=["frontier", "all"], default="frontier")
    parser.add_argument("--objectives", default="all")
    parser.add_argument("--source-root", action="append", default=[])
    parser.add_argument("--source-limit", type=int, default=8)
    parser.add_argument("--json-out")
    parser.add_argument("--fixture-out")
    parser.add_argument("--coverage-out")
    parser.add_argument("--rust-fixtures-out")
    args = parser.parse_args(argv)

    records = frontier_records(args)
    fixtures = [fixture_from_record(item) for item in records]
    rust_fixtures = [rust_fixture_from_record(item) for item in records]
    output = {
        "profile": args.profile,
        "search_mode": args.search_mode,
        "objectives": args.objectives,
        "source_roots": args.source_root,
        "records": records,
        "fixtures": fixtures,
        "rust_fixtures": rust_fixtures,
        "coverage_summary": coverage_summary(records),
        "frontier": pareto_frontier(records),
    }
    text = json.dumps(output, indent=2, sort_keys=True, default=schema_json_default)
    if args.json_out:
        with open(args.json_out, "w") as handle:
            handle.write(text + "\n")
    else:
        print(text)
    if args.fixture_out:
        with open(args.fixture_out, "w") as handle:
            handle.write(json.dumps({"fixtures": fixtures}, indent=2, sort_keys=True, default=schema_json_default) + "\n")
    if args.coverage_out:
        with open(args.coverage_out, "w") as handle:
            handle.write(json.dumps(output["coverage_summary"], indent=2, sort_keys=True, default=schema_json_default) + "\n")
    if args.rust_fixtures_out:
        with open(args.rust_fixtures_out, "w") as handle:
            handle.write(json.dumps({"fixtures": rust_fixtures}, indent=2, sort_keys=True, default=schema_json_default) + "\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
