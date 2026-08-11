import argparse
import hashlib
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(sys.argv[0]))), "scenario_schema.sage"))


OBJECTIVES = [
    "all",
    "security",
    "runtime_budget",
    "metering",
    "parallel_eval",
    "casper_replay",
    "settlement",
    "slashing",
    "legacy_quarantine",
    "source_anchored",
    "coverage_adequacy",
]


def objective_enabled(selected, objective):
    selected_items = [item.strip() for item in str(selected).split(",") if item.strip()]
    return (
        "all" in selected_items
        or "source_anchored" in selected_items
        or objective in selected_items
        or ("security" in selected_items and objective != "coverage_adequacy")
    )


def adequacy_required(selected):
    selected_items = [item.strip() for item in str(selected).split(",") if item.strip()]
    if not selected_items:
        return True
    return bool(set(["all", "security", "source_anchored", "coverage_adequacy"]).intersection(selected_items))


def digest_value(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, default=schema_json_default).encode("utf-8")).hexdigest()[:16]


def command_for_fixture(test_name):
    return "COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang {}".format(test_name)


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
            return (consumed, count, True, False)
        if consumed + weight > budget:
            return (budget, count + 1, False, True)
        consumed += weight
        count += 1
    return (consumed, count, False, False)


def load_source_surface(path):
    if not path:
        raise SystemExit("--source-surface-json is required for v11 source-anchored search")
    with open(path, "r") as handle:
        manifest = json.load(handle)
    surfaces = manifest.get("surfaces", [])
    if not surfaces:
        raise SystemExit("v11 source-anchored search requires at least one source surface")
    return manifest, surfaces


def classification_for(surface):
    expected_state = str(surface.get("expected_state", "present"))
    found = bool(surface.get("found", False))
    if expected_state == "absent":
        return "confirmed_current_bug" if found else "confirmed_safe"
    return "confirmed_safe" if found else "needs_source_audit"


def promotion_target_for(classification):
    if classification == "confirmed_current_bug":
        return "source_fix:cost_accounting"
    if classification == "needs_source_audit":
        return "audit:source_surface"
    return "rust:test"


def rust_test_for(cost_surface):
    if cost_surface in ["runtime_budget", "metering", "parallel_eval"]:
        return "generated_frontier_v11_runtime_budget_source_risks_hold"
    if cost_surface in ["casper_replay", "settlement", "slashing", "legacy_quarantine"]:
        return "generated_frontier_v11_casper_settlement_slashing_source_risks_hold"
    return "generated_frontier_v11_source_anchored_fixtures_hold"


def threat_family_for(cost_surface):
    return "source_anchored_{}".format(cost_surface)


def event_for_surface(surface, index):
    risk = str(surface.get("source_risk", "source_surface"))
    descriptor = "v11/{}/{}".format(surface.get("cost_surface", "cost"), risk)
    if risk == "invalid_admission_before_mutation":
        return canonical_event("primitive", 0, descriptor=descriptor, primitive_descriptor=descriptor, deploy=index % 4, path=[index])
    if risk == "oop_boundary_singleton":
        return canonical_event("source", 5, descriptor=descriptor, deploy=index % 4, path=[index])
    if str(surface.get("cost_surface", "")) == "metering":
        return canonical_event("substitution", 1 + (index % 2), descriptor=descriptor, deploy=index % 4, path=[index])
    if str(surface.get("cost_surface", "")) == "parallel_eval":
        return canonical_event("source", 1 + (index % 3), descriptor=descriptor, deploy=index % 4, path=[index, index % 2])
    return canonical_event("source", 1 + (index % 2), descriptor=descriptor, deploy=index % 4, path=[index])


def initial_budget_for(surface, event):
    if str(surface.get("source_risk", "")) == "oop_boundary_singleton":
        return 3
    if int(event.get("weight", 0)) <= 0:
        return 4
    return int(event.get("weight", 0)) + 4


def settlement_for(surface):
    cost_surface = str(surface.get("cost_surface", ""))
    if cost_surface == "settlement":
        return {"authority": "casper", "escrow": 15, "token_cost": 5, "refund": 10}
    if cost_surface == "slashing":
        return {
            "kind": "slash_after_evaluation",
            "authority": "casper",
            "escrow": 12,
            "token_cost": 4,
            "refund": 8,
            "slashing_scope": "post_eval",
        }
    return {}


def replay_mutations_for(surface):
    cost_surface = str(surface.get("cost_surface", ""))
    if cost_surface == "casper_replay":
        return ["cost_trace_digest", "cost_trace_event_count", "cost_trace_present", "block_hash", "signature"]
    if cost_surface == "settlement":
        return ["cost", "cost_trace_digest", "cost_trace_event_count", "refund"]
    if cost_surface == "slashing":
        return ["slash_fields", "cost_trace_digest", "cost_trace_event_count", "genesis", "block_hash", "signature"]
    if cost_surface == "legacy_quarantine":
        return ["cost_trace_present", "cost_trace_digest", "cost_trace_event_count", "block_hash"]
    return []


