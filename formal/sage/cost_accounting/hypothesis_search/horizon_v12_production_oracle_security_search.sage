import argparse
import hashlib
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(sys.argv[0]))), "scenario_schema.sage"))


ORACLES = [
    {
        "id": "v12_runtime_budget_reserve_accept",
        "oracle_surface": "runtime_budget",
        "oracle_kind": "runtime_budget_reserve",
        "source_risk": "trace_slot_capacity",
        "mutation_axis": "accepted_reservation",
        "expected_disposition": "accepted",
        "event": canonical_event("source", 2, descriptor="v12/runtime/accepted", deploy=0, path=[0]),
        "initial_budget": 8,
    },
    {
        "id": "v12_runtime_budget_invalid_rejects_before_mutation",
        "oracle_surface": "runtime_budget",
        "oracle_kind": "runtime_budget_invalid_admission",
        "source_risk": "invalid_admission_before_mutation",
        "mutation_axis": "zero_weight",
        "expected_disposition": "rejected_before_mutation",
        "event": canonical_event("primitive", 0, descriptor="v12/runtime/zero", primitive_descriptor="v12/runtime/zero", deploy=0, path=[1]),
        "initial_budget": 8,
    },
    {
        "id": "v12_runtime_budget_oop_single_boundary",
        "oracle_surface": "runtime_budget",
        "oracle_kind": "runtime_budget_oop_boundary",
        "source_risk": "oop_boundary_singleton",
        "mutation_axis": "oop_boundary",
        "expected_disposition": "oop_boundary",
        "event": canonical_event("source", 9, descriptor="v12/runtime/oop", deploy=0, path=[2]),
        "initial_budget": 4,
    },
    {
        "id": "v12_metering_canonical_drain_order",
        "oracle_surface": "metering",
        "oracle_kind": "metering_canonical_drain",
        "source_risk": "pending_queue_ordering",
        "mutation_axis": "frame_order",
        "expected_disposition": "accepted",
        "event": canonical_event("substitution", 1, descriptor="v12/metering/drain", deploy=0, path=[3]),
        "initial_budget": 10,
    },
    {
        "id": "v12_metering_nonbillable_trace_exclusion",
        "oracle_surface": "metering",
        "oracle_kind": "metering_nonbillable_trace_exclusion",
        "source_risk": "local_index_determinism",
        "mutation_axis": "nonbillable_frames",
        "expected_disposition": "accepted",
        "event": canonical_event("source", 1, descriptor="v12/metering/nonbillable", deploy=0, path=[4]),
        "initial_budget": 10,
    },
    {
        "id": "v12_parallel_completion_order_digest_stable",
        "oracle_surface": "parallel_eval",
        "oracle_kind": "parallel_digest_completion_order",
        "source_risk": "completion_order_parallelism",
        "mutation_axis": "completion_order",
        "expected_disposition": "accepted",
        "event": canonical_event("source", 2, descriptor="v12/parallel/a", deploy=0, path=[5, 0]),
        "extra_event": canonical_event("source", 1, descriptor="v12/parallel/b", deploy=1, path=[5, 1]),
        "initial_budget": 10,
    },
    {
        "id": "v12_casper_replay_cost_trace_digest_mutation",
        "oracle_surface": "casper_replay",
        "oracle_kind": "casper_replay_payload_hash",
        "source_risk": "replay_auth_digest_count",
        "mutation_axis": "cost_trace_digest",
        "expected_disposition": "replay_invalid",
        "event": canonical_event("source", 1, descriptor="v12/casper/digest", deploy=0, path=[6]),
        "initial_budget": 8,
    },
    {
        "id": "v12_casper_replay_signature_mutation",
        "oracle_surface": "casper_replay",
        "oracle_kind": "casper_replay_payload_hash",
        "source_risk": "replay_payload_cache_key",
        "mutation_axis": "signature",
        "expected_disposition": "replay_invalid",
        "event": canonical_event("source", 1, descriptor="v12/casper/signature", deploy=0, path=[7]),
        "initial_budget": 8,
    },
    {
        "id": "v12_settlement_refund_bounded",
        "oracle_surface": "settlement",
        "oracle_kind": "settlement_refund_projection",
        "source_risk": "refund_as_fuel",
        "mutation_axis": "refund",
        "expected_disposition": "settlement_bounded",
        "event": canonical_event("source", 2, descriptor="v12/settlement/refund", deploy=0, path=[8]),
        "initial_budget": 8,
        "settlement": {"escrow": 15, "token_cost": 5, "refund": 10, "authority": "casper", "phlo_limit": 15, "phlo_price": 1},
    },
    {
        "id": "v12_settlement_overflow_rejected",
        "oracle_surface": "settlement",
        "oracle_kind": "settlement_overflow_rejected",
        "source_risk": "refund_overflow",
        "mutation_axis": "phlo_charge_overflow",
        "expected_disposition": "rejected_before_mutation",
        "event": canonical_event("source", 1, descriptor="v12/settlement/overflow", deploy=0, path=[9]),
        "initial_budget": 8,
        "settlement": {"escrow": 0, "token_cost": 0, "refund": 0, "authority": "casper", "phlo_limit": 9223372036854775807, "phlo_price": 2},
    },
    {
        "id": "v12_slashing_fields_replay_authenticated",
        "oracle_surface": "slashing",
        "oracle_kind": "slashing_replay_payload_hash",
        "source_risk": "slash_field_authentication",
        "mutation_axis": "slash_fields",
        "expected_disposition": "replay_invalid",
        "event": canonical_event("source", 1, descriptor="v12/slashing/fields", deploy=0, path=[10]),
        "initial_budget": 8,
        "settlement": {"kind": "slash_after_evaluation", "authority": "casper", "escrow": 12, "token_cost": 4, "refund": 8, "slashing_scope": "post_eval"},
    },
    {
        "id": "v12_slashing_post_eval_no_user_cost_mutation",
        "oracle_surface": "slashing",
        "oracle_kind": "slashing_post_eval_isolation",
        "source_risk": "slashing_evidence_gap",
        "mutation_axis": "post_eval_user_cost",
        "expected_disposition": "accepted",
        "event": canonical_event("source", 2, descriptor="v12/slashing/isolation", deploy=0, path=[11]),
        "initial_budget": 8,
        "settlement": {"kind": "slash_after_evaluation", "authority": "casper", "escrow": 12, "token_cost": 4, "refund": 8, "slashing_scope": "post_eval"},
    },
    {
        "id": "v12_legacy_absent_trace_quarantined",
        "oracle_surface": "legacy_quarantine",
        "oracle_kind": "legacy_absent_trace_quarantine",
        "source_risk": "legacy_runtime_metering_downgrade",
        "mutation_axis": "cost_trace_present",
        "expected_disposition": "source_absent",
        "event": canonical_event("source", 1, descriptor="v12/legacy/absent", deploy=0, path=[12]),
        "initial_budget": 8,
    },
]


