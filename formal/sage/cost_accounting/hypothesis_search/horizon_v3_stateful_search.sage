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
    "stateful",
    "production_path",
    "source_corpus",
    "exploit_cross_product",
    "replay",
    "settlement",
    "resource",
    "slashing",
    "concurrency",
]


def objective_enabled(selected, objective):
    selected_items = [item.strip() for item in str(selected).split(",") if item.strip()]
    return "all" in selected_items or objective in selected_items


def hypothesis_settings(profile, search_mode):
    if profile == "quick":
        max_examples = 96
    elif profile == "corpus":
        max_examples = 768
    else:
        max_examples = 3072
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
    weight = max(1, min(32, ((size % 29) + lines) % 33))
    descriptor = "{}#{}".format(seed.get("path", "seed"), seed.get("sha256_prefix", "digest"))
    return canonical_event("source", weight, descriptor=descriptor[:512], deploy=int(index % 4), path=[int(index), int(lines % 512)])


def expected_fixture_values(scenario):
    events = scenario.get("events", [])
    invalid = [
        event
        for event in events
        if int(event.get("weight", 0)) <= 0
        or len(event.get("path", [])) > 1024
        or (
            str(event.get("kind", "")) == "primitive"
            and len(str(event.get("primitive_descriptor", event.get("descriptor", "")))) > 512
        )
    ]
    positive = [event for event in events if int(event.get("weight", 0)) > 0 and event not in invalid]
    total = sum(int(event.get("weight", 0)) for event in positive)
    budget = int(scenario.get("initial_budget", 0))
    if invalid:
        return (0, 0, True, False)
    return (min(total, budget), len(positive), False, bool(events and total > budget and budget >= 0))


