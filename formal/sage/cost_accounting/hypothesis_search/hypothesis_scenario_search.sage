import argparse
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
    "security",
    "concurrency",
    "settlement",
    "replay",
    "resource",
    "metamorphic",
    "slashing",
]


def objective_enabled(selected, objective):
    return selected == "all" or objective in selected.split(",")


def hypothesis_settings(profile, search_mode):
    if profile == "quick":
        max_examples = 64
    elif profile == "corpus":
        max_examples = 512
    else:
        max_examples = 2048
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


def event_sequence_cost(events):
    return sum(int(event.get("weight", 0)) for event in events if int(event.get("weight", 0)) > 0)


def frontier_records(profile, search_mode, objectives):
    cfg = hypothesis_settings(profile, search_mode)
    records = []

    if objective_enabled(objectives, "concurrency"):
        schedule_strategy = st.lists(
            st.sampled_from(["precharge", "admit", "reserve", "finalize", "worker_join", "settle", "replay"]),
            min_size=int(4),
            max_size=int(7),
        )
        early_finalize = find_or_none(
            schedule_strategy,
            lambda xs: "reserve" in xs
            and "finalize" in xs
            and "worker_join" in xs
            and xs.index("reserve") < xs.index("finalize") < xs.index("worker_join"),
            cfg,
        ) or ["precharge", "admit", "reserve", "finalize", "worker_join", "settle", "replay"]
        scenario = canonical_scenario(
            "hypothesis_finalization_before_worker_join",
            lifecycle=early_finalize,
            events=[canonical_event("source", 1, descriptor="worker-a", path=[0])],
            initial_budget=4,
            concurrency={"finalization_before_worker_join": True},
            threat_family="concurrency_schedule",
            expected_invariants=["finalization_after_worker_completion"],
            rust_reproducer={"test": "finalization_after_workers_observes_complete_trace_count"},
            promotion_target="rust:loom",
            expected_classification="projection_risk",
        )
        records.append(
            record(
                "hypothesis_concurrency_lifecycle",
                "projection_risk",
                "hypothesis_finalization_requires_worker_join",
                "Hypothesis minimizes schedules where finalization can observe a worker reservation before append completion.",
                scenario,
                {"lifecycle": early_finalize, "finalization": "before_worker_join", "metamorphic": "schedule_prefix"},
                ["Rust: loom_cost_trace_slots", "TLA+: RuntimeBudgetReplay"],
            )
        )

    if objective_enabled(objectives, "replay") or objective_enabled(objectives, "security"):
        fields = [
            "cost",
            "cost_trace_digest",
            "cost_trace_event_count",
            "cost_trace_present",
            "failed",
            "user_error",
            "system_error",
            "system_kind",
            "slash_fields",
            "genesis",
        ]
        mutation_set = find_or_none(
            st.lists(st.sampled_from(fields), min_size=int(1), max_size=int(4), unique=True),
            lambda xs: "cost_trace_digest" in xs or "cost_trace_event_count" in xs,
            cfg,
        ) or ["cost_trace_digest"]
        scenario = canonical_scenario(
            "hypothesis_replay_mutation_matrix",
            replay_fields={"fields": fields, "mode": "cost_accounted"},
            replay_mutations=mutation_set,
            threat_family="replay_authentication",
            expected_invariants=["replay_payload_field_sensitivity"],
            rust_reproducer={"fuzz": "replay_payload_cost_fields"},
            promotion_target="rust:fuzz",
            expected_classification="confirmed_safe",
        )
        records.append(
            record(
                "hypothesis_replay_mutation",
                "confirmed_safe",
                "hypothesis_replay_mutation_matrix",
                "Every minimized replay mutation set contains an authenticated cost-trace field or promotes to replay sensitivity coverage.",
                scenario,
                {"replay_mutation": mutation_set, "cache": "payload_sensitive"},
                ["Rocq: uc_ca_071_replay_mutation_frontier", "Fuzz: replay_payload_cost_fields"],
            )
        )

    if objective_enabled(objectives, "settlement") or objective_enabled(objectives, "security"):
        settlement_input = find_or_none(
            st.tuples(
                st.integers(min_value=int(0), max_value=int(8)),
                st.integers(min_value=int(0), max_value=int(4)),
                st.integers(min_value=int(0), max_value=int(12)),
            ),
            lambda values: values[2] > values[0] and values[1] > 0,
            cfg,
        ) or (0, 1, 1)
        phlo_limit, phlo_price, token_cost = [int(value) for value in settlement_input]
        escrow = phlo_limit * phlo_price
        scenario = canonical_scenario(
            "hypothesis_settlement_overrun",
            deploy_count=1,
            phlo_limit=phlo_limit,
            phlo_price=phlo_price,
            token_cost=token_cost,
            settlement={"escrow": escrow, "authority": "casper", "refund": 0},
            threat_family="settlement",
            expected_invariants=[
                "uc_ca_009_refund_is_bounded_by_escrow",
                "uc_ca_058_refund_cannot_replenish_runtime_fuel",
            ],
            rust_reproducer={"test": "settlement_edge_cases_are_total_and_deterministic"},
            promotion_target="rust:test",
            expected_classification="confirmed_safe",
        )
        records.append(
            record(
                "hypothesis_settlement_authority",
                "confirmed_safe",
                "hypothesis_settlement_overrun_is_total",
                "A token cost above prepaid escrow is minimized to a zero-refund settlement case, not a runtime-fuel mutation.",
                scenario,
                {"precharge": escrow, "token_cost": token_cost, "refund": 0, "authority": "system"},
                ["Rocq: uc_ca_058_refund_cannot_replenish_runtime_fuel", "Rust: settlement property tests"],
            )
        )

    if objective_enabled(objectives, "resource"):
        descriptor_len = find_or_none(
            st.integers(min_value=int(0), max_value=int(1024)),
            lambda value: value > 512,
            cfg,
        ) or 513
        scenario = canonical_scenario(
            "hypothesis_descriptor_bound",
            events=[canonical_event("primitive", 1, descriptor="x" * int(descriptor_len))],
            initial_budget=4,
            resource_bounds={"max_descriptor_bytes": 512},
            threat_family="resource_exhaustion",
            expected_invariants=["oversized_descriptor_rejected_before_mutation"],
            rust_reproducer={"fuzz": "runtime_budget_admission"},
            promotion_target="rust:fuzz",
            expected_classification="confirmed_safe",
        )
        records.append(
            record(
                "hypothesis_resource_bounds",
                "confirmed_safe",
                "hypothesis_descriptor_bound_rejection",
                "Descriptor amplification is minimized to the first byte above the consensus replay bound.",
                scenario,
                {"descriptor": "x" * int(descriptor_len), "descriptor_bytes": int(descriptor_len), "trace_mutated": False},
                ["Rocq: uc_ca_074_resource_exhaustion_frontier", "Fuzz: runtime_budget_admission"],
            )
        )

    if objective_enabled(objectives, "metamorphic"):
        permutation_input = find_or_none(
            st.permutations([0, 1, 2]),
            lambda order: list(order) != [0, 1, 2],
            cfg,
        ) or (1, 0, 2)
        events = [
            canonical_event("source", 1, descriptor="parallel", path=[0]),
            canonical_event("primitive", 2, descriptor="parallel-primitive", path=[1]),
            canonical_event("substitution", 1, descriptor="parallel-substitution", path=[2]),
        ]
        scenario = canonical_scenario(
            "hypothesis_event_permutation",
            events=events,
            initial_budget=8,
            concurrency={"permutation": list(permutation_input)},
            threat_family="concurrency_schedule",
            expected_invariants=["parallel_trace_digest_permutation_invariance"],
            rust_reproducer={"test": "generated_frontier_metamorphic_fixtures_hold"},
            promotion_target="rust:test",
            expected_classification="confirmed_safe",
        )
        records.append(
            record(
                "hypothesis_metamorphic_trace",
                "confirmed_safe",
                "hypothesis_event_permutation_digest_invariant",
                "A minimized non-identity event permutation must preserve canonical trace cost and digest.",
                scenario,
                {"metamorphic": "permutation", "permutation": list(permutation_input), "cost": event_sequence_cost(events)},
                ["Rocq: uc_ca_051_parallel_trace_and_cost_determinism", "Rust: metamorphic frontier tests"],
            )
        )

    if objective_enabled(objectives, "slashing"):
        scenario = canonical_scenario(
            "hypothesis_slashing_post_eval",
            deploy_count=1,
            settlement={"kind": "slash_after_evaluation", "cost_invalid_evidence": True},
            threat_family="slashing_composition",
            expected_invariants=[
                "slash_system_effect_is_unmetered_for_user_budget",
                "slash_preserves_fee_settlement_inputs",
            ],
            rust_reproducer={"test": "cost_accounting_frontier_generated_fixtures_are_classified"},
            promotion_target="rocq:uc_ca_073",
            expected_classification="confirmed_safe",
        )
        records.append(
            record(
                "hypothesis_slashing_composition",
                "confirmed_safe",
                "hypothesis_slashing_post_eval_is_unmetered",
                "Cost-invalid slashing evidence remains post-evaluation system evidence and cannot become user runtime fuel.",
                scenario,
                {"slashing": "post_evaluation", "runtime_fuel_added": False, "settlement_inputs_preserved": True},
                ["Rocq: uc_ca_073_slashing_composition_frontier", "Rust: slashing composition fixtures"],
            )
        )

    return records