def selected_objectives(value):
    return [item.strip() for item in str(value).split(",") if item.strip()]


def objective_enabled(selected, surface):
    items = selected_objectives(selected)
    return (
        not items
        or "all" in items
        or "security" in items
        or "production_oracle" in items
        or str(surface) in items
    )


def digest_value(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, default=schema_json_default).encode("utf-8")).hexdigest()[:16]


def load_source_surface(path):
    if not path:
        raise SystemExit("--source-surface-json is required for v12 production-oracle search")
    with open(path, "r") as handle:
        manifest = json.load(handle)
    surfaces = manifest.get("surfaces", [])
    if not surfaces:
        raise SystemExit("v12 production-oracle search requires at least one source surface")
    return surfaces


def surface_for(surfaces, oracle):
    target_surface = oracle["oracle_surface"]
    target_risk = oracle["source_risk"]
    for surface in surfaces:
        if surface.get("cost_surface") == target_surface and surface.get("source_risk") == target_risk:
            return surface
    for surface in surfaces:
        if surface.get("cost_surface") == target_surface:
            return surface
    raise SystemExit("missing source surface for oracle {} ({})".format(oracle["id"], target_surface))


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


def rust_test_for(surface):
    if surface in ["runtime_budget", "metering", "parallel_eval"]:
        return "generated_frontier_v12_runtime_metering_parallel_oracles_hold"
    if surface in ["casper_replay", "settlement", "slashing", "legacy_quarantine"]:
        return "generated_frontier_v12_casper_settlement_slashing_oracles_hold"
    return "generated_frontier_v12_production_oracle_fixtures_hold"


def replay_mutations_for(oracle):
    surface = oracle["oracle_surface"]
    axis = oracle["mutation_axis"]
    if surface in ["casper_replay", "slashing", "legacy_quarantine"]:
        return [axis, "cost_trace_digest", "cost_trace_event_count", "block_hash", "signature"]
    if surface == "settlement":
        return [axis, "cost", "refund"]
    return []


