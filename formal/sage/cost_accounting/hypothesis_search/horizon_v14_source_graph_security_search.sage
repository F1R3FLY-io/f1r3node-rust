import argparse
import hashlib
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(sys.argv[0]))), "scenario_schema.sage"))


CASES = [
    {
        "id": "v14_runtime_replay_api_commitment",
        "primary_surface": "runtime_budget",
        "primary_risk": "invalid_admission_before_mutation",
        "secondary_surface": "api_ingress",
        "secondary_risk": "private_name_preview_input",
        "security_surface": "api_to_runtime_replay",
        "external_input_kind": "deploy_api_request",
        "auth_boundary": "cost_trace_digest",
        "replay_boundary": "replay_payload_hash",
        "expected_disposition": "accepted",
        "events": [
            canonical_event("source", 2, descriptor="v14/api/runtime/a", deploy=0, path=[14, 0]),
            canonical_event("primitive", 1, descriptor="v14/api/runtime/b", primitive_descriptor="v14/api/runtime/b", deploy=0, path=[14, 1]),
        ],
        "initial_budget": 8,
        "replay_mutations": ["api_ingress", "cost_trace_digest", "cost_trace_event_count", "replay_payload_hash"],
        "source_facets": ["runtime_budget", "admission", "api_ingress", "auth_boundary"],
    },
    {
        "id": "v14_replay_cache_payload_binding",
        "primary_surface": "replay_cache",
        "primary_risk": "cache_event_log_bound",
        "secondary_surface": "casper_replay",
        "secondary_risk": "replay_payload_cache_key",
        "security_surface": "replay_cache_payload_binding",
        "external_input_kind": "block_replay_payload",
        "auth_boundary": "replay_payload_hash",
        "replay_boundary": "replay_cache_key",
        "expected_disposition": "accepted",
        "events": [
            canonical_event("source", 1, descriptor="v14/cache/replay", deploy=0, path=[14, 2]),
        ],
        "initial_budget": 6,
        "replay_mutations": ["replay_cache", "payload_hash", "cost_trace_digest", "cost_trace_event_count"],
        "source_facets": ["replay_cache", "event_log_bound", "casper_replay", "payload_hash"],
    },
    {
        "id": "v14_slashing_authorization_epoch_binding",
        "primary_surface": "slashing",
        "primary_risk": "slash_epoch_authorization",
        "secondary_surface": "slashing",
        "secondary_risk": "slash_field_authentication",
        "security_surface": "slashing_authorization",
        "external_input_kind": "slash_system_deploy",
        "auth_boundary": "slash_epoch_and_payload",
        "replay_boundary": "system_deploy_payload_hash",
        "expected_disposition": "replay_invalid",
        "events": [
            canonical_event("source", 1, descriptor="v14/slashing/auth", deploy=0, path=[14, 3]),
        ],
        "initial_budget": 6,
        "settlement": {"kind": "slash_after_evaluation", "authority": "casper", "escrow": 10, "token_cost": 3, "refund": 7, "phlo_limit": 10, "phlo_price": 1},
        "slashing_authorization": {
            "current_epoch": 2,
            "evidence_epoch": 2,
            "target_activation_epoch": 2,
            "parent_pre_state_bond": 1,
            "ambient_bond": 0,
            "execution_bond": 1,
        },
        "replay_mutations": [
            "slash_epoch",
            "slash_fields",
            "target_activation_epoch",
            "evidence_epoch",
            "parent_pre_state_bond",
            "block_hash",
            "signature",
            "cost_trace_digest",
        ],
        "source_facets": ["slashing", "authorization", "epoch_boundary", "payload_hash", "parent_pre_state", "current_evidence"],
    },
    {
        "id": "v14_slashing_recovered_rejected_current_evidence",
        "primary_surface": "slashing",
        "primary_risk": "recovered_rejected_current_evidence",
        "secondary_surface": "slashing",
        "secondary_risk": "recovered_slash_current_epoch_filter",
        "security_surface": "slashing_authorization",
        "external_input_kind": "recovered_rejected_slash",
        "auth_boundary": "current_evidence_epoch_and_parent_pre_state",
        "replay_boundary": "recovered_slash_payload_hash",
        "expected_disposition": "replay_invalid",
        "events": [
            canonical_event("source", 1, descriptor="v14/slashing/recovered-current", deploy=0, path=[14, 7]),
        ],
        "initial_budget": 6,
        "settlement": {"kind": "slash_after_evaluation", "authority": "casper", "escrow": 10, "token_cost": 3, "refund": 7, "phlo_limit": 10, "phlo_price": 1},
        "slashing_authorization": {
            "current_epoch": 2,
            "evidence_epoch": 2,
            "target_activation_epoch": 2,
            "parent_pre_state_bond": 1,
            "ambient_bond": 0,
            "execution_bond": 1,
        },
        "replay_mutations": [
            "slash_epoch",
            "slash_fields",
            "target_activation_epoch",
            "evidence_epoch",
            "parent_pre_state_bond",
            "recovered_rejected_slash",
            "block_hash",
            "signature",
            "cost_trace_digest",
        ],
        "source_facets": ["slashing", "recovered_rejected", "authorization", "epoch_boundary", "parent_pre_state", "current_evidence"],
    },
    {
        "id": "v14_mergeable_bitmask_or_roundtrip",
        "primary_surface": "mergeable_channels",
        "primary_risk": "typed_bitmask_diff_roundtrip",
        "secondary_surface": "mergeable_channels",
        "secondary_risk": "bitmask_or_combine",
        "security_surface": "typed_mergeable_channel",
        "external_input_kind": "mergeable_channel_final_value",
        "auth_boundary": "merge_type_and_channel_diff",
        "replay_boundary": "mergeable_channel_cache",
        "expected_disposition": "accepted",
        "events": [
            canonical_event("source", 1, descriptor="v14/mergeable/bitmask", deploy=0, path=[14, 4]),
        ],
        "initial_budget": 6,
        "replay_mutations": ["merge_type", "mergeable_diff", "bitmask_bits", "mergeable_channel_cache"],
        "source_facets": ["mergeable_channels", "bitmask_or", "diff_roundtrip", "type_persistence"],
    },
    {
        "id": "v14_mergeable_non_numeric_fallback",
        "primary_surface": "mergeable_channels",
        "primary_risk": "non_numeric_mergeable_fallback",
        "secondary_surface": "mergeable_channels",
        "secondary_risk": "mergeable_tag_type_propagation",
        "security_surface": "typed_mergeable_channel",
        "external_input_kind": "tagged_non_numeric_channel_value",
        "auth_boundary": "mergeable_payload_type",
        "replay_boundary": "conflict_rejection_path",
        "expected_disposition": "accepted",
        "events": [
            canonical_event("source", 1, descriptor="v14/mergeable/non-numeric", deploy=0, path=[14, 5]),
        ],
        "initial_budget": 6,
        "replay_mutations": ["merge_type", "payload_kind", "conflict_path", "cost_trace_digest"],
        "source_facets": ["mergeable_channels", "non_numeric", "fallback_conflict_path", "type_propagation"],
    },
    {
        "id": "v14_mergeable_store_type_persistence",
        "primary_surface": "mergeable_channels",
        "primary_risk": "merge_type_persistence",
        "secondary_surface": "mergeable_channels",
        "secondary_risk": "merge_type_domain",
        "security_surface": "typed_mergeable_channel",
        "external_input_kind": "mergeable_channel_cache_entry",
        "auth_boundary": "merge_type_serialization",
        "replay_boundary": "mergeable_channel_cache",
        "expected_disposition": "accepted",
        "events": [
            canonical_event("source", 1, descriptor="v14/mergeable/store", deploy=0, path=[14, 6]),
        ],
        "initial_budget": 6,
        "replay_mutations": ["merge_type", "mergeable_diff", "serialized_mergeable_entry", "post_state_hash"],
        "source_facets": ["mergeable_channels", "store_wire", "type_persistence", "type_domain"],
    },
    {
        "id": "v14_transport_tls_peer_certificate_boundary",
        "primary_surface": "transport_tls",
        "primary_risk": "peer_certificate_extraction",
        "secondary_surface": "transport_tls",
        "secondary_risk": "tls_key_material_path_config",
        "security_surface": "transport_tls",
        "external_input_kind": "p2p_peer_certificate",
        "auth_boundary": "tls_peer_certificate",
        "replay_boundary": "none",
        "expected_disposition": "accepted",
        "events": [],
        "initial_budget": 0,
        "replay_mutations": ["peer_certificate_chain", "tls_key_path"],
        "source_facets": ["transport_tls", "peer_identity", "certificate_boundary", "key_material"],
    },
    {
        "id": "v14_private_key_debug_audit",
        "primary_surface": "crypto_key_material",
        "primary_risk": "debug_secret_exposure",
        "secondary_surface": "api_ingress",
        "secondary_risk": "private_name_preview_input",
        "security_surface": "crypto_key_material",
        "external_input_kind": "operator_key_material",
        "auth_boundary": "private_key_debug",
        "replay_boundary": "none",
        "expected_disposition": "audit_required",
        "classification": "needs_source_audit",
        "promotion_target": "audit:crypto_key_material",
        "secret_material_touched": True,
        "events": [],
        "initial_budget": 0,
        "replay_mutations": ["private_key_debug", "secret_material"],
        "source_facets": ["crypto_key_material", "secret_material", "debug_boundary"],
    },
    {
        "id": "v14_dependency_rustsec_policy_audit",
        "primary_surface": "dependency_advisory",
        "primary_risk": "accepted_rustsec_exception",
        "secondary_surface": "transport_tls",
        "secondary_risk": "tls_key_material_path_config",
        "security_surface": "dependency_advisory",
        "external_input_kind": "dependency_resolution",
        "auth_boundary": "cargo_deny_policy",
        "replay_boundary": "none",
        "expected_disposition": "audit_required",
        "classification": "needs_source_audit",
        "promotion_target": "audit:dependency_advisory",
        "dependency_advisory_id": "RUSTSEC-2026-0098",
        "events": [],
        "initial_budget": 0,
        "replay_mutations": ["rustsec_advisory", "accepted_exception"],
        "source_facets": ["dependency_advisory", "rustsec", "accepted_exception"],
    },
]