def v11_record(surface, index):
    cost_surface = str(surface.get("cost_surface", "source_anchored"))
    source_risk = str(surface.get("source_risk", "source_surface"))
    classification = classification_for(surface)
    rust_test = rust_test_for(cost_surface)
    event = event_for_surface(surface, index)
    source_file = str(surface.get("source_file", ""))
    source_symbol = str(surface.get("source_symbol", ""))
    source_line = int(surface.get("source_line", 0) or 0)
    source_status = str(surface.get("source_surface_status", "absent"))
    promotion_target = promotion_target_for(classification)
    replay_mutations = replay_mutations_for(surface)
    negative_mutations = replay_mutations if cost_surface in ["casper_replay", "settlement", "slashing", "legacy_quarantine"] else []
    scenario = canonical_scenario(
        surface.get("id", "v11_source_surface_{}".format(index)),
        events=[event],
        initial_budget=initial_budget_for(surface, event),
        settlement=settlement_for(surface),
        replay_fields={"fields": ["cost", "cost_trace_digest", "cost_trace_event_count"]} if replay_mutations else {},
        replay_mutations=replay_mutations,
        negative_mutations=negative_mutations,
        source_seed={"source_surface": surface},
        rho_source='@0!("v11-source-anchored")',
        production_oracle="source_surface_replay",
        expected_outcome="source_surface_{}".format(source_status),
        differential_axes=["cost", "digest", "count", "source_surface", cost_surface],
        eval_phlo=100000,
        expected_error_kind="none",
        eval_result_axes=["cost", "digest", "count", "errors"],
        rho_source_digest=digest_value({"source_file": source_file, "source_symbol": source_symbol}),
        replay_mode="play_replay" if replay_mutations else "eval_only",
        term_family=source_risk,
        term_parameters={"cost_surface": cost_surface, "source_status": source_status},
        adequacy_requirements=["source_surface", "source_file", "source_symbol", "cost_surface", "source_risk"],
        attack_campaign="v11_source_anchored_{}".format(cost_surface),
        oracle_kind="source_anchored_{}".format(cost_surface),
        production_path="{}::{}:{}".format(source_file, source_symbol, source_line),
        campaign_steps=["extract_source_surface", "project_to_fixture", "replay_on_production_gate"],
        minimized_input_digest=digest_value(surface),
        reproducer_command=command_for_fixture(rust_test),
        production_replay_target=rust_test,
        promotion_gate="source_anchor_before_promotion",
        threat_family=threat_family_for(cost_surface),
        expected_invariants=["v11_source_surface_metadata_matches_f1r3node_rust_source"],
        rust_reproducer={"test": rust_test, "source_file": source_file, "source_symbol": source_symbol},
        promotion_target=promotion_target,
        expected_classification=classification,
        source_file=source_file,
        source_line=source_line,
        source_symbol=source_symbol,
        cost_surface=cost_surface,
        source_risk=source_risk,
        reachable_from_user_deploy=bool(surface.get("reachable_from_user_deploy", False)),
        source_surface_status=source_status,
    )
    witness = {
        "source_anchored": True,
        "cost_surface": cost_surface,
        "source_risk": source_risk,
        "source_file": source_file,
        "source_symbol": source_symbol,
        "source_line": source_line,
        "source_surface_status": source_status,
        "reachable_from_user_deploy": bool(surface.get("reachable_from_user_deploy", False)),
        "expected_state": surface.get("expected_state", "present"),
        "found": bool(surface.get("found", False)),
    }
    witness[cost_surface] = True
    return record(
        "horizon_v11_source_anchored_security",
        classification,
        scenario["name"],
        "Source surface {} anchors {} risk {} to f1r3node-rust.".format(source_file, cost_surface, source_risk),
        scenario,
        witness,
        ["Rust: {}".format(rust_test), "Sage: v11 source-anchored security frontier"],
    )