def record_for_oracle(oracle, surface):
    rust_test = rust_test_for(oracle["oracle_surface"])
    events = [oracle["event"]]
    if "extra_event" in oracle:
        events.append(oracle["extra_event"])
    classification = "confirmed_safe" if surface.get("found", False) or surface.get("expected_state") == "absent" else "needs_source_audit"
    scenario = canonical_scenario(
        oracle["id"],
        events=events,
        initial_budget=oracle["initial_budget"],
        settlement=oracle.get("settlement", {}),
        replay_fields={"fields": ["cost", "cost_trace_digest", "cost_trace_event_count"]},
        replay_mutations=replay_mutations_for(oracle),
        negative_mutations=replay_mutations_for(oracle),
        source_seed={"source_surface": surface, "production_oracle": oracle},
        rho_source='@0!("v12-production-oracle")',
        production_oracle=oracle["oracle_kind"],
        expected_outcome=oracle["expected_disposition"],
        differential_axes=["cost", "digest", "count", oracle["mutation_axis"], oracle["oracle_surface"]],
        eval_phlo=100000,
        expected_error_kind="none" if oracle["expected_disposition"] == "accepted" else oracle["expected_disposition"],
        eval_result_axes=["cost", "digest", "count", "errors"],
        rho_source_digest=digest_value({"oracle": oracle["id"], "surface": surface}),
        replay_mode="play_replay",
        term_family=oracle["oracle_kind"],
        term_parameters={"oracle_surface": oracle["oracle_surface"], "mutation_axis": oracle["mutation_axis"]},
        adequacy_requirements=["production_oracle_surface", "mutation_axis", "expected_disposition"],
        attack_campaign="v12_production_oracle_{}".format(oracle["oracle_surface"]),
        oracle_kind=oracle["oracle_kind"],
        production_path="{}::{}:{}".format(surface.get("source_file", ""), surface.get("source_symbol", ""), int(surface.get("source_line", 0) or 0)),
        campaign_steps=["extract_source_surface", "generate_production_oracle", "replay_native_api"],
        minimized_input_digest=digest_value({"oracle": oracle, "surface": surface}),
        reproducer_command=command_for_fixture(rust_test),
        production_replay_target=rust_test,
        promotion_gate="native_production_oracle_before_promotion",
        threat_family="production_oracle_{}".format(oracle["oracle_surface"]),
        expected_invariants=["v12_production_oracle_replays_native_api_before_promotion"],
        rust_reproducer={"test": rust_test, "oracle_kind": oracle["oracle_kind"]},
        promotion_target="rust:test",
        expected_classification=classification,
        source_file=surface.get("source_file", ""),
        source_line=int(surface.get("source_line", 0) or 0),
        source_symbol=surface.get("source_symbol", ""),
        cost_surface=surface.get("cost_surface", ""),
        source_risk=surface.get("source_risk", ""),
        reachable_from_user_deploy=bool(surface.get("reachable_from_user_deploy", False)),
        source_surface_status=surface.get("source_surface_status", ""),
        oracle_surface=oracle["oracle_surface"],
        mutation_axis=oracle["mutation_axis"],
        expected_disposition=oracle["expected_disposition"],
    )
    witness = {
        "production_oracle_surface": oracle["oracle_surface"],
        "oracle_kind": oracle["oracle_kind"],
        "mutation_axis": oracle["mutation_axis"],
        "expected_disposition": oracle["expected_disposition"],
        "source_file": surface.get("source_file", ""),
        "source_symbol": surface.get("source_symbol", ""),
        "runtime_budget": oracle["oracle_surface"] == "runtime_budget",
        "metering": oracle["oracle_surface"] == "metering",
        "parallel_eval": oracle["oracle_surface"] == "parallel_eval",
        "casper_replay": oracle["oracle_surface"] == "casper_replay",
        "settlement": oracle["oracle_surface"] == "settlement",
        "slashing": oracle["oracle_surface"] == "slashing",
        "legacy_quarantine": oracle["oracle_surface"] == "legacy_quarantine",
    }
    return record(
        "horizon_v12_production_oracle_security",
        classification,
        oracle["id"],
        "Production oracle {} replays source-anchored surface {} through native Rust behavior.".format(oracle["oracle_kind"], oracle["oracle_surface"]),
        scenario,
        witness,
        ["Rust: {}".format(rust_test), "Sage: v12 production-oracle frontier"],
    )