def selected_objectives(value):
    return [item.strip() for item in str(value).split(",") if item.strip()]


def objective_enabled(selected, case):
    items = selected_objectives(selected)
    surfaces = [case.get("primary_surface", ""), case.get("secondary_surface", ""), case.get("security_surface", "")]
    return (
        not items
        or "all" in items
        or "security" in items
        or "source_graph_security" in items
        or any(surface in items for surface in surfaces)
    )


def digest_value(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, default=schema_json_default).encode("utf-8")).hexdigest()[:16]


def load_source_surface(path):
    if not path:
        raise SystemExit("--source-surface-json is required for v14 source-graph search")
    with open(path, "r") as handle:
        manifest = json.load(handle)
    surfaces = manifest.get("surfaces", [])
    if not surfaces:
        raise SystemExit("v14 source-graph search requires at least one source surface")
    return surfaces


def surface_for(surfaces, cost_surface, source_risk):
    for surface in surfaces:
        if surface.get("cost_surface") == cost_surface and surface.get("source_risk") == source_risk:
            return surface
    raise SystemExit("missing source surface for {} ({})".format(source_risk, cost_surface))


def source_facets_for(primary, secondary, case):
    facets = []
    for item in primary.get("source_facets", []) + secondary.get("source_facets", []) + case.get("source_facets", []):
        if item not in facets:
            facets.append(item)
    return facets