def adequacy_record():
    scenario = canonical_scenario(
        "horizon_v11_source_anchored_coverage_adequacy",
        events=[],
        initial_budget=0,
        source_seed={"adequacy": "v11_source_anchored"},
        rho_source="Nil",
        production_oracle="source_surface_adequacy",
        expected_outcome="coverage_adequacy",
        term_family="v11_source_anchored_coverage_adequacy",
        adequacy_requirements=[
            "runtime_budget",
            "metering",
            "parallel_eval",
            "casper_replay",
            "settlement",
            "slashing",
            "legacy_quarantine",
        ],
        attack_campaign="v11_source_anchored_coverage_adequacy",
        oracle_kind="source_anchored_adequacy",
        production_path="scripts/cost-accounting-source-surface.sh::surface_json:1",
        campaign_steps=["extract_source_surface", "assert_cost_surface_coverage"],
        minimized_input_digest="v11-source-anchored-adequacy",
        reproducer_command=command_for_fixture("generated_frontier_v11_coverage_adequacy_holds"),
        production_replay_target="generated_frontier_v11_coverage_adequacy_holds",
        promotion_gate="v11_source_adequacy_gate",
        threat_family="coverage_adequacy",
        expected_invariants=["v11_source_anchored_frontier_covers_all_cost_surfaces"],
        rust_reproducer={"test": "generated_frontier_v11_coverage_adequacy_holds"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
        cost_surface="coverage_adequacy",
        source_risk="source_surface_coverage",
        source_file="scripts/cost-accounting-source-surface.sh",
        source_line=1,
        source_symbol="surface_json",
        source_surface_status="present",
    )
    return record(
        "horizon_v11_source_anchored_security",
        "confirmed_safe",
        scenario["name"],
        "The V11 source-anchored frontier fails closed if required cost surfaces are not represented.",
        scenario,
        {"source_anchored": True, "coverage_adequacy": True, "cost_surface": "coverage_adequacy"},
        ["Rust: generated_frontier_v11_coverage_adequacy_holds"],
    )


def frontier_records(args):
    manifest, surfaces = load_source_surface(args.source_surface_json)
    records = []
    for index, surface in enumerate(surfaces):
        if objective_enabled(args.objectives, str(surface.get("cost_surface", ""))):
            records.append(v11_record(surface, index))
    if objective_enabled(args.objectives, "coverage_adequacy"):
        records.append(adequacy_record())
    if not records:
        raise SystemExit("v11 source-anchored search selected no records")
    return records


def assert_adequacy(records):
    features = set()
    surfaces = set()
    classes = set()
    statuses = set()
    for item in records:
        classes.add(item["classification"])
        scenario = item["scenario"]
        cost_surface = scenario.get("cost_surface", "")
        if cost_surface and cost_surface != "coverage_adequacy":
            surfaces.add(cost_surface)
        status = scenario.get("source_surface_status", "")
        if status:
            statuses.add(status)
        for feature in item.get("coverage_features", []):
            features.add(feature)
    required_surfaces = set([
        "runtime_budget",
        "metering",
        "parallel_eval",
        "casper_replay",
        "settlement",
        "slashing",
        "legacy_quarantine",
    ])
    required_features = set([
        "source_anchored",
        "source_file",
        "source_symbol",
        "cost_surface",
        "source_risk",
        "production_replay_target",
        "promotion_gate",
        "coverage_adequacy",
    ])
    missing_surfaces = sorted(required_surfaces - surfaces)
    missing_features = sorted(required_features - features)
    if missing_surfaces or missing_features or "confirmed_safe" not in classes or "present" not in statuses:
        raise SystemExit(
            "v11 source adequacy failure: missing_surfaces={} missing_features={} classes={} statuses={}".format(
                missing_surfaces, missing_features, sorted(classes), sorted(statuses)
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
            "production_replay_target": scenario.get("production_replay_target", ""),
            "promotion_gate": scenario.get("promotion_gate", ""),
            "source_file": scenario.get("source_file", ""),
            "source_symbol": scenario.get("source_symbol", ""),
            "cost_surface": scenario.get("cost_surface", ""),
            "source_risk": scenario.get("source_risk", ""),
        },
        assertions=[
            "classification != unexpected",
            "source_file != empty",
            "source_symbol != empty",
            "cost_surface != empty",
            "promotion_gate != empty",
            "rust_replay_before_source_action",
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
        "rho_source": scenario.get("rho_source", ""),
        "production_oracle": scenario.get("production_oracle", ""),
        "expected_outcome": scenario.get("expected_outcome", ""),
        "differential_axes": scenario.get("differential_axes", []),
        "eval_phlo": int(scenario.get("eval_phlo", 0)),
        "expected_error_kind": scenario.get("expected_error_kind", ""),
        "eval_result_axes": scenario.get("eval_result_axes", []),
        "rho_source_digest": scenario.get("rho_source_digest", ""),
        "replay_mode": scenario.get("replay_mode", ""),
        "term_family": scenario.get("term_family", ""),
        "term_parameters": scenario.get("term_parameters", {}),
        "adequacy_budget": scenario.get("adequacy_budget", {}),
        "production_replay_target": scenario.get("production_replay_target", ""),
        "promotion_gate": scenario.get("promotion_gate", ""),
        "production_disposition": production_disposition(item["classification"]),
        "production_guards": production_guards(item["name"], item["classification"], scenario),
        "trace_digest_fields": scenario.get("trace_digest_fields", []),
        "source_file": scenario.get("source_file", ""),
        "source_line": int(scenario.get("source_line", 0)),
        "source_symbol": scenario.get("source_symbol", ""),
        "cost_surface": scenario.get("cost_surface", ""),
        "source_risk": scenario.get("source_risk", ""),
        "reachable_from_user_deploy": bool(scenario.get("reachable_from_user_deploy", False)),
        "source_surface_status": scenario.get("source_surface_status", ""),
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
    if adequacy_required(args.objectives):
        assert_adequacy(records)
    fixtures = [fixture_from_record(item) for item in records]
    rust_fixtures = [rust_fixture_from_record(item) for item in records]
    output = {
        "profile": args.profile,
        "search_mode": args.search_mode,
        "objectives": args.objectives,
        "source_surface_json": args.source_surface_json,
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
