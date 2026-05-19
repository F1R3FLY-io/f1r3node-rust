import argparse
import hashlib
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(sys.argv[0]))), "scenario_schema.sage"))


CASES = [
    {
        "id": "v13_runtime_to_replay_trace_commitment",
        "semantic_oracle": "runtime_to_replay_trace_commitment",
        "primary_surface": "runtime_budget",
        "primary_risk": "trace_slot_capacity",
        "secondary_surface": "casper_replay",
        "secondary_risk": "replay_auth_digest_count",
        "cross_surface_role": "source_to_sink",
        "expected_disposition": "accepted",
        "mutation_axis": "runtime_to_replay",
        "events": [
            canonical_event("source", 2, descriptor="v13/runtime/replay/a", deploy=0, path=[13, 0]),
            canonical_event("source", 1, descriptor="v13/runtime/replay/b", deploy=0, path=[13, 1]),
        ],
        "initial_budget": 8,
        "replay_mutations": ["runtime_to_replay", "cost_trace_digest", "cost_trace_event_count"],
        "source_facets": ["runtime_budget", "trace_commitment", "casper_replay", "digest_count"],
    },
    {
        "id": "v13_runtime_to_settlement_fuel_isolation",
        "semantic_oracle": "runtime_to_settlement_fuel_isolation",
        "primary_surface": "runtime_budget",
        "primary_risk": "oop_boundary_singleton",
        "secondary_surface": "settlement",
        "secondary_risk": "refund_as_fuel",
        "cross_surface_role": "source_to_settlement",
        "expected_disposition": "settlement_bounded",
        "mutation_axis": "runtime_to_settlement",
        "events": [
            canonical_event("source", 9, descriptor="v13/runtime/settlement/oop", deploy=0, path=[13, 2]),
        ],
        "initial_budget": 4,
        "settlement": {"escrow": 12, "token_cost": 4, "refund": 8, "authority": "casper", "phlo_limit": 12, "phlo_price": 1},
        "replay_mutations": ["runtime_to_settlement", "cost", "refund"],
        "source_facets": ["runtime_budget", "oop_boundary", "settlement", "fuel_isolation"],
    },
    {
        "id": "v13_metering_to_parallel_digest_stability",
        "semantic_oracle": "metering_to_parallel_digest_stability",
        "primary_surface": "metering",
        "primary_risk": "pending_queue_ordering",
        "secondary_surface": "parallel_eval",
        "secondary_risk": "completion_order_parallelism",
        "cross_surface_role": "bridge_to_bridge",
        "expected_disposition": "accepted",
        "mutation_axis": "metering_to_parallel",
        "events": [
            canonical_event("source", 2, descriptor="v13/parallel/a", deploy=0, path=[13, 3]),
            canonical_event("substitution", 1, descriptor="v13/parallel/b", deploy=1, path=[13, 4]),
            canonical_event("primitive", 1, descriptor="v13/parallel/c", primitive_descriptor="v13/parallel/c", deploy=2, path=[13, 5]),
        ],
        "initial_budget": 10,
        "replay_mutations": ["completion_order", "cost_trace_digest", "cost_trace_event_count"],
        "source_facets": ["metering", "queue_order", "parallel_eval", "completion_order"],
    },
    {
        "id": "v13_replay_to_slashing_authentication",
        "semantic_oracle": "replay_to_slashing_authentication",
        "primary_surface": "casper_replay",
        "primary_risk": "replay_payload_cache_key",
        "secondary_surface": "slashing",
        "secondary_risk": "slash_field_authentication",
        "cross_surface_role": "sink_to_sink",
        "expected_disposition": "replay_invalid",
        "mutation_axis": "replay_to_slashing",
        "events": [
            canonical_event("source", 1, descriptor="v13/replay/slashing", deploy=0, path=[13, 6]),
        ],
        "initial_budget": 8,
        "settlement": {"kind": "slash_after_evaluation", "authority": "casper", "escrow": 12, "token_cost": 4, "refund": 8, "slashing_scope": "post_eval"},
        "replay_mutations": ["replay_to_slashing", "slash_fields", "cost_trace_digest", "cost_trace_event_count", "block_hash", "signature"],
        "source_facets": ["casper_replay", "payload_hash", "slashing", "field_authentication"],
    },
    {
        "id": "v13_legacy_to_runtime_quarantine",
        "semantic_oracle": "legacy_to_runtime_quarantine",
        "primary_surface": "legacy_quarantine",
        "primary_risk": "legacy_runtime_metering_downgrade",
        "secondary_surface": "runtime_budget",
        "secondary_risk": "invalid_admission_before_mutation",
        "cross_surface_role": "quarantine_to_source",
        "expected_disposition": "source_absent",
        "mutation_axis": "legacy_to_runtime",
        "events": [
            canonical_event("source", 1, descriptor="v13/legacy/runtime", deploy=0, path=[13, 7]),
        ],
        "initial_budget": 8,
        "replay_mutations": ["legacy_to_runtime", "cost_trace_present"],
        "source_facets": ["legacy_quarantine", "absent_surface", "runtime_budget", "downgrade_guard"],
    },
]