def stateful_campaign_records(profile, search_mode, objectives):
    if not (objective_enabled(objectives, "stateful") or objective_enabled(objectives, "security")):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    steps = [
        "precharge",
        "admit",
        "reserve",
        "oop",
        "rollback",
        "finalize",
        "replay",
        "settle",
        "slash",
        "clear_diagnostic",
        "reset",
    ]
    campaign = find_or_none(
        st.lists(st.sampled_from(steps), min_size=int(6), max_size=int(10), unique=True),
        lambda xs: "reserve" in xs
        and "rollback" in xs
        and "finalize" in xs
        and "settle" in xs
        and xs.index("reserve") < xs.index("finalize")
        and xs.index("finalize") < xs.index("settle"),
        cfg,
    ) or ["precharge", "admit", "reserve", "rollback", "finalize", "replay", "settle", "clear_diagnostic"]
    events = [
        canonical_event("source", 2, descriptor="stateful/source", deploy=0, path=[0]),
        canonical_event("primitive", 1, descriptor="stateful/primitive", deploy=0, path=[1]),
    ]
    scenario = canonical_scenario(
        "horizon_v3_stateful_budget_campaign",
        events=events,
        lifecycle=campaign,
        initial_budget=6,
        phlo_limit=6,
        phlo_price=2,
        token_cost=3,
        settlement={"escrow": 12, "token_cost": 6, "refund": 6, "authority": "casper"},
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "failed"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        concurrency={"campaign_steps": campaign, "finalize_after_reserve": True},
        rust_replay={"fixture": "generated_frontier_stateful_campaign_fixtures_hold"},
        oracle_kind="stateful_runtime_budget_campaign",
        production_path="rholang::RuntimeBudget::reserve_canonical",
        campaign_steps=campaign,
        minimized_input_digest=digest_value(campaign),
        reproducer_command="COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang generated_frontier_stateful_campaign_fixtures_hold",
        attack_campaign="stateful_budget_lifecycle",
        threat_family="stateful_campaign",
        expected_invariants=[
            "stateful_campaign_preserves_budget_conservation",
            "finalization_after_trace_completion",
            "settlement_after_evaluation",
        ],
        rust_reproducer={"test": "generated_frontier_stateful_campaign_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="proof_or_model_strengthening",
    )
    return [
        record(
            "horizon_v3_stateful_campaign",
            "proof_or_model_strengthening",
            "horizon_v3_stateful_budget_campaign",
            "A minimized stateful campaign composes runtime reservation, rollback/finalization ordering, replay evidence, and settlement.",
            scenario,
            {"stateful": campaign, "production_path": scenario["production_path"], "oracle": scenario["oracle_kind"]},
            ["TLA+: CostAccountingSearchFrontier metadata invariants", "Rust: generated stateful campaign replay"],
        )
    ]


def source_corpus_records(seed_roots, source_limit, objectives):
    if not (objective_enabled(objectives, "source_corpus") or objective_enabled(objectives, "production_path")):
        return []
    seeds = discover_source_seeds(seed_roots, source_limit)
    events = [event_from_seed(seed, index) for index, seed in enumerate(seeds)]
    total = sum(int(event["weight"]) for event in events)
    scenario = canonical_scenario(
        "horizon_v3_source_corpus_production_projection",
        events=events,
        deploy_count=max(1, len(set(int(event["deploy"]) for event in events))),
        initial_budget=total + 8,
        replay_fields={"mode": "cost_accounted", "source_seed_count": len(seeds)},
        rust_replay={"fixture": "generated_frontier_stateful_campaign_fixtures_hold"},
        source_seed={"seeds": seeds},
        oracle_kind="source_corpus_projection",
        production_path="rholang::RuntimeBudget::cost_trace_digest",
        campaign_steps=["source_seed", "reserve", "finalize", "replay"],
        minimized_input_digest=digest_value(seeds),
        reproducer_command="sage horizon_v3_stateful_search.sage -- --objectives source_corpus --rust-fixtures-out <fixtures>",
        threat_family="source_corpus",
        expected_invariants=["source_corpus_descriptors_preserve_trace_identity"],
        rust_reproducer={"test": "generated_frontier_stateful_campaign_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="bisimilar",
    )
    return [
        record(
            "horizon_v3_source_corpus",
            "bisimilar",
            "horizon_v3_source_corpus_production_projection",
            "Real Rholang source paths are minimized into production-shaped cost-trace replay fixtures.",
            scenario,
            {"source_corpus": [seed["path"] for seed in seeds], "production_path": scenario["production_path"]},
            ["Rust: generated stateful campaign replay", "Sage: v3 source corpus search"],
        )
    ]


def production_path_diff_records(profile, search_mode, objectives):
    if not (objective_enabled(objectives, "production_path") or objective_enabled(objectives, "replay")):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    mutation = find_or_none(
        st.lists(
            st.sampled_from(["cost", "cost_trace_digest", "cost_trace_event_count", "failed", "system_error", "block_hash"]),
            min_size=int(2),
            max_size=int(4),
            unique=True,
        ),
        lambda xs: "cost_trace_digest" in xs and ("cost_trace_event_count" in xs or "block_hash" in xs),
        cfg,
    ) or ["cost_trace_digest", "cost_trace_event_count"]
    events = [canonical_event("source", 1, descriptor="prod-path", deploy=1, path=[0])]
    scenario = canonical_scenario(
        "horizon_v3_replay_settlement_production_diff",
        events=events,
        initial_budget=4,
        phlo_limit=4,
        phlo_price=3,
        token_cost=1,
        settlement={"escrow": 12, "token_cost": 3, "refund": 9, "authority": "casper"},
        replay_fields={"fields": mutation, "mode": "cost_accounted"},
        replay_mutations=mutation,
        oracle_kind="production_path_differential",
        production_path="ProcessedDeploy::to_proto/from_proto + DeployData::refund_amount_for_token_cost",
        campaign_steps=["precharge", "reserve", "finalize", "serialize", "replay", "settle"],
        minimized_input_digest=digest_value(mutation),
        reproducer_command="COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang generated_frontier_stateful_campaign_fixtures_hold",
        attack_campaign="replay_settlement_differential",
        threat_family="production_path_diff",
        expected_invariants=["cost_fields_survive_wire_roundtrip", "uc_ca_009_refund_is_bounded_by_escrow"],
        rust_reproducer={"test": "generated_frontier_stateful_campaign_fixtures_hold"},
        promotion_target="rust:fuzz",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v3_production_path_diff",
            "confirmed_safe",
            "horizon_v3_replay_settlement_production_diff",
            "A production-path differential campaign ties replay cost fields to bounded Casper settlement arithmetic.",
            scenario,
            {"production_path": scenario["production_path"], "replay_mutation": mutation, "refund": 9},
            ["Rust: generated_frontier_stateful_campaign_fixtures_hold", "Rust: generated stateful campaign replay"],
        )
    ]


def resource_campaign_records(objectives):
    if not (objective_enabled(objectives, "resource") or objective_enabled(objectives, "security")):
        return []
    scenario = canonical_scenario(
        "horizon_v3_source_descriptor_resource_campaign",
        events=[canonical_event("primitive", 1, descriptor="r" * 513, deploy=2, path=[0])],
        initial_budget=8,
        resource_bounds={"max_descriptor_bytes": 512, "max_source_path_components": 1024},
        oracle_kind="resource_bound_production_rejection",
        production_path="rholang::RuntimeBudget::reserve_canonical",
        campaign_steps=["admit", "reserve", "reject_before_trace"],
        minimized_input_digest=digest_value({"descriptor_bytes": 513}),
        reproducer_command="COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang generated_frontier_stateful_campaign_fixtures_hold",
        attack_campaign="source_descriptor_resource_campaign",
        threat_family="resource_exhaustion",
        expected_invariants=["oversized_descriptor_rejected_before_mutation"],
        rust_reproducer={"test": "generated_frontier_stateful_campaign_fixtures_hold"},
        promotion_target="rust:fuzz",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v3_resource_campaign",
            "confirmed_safe",
            "horizon_v3_source_descriptor_resource_campaign",
            "A source/descriptor resource campaign keeps over-bound descriptors on the reject-before-mutation path.",
            scenario,
            {"resource_campaign": "descriptor_513", "trace_mutated": False, "production_path": scenario["production_path"]},
            ["Rust: generated_frontier_stateful_campaign_fixtures_hold", "Rocq: oversized billable event rejection"],
        )
    ]


