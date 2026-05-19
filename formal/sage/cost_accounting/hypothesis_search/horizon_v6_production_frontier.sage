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
    "production",
    "rholang_eval",
    "replay",
    "settlement",
    "slashing",
    "scheduler",
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
    weight = max(1, min(32, ((size % 31) + lines) % 33))
    descriptor = "{}#{}".format(seed.get("path", "seed"), seed.get("sha256_prefix", "digest"))
    return canonical_event("source", weight, descriptor=descriptor[:512], deploy=int(index % 3), path=[int(index), int(lines % 512)])


def command_for_fixture():
    return "COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang generated_frontier_production_fixtures_hold"


def runtime_budget_records(profile, search_mode, objectives):
    if not (objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    weights = find_or_none(
        st.lists(st.integers(min_value=1, max_value=4), min_size=int(2), max_size=int(4)),
        lambda xs: sum(xs) >= 3 and sum(xs) <= 10,
        cfg,
    ) or [1, 2, 3]
    events = [
        canonical_event(kind, weight, descriptor="v6/runtime/{}-{}".format(kind, index), deploy=0, path=[index])
        for index, (kind, weight) in enumerate(zip(["source", "primitive", "substitution", "source"], weights))
    ]
    accepted = canonical_scenario(
        "horizon_v6_runtime_budget_production_acceptance",
        events=events,
        initial_budget=sum(int(event["weight"]) for event in events) + 2,
        replay_fields={"fields": ["cost", "cost_trace_digest", "cost_trace_event_count"]},
        replay_mutations=["cost", "cost_trace_digest", "cost_trace_event_count"],
        rho_source="Nil",
        production_oracle="runtime_budget",
        expected_outcome="accept",
        differential_axes=["cost", "digest", "count"],
        attack_campaign="v6_runtime_budget_production_differential",
        oracle_kind="production_runtime_budget_acceptance",
        production_path="rholang::RuntimeBudget::reserve_canonical",
        campaign_steps=["reserve_sequence", "compare_cost_digest_count"],
        minimized_input_digest=digest_value(weights),
        reproducer_command=command_for_fixture(),
        threat_family="production_differential",
        expected_invariants=["production_runtime_budget_matches_fixture_projection"],
        rust_reproducer={"test": "generated_frontier_production_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    oop = canonical_scenario(
        "horizon_v6_runtime_budget_oop_boundary",
        events=[
            canonical_event("source", 3, descriptor="v6/oop/source", deploy=0, path=[0]),
            canonical_event("primitive", 4, descriptor="v6/oop/boundary", deploy=0, path=[1]),
        ],
        initial_budget=5,
        replay_fields={"fields": ["cost", "cost_trace_digest", "cost_trace_event_count", "failed"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        rho_source="Nil",
        production_oracle="runtime_budget",
        expected_outcome="oop",
        differential_axes=["cost", "digest", "count", "failed"],
        attack_campaign="v6_runtime_budget_oop_boundary",
        oracle_kind="production_runtime_budget_oop_boundary",
        production_path="rholang::RuntimeBudget::reserve_canonical",
        campaign_steps=["reserve", "cross_oop_boundary", "replay"],
        minimized_input_digest=digest_value("v6-oop"),
        reproducer_command=command_for_fixture(),
        threat_family="production_differential",
        expected_invariants=["oop_boundary_matches_production_projection"],
        rust_reproducer={"test": "generated_frontier_production_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v6_runtime_budget",
            "confirmed_safe",
            "horizon_v6_runtime_budget_production_acceptance",
            "RuntimeBudget production projection preserves generated cost, digest, and event-count evidence.",
            accepted,
            {"production_differential": "runtime_budget_acceptance", "production_oracle": "runtime_budget", "differential_axes": accepted["differential_axes"]},
            ["Rust: generated_frontier_production_fixtures_hold", "Rocq: runtime budget conservation"],
        ),
        record(
            "horizon_v6_runtime_budget",
            "confirmed_safe",
            "horizon_v6_runtime_budget_oop_boundary",
            "OOP boundary witnesses replay through the production RuntimeBudget fixture path.",
            oop,
            {"production_differential": "runtime_budget_oop", "expected_outcome": "oop"},
            ["Rust: generated_frontier_production_fixtures_hold", "Rocq: OOP boundary preservation"],
        ),
    ]


def rholang_eval_records(seed_roots, source_limit, objectives):
    if not (objective_enabled(objectives, "rholang_eval") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    seeds = discover_source_seeds(seed_roots, source_limit)
    first_seed = seeds[0]
    event = event_from_seed(first_seed, 0)
    scenario = canonical_scenario(
        "horizon_v6_rholang_eval_source_smoke",
        events=[event],
        initial_budget=int(event["weight"]) + 4,
        replay_fields={"mode": "cost_accounted", "source_seed_count": len(seeds)},
        rho_source="Nil",
        source_seed={"seeds": seeds},
        production_oracle="rholang_eval",
        expected_outcome="accept",
        differential_axes=["cost", "digest", "count", "trace_presence"],
        attack_campaign="v6_rholang_eval_source_shape",
        oracle_kind="production_rholang_eval_smoke",
        production_path="RhoRuntime::evaluate_with_term",
        campaign_steps=["source_seed", "evaluate_rho_source", "compare_runtime_budget_projection"],
        minimized_input_digest=digest_value(first_seed),
        reproducer_command=command_for_fixture(),
        threat_family="production_rholang_eval",
        expected_invariants=["generated_rho_source_evaluates_without_error"],
        rust_reproducer={"test": "generated_frontier_rholang_eval_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="bisimilar",
    )
    return [
        record(
            "horizon_v6_rholang_eval",
            "bisimilar",
            "horizon_v6_rholang_eval_source_smoke",
            "Source-shaped v6 fixtures evaluate through the production Rholang runtime before promotion.",
            scenario,
            {"production_rholang_eval": True, "rho_source": "Nil", "source_seed": first_seed.get("path", "synthetic")},
            ["Rust: generated_frontier_rholang_eval_fixtures_hold", "Sage: v6 source-shaped production frontier"],
        )
    ]


def replay_security_records(objectives):
    if not (objective_enabled(objectives, "replay") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    mutations = ["cost_trace_digest", "cost_trace_event_count", "cost_trace_present", "signature", "block_hash"]
    scenario = canonical_scenario(
        "horizon_v6_replay_downgrade_rejection",
        events=[
            canonical_event("source", 1, descriptor="v6/replay/source", deploy=0, path=[0]),
            canonical_event("substitution", 1, descriptor="v6/replay/subst", deploy=1, path=[1]),
        ],
        deploy_count=2,
        initial_budget=4,
        replay_fields={"fields": mutations, "mode": "cost_accounted"},
        negative_mutations=mutations,
        rho_source="Nil",
        production_oracle="casper_replay_payload",
        expected_outcome="replay_reject",
        differential_axes=["digest", "count", "trace_presence", "signature", "block_hash"],
        attack_campaign="v6_replay_downgrade_rejection",
        oracle_kind="production_replay_payload_downgrade_rejection",
        production_path="RuntimeManager replay payload + block hash authentication",
        campaign_steps=["reserve", "finalize", "remove_or_mutate_cost_trace", "replay"],
        minimized_input_digest=digest_value(mutations),
        reproducer_command=command_for_fixture(),
        threat_family="production_replay",
        expected_invariants=["cost_accounted_replay_rejects_missing_or_mutated_trace"],
        rust_reproducer={"test": "generated_frontier_casper_boundary_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v6_replay_security",
            "confirmed_safe",
            "horizon_v6_replay_downgrade_rejection",
            "Replay downgrade attempts remain negative-authentication witnesses on the production replay payload boundary.",
            scenario,
            {"replay_downgrade": mutations, "casper_boundary": "replay_payload"},
            ["Rust: generated_frontier_casper_boundary_fixtures_hold", "Casper: replay/block hash authentication tests"],
        )
    ]


def settlement_slashing_records(objectives):
    if not (objective_enabled(objectives, "settlement") or objective_enabled(objectives, "slashing") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    settlement = canonical_scenario(
        "horizon_v6_casper_settlement_isolation",
        events=[canonical_event("source", 2, descriptor="v6/settlement/source", deploy=0, path=[0])],
        initial_budget=4,
        phlo_limit=4,
        phlo_price=3,
        token_cost=2,
        settlement={"authority": "casper", "escrow": 12, "token_cost": 6, "refund": 6},
        replay_fields={"fields": ["cost", "cost_trace_digest", "cost_trace_event_count"]},
        replay_mutations=["cost", "cost_trace_digest"],
        rho_source="Nil",
        production_oracle="casper_settlement",
        expected_outcome="settlement_isolated",
        differential_axes=["cost", "refund", "digest"],
        attack_campaign="v6_casper_settlement_isolation",
        oracle_kind="production_casper_settlement_isolation",
        production_path="DeployData::refund_amount_for_token_cost + RuntimeBudget",
        campaign_steps=["precharge", "reserve", "settle", "assert_no_runtime_fuel_replenish"],
        minimized_input_digest=digest_value("v6-settlement-isolation"),
        reproducer_command=command_for_fixture(),
        threat_family="production_settlement",
        expected_invariants=[
            "uc_ca_058_refund_cannot_replenish_runtime_fuel",
            "uc_ca_009_charged_plus_refund_equals_escrow",
        ],
        rust_reproducer={"test": "generated_frontier_casper_boundary_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    stale = canonical_scenario(
        "horizon_v6_stale_slashing_duplicate_boundary",
        events=[canonical_event("source", 1, descriptor="v6/stale-slashing/source", deploy=0, path=[0])],
        initial_budget=4,
        settlement={"kind": "slash_after_evaluation", "authority": "casper", "escrow": 8, "token_cost": 2, "refund": 6, "stale_evidence": True, "duplicate_evidence": True},
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "slash_fields", "genesis", "block_hash"]},
        negative_mutations=["slash_fields", "genesis", "block_hash", "cost_trace_digest"],
        rho_source="Nil",
        production_oracle="slashing_evidence",
        expected_outcome="guarded_projection",
        differential_axes=["slash_fields", "genesis", "block_hash", "digest"],
        attack_campaign="v6_stale_slashing_duplicate_boundary",
        oracle_kind="production_stale_duplicate_slashing_boundary",
        production_path="slashing evidence validation + replay payload authentication",
        campaign_steps=["reserve", "finalize", "fork", "submit_duplicate_stale_evidence", "replay"],
        minimized_input_digest=digest_value("v6-stale-duplicate"),
        reproducer_command=command_for_fixture(),
        threat_family="slashing_composition",
        expected_invariants=["stale_cost_evidence_rejected", "duplicate_slash_does_not_mutate_user_cost_trace"],
        rust_reproducer={"test": "generated_frontier_casper_boundary_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="projection_risk",
    )
    return [
        record(
            "horizon_v6_settlement_slashing",
            "confirmed_safe",
            "horizon_v6_casper_settlement_isolation",
            "Casper settlement remains isolated from runtime fuel on production-shaped v6 fixtures.",
            settlement,
            {"settlement_isolation": True, "casper_boundary": "settlement", "refund": 6},
            ["Rust: generated_frontier_casper_boundary_fixtures_hold", "Rocq: refund cannot replenish runtime fuel"],
        ),
        record(
            "horizon_v6_settlement_slashing",
            "projection_risk",
            "horizon_v6_stale_slashing_duplicate_boundary",
            "Duplicate stale slashing evidence remains guarded at the production slashing boundary.",
            stale,
            {"casper_boundary": "slashing_evidence", "stale_evidence": True, "replay_downgrade": False},
            ["Rust: generated_frontier_casper_boundary_fixtures_hold", "Sage stale-evidence guard labels", "Rocq: stale_cost_evidence_sound"],
        ),
    ]


def scheduler_resource_records(objectives):
    records = []
    if objective_enabled(objectives, "scheduler") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security"):
        scheduler = canonical_scenario(
            "horizon_v6_scheduler_finalization_join_boundary",
            events=[canonical_event("source", 1, descriptor="v6/scheduler/join", deploy=0, path=[0])],
            lifecycle=["reserve", "worker_join", "finalize", "replay"],
            initial_budget=4,
            replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "failed"]},
            replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
            concurrency={"worker_join_before_finalize": True},
            rho_source="Nil",
            production_oracle="runtime_budget",
            expected_outcome="accept",
            differential_axes=["digest", "count", "failed"],
            attack_campaign="v6_scheduler_finalization_join_boundary",
            oracle_kind="production_scheduler_finalization_join_boundary",
            production_path="parallel evaluation join + RuntimeBudget finalization",
            campaign_steps=["reserve", "worker_join", "finalize", "replay"],
            minimized_input_digest=digest_value("v6-scheduler"),
            reproducer_command=command_for_fixture(),
            threat_family="production_scheduler",
            expected_invariants=["finalization_after_trace_completion", "parallelism_preserved_before_join"],
            rust_reproducer={"test": "generated_frontier_production_fixtures_hold"},
            promotion_target="rust:test",
            expected_classification="confirmed_safe",
        )
        records.append(
            record(
                "horizon_v6_scheduler",
                "confirmed_safe",
                "horizon_v6_scheduler_finalization_join_boundary",
                "Scheduler finalization witnesses preserve complete trace evidence without serializing evaluation bodies.",
                scheduler,
                {"scheduler_finalization": "join_before_finalize", "production_oracle": "runtime_budget"},
                ["Rust: generated_frontier_production_fixtures_hold", "TLA+: RuntimeBudgetReplay"],
            )
        )
    if objective_enabled(objectives, "resource") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security"):
        invalid = canonical_scenario(
            "horizon_v6_resource_invalid_admission_boundary",
            events=[canonical_event("source", 0, descriptor="v6/resource/zero-weight", deploy=0, path=[0])],
            initial_budget=4,
            resource_bounds={"zero_weight_rejected": True, "max_descriptor_bytes": 512},
            rho_source="Nil",
            production_oracle="runtime_budget",
            expected_outcome="invalid_admission",
            differential_axes=["cost", "digest", "count"],
            attack_campaign="v6_resource_invalid_admission_boundary",
            oracle_kind="production_invalid_admission_boundary",
            production_path="RuntimeBudget::validate_billable_event via reserve_canonical",
            campaign_steps=["admit", "reject_before_trace_mutation"],
            minimized_input_digest=digest_value("v6-invalid-admission"),
            reproducer_command=command_for_fixture(),
            threat_family="production_resource",
            expected_invariants=["invalid_admission_preserves_budget_and_trace"],
            rust_reproducer={"test": "generated_frontier_production_fixtures_hold"},
            promotion_target="rust:test",
            expected_classification="confirmed_safe",
        )
        records.append(
            record(
                "horizon_v6_resource",
                "confirmed_safe",
                "horizon_v6_resource_invalid_admission_boundary",
                "Invalid admission witnesses reject before production budget or trace mutation.",
                invalid,
                {"production_resource": "invalid_admission", "admission": "reject"},
                ["Rust: generated_frontier_production_fixtures_hold", "Rocq: zero-weight admission rejection"],
            )
        )
    return records


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
            "production_oracle": scenario.get("production_oracle", ""),
            "expected_outcome": scenario.get("expected_outcome", ""),
        },
        assertions=[
            "classification != unexpected",
            "production_oracle != empty",
            "expected_outcome in accepted_set",
            "differential_axes != empty",
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
        "rho_source": scenario.get("rho_source", ""),
        "production_oracle": scenario.get("production_oracle", ""),
        "expected_outcome": scenario.get("expected_outcome", ""),
        "differential_axes": scenario.get("differential_axes", []),
        "production_disposition": production_disposition(item["classification"]),
        "production_guards": production_guards(item["name"], item["classification"], scenario),
        "trace_digest_fields": scenario.get("trace_digest_fields", []),
    }


def frontier_records(args):
    records = []
    records.extend(runtime_budget_records(args.profile, args.search_mode, args.objectives))
    records.extend(rholang_eval_records(args.source_root, args.source_limit, args.objectives))
    records.extend(replay_security_records(args.objectives))
    records.extend(settlement_slashing_records(args.objectives))
    records.extend(scheduler_resource_records(args.objectives))
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