def selected_objectives(value):
    return [item.strip() for item in str(value).split(",") if item.strip()]


def objective_enabled(selected, case):
    items = selected_objectives(selected)
    surfaces = [case.get("primary_surface", ""), case.get("secondary_surface", "")]
    return (
        not items
        or "all" in items
        or "security" in items
        or "source_semantic_oracle" in items
        or case.get("semantic_oracle", "") in items
        or any(surface in items for surface in surfaces)
    )


def digest_value(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, default=schema_json_default).encode("utf-8")).hexdigest()[:16]


def load_source_surface(path):
    if not path:
        raise SystemExit("--source-surface-json is required for v13 source-semantic search")
    with open(path, "r") as handle:
        manifest = json.load(handle)
    surfaces = manifest.get("surfaces", [])
    if not surfaces:
        raise SystemExit("v13 source-semantic search requires at least one source surface")
    return surfaces


def surface_for(surfaces, cost_surface, source_risk):
    for surface in surfaces:
        if surface.get("cost_surface") == cost_surface and surface.get("source_risk") == source_risk:
            return surface
    for surface in surfaces:
        if surface.get("cost_surface") == cost_surface:
            return surface
    raise SystemExit("missing source surface for {} ({})".format(source_risk, cost_surface))


def source_facets_for(primary, secondary, case):
    facets = []
    for item in primary.get("source_facets", []) + secondary.get("source_facets", []) + case.get("source_facets", []):
        if item not in facets:
            facets.append(item)
    return facets


def source_anchor_digest_for(primary, secondary, case):
    anchors = [primary.get("source_anchor_digest", ""), secondary.get("source_anchor_digest", ""), case.get("id", "")]
    return digest_value([anchor for anchor in anchors if anchor])


def expected_values(scenario):
    consumed = 0
    count = 0
    budget = int(scenario.get("initial_budget", 0))
    for event in scenario.get("events", []):
        weight = int(event.get("weight", 0))
        if weight <= 0 or weight > 2**63 - 1:
            return (consumed, count, True, False)
        if consumed + weight > budget:
            return (budget, count + 1, False, True)
        consumed += weight
        count += 1
    return (consumed, count, False, False)


def command_for_fixture(test_name):
    return "COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang {}".format(test_name)


def rust_test_for(semantic_oracle):
    if semantic_oracle in [
        "runtime_to_replay_trace_commitment",
        "runtime_to_settlement_fuel_isolation",
        "metering_to_parallel_digest_stability",
    ]:
        return "generated_frontier_v13_runtime_metering_parallel_oracles_hold"
    if semantic_oracle in ["replay_to_slashing_authentication", "legacy_to_runtime_quarantine"]:
        return "generated_frontier_v13_casper_settlement_slashing_oracles_hold"
    return "generated_frontier_v13_source_semantic_oracles_hold"