def source_anchor_digest_for(primary, secondary, case):
    anchors = [primary.get("source_anchor_digest", ""), secondary.get("source_anchor_digest", ""), case.get("id", ""), case.get("security_surface", "")]
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


def rust_test_for(case):
    surface = case.get("security_surface", "")
    if surface in ["api_to_runtime_replay", "replay_cache_payload_binding"]:
        return "generated_frontier_v14_source_graph_oracles_hold"
    if surface == "slashing_authorization":
        return "generated_frontier_v14_slashing_security_oracles_hold"
    if surface == "typed_mergeable_channel":
        return "generated_frontier_v14_mergeable_channel_oracles_hold"
    if surface in ["transport_tls", "crypto_key_material", "dependency_advisory"]:
        return "generated_frontier_v14_node_security_oracles_hold"
    return "generated_frontier_v14_source_graph_oracles_hold"


def command_for_fixture(test_name):
    return "COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang {}".format(test_name)


def record_for_case(case, primary, secondary):
    rust_test = rust_test_for(case)
    classification = case.get("classification", "confirmed_safe")
    promotion_target = case.get("promotion_target", "rust:test")
    scenario = canonical_scenario(
        case["id"],
        events=case.get("events", []),
        initial_budget=case.get("initial_budget", 0),
        settlement=case.get("settlement", {}),
        replay_fields={"fields": ["security_surface", "auth_boundary", "replay_boundary", "source_anchor_digest"]},
        replay_mutations=case.get("replay_mutations", []),
        negative_mutations=case.get("replay_mutations", []),
        source_seed={"primary_surface": primary, "secondary_surface": secondary, "source_graph_case": case},
        rho_source='@0!("v14-source-graph-security")',
        production_oracle=case["security_surface"],
        expected_outcome=case["expected_disposition"],
        differential_axes=["security_surface", "auth_boundary", "replay_boundary", case["primary_surface"], case["secondary_surface"]],
        eval_phlo=100000 if case.get("events") else 0,
        expected_error_kind="none" if case["expected_disposition"] == "accepted" else case["expected_disposition"],
        eval_result_axes=["cost", "digest", "count", "security_surface"],
        rho_source_digest=digest_value({"case": case["id"], "primary": primary, "secondary": secondary}),
        replay_mode="source_graph_security",
        term_family=case["security_surface"],
        term_parameters={"primary_surface": case["primary_surface"], "secondary_surface": case["secondary_surface"], "security_surface": case["security_surface"]},
        adequacy_requirements=["source_graph_security", "security_surface", "auth_boundary", "replay_boundary", "source_anchor_status"],
        attack_campaign="v14_source_graph_{}".format(case["security_surface"]),
        oracle_kind="source_graph_{}".format(case["security_surface"]),
        production_path="{}::{}:{} -> {}::{}:{}".format(
            primary.get("source_file", ""),
            primary.get("source_symbol", ""),
            int(primary.get("source_line", 0) or 0),
            secondary.get("source_file", ""),
            secondary.get("source_symbol", ""),
            int(secondary.get("source_line", 0) or 0),
        ),
        campaign_steps=["extract_source_graph_surfaces", "classify_security_boundary", "replay_source_graph_oracle"],
        minimized_input_digest=digest_value({"case": case, "primary": primary, "secondary": secondary}),
        reproducer_command=command_for_fixture(rust_test),
        production_replay_target=rust_test,
        promotion_gate="source_graph_security_before_promotion",
        threat_family="source_graph_{}_security".format(case["security_surface"]),
        expected_invariants=["v14_source_graph_security_matches_f1r3node_rust_surfaces"],
        rust_reproducer={"test": rust_test, "security_surface": case["security_surface"]},
        promotion_target=promotion_target,
        expected_classification=classification,
        source_file=primary.get("source_file", ""),
        source_line=int(primary.get("source_line", 0) or 0),
        source_symbol=primary.get("source_symbol", ""),
        cost_surface=primary.get("cost_surface", ""),
        source_risk=primary.get("source_risk", ""),
        reachable_from_user_deploy=bool(primary.get("reachable_from_user_deploy", False)),
        source_surface_status=primary.get("source_surface_status", ""),
        expected_disposition=case["expected_disposition"],
        source_facets=source_facets_for(primary, secondary, case),
        source_anchor_digest=source_anchor_digest_for(primary, secondary, case),
        cross_surface_role="source_graph_{}".format(primary.get("cross_surface_role", "")),
        security_surface=case["security_surface"],
        external_input_kind=case["external_input_kind"],
        auth_boundary=case["auth_boundary"],
        replay_boundary=case["replay_boundary"],
        slashing_authorization=case.get("slashing_authorization", {}),
        secret_material_touched=bool(case.get("secret_material_touched", False)),
        source_anchor_status=primary.get("source_surface_status", ""),
        dependency_advisory_id=case.get("dependency_advisory_id", ""),
    )
    witness = {
        "source_graph_security": True,
        "security_surface": case["security_surface"],
        "external_input_kind": case["external_input_kind"],
        "auth_boundary": case["auth_boundary"],
        "replay_boundary": case["replay_boundary"],
        "source_anchor_status": scenario["source_anchor_status"],
        "secret_material_touched": scenario["secret_material_touched"],
        "dependency_advisory_id": scenario["dependency_advisory_id"],
        "slashing_authorization": scenario.get("slashing_authorization", {}),
    }
    return record(
        "horizon_v14_source_graph_security",
        classification,
        case["id"],
        "Source-graph security oracle {} binds {} to {} through current f1r3node-rust surfaces.".format(
            case["security_surface"], case["primary_surface"], case["secondary_surface"]
        ),
        scenario,
        witness,
        ["Rust: {}".format(rust_test), "Sage: v14 source-graph security frontier"],
    )