def adequacy_record():
    scenario = canonical_scenario(
        "horizon_v12_production_oracle_coverage_adequacy",
        events=[],
        initial_budget=0,
        source_seed={"adequacy": "v12_production_oracle"},
        rho_source="Nil",
        production_oracle="v12_production_oracle_adequacy",
        expected_outcome="coverage_adequacy",
        term_family="v12_production_oracle_coverage_adequacy",
        adequacy_requirements=["runtime_budget", "metering", "parallel_eval", "casper_replay", "settlement", "slashing", "legacy_quarantine"],
        attack_campaign="v12_production_oracle_coverage_adequacy",
        oracle_kind="v12_production_oracle_adequacy",
        production_path="formal/sage/cost_accounting/hypothesis_search/horizon_v12_production_oracle_security_search.sage::ORACLES:1",
        campaign_steps=["generate_production_oracles", "assert_oracle_surface_coverage"],
        minimized_input_digest="v12-production-oracle-adequacy",
        reproducer_command=command_for_fixture("generated_frontier_v12_coverage_adequacy_holds"),
        production_replay_target="generated_frontier_v12_coverage_adequacy_holds",
        promotion_gate="v12_production_oracle_adequacy_gate",
        threat_family="coverage_adequacy",
        expected_invariants=["v12_production_oracle_frontier_covers_all_native_surfaces"],
        rust_reproducer={"test": "generated_frontier_v12_coverage_adequacy_holds"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
        source_file="formal/sage/cost_accounting/hypothesis_search/horizon_v12_production_oracle_security_search.sage",
        source_line=1,
        source_symbol="ORACLES",
        cost_surface="coverage_adequacy",
        source_risk="production_oracle_coverage",
        source_surface_status="present",
        oracle_surface="coverage_adequacy",
        mutation_axis="coverage",
        expected_disposition="coverage_adequacy",
    )
    return record(
        "horizon_v12_production_oracle_security",
        "confirmed_safe",
        scenario["name"],
        "The V12 frontier fails closed if required native production-oracle surfaces disappear.",
        scenario,
        {"production_oracle_surface": "coverage_adequacy", "coverage_adequacy": True},
        ["Rust: generated_frontier_v12_coverage_adequacy_holds"],
    )


def frontier_records(args):
    surfaces = load_source_surface(args.source_surface_json)
    records = []
    for oracle in ORACLES:
        if objective_enabled(args.objectives, oracle["oracle_surface"]):
            records.append(record_for_oracle(oracle, surface_for(surfaces, oracle)))
    if objective_enabled(args.objectives, "coverage_adequacy"):
        records.append(adequacy_record())
    return records


def assert_adequacy(records):
    surfaces = set()
    dispositions = set()
    features = set()
    for item in records:
        scenario = item["scenario"]
        surface = scenario.get("oracle_surface", "")
        if surface and surface != "coverage_adequacy":
            surfaces.add(surface)
        disposition = scenario.get("expected_disposition", "")
        if disposition:
            dispositions.add(disposition)
        for feature in item.get("coverage_features", []):
            features.add(feature)
    required_surfaces = set(["runtime_budget", "metering", "parallel_eval", "casper_replay", "settlement", "slashing", "legacy_quarantine"])
    required_dispositions = set(["accepted", "rejected_before_mutation", "oop_boundary", "replay_invalid", "settlement_bounded", "source_absent"])
    required_features = set(["production_oracle_surface", "mutation_axis", "expected_disposition", "production_replay_target", "promotion_gate", "coverage_adequacy"])
    missing_surfaces = sorted(required_surfaces - surfaces)
    missing_dispositions = sorted(required_dispositions - dispositions)
    missing_features = sorted(required_features - features)
    if missing_surfaces or missing_dispositions or missing_features:
        raise SystemExit(
            "v12 production oracle adequacy failure: missing_surfaces={} missing_dispositions={} missing_features={}".format(
                missing_surfaces, missing_dispositions, missing_features
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
            "oracle_surface": scenario.get("oracle_surface", ""),
            "oracle_kind": scenario.get("oracle_kind", ""),
            "mutation_axis": scenario.get("mutation_axis", ""),
            "expected_disposition": scenario.get("expected_disposition", ""),
        },
        assertions=["classification != unexpected", "oracle_surface != empty", "mutation_axis != empty", "native_production_oracle_before_promotion"],
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
    if "all" in selected_objectives(args.objectives) or "security" in selected_objectives(args.objectives) or "production_oracle" in selected_objectives(args.objectives):
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