def record_for_case(case, primary, secondary):
    rust_test = rust_test_for(case["semantic_oracle"])
    classification = (
        "confirmed_safe"
        if primary.get("found", False)
        or primary.get("expected_state") == "absent"
        or case["expected_disposition"] == "source_absent"
        else "needs_source_audit"
    )
    scenario = canonical_scenario(
        case["id"],
        events=case.get("events", []),
        initial_budget=case.get("initial_budget", 0),
        settlement=case.get("settlement", {}),
        replay_fields={"fields": ["cost", "cost_trace_digest", "cost_trace_event_count", "source_semantic_oracle"]},
        replay_mutations=case.get("replay_mutations", []),
        negative_mutations=case.get("replay_mutations", []),
        source_seed={"primary_surface": primary, "secondary_surface": secondary, "source_semantic_case": case},
        rho_source='@0!("v13-source-semantic")',
        production_oracle=case["semantic_oracle"],
        expected_outcome=case["expected_disposition"],
        differential_axes=["cost", "digest", "count", case["mutation_axis"], case["primary_surface"], case["secondary_surface"]],
        eval_phlo=100000,
        expected_error_kind="none" if case["expected_disposition"] == "accepted" else case["expected_disposition"],
        eval_result_axes=["cost", "digest", "count", "errors", "parallel_completion_order"],
        rho_source_digest=digest_value({"case": case["id"], "primary": primary, "secondary": secondary}),
        replay_mode="play_replay",
        term_family=case["semantic_oracle"],
        term_parameters={"primary_surface": case["primary_surface"], "secondary_surface": case["secondary_surface"], "mutation_axis": case["mutation_axis"]},
        adequacy_requirements=["source_semantic_oracle", "source_facet", "source_anchor_digest", "cross_surface_role"],
        attack_campaign="v13_source_semantic_{}".format(case["semantic_oracle"]),
        oracle_kind=case["semantic_oracle"],
        production_path="{}::{}:{} -> {}::{}:{}".format(
            primary.get("source_file", ""),
            primary.get("source_symbol", ""),
            int(primary.get("source_line", 0) or 0),
            secondary.get("source_file", ""),
            secondary.get("source_symbol", ""),
            int(secondary.get("source_line", 0) or 0),
        ),
        campaign_steps=["extract_source_surfaces", "compose_cross_surface_oracle", "replay_native_api"],
        minimized_input_digest=digest_value({"case": case, "primary": primary, "secondary": secondary}),
        reproducer_command=command_for_fixture(rust_test),
        production_replay_target=rust_test,
        promotion_gate="source_semantic_oracle_before_promotion",
        threat_family="source_semantic_{}".format(case["semantic_oracle"]),
        expected_invariants=["v13_source_semantic_oracle_matches_f1r3node_rust_surfaces"],
        rust_reproducer={"test": rust_test, "semantic_oracle": case["semantic_oracle"]},
        promotion_target="rust:test",
        expected_classification=classification,
        source_file=primary.get("source_file", ""),
        source_line=int(primary.get("source_line", 0) or 0),
        source_symbol=primary.get("source_symbol", ""),
        cost_surface=primary.get("cost_surface", ""),
        source_risk=primary.get("source_risk", ""),
        reachable_from_user_deploy=bool(primary.get("reachable_from_user_deploy", False)),
        source_surface_status=primary.get("source_surface_status", ""),
        oracle_surface=case["primary_surface"],
        mutation_axis=case["mutation_axis"],
        expected_disposition=case["expected_disposition"],
        source_facets=source_facets_for(primary, secondary, case),
        source_anchor_digest=source_anchor_digest_for(primary, secondary, case),
        cross_surface_role=case["cross_surface_role"],
        semantic_oracle=case["semantic_oracle"],
    )
    witness = {
        "source_semantic_oracle": case["semantic_oracle"],
        "semantic_oracle": case["semantic_oracle"],
        "source_facet": scenario["source_facets"],
        "source_anchor_digest": scenario["source_anchor_digest"],
        "cross_surface_role": case["cross_surface_role"],
        "runtime_to_replay": case["mutation_axis"] == "runtime_to_replay",
        "runtime_to_settlement": case["mutation_axis"] == "runtime_to_settlement",
        "metering_to_parallel": case["mutation_axis"] == "metering_to_parallel",
        "replay_to_slashing": case["mutation_axis"] == "replay_to_slashing",
        "legacy_to_runtime": case["mutation_axis"] == "legacy_to_runtime",
    }
    return record(
        "horizon_v13_source_semantic_security",
        classification,
        case["id"],
        "Source-semantic oracle {} composes {} with {} through f1r3node-rust surfaces.".format(
            case["semantic_oracle"], case["primary_surface"], case["secondary_surface"]
        ),
        scenario,
        witness,
        ["Rust: {}".format(rust_test), "Sage: v13 source-semantic frontier"],
    )