def exploit_cross_product_records(objectives):
    if not (objective_enabled(objectives, "exploit_cross_product") or objective_enabled(objectives, "security")):
        return []
    events = [
        canonical_event("source", 2, descriptor="slash-refund", deploy=0, path=[0]),
        canonical_event("substitution", 1, descriptor="auth", deploy=1, path=[1]),
    ]
    scenario = canonical_scenario(
        "horizon_v3_exploit_cross_product_campaign",
        events=events,
        deploy_count=2,
        initial_budget=7,
        phlo_limit=7,
        phlo_price=2,
        token_cost=3,
        settlement={"escrow": 14, "token_cost": 6, "refund": 8, "kind": "slash_after_evaluation"},
        replay_fields={"fields": ["cost_trace_digest", "cost_trace_event_count", "signature", "slash_fields", "genesis"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        resource_bounds={"max_descriptor_bytes": 512},
        oracle_kind="exploit_cross_product_oracle",
        production_path="runtime budget + processed deploy wire payload + settlement projection",
        campaign_steps=["precharge", "reserve", "finalize", "replay", "settle", "slash"],
        minimized_input_digest=digest_value(events),
        reproducer_command="COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang generated_frontier_stateful_campaign_fixtures_hold",
        attack_campaign="slashing_refund_replay_cross_product",
        threat_family="exploit_cross_product",
        expected_invariants=[
            "slash_system_effect_is_unmetered_for_user_budget",
            "replay_payload_authenticates_cost_trace_payload",
            "uc_ca_058_refund_cannot_replenish_runtime_fuel",
        ],
        rust_reproducer={"test": "generated_frontier_stateful_campaign_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v3_exploit_cross_product",
            "confirmed_safe",
            "horizon_v3_exploit_cross_product_campaign",
            "A cross-product exploit campaign combines slashing/refund confusion with replay authentication and resource bounds, and production replay/block hashing now authenticates every composed field.",
            scenario,
            {"exploit_cross_product": True, "slashing": "post_evaluation", "refund": 8, "block_hash": "cost_fields"},
            [
                "Sage guard: cross_product_replay_payload_and_block_hash_authenticates_user_cost_trace_and_slash_fields",
                "Sage guard: refund_uses_scalar_cost_without_mutating_authenticated_trace_fields",
                "Rust: generated_frontier_stateful_campaign_fixtures_hold",
                "TLA+: CostAccountingSearchFrontier metadata invariants",
            ],
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
            "stateful_campaign_names_steps",
            "production_path_diff_names_oracle",
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
    records.extend(stateful_campaign_records(args.profile, args.search_mode, args.objectives))
    records.extend(source_corpus_records(args.source_root, args.source_limit, args.objectives))
    records.extend(production_path_diff_records(args.profile, args.search_mode, args.objectives))
    records.extend(resource_campaign_records(args.objectives))
    records.extend(exploit_cross_product_records(args.objectives))

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