def adequacy_record():
    scenario = canonical_scenario(
        "horizon_v14_source_graph_coverage_adequacy",
        events=[],
        initial_budget=0,
        source_seed={"adequacy": "v14_source_graph_security"},
        rho_source="Nil",
        production_oracle="v14_source_graph_adequacy",
        expected_outcome="coverage_adequacy",
        expected_disposition="coverage_adequacy",
        term_family="v14_source_graph_coverage_adequacy",
        adequacy_requirements=[
            "runtime_budget",
            "casper_replay",
            "replay_cache",
            "slashing",
            "mergeable_channels",
            "transport_tls",
            "crypto_key_material",
            "api_ingress",
            "dependency_advisory",
        ],
        attack_campaign="v14_source_graph_coverage_adequacy",
        oracle_kind="v14_source_graph_adequacy",
        production_path="formal/sage/cost_accounting/hypothesis_search/horizon_v14_source_graph_security_search.sage::CASES:1",
        campaign_steps=["generate_source_graph_security_oracles", "assert_security_surface_coverage"],
        minimized_input_digest="v14-source-graph-adequacy",
        reproducer_command=command_for_fixture("generated_frontier_v14_coverage_adequacy_holds"),
        production_replay_target="generated_frontier_v14_coverage_adequacy_holds",
        promotion_gate="v14_source_graph_adequacy_gate",
        threat_family="coverage_adequacy",
        expected_invariants=["v14_source_graph_frontier_covers_required_security_surfaces"],
        rust_reproducer={"test": "generated_frontier_v14_coverage_adequacy_holds"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
        source_file="formal/sage/cost_accounting/hypothesis_search/horizon_v14_source_graph_security_search.sage",
        source_line=1,
        source_symbol="CASES",
        cost_surface="coverage_adequacy",
        source_risk="source_graph_coverage",
        source_surface_status="present",
        source_facets=["coverage_adequacy", "source_graph_security"],
        source_anchor_digest="v14-source-graph-adequacy",
        cross_surface_role="coverage",
        security_surface="coverage_adequacy",
        external_input_kind="coverage",
        auth_boundary="coverage",
        replay_boundary="coverage",
        source_anchor_status="present",
    )
    return record(
        "horizon_v14_source_graph_security",
        "confirmed_safe",
        scenario["name"],
        "The V14 frontier fails closed if required source-graph security surfaces disappear.",
        scenario,
        {"source_graph_security": True, "coverage_adequacy": True},
        ["Rust: generated_frontier_v14_coverage_adequacy_holds"],
    )


def frontier_records(args):
    surfaces = load_source_surface(args.source_surface_json)
    records = []
    for case in CASES:
        if objective_enabled(args.objectives, case):
            primary = surface_for(surfaces, case["primary_surface"], case["primary_risk"])
            secondary = surface_for(surfaces, case["secondary_surface"], case["secondary_risk"])
            records.append(record_for_case(case, primary, secondary))
    if "all" in selected_objectives(args.objectives) or "security" in selected_objectives(args.objectives) or "source_graph_security" in selected_objectives(args.objectives):
        records.append(adequacy_record())
    return records


def assert_adequacy(records):
    surfaces = set()
    features = set()
    boundaries = set()
    for item in records:
        scenario = item["scenario"]
        if scenario.get("security_surface") and scenario.get("security_surface") != "coverage_adequacy":
            surfaces.add(scenario.get("cost_surface", ""))
        for facet in scenario.get("source_facets", []):
            if facet in ["runtime_budget", "casper_replay", "replay_cache", "slashing", "mergeable_channels", "transport_tls", "crypto_key_material", "api_ingress", "dependency_advisory"]:
                surfaces.add(facet)
        if scenario.get("auth_boundary"):
            boundaries.add(scenario.get("auth_boundary"))
        for feature in item.get("coverage_features", []):
            features.add(feature)
    required_surfaces = set(["runtime_budget", "casper_replay", "replay_cache", "slashing", "mergeable_channels", "transport_tls", "crypto_key_material", "api_ingress", "dependency_advisory"])
    required_features = set(["source_graph_security", "security_surface", "external_input_kind", "auth_boundary", "replay_boundary", "source_anchor_status", "production_replay_target", "promotion_gate", "coverage_adequacy", "slashing_authorization"])
    missing_surfaces = sorted(required_surfaces - surfaces)
    missing_features = sorted(required_features - features)
    if missing_surfaces or missing_features or not boundaries:
        raise SystemExit(
            "v14 source-graph adequacy failure: missing_surfaces={} missing_features={} boundaries={}".format(
                missing_surfaces, missing_features, sorted(boundaries)
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
            "security_surface": scenario.get("security_surface", ""),
            "source_anchor_status": scenario.get("source_anchor_status", ""),
        },
        assertions=["classification != unexpected", "security_surface != empty", "source_graph_security_before_promotion"],
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
        "source_facets": scenario.get("source_facets", []),
        "source_anchor_digest": scenario.get("source_anchor_digest", ""),
        "cross_surface_role": scenario.get("cross_surface_role", ""),
        "security_surface": scenario.get("security_surface", ""),
        "external_input_kind": scenario.get("external_input_kind", ""),
        "auth_boundary": scenario.get("auth_boundary", ""),
        "replay_boundary": scenario.get("replay_boundary", ""),
        "secret_material_touched": bool(scenario.get("secret_material_touched", False)),
        "source_anchor_status": scenario.get("source_anchor_status", ""),
        "dependency_advisory_id": scenario.get("dependency_advisory_id", ""),
        "slashing_authorization": scenario.get("slashing_authorization", {}),
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
    if "all" in selected_objectives(args.objectives) or "security" in selected_objectives(args.objectives) or "source_graph_security" in selected_objectives(args.objectives):
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
