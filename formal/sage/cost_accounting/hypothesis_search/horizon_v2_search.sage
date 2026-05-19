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
    "cross_product",
    "source",
    "differential",
    "exploit",
    "concurrency",
    "settlement",
    "replay",
    "resource",
    "slashing",
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
                digest = hashlib.sha256(content).hexdigest()
                seeds.append(
                    {
                        "path": os.path.relpath(path, root),
                        "root": root,
                        "bytes_sampled": len(content),
                        "size_bytes": int(size),
                        "line_count": int(content.count(b"\n") + 1 if content else 0),
                        "sha256_prefix": digest[:16],
                    }
                )
                if len(seeds) >= int(limit):
                    return seeds
    if not seeds:
        seeds.append(
            {
                "path": "synthetic/empty-par.rho",
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
    weight = max(1, min(16, (size + lines) % 17))
    descriptor = "{}#{}".format(seed.get("path", "seed"), seed.get("sha256_prefix", "digest"))
    return canonical_event("source", weight, descriptor=descriptor[:512], deploy=int(index % 4), path=[int(index), int(lines % 256)])


def source_seed_records(seed_roots, source_limit):
    seeds = discover_source_seeds(seed_roots, source_limit)
    events = [event_from_seed(seed, index) for index, seed in enumerate(seeds)]
    budget = sum(int(event["weight"]) for event in events) + 4
    scenario = canonical_scenario(
        "horizon_v2_source_seed_differential_replay",
        events=events,
        deploy_count=max(1, len(set(int(event["deploy"]) for event in events))),
        initial_budget=budget,
        replay_fields={"mode": "cost_accounted", "source_seed_count": len(seeds)},
        concurrency={"permutation": "source_seed_stable_identity"},
        rust_replay={"fixture": "generated_frontier_differential_fixtures_hold"},
        source_seed={"seeds": seeds},
        threat_family="differential_replay",
        expected_invariants=["source_seed_replay_matches_runtime_budget_projection"],
        rust_reproducer={"test": "generated_frontier_differential_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="bisimilar",
    )
    return [
        record(
            "horizon_v2_source_seed",
            "bisimilar",
            "horizon_v2_source_seed_differential_replay",
            "Source-derived Rholang seeds replay through production-shaped runtime-budget fixtures with stable cost and trace identity.",
            scenario,
            {
                "source_seed": [seed["path"] for seed in seeds],
                "differential": "model_events_to_rust_runtime_budget",
                "multi_axis": "source+replay+concurrency",
            },
            ["Rust: generated frontier differential replay", "docs: source-aware search horizon"],
        )
    ]


def cross_product_records(profile, search_mode, objectives):
    if not (
        objective_enabled(objectives, "cross_product")
        or objective_enabled(objectives, "security")
        or objective_enabled(objectives, "differential")
    ):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    event_input = find_or_none(
        st.lists(
            st.tuples(
                st.sampled_from(["source", "primitive", "substitution"]),
                st.integers(min_value=int(1), max_value=int(8)),
                st.integers(min_value=int(0), max_value=int(2)),
                st.integers(min_value=int(0), max_value=int(7)),
            ),
            min_size=int(3),
            max_size=int(6),
        ),
        lambda xs: len(set(item[2] for item in xs)) >= 2 and sum(item[1] for item in xs) >= 6,
        cfg,
    ) or [("source", 1, 0, 0), ("primitive", 2, 1, 1), ("substitution", 3, 0, 2)]
    events = [
        canonical_event(kind, weight, descriptor="v2-{}-{}".format(kind, salt), deploy=deploy, path=[deploy, index, salt])
        for index, (kind, weight, deploy, salt) in enumerate(event_input)
    ]
    weight_sum = sum(int(event["weight"]) for event in events)
    deploys = sorted(set(int(event["deploy"]) for event in events))
    scenario = canonical_scenario(
        "horizon_v2_multi_axis_cross_product",
        events=events,
        deploy_count=max(deploys) + 1 if deploys else 1,
        initial_budget=weight_sum + 2,
        phlo_limit=weight_sum + 3,
        phlo_price=2,
        token_cost=weight_sum,
        replay_fields={
            "fields": ["cost", "cost_trace_digest", "cost_trace_event_count", "failed", "slash_fields", "genesis"],
            "mode": "cost_accounted",
        },
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        settlement={"escrow": (weight_sum + 3) * 2, "token_cost": weight_sum * 2, "refund": 6, "authority": "casper"},
        concurrency={"permutation": "canonical_identity", "deploys": deploys},
        resource_bounds={"max_descriptor_bytes": 512, "max_path_entries": 16},
        rust_replay={"fixture": "generated_frontier_differential_fixtures_hold"},
        attack_campaign="multi_axis_cross_product",
        threat_family="differential_replay",
        expected_invariants=[
            "deploy_local_budget_isolation",
            "trace_digest_count_authenticated",
            "settlement_does_not_mutate_runtime_fuel",
        ],
        rust_reproducer={"test": "generated_frontier_differential_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="proof_or_model_strengthening",
    )
    return [
        record(
            "horizon_v2_cross_product",
            "proof_or_model_strengthening",
            "horizon_v2_multi_axis_cross_product",
            "A minimized cross-product witness combines multi-deploy events, replay-field mutation, settlement, resource bounds, and canonical trace identity.",
            scenario,
            {
                "cross_product": ["multi_deploy", "replay_mutation", "settlement", "resource_bounds", "differential"],
                "multi_axis": True,
                "deploys": deploys,
                "event_weight_sum": int(weight_sum),
            },
            ["Rocq: use-case adequacy extension", "Rust: generated differential replay", "Sage: v2 horizon frontier"],
        )
    ]


def lifecycle_attack_records(profile, search_mode, objectives):
    if not (objective_enabled(objectives, "concurrency") or objective_enabled(objectives, "exploit")):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    steps = ["precharge", "admit", "reserve", "rollback", "finalize", "replay", "settle", "slash"]
    lifecycle = find_or_none(
        st.lists(st.sampled_from(steps), min_size=int(5), max_size=int(8), unique=True),
        lambda xs: "reserve" in xs
        and "rollback" in xs
        and "finalize" in xs
        and "settle" in xs
        and xs.index("finalize") < xs.index("settle")
        and xs.index("reserve") < xs.index("rollback"),
        cfg,
    ) or ["precharge", "admit", "reserve", "rollback", "finalize", "settle", "replay", "slash"]
    scenario = canonical_scenario(
        "horizon_v2_lifecycle_attack_campaign",
        events=[canonical_event("source", 2, descriptor="rollback-boundary", path=[0])],
        lifecycle=lifecycle,
        initial_budget=4,
        settlement={"escrow": 8, "token_cost": 4, "refund": 4, "authority": "casper"},
        concurrency={"lifecycle": lifecycle},
        rust_replay={"fixture": "generated_frontier_differential_fixtures_hold"},
        attack_campaign="rollback_finalize_replay_settlement",
        threat_family="exploit_campaign",
        expected_invariants=["rollback_cannot_erase_authenticated_cost_evidence", "settlement_after_evaluation"],
        rust_reproducer={"test": "generated_frontier_differential_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="projection_risk",
    )
    return [
        record(
            "horizon_v2_lifecycle_attack",
            "projection_risk",
            "horizon_v2_lifecycle_attack_campaign",
            "Lifecycle attack search keeps rollback, finalization, replay, settlement, and slashing phases in one classified campaign witness.",
            scenario,
            {"exploit_campaign": "rollback_finalize_replay_settlement", "lifecycle": lifecycle, "tamper": "rollback_trace_erasure"},
            ["Rust: differential replay fixture", "TLA+: RuntimeBudgetReplay lifecycle ordering"],
        )
    ]


def exploit_campaign_records(objectives):
    if not (objective_enabled(objectives, "exploit") or objective_enabled(objectives, "security")):
        return []
    cases = [
        (
            "horizon_v2_replay_cache_substitution_campaign",
            "replay_authentication",
            "replay_cache_substitution",
            canonical_scenario(
                "horizon_v2_replay_cache_substitution_campaign",
                replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "signature", "genesis"]},
                replay_mutations=["cost_trace_digest", "signature"],
                rust_replay={"fixture": "replay_payload_cost_fields"},
                attack_campaign="replay_cache_substitution",
                threat_family="exploit_campaign",
                expected_invariants=["replay_cache_key_authenticates_cost_trace_payload"],
                promotion_target="rust:fuzz",
                expected_classification="confirmed_safe",
            ),
            {"exploit": "replay_cache_substitution", "replay_cache": "payload_sensitive", "signed_payload": True},
        ),
        (
            "horizon_v2_refund_replenish_campaign",
            "settlement",
            "refund_replenish_attempt",
            canonical_scenario(
                "horizon_v2_refund_replenish_campaign",
                events=[canonical_event("source", 3, descriptor="refund-attempt", path=[0])],
                initial_budget=5,
                settlement={"escrow": 10, "token_cost": 12, "refund": 0, "authority": "casper"},
                attack_campaign="refund_replenish_attempt",
                threat_family="exploit_campaign",
                expected_invariants=["uc_ca_058_refund_cannot_replenish_runtime_fuel"],
                promotion_target="rust:test",
                expected_classification="confirmed_safe",
            ),
            {"exploit": "refund_replenish_attempt", "refund": 0, "precharge": 10},
        ),
        (
            "horizon_v2_slashing_refund_confusion_campaign",
            "slashing_composition",
            "slashing_refund_confusion",
            canonical_scenario(
                "horizon_v2_slashing_refund_confusion_campaign",
                events=[canonical_event("source", 2, descriptor="slash-cost-invalid", path=[0])],
                initial_budget=6,
                settlement={"escrow": 12, "token_cost": 4, "refund": 8, "kind": "slash_after_evaluation"},
                attack_campaign="slashing_refund_confusion",
                threat_family="exploit_campaign",
                expected_invariants=[
                    "slash_system_effect_is_unmetered_for_user_budget",
                    "slash_preserves_fee_settlement_inputs",
                ],
                promotion_target="rocq:uc_ca_073",
                expected_classification="confirmed_safe",
            ),
            {"exploit": "slashing_refund_confusion", "slashing": "post_evaluation", "runtime_fuel_added": False},
        ),
        (
            "horizon_v2_descriptor_inflation_campaign",
            "resource_exhaustion",
            "descriptor_inflation",
            canonical_scenario(
                "horizon_v2_descriptor_inflation_campaign",
                events=[canonical_event("primitive", 1, descriptor="x" * 513, path=[0])],
                initial_budget=8,
                resource_bounds={"max_descriptor_bytes": 512},
                attack_campaign="descriptor_inflation",
                threat_family="exploit_campaign",
                expected_invariants=["oversized_descriptor_rejected_before_mutation"],
                promotion_target="rust:fuzz",
                expected_classification="confirmed_safe",
            ),
            {"exploit": "descriptor_inflation", "descriptor": "over_bound", "trace_mutated": False},
        ),
    ]
    records = []
    for name, family, campaign, scenario, witness in cases:
        records.append(
            record(
                "horizon_v2_exploit_campaign",
                "confirmed_safe",
                name,
                "Exploit campaign '{}' is classified against the cost-accounting protection boundary for {}.".format(campaign, family),
                scenario,
                witness,
                ["Rust/Sage/TLA: campaign remains in documented bucket", "docs: cost-accounting threat model"],
            )
        )
    return records


def rust_fixture_from_record(item):
    scenario = item["scenario"]
    events = scenario.get("events", [])
    positive_events = [event for event in events if int(event.get("weight", 0)) > 0]
    invalid_events = [event for event in events if int(event.get("weight", 0)) <= 0]
    oversized_primitive_descriptor = any(
        str(event.get("kind", "")) == "primitive"
        and len(str(event.get("primitive_descriptor", event.get("descriptor", "")))) > 512
        for event in events
    )
    oversized_source_path = any(len(event.get("path", [])) > 1024 for event in events)
    weight_sum = sum(int(event.get("weight", 0)) for event in positive_events)
    expected_event_count = len(positive_events)
    if oversized_primitive_descriptor or oversized_source_path:
        expected_event_count = 0
        weight_sum = 0
    return {
        "id": item["name"],
        "classification": item["classification"],
        "threat_family": scenario.get("threat_family", "search_governance"),
        "promotion_target": scenario.get("promotion_target", "record"),
        "initial_budget": int(scenario.get("initial_budget", 0)),
        "events": events,
        "expected_total_cost": int(min(weight_sum, int(scenario.get("initial_budget", 0))) if events else 0),
        "expected_event_count": int(expected_event_count),
        "expects_invalid_admission": len(invalid_events) > 0
        or oversized_primitive_descriptor
        or oversized_source_path,
        "expects_oop": bool(events and weight_sum > int(scenario.get("initial_budget", 0)) and int(scenario.get("initial_budget", 0)) >= 0),
        "settlement": scenario.get("settlement", {}),
        "replay_mutations": scenario.get("replay_mutations", []),
        "coverage_features": item.get("coverage_features", []),
        "source_seed": scenario.get("source_seed", {}),
        "attack_campaign": scenario.get("attack_campaign", ""),
        "production_disposition": production_disposition(item["classification"]),
        "production_guards": production_guards(item["name"], item["classification"], scenario),
        "trace_digest_fields": scenario.get("trace_digest_fields", []),
    }


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
            "source_action_requires_rust_or_invariant_evidence",
        ],
    )


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

    records = []
    if objective_enabled(args.objectives, "source") or objective_enabled(args.objectives, "differential"):
        records.extend(source_seed_records(args.source_root, args.source_limit))
    records.extend(cross_product_records(args.profile, args.search_mode, args.objectives))
    records.extend(lifecycle_attack_records(args.profile, args.search_mode, args.objectives))
    records.extend(exploit_campaign_records(args.objectives))

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