def adequacy_record():
    scenario = canonical_scenario(
        "horizon_v13_source_semantic_coverage_adequacy",
        events=[],
        initial_budget=0,
        source_seed={"adequacy": "v13_source_semantic"},
        rho_source="Nil",
        production_oracle="v13_source_semantic_adequacy",
        expected_outcome="coverage_adequacy",
        term_family="v13_source_semantic_coverage_adequacy",
        adequacy_requirements=[
            "runtime_to_replay_trace_commitment",
            "runtime_to_settlement_fuel_isolation",
            "metering_to_parallel_digest_stability",
            "replay_to_slashing_authentication",
            "legacy_to_runtime_quarantine",
        ],
        attack_campaign="v13_source_semantic_coverage_adequacy",
        oracle_kind="v13_source_semantic_adequacy",
        production_path="formal/sage/cost_accounting/hypothesis_search/horizon_v13_source_semantic_security_search.sage::CASES:1",
        campaign_steps=["generate_source_semantic_oracles", "assert_cross_surface_coverage"],
        minimized_input_digest="v13-source-semantic-adequacy",
        reproducer_command=command_for_fixture("generated_frontier_v13_coverage_adequacy_holds"),
        production_replay_target="generated_frontier_v13_coverage_adequacy_holds",
        promotion_gate="v13_source_semantic_adequacy_gate",
        threat_family="coverage_adequacy",
        expected_invariants=["v13_source_semantic_frontier_covers_all_required_cross_surface_oracles"],
        rust_reproducer={"test": "generated_frontier_v13_coverage_adequacy_holds"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
        source_file="formal/sage/cost_accounting/hypothesis_search/horizon_v13_source_semantic_security_search.sage",
        source_line=1,
        source_symbol="CASES",
        cost_surface="coverage_adequacy",
        source_risk="source_semantic_coverage",
        source_surface_status="present",
        oracle_surface="coverage_adequacy",
        mutation_axis="coverage",
        expected_disposition="coverage_adequacy",
        source_facets=["coverage_adequacy", "source_semantic_oracle"],
        source_anchor_digest="v13-source-semantic-adequacy",
        cross_surface_role="coverage",
        semantic_oracle="coverage_adequacy",
    )
    return record(
        "horizon_v13_source_semantic_security",
        "confirmed_safe",
        scenario["name"],
        "The V13 frontier fails closed if required source-semantic oracles disappear.",
        scenario,
        {"source_semantic_oracle": "coverage_adequacy", "coverage_adequacy": True},
        ["Rust: generated_frontier_v13_coverage_adequacy_holds"],
    )


def frontier_records(args):
    surfaces = load_source_surface(args.source_surface_json)
    records = []
    for case in CASES:
        if objective_enabled(args.objectives, case):
            primary = surface_for(surfaces, case["primary_surface"], case["primary_risk"])
            secondary = surface_for(surfaces, case["secondary_surface"], case["secondary_risk"])
            records.append(record_for_case(case, primary, secondary))
    if "all" in selected_objectives(args.objectives) or "security" in selected_objectives(args.objectives) or "source_semantic_oracle" in selected_objectives(args.objectives):
        records.append(adequacy_record())
    return records


def assert_adequacy(records):
    semantic_oracles = set()
    facets = set()
    features = set()
    roles = set()
    for item in records:
        scenario = item["scenario"]
        semantic = scenario.get("semantic_oracle", "")
        if semantic and semantic != "coverage_adequacy":
            semantic_oracles.add(semantic)
        roles.add(scenario.get("cross_surface_role", ""))
        for facet in scenario.get("source_facets", []):
            facets.add(facet)
        for feature in item.get("coverage_features", []):
            features.add(feature)
    required_oracles = set([
        "runtime_to_replay_trace_commitment",
        "runtime_to_settlement_fuel_isolation",
        "metering_to_parallel_digest_stability",
        "replay_to_slashing_authentication",
        "legacy_to_runtime_quarantine",
    ])
    required_facets = set(["runtime_budget", "casper_replay", "settlement", "metering", "parallel_eval", "slashing", "legacy_quarantine"])
    required_features = set(["source_semantic_oracle", "source_facet", "source_anchor_digest", "cross_surface_role", "production_replay_target", "promotion_gate", "coverage_adequacy"])
    missing_oracles = sorted(required_oracles - semantic_oracles)
    missing_facets = sorted(required_facets - facets)
    missing_features = sorted(required_features - features)
    if missing_oracles or missing_facets or missing_features:
        raise SystemExit(
            "v13 source-semantic adequacy failure: missing_oracles={} missing_facets={} missing_features={}".format(
                missing_oracles, missing_facets, missing_features
            )
        )


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
            "semantic_oracle": scenario.get("semantic_oracle", ""),
            "source_facets": scenario.get("source_facets", []),
            "cross_surface_role": scenario.get("cross_surface_role", ""),
        },
        assertions=["classification != unexpected", "semantic_oracle != empty", "source_facets != empty", "source_semantic_oracle_before_promotion"],
    )


