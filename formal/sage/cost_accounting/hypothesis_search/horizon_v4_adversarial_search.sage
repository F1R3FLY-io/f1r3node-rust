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
    "adversarial",
    "budget",
    "replay",
    "settlement",
    "slashing",
    "lifecycle",
    "source_corpus",
    "resource",
    "concurrency",
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
    weight = max(1, min(48, ((size % 41) + lines) % 49))
    descriptor = "{}#{}".format(seed.get("path", "seed"), seed.get("sha256_prefix", "digest"))
    return canonical_event("source", weight, descriptor=descriptor[:512], deploy=int(index % 4), path=[int(index), int(lines % 512)])


def command_for_fixture():
    return "COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang generated_frontier_adversarial_fixtures_hold"


def adversarial_budget_records(profile, search_mode, objectives):
    if not (objective_enabled(objectives, "adversarial") or objective_enabled(objectives, "budget") or objective_enabled(objectives, "security")):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    boundary = find_or_none(
        st.tuples(st.integers(min_value=1, max_value=8), st.integers(min_value=1, max_value=8)),
        lambda pair: pair[0] < pair[0] + pair[1] and pair[0] + pair[1] > pair[0] + 1,
        cfg,
    ) or (3, 4)
    first, second = int(boundary[0]), int(boundary[1])
    budget = first + max(1, second - 2)
    repeated_oop = canonical_scenario(
        "horizon_v4_repeated_oop_boundary",
        events=[
            canonical_event("source", first, descriptor="v4/oop-first", deploy=0, path=[0]),
            canonical_event("primitive", second, descriptor="v4/oop-boundary", deploy=0, path=[1]),
        ],
        initial_budget=budget,
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "failed"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        resource_bounds={"max_descriptor_bytes": 512, "max_billable_weight": 2**63 - 1},
        attack_campaign="adversarial_budget_repeated_oop",
        oracle_kind="runtime_budget_oop_boundary",
        production_path="rholang::RuntimeBudget::reserve_canonical",
        campaign_steps=["precharge", "reserve", "oop", "reserve_again", "finalize"],
        minimized_input_digest=digest_value([first, second, budget]),
        reproducer_command=command_for_fixture(),
        threat_family="adversarial_budget",
        expected_invariants=["repeated_oop_commits_single_boundary", "cost_trace_event_count_success_and_oop"],
        rust_reproducer={"test": "generated_frontier_adversarial_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    invalid_weight = canonical_scenario(
        "horizon_v4_zero_weight_rejection",
        events=[canonical_event("primitive", 0, descriptor="v4/zero-weight", deploy=0, path=[0])],
        initial_budget=4,
        resource_bounds={"max_descriptor_bytes": 512, "zero_weight_rejected": True},
        attack_campaign="adversarial_budget_zero_weight",
        oracle_kind="runtime_budget_invalid_admission",
        production_path="rholang::RuntimeBudget::validate_billable_event via reserve_canonical",
        campaign_steps=["admit", "reject_before_mutation"],
        minimized_input_digest=digest_value("zero-weight"),
        reproducer_command=command_for_fixture(),
        threat_family="adversarial_budget",
        expected_invariants=["zero_weight_billable_event_rejected"],
        rust_reproducer={"test": "generated_frontier_adversarial_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    oversized_descriptor = canonical_scenario(
        "horizon_v4_descriptor_inflation_rejection",
        events=[canonical_event("primitive", 1, descriptor="x" * 513, deploy=0, path=[0])],
        initial_budget=8,
        resource_bounds={"max_descriptor_bytes": 512},
        attack_campaign="adversarial_budget_descriptor_inflation",
        oracle_kind="runtime_budget_descriptor_bound",
        production_path="rholang::RuntimeBudget::validate_billable_event via reserve_canonical",
        campaign_steps=["admit", "reject_before_mutation"],
        minimized_input_digest=digest_value("descriptor-inflation"),
        reproducer_command=command_for_fixture(),
        threat_family="adversarial_budget",
        expected_invariants=["oversized_billable_event_rejected"],
        rust_reproducer={"test": "generated_frontier_adversarial_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v4_adversarial_budget",
            "confirmed_safe",
            "horizon_v4_repeated_oop_boundary",
            "Repeated OOP attempts commit a single boundary trace and cannot spend past the initial budget.",
            repeated_oop,
            {"adversarial_budget": "repeated_oop_boundary", "oop": True, "events": repeated_oop["events"]},
            ["Rust: generated_frontier_adversarial_fixtures_hold", "Rocq: rb_repeated_oop_boundary_frontier"],
        ),
        record(
            "horizon_v4_adversarial_budget",
            "confirmed_safe",
            "horizon_v4_zero_weight_rejection",
            "Zero-weight billable events are rejected before trace or budget mutation.",
            invalid_weight,
            {"adversarial_budget": "zero_weight", "admission": "reject"},
            ["Rust: generated_frontier_adversarial_fixtures_hold", "Rocq: rb_zero_weight_admission_rejection_preserves_trace"],
        ),
        record(
            "horizon_v4_adversarial_budget",
            "confirmed_safe",
            "horizon_v4_descriptor_inflation_rejection",
            "Oversized descriptors are rejected before trace or budget mutation.",
            oversized_descriptor,
            {"adversarial_budget": "descriptor_inflation", "descriptor": 513},
            ["Rust: generated_frontier_adversarial_fixtures_hold", "Rocq: rb_oversized_weight_admission_rejection_preserves_trace"],
        ),
    ]


def adversarial_replay_records(profile, search_mode, objectives):
    if not (objective_enabled(objectives, "adversarial") or objective_enabled(objectives, "replay") or objective_enabled(objectives, "security")):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    mutations = find_or_none(
        st.lists(
            st.sampled_from(["cost_trace_digest", "cost_trace_event_count", "cost_trace_present", "signature", "block_hash", "failed"]),
            min_size=int(3),
            max_size=int(6),
            unique=True,
        ),
        lambda xs: "cost_trace_digest" in xs and "cost_trace_event_count" in xs and ("signature" in xs or "block_hash" in xs),
        cfg,
    ) or ["cost_trace_digest", "cost_trace_event_count", "signature", "block_hash"]
    scenario = canonical_scenario(
        "horizon_v4_replay_auth_mutation_matrix",
        events=[
            canonical_event("source", 1, descriptor="v4/replay-source", deploy=0, path=[0]),
            canonical_event("substitution", 2, descriptor="v4/replay-subst", deploy=1, path=[1]),
        ],
        deploy_count=2,
        initial_budget=8,
        replay_fields={"fields": mutations, "mode": "cost_accounted"},
        replay_mutations=mutations,
        rust_replay={"fixture": "generated_frontier_adversarial_fixtures_hold"},
        attack_campaign="adversarial_replay_auth_mutation_matrix",
        oracle_kind="replay_payload_field_sensitivity",
        production_path="ProcessedDeploy replay payload + block hash authentication",
        campaign_steps=["reserve", "finalize", "mutate_replay_payload", "replay"],
        minimized_input_digest=digest_value(mutations),
        reproducer_command=command_for_fixture(),
        threat_family="adversarial_replay",
        expected_invariants=["full_replay_payload_authenticates_cost_trace_fields", "block_authenticates_cost_trace_payload"],
        rust_reproducer={"test": "generated_frontier_adversarial_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    trace_collision = canonical_scenario(
        "horizon_v4_trace_collision_cross_deploy",
        events=[
            canonical_event("source", 1, descriptor="v4/collision", deploy=0, path=[0]),
            canonical_event("source", 1, descriptor="v4/collision", deploy=1, path=[0]),
        ],
        deploy_count=2,
        initial_budget=4,
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "deploy"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        attack_campaign="adversarial_replay_trace_collision",
        oracle_kind="trace_collision_domain_separation",
        production_path="rholang::RuntimeBudget::cost_trace_digest",
        campaign_steps=["reserve_deploy_0", "reserve_deploy_1", "finalize", "replay"],
        minimized_input_digest=digest_value("trace-collision"),
        reproducer_command=command_for_fixture(),
        threat_family="adversarial_replay",
        expected_invariants=["trace_entry_id_change_detected", "trace_duplicate_multiplicity_detected"],
        rust_reproducer={"test": "generated_frontier_adversarial_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v4_adversarial_replay",
            "confirmed_safe",
            "horizon_v4_replay_auth_mutation_matrix",
            "Replay payload mutations across cost trace, status, signature, and block hash fields remain authenticated.",
            scenario,
            {"adversarial_replay": mutations, "tamper": True, "signed_payload": True},
            ["Rust: generated_frontier_adversarial_fixtures_hold", "Casper: replay/block hash field sensitivity tests"],
        ),
        record(
            "horizon_v4_adversarial_replay",
            "confirmed_safe",
            "horizon_v4_trace_collision_cross_deploy",
            "Deploy id and path separation prevent same-descriptor cross-deploy trace collision.",
            trace_collision,
            {"adversarial_replay": "trace_collision", "trace_collision": True, "multi_deploy": True},
            ["Rust: generated_frontier_adversarial_fixtures_hold", "Rocq: trace entry id sensitivity"],
        ),
    ]


def adversarial_settlement_records(profile, search_mode, objectives):
    if not (objective_enabled(objectives, "adversarial") or objective_enabled(objectives, "settlement") or objective_enabled(objectives, "security")):
        return []
    scenario = canonical_scenario(
        "horizon_v4_refund_as_runtime_fuel_attempt",
        events=[canonical_event("source", 2, descriptor="v4/refund-as-fuel", deploy=0, path=[0])],
        initial_budget=4,
        phlo_limit=4,
        phlo_price=3,
        token_cost=2,
        settlement={"escrow": 12, "token_cost": 6, "refund": 6, "authority": "casper", "attempted_fuel_replenish": False},
        replay_fields={"fields": ["cost", "cost_trace_digest", "cost_trace_event_count"]},
        replay_mutations=["cost", "cost_trace_digest"],
        attack_campaign="adversarial_settlement_refund_as_fuel",
        oracle_kind="settlement_isolation",
        production_path="DeployData::refund_amount_for_token_cost + RuntimeBudget",
        campaign_steps=["precharge", "reserve", "settle", "attempt_refund_as_fuel", "replay"],
        minimized_input_digest=digest_value("refund-as-fuel"),
        reproducer_command=command_for_fixture(),
        threat_family="adversarial_settlement",
        expected_invariants=[
            "uc_ca_058_refund_cannot_replenish_runtime_fuel",
            "uc_ca_009_charged_plus_refund_equals_escrow",
        ],
        rust_reproducer={"test": "generated_frontier_adversarial_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    low_price = canonical_scenario(
        "horizon_v4_low_price_cost_invalid_boundary",
        events=[canonical_event("source", 1, descriptor="v4/low-price", deploy=0, path=[0])],
        initial_budget=2,
        phlo_limit=2,
        phlo_price=0,
        token_cost=1,
        settlement={"escrow": 0, "token_cost": 0, "refund": 0, "authority": "casper", "cost_invalid_evidence": True},
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "slash_fields"]},
        replay_mutations=["slash_fields", "cost_trace_digest"],
        attack_campaign="adversarial_settlement_low_price",
        oracle_kind="low_deploy_price_boundary",
        production_path="cost invalid evidence classification",
        campaign_steps=["precharge", "reserve", "detect_low_price", "classify_cost_invalid"],
        minimized_input_digest=digest_value("low-price"),
        reproducer_command=command_for_fixture(),
        threat_family="adversarial_settlement",
        expected_invariants=["low_deploy_price_violation_sound"],
        rust_reproducer={"test": "generated_frontier_adversarial_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="proof_or_model_strengthening",
    )
    return [
        record(
            "horizon_v4_adversarial_settlement",
            "confirmed_safe",
            "horizon_v4_refund_as_runtime_fuel_attempt",
            "Casper refund arithmetic cannot replenish runtime fuel after evaluation.",
            scenario,
            {"adversarial_settlement": "refund_as_fuel", "refund_as_fuel": True, "refund": 6},
            ["Rust: generated_frontier_adversarial_fixtures_hold", "Rocq: uc_ca_058_refund_cannot_replenish_runtime_fuel"],
        ),
        record(
            "horizon_v4_adversarial_settlement",
            "proof_or_model_strengthening",
            "horizon_v4_low_price_cost_invalid_boundary",
            "Zero-price settlement is cost-invalid evidence rather than runtime fuel mutation.",
            low_price,
            {"adversarial_settlement": "low_price", "authority": "casper"},
            ["Rust: generated_frontier_adversarial_fixtures_hold", "Rocq: low_deploy_price_violation_sound"],
        ),
    ]


def adversarial_slashing_records(profile, search_mode, objectives):
    if not (objective_enabled(objectives, "adversarial") or objective_enabled(objectives, "slashing") or objective_enabled(objectives, "security")):
        return []
    stale = canonical_scenario(
        "horizon_v4_stale_slashing_evidence_replay",
        events=[canonical_event("source", 1, descriptor="v4/stale-slash-evidence", deploy=0, path=[0])],
        initial_budget=4,
        settlement={"kind": "slash_after_evaluation", "escrow": 8, "token_cost": 2, "refund": 6, "stale_evidence": True},
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "slash_fields", "genesis"]},
        replay_mutations=["slash_fields", "genesis", "cost_trace_digest"],
        attack_campaign="adversarial_slashing_stale_cost_evidence",
        oracle_kind="stale_cost_evidence_boundary",
        production_path="slashing evidence validation + replay payload authentication",
        campaign_steps=["reserve", "finalize", "fork", "submit_stale_cost_evidence", "replay"],
        minimized_input_digest=digest_value("stale-slashing-evidence"),
        reproducer_command=command_for_fixture(),
        threat_family="adversarial_slashing",
        expected_invariants=["stale_cost_evidence_rejected", "slash_preserves_fee_settlement_inputs"],
        rust_reproducer={"test": "generated_frontier_adversarial_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="projection_risk",
    )
    composed = canonical_scenario(
        "horizon_v4_slash_refund_replay_composition",
        events=[
            canonical_event("source", 2, descriptor="v4/slash-refund", deploy=0, path=[0]),
            canonical_event("substitution", 1, descriptor="v4/slash-auth", deploy=1, path=[1]),
        ],
        deploy_count=2,
        initial_budget=6,
        phlo_limit=6,
        phlo_price=2,
        token_cost=3,
        settlement={"kind": "slash_after_evaluation", "escrow": 12, "token_cost": 6, "refund": 6},
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "signature", "slash_fields", "genesis"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count", "slash_fields"],
        attack_campaign="adversarial_slashing_refund_replay_composition",
        oracle_kind="slashing_refund_replay_composition",
        production_path="runtime budget + processed deploy payload + settlement projection",
        campaign_steps=["precharge", "reserve", "finalize", "settle", "slash", "replay"],
        minimized_input_digest=digest_value("slash-refund-replay"),
        reproducer_command=command_for_fixture(),
        threat_family="adversarial_slashing",
        expected_invariants=[
            "slash_system_effect_is_unmetered_for_user_budget",
            "uc_ca_058_refund_cannot_replenish_runtime_fuel",
            "replay_payload_authenticates_cost_trace_payload",
        ],
        rust_reproducer={"test": "generated_frontier_adversarial_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v4_adversarial_slashing",
            "projection_risk",
            "horizon_v4_stale_slashing_evidence_replay",
            "Replayed stale cost-invalid evidence is a projection risk unless production rejects it at the slashing boundary.",
            stale,
            {"adversarial_slashing": "stale_evidence", "stale_evidence": True, "replay": True},
            ["Rust: generated_frontier_adversarial_fixtures_hold", "Rocq: stale_cost_evidence_sound"],
        ),
        record(
            "horizon_v4_adversarial_slashing",
            "confirmed_safe",
            "horizon_v4_slash_refund_replay_composition",
            "Slashing, refund, replay authentication, and trace accounting compose without mutating user runtime fuel.",
            composed,
            {"adversarial_slashing": "slash_refund_replay", "cross_product": True, "slashing": "post_evaluation"},
            ["Rust: generated_frontier_adversarial_fixtures_hold", "Rocq: uc_ca_073_slashing_composition_frontier"],
        ),
    ]


def adversarial_lifecycle_records(profile, search_mode, objectives):
    if not (objective_enabled(objectives, "adversarial") or objective_enabled(objectives, "lifecycle") or objective_enabled(objectives, "concurrency") or objective_enabled(objectives, "security")):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    steps = find_or_none(
        st.lists(
            st.sampled_from(["precharge", "reserve", "finalize", "worker_join", "rollback", "replay", "settle"]),
            min_size=int(5),
            max_size=int(7),
            unique=True,
        ),
        lambda xs: "reserve" in xs and "finalize" in xs and "worker_join" in xs and xs.index("finalize") < xs.index("worker_join"),
        cfg,
    ) or ["precharge", "reserve", "finalize", "worker_join", "settle"]
    finalize_before_join = canonical_scenario(
        "horizon_v4_finalize_before_worker_join",
        events=[canonical_event("source", 1, descriptor="v4/finalize-before-join", deploy=0, path=[0])],
        lifecycle=steps,
        initial_budget=4,
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "failed"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        concurrency={"finalize_before_worker_join": True, "campaign_steps": steps},
        attack_campaign="adversarial_lifecycle_finalize_before_join",
        oracle_kind="finalization_requires_trace_completion",
        production_path="runtime budget finalization boundary",
        campaign_steps=steps,
        minimized_input_digest=digest_value(steps),
        reproducer_command=command_for_fixture(),
        threat_family="adversarial_lifecycle",
        expected_invariants=["concurrent_finalization_trace_completeness", "finalization_after_trace_completion"],
        rust_reproducer={"test": "generated_frontier_adversarial_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="projection_risk",
    )
    rollback_after_reserve = canonical_scenario(
        "horizon_v4_rollback_after_reserve",
        events=[canonical_event("source", 2, descriptor="v4/rollback-reserve", deploy=0, path=[0])],
        lifecycle=["precharge", "reserve", "rollback", "reserve", "finalize", "replay"],
        initial_budget=6,
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        concurrency={"rollback_after_reserve": True},
        attack_campaign="adversarial_lifecycle_rollback_after_reserve",
        oracle_kind="rollback_reservation_boundary",
        production_path="RuntimeBudget finalization-read/reset boundary",
        campaign_steps=["precharge", "reserve", "rollback", "reserve", "finalize", "replay"],
        minimized_input_digest=digest_value("rollback-after-reserve"),
        reproducer_command=command_for_fixture(),
        threat_family="adversarial_lifecycle",
        expected_invariants=["rollback_preserves_authenticated_trace_boundary"],
        rust_reproducer={"test": "generated_frontier_adversarial_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="proof_or_model_strengthening",
    )
    return [
        record(
            "horizon_v4_adversarial_lifecycle",
            "projection_risk",
            "horizon_v4_finalize_before_worker_join",
            "Finalization before worker trace completion remains a projection risk guarded by trace-completion replay evidence.",
            finalize_before_join,
            {"adversarial_lifecycle": "finalize_before_join", "finalize_before_join": True, "lifecycle": steps},
            ["Rust: generated_frontier_adversarial_fixtures_hold", "TLA+: RuntimeBudgetReplay"],
        ),
        record(
            "horizon_v4_adversarial_lifecycle",
            "proof_or_model_strengthening",
            "horizon_v4_rollback_after_reserve",
            "Rollback after reservation requires replay evidence to distinguish retained and discarded trace windows.",
            rollback_after_reserve,
            {"adversarial_lifecycle": "rollback_after_reserve", "rollback": True},
            ["Rust: generated_frontier_adversarial_fixtures_hold", "TLA+: RuntimeBudgetReplay"],
        ),
    ]


def adversarial_source_corpus_records(seed_roots, source_limit, objectives):
    if not (objective_enabled(objectives, "adversarial") or objective_enabled(objectives, "source_corpus") or objective_enabled(objectives, "security")):
        return []
    seeds = discover_source_seeds(seed_roots, source_limit)
    events = [event_from_seed(seed, index) for index, seed in enumerate(seeds)]
    total = sum(int(event["weight"]) for event in events)
    scenario = canonical_scenario(
        "horizon_v4_adversarial_source_corpus_projection",
        events=events,
        deploy_count=max(1, len(set(int(event["deploy"]) for event in events))),
        initial_budget=total + 16,
        replay_fields={"mode": "cost_accounted", "source_seed_count": len(seeds)},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        rust_replay={"fixture": "generated_frontier_adversarial_fixtures_hold"},
        source_seed={"seeds": seeds},
        attack_campaign="adversarial_source_corpus_projection",
        oracle_kind="source_corpus_projection",
        production_path="rholang::RuntimeBudget::cost_trace_digest",
        campaign_steps=["source_seed", "reserve", "mutate_descriptor", "finalize", "replay"],
        minimized_input_digest=digest_value(seeds),
        reproducer_command=command_for_fixture(),
        threat_family="adversarial_source_corpus",
        expected_invariants=["source_corpus_paths_preserve_trace_identity", "primitive_descriptor_mutation_changes_digest"],
        rust_reproducer={"test": "generated_frontier_adversarial_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="bisimilar",
    )
    return [
        record(
            "horizon_v4_adversarial_source_corpus",
            "bisimilar",
            "horizon_v4_adversarial_source_corpus_projection",
            "Real Rholang source paths seed adversarial trace identity, while primitive descriptors seed digest mutation checks.",
            scenario,
            {"adversarial_source_corpus": [seed["path"] for seed in seeds], "source_corpus": True},
            ["Rust: generated_frontier_adversarial_fixtures_hold", "Sage: v4 adversarial source corpus"],
        )
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
            "adversarial_witness_names_attack_campaign",
            "production_path_or_oracle_named",
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
        "coverage_features": item.get("coverage_features", []),
        "source_seed": scenario.get("source_seed", {}),
        "attack_campaign": scenario.get("attack_campaign", ""),
        "oracle_kind": scenario.get("oracle_kind", ""),
        "production_path": scenario.get("production_path", ""),
        "campaign_steps": scenario.get("campaign_steps", []),
        "minimized_input_digest": scenario.get("minimized_input_digest", ""),
        "reproducer_command": scenario.get("reproducer_command", ""),
        "production_disposition": production_disposition(item["classification"]),
        "production_guards": production_guards(item["name"], item["classification"], scenario),
        "trace_digest_fields": scenario.get("trace_digest_fields", []),
    }


def frontier_records(args):
    records = []
    records.extend(adversarial_budget_records(args.profile, args.search_mode, args.objectives))
    records.extend(adversarial_replay_records(args.profile, args.search_mode, args.objectives))
    records.extend(adversarial_settlement_records(args.profile, args.search_mode, args.objectives))
    records.extend(adversarial_slashing_records(args.profile, args.search_mode, args.objectives))
    records.extend(adversarial_lifecycle_records(args.profile, args.search_mode, args.objectives))
    records.extend(adversarial_source_corpus_records(args.source_root, args.source_limit, args.objectives))
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