def fixture_from_record(item):
    scenario = item["scenario"]
    oracle = item["deterministic_witness"]
    expected = {
        "classification": item["classification"],
        "promotion_target": scenario.get("promotion_target", "record"),
        "threat_family": scenario.get("threat_family", "search_governance"),
    }
    return scenario_fixture(
        item["name"],
        item["classification"],
        scenario,
        oracle,
        expected,
        assertions=[
            "classification != unexpected",
            "promotion_target != none",
            "threat_family != empty",
        ],
    )


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
    if scenario.get("concurrency", {}).get("finalization_before_worker_join"):
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
        "production_disposition": production_disposition(item["classification"]),
        "production_guards": production_guards(item["name"], item["classification"], scenario),
        "trace_digest_fields": scenario.get("trace_digest_fields", []),
    }


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", choices=["quick", "corpus", "deep"], default="quick")
    parser.add_argument("--search-mode", choices=["frontier", "all"], default="frontier")
    parser.add_argument("--objectives", default="all")
    parser.add_argument("--json-out")
    parser.add_argument("--fixture-out")
    parser.add_argument("--coverage-out")
    parser.add_argument("--rust-fixtures-out")
    args = parser.parse_args(argv)

    records = frontier_records(args.profile, args.search_mode, args.objectives)
    fixtures = [fixture_from_record(item) for item in records]
    rust_fixtures = [rust_fixture_from_record(item) for item in records]
    output = {
        "profile": args.profile,
        "search_mode": args.search_mode,
        "objectives": args.objectives,
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