def rust_fixture_from_record(item):
    scenario = item["scenario"]
    total, count, invalid, oop = expected_values(scenario)
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
        "production_replay_target": scenario.get("production_replay_target", ""),
        "promotion_gate": scenario.get("promotion_gate", ""),
        "trace_digest_fields": scenario.get("trace_digest_fields", []),
        "source_file": scenario.get("source_file", ""),
        "source_line": int(scenario.get("source_line", 0)),
        "source_symbol": scenario.get("source_symbol", ""),
        "cost_surface": scenario.get("cost_surface", ""),
        "source_risk": scenario.get("source_risk", ""),
        "reachable_from_user_deploy": bool(scenario.get("reachable_from_user_deploy", False)),
        "source_surface_status": scenario.get("source_surface_status", ""),
        "oracle_surface": scenario.get("oracle_surface", ""),
        "mutation_axis": scenario.get("mutation_axis", ""),
        "expected_disposition": scenario.get("expected_disposition", ""),
        "source_facets": scenario.get("source_facets", []),
        "source_anchor_digest": scenario.get("source_anchor_digest", ""),
        "cross_surface_role": scenario.get("cross_surface_role", ""),
        "semantic_oracle": scenario.get("semantic_oracle", ""),
    }


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", choices=["quick", "corpus", "deep"], default="quick")
    parser.add_argument("--search-mode", choices=["frontier", "all"], default="frontier")
    parser.add_argument("--objectives", default="all")
    parser.add_argument("--source-surface-json", required=True)
    parser.add_argument("--json-out")
    parser.add_argument("--fixture-out")
    parser.add_argument("--coverage-out")
    parser.add_argument("--rust-fixtures-out")
    args = parser.parse_args(argv)

    records = frontier_records(args)
    if "all" in selected_objectives(args.objectives) or "security" in selected_objectives(args.objectives) or "source_semantic_oracle" in selected_objectives(args.objectives):
        assert_adequacy(records)
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
