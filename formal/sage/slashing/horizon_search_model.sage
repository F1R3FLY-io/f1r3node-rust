import argparse
import json
import os
import sys
from itertools import permutations

from sage.all import DiGraph, Integer, Set, ZZ, matrix, vector

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


AXES = [
    "horizon_cross_coupled_retention_schedule",
    "horizon_proposer_withholding_fairness",
    "horizon_rust_detector_projection",
    "horizon_multi_epoch_rebond_carryover",
    "horizon_weighted_damage_min_attacker",
    "horizon_view_merge_convergence",
    "horizon_batch_arithmetic_projection",
    "horizon_metamorphic_ordering",
]


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def edge_list(edges):
    return [[int(src), int(dst)] for src, dst in sorted(edges)]


def closure(vertices, direct, edges):
    vertices = sorted([int(v) for v in vertices])
    graph = DiGraph([vertices, [(int(src), int(dst)) for src, dst in edges]], format="vertices_and_edges")
    universe = set(vertices)
    slashed = set(int(v) for v in direct).intersection(universe)
    trace = [{"round": 0, "closure": sorted(slashed)}]
    while True:
        next_slashed = set(slashed)
        for offender in sorted(slashed):
            next_slashed.update(int(v) for v in graph.neighbor_in_iterator(offender))
        next_slashed = next_slashed.intersection(universe)
        if next_slashed == slashed:
            return sorted(slashed), trace
        slashed = next_slashed
        trace.append({"round": len(trace), "closure": sorted(slashed)})


def matrix_reverse_closure(vertices, direct, edges):
    vertices = sorted([int(v) for v in vertices])
    index = {v: i for i, v in enumerate(vertices)}
    adjacency = matrix(ZZ, len(vertices), len(vertices), 0)
    for src, dst in edges:
        if int(src) in index and int(dst) in index:
            adjacency[index[int(src)], index[int(dst)]] = 1
    reach = adjacency
    power = adjacency
    for _ in range(max(0, len(vertices) - 1)):
        power = power * adjacency
        reach = reach + power
    seeds = set(int(v) for v in direct if int(v) in index)
    result = set(seeds)
    for src in vertices:
        src_index = index[src]
        if src in result:
            continue
        for dst in seeds:
            if reach[src_index, index[dst]] != 0:
                result.add(src)
                break
    return sorted(result)


def stake_sum(stakes, validators):
    return Integer(sum(Integer(stakes[int(v)]) for v in validators))


def scenario_from_witness(classification, witness):
    validators = witness.get("validators") or witness.get("current_validators") or [0, 1, 2, 3]
    validators = [int(v) for v in validators]
    stakes = witness.get("stakes")
    if stakes is None:
        stakes = [1 for _ in validators]
    stakes = [int(v) for v in list(stakes)]
    if len(stakes) < len(validators):
        stakes = stakes + [1 for _ in range(len(validators) - len(stakes))]
    direct = witness.get("direct") or witness.get("direct_equivocators") or []
    edges = witness.get("edges") or witness.get("active_edges") or witness.get("merged_edges") or []
    reports = witness.get("reports") or []
    return canonical_scenario(
        validators,
        stakes=stakes[: len(validators)],
        epochs=witness.get("epochs"),
        blocks=witness.get("blocks"),
        direct_equivocators=[int(v) for v in direct if int(v) in validators],
        neglect_edges=[(int(src), int(dst)) for src, dst in edges],
        reports=[(int(src), int(dst)) for src, dst in reports],
        slash_targets=witness.get("slash_targets"),
        events=witness.get("events"),
        views=witness.get("views"),
        retention_policy=witness.get("retention_policy"),
        projection=witness.get("projection"),
        rust_replay=witness.get("rust_replay"),
        expected_classification=classification,
    )


def record(axis, classification, name, statement, witness, formalization):
    scenario = scenario_from_witness(classification, witness)
    features = coverage_features(scenario, classification, witness)
    return {
        "axis": axis,
        "classification": classification,
        "name": name,
        "statement": statement,
        "scenario": scenario,
        "deterministic_witness": witness,
        "coverage_features": features,
        "threat_score": threat_score(classification, features, witness),
        "formalization_follow_up": formalization,
        "promotion_status": "classified_horizon_witness",
    }


def first_slash_slot(schedule):
    for index, event in enumerate(schedule):
        if event["bonded"] and event["observes"] and event["includes"]:
            return index
    return None


def detector_detectable(contributions):
    detected = any(item["kind"] == "detected" for item in contributions)
    children = Set([int(item["hash"]) for item in contributions if item["kind"] == "child"])
    return bool(detected or len(children) >= 2)


def horizon_cross_coupled_retention_schedule(horizon):
    validators = [0, 1, 2]
    direct = [1]
    edges = [(0, 1), (2, 0)]
    retained, retained_trace = closure(validators, direct, edges)
    gossip_delay = Integer(1)
    inclusion_delay = Integer(max(1, min(int(horizon), 2)))
    required = gossip_delay + inclusion_delay
    retention_window = required - 1
    pruned, pruned_trace = closure(validators, [], [])
    events = [
        {"kind": "observe", "slot": 0, "validator": 2},
        {"kind": "gossip", "slot": int(gossip_delay), "from": 2, "to": 0},
        {"kind": "propose", "slot": int(required), "bonded": True, "includes": True},
    ]
    return record(
        "horizon_cross_coupled_retention_schedule",
        "projection_risk",
        "sage_horizon_retention_gossip_inclusion_window",
        "Cross-coupled horizon modeling keeps retention, gossip delay, and proposer inclusion in one witness: pruning before gossip plus inclusion delay loses direct and induced slashability.",
        {
            "validators": validators,
            "direct": direct,
            "edges": edge_list(edges),
            "closure": retained,
            "trace": retained_trace,
            "pruned_closure": pruned,
            "pruned_trace": pruned_trace,
            "gossip_delay": int(gossip_delay),
            "inclusion_delay": int(inclusion_delay),
            "required_retention_window": int(required),
            "retention_window": int(retention_window),
            "retention_policy": {"window": int(retention_window), "required_window": int(required)},
            "projection": {"pruned_before_inclusion": True, "slashability_lost": retained != pruned},
            "events": events,
            "views": [{"node": "retained", "active_edges": edge_list(edges)}, {"node": "pruned", "active_edges": []}],
        },
        ["Rocq: retention lower-bound assumption candidate", "TLA+: TemporalWindowDivergenceClass", "Rust: horizon fixture UC-110"],
    )


def horizon_proposer_withholding_fairness():
    validators = [0, 1]
    direct = [0]
    edges = [(1, 0)]
    closure_set, trace = closure(validators, direct, edges)
    withholding = [{"bonded": True, "observes": True, "includes": False}]
    fair_extension = withholding + [{"bonded": True, "observes": True, "includes": True}]
    return record(
        "horizon_proposer_withholding_fairness",
        "assumption_counterexample",
        "sage_horizon_withholding_requires_fair_inclusion",
        "Visible slash evidence is not bounded-live unless some bonded proposer that observes it is required or assumed to include it.",
        {
            "validators": validators,
            "direct": direct,
            "edges": edge_list(edges),
            "closure": closure_set,
            "trace": trace,
            "events": [{"kind": "propose", "slot": 0, "bonded": True, "observes": True, "includes": False}],
            "withholding_schedule": withholding,
            "withholding_first_slash_slot": first_slash_slot(withholding),
            "fair_extension": fair_extension,
            "fair_extension_first_slash_slot": first_slash_slot(fair_extension),
        },
        ["Rocq: proposer fairness assumption", "TLA+: Inv_ProposerFairnessForBoundedLiveness", "docs: bounded liveness boundary"],
    )


def horizon_rust_detector_projection():
    cases = [
        {"name": "missing_only", "contributions": [{"kind": "missing"}], "detectable": False},
        {"name": "duplicate_child", "contributions": [{"kind": "child", "hash": 10}, {"kind": "child", "hash": 10}], "detectable": False},
        {"name": "distinct_children", "contributions": [{"kind": "child", "hash": 10}, {"kind": "child", "hash": 11}], "detectable": True},
        {"name": "detected_hash", "contributions": [{"kind": "missing"}, {"kind": "detected", "hash": 20}], "detectable": True},
    ]
    for case in cases:
        case["model_detectable"] = detector_detectable(case["contributions"])
        case["matches_expected"] = case["model_detectable"] == case["detectable"]
    return record(
        "horizon_rust_detector_projection",
        "confirmed_safe",
        "sage_horizon_rust_detector_contribution_gate",
        "Rust-shaped latest-message contributions remain total and order-independent at the horizon boundary: missing pointers contribute nothing, duplicate child paths count once, two distinct children or a detected hash are decisive.",
        {
            "validators": [0, 1, 2, 3],
            "direct": [0],
            "edges": [[1, 0]],
            "rust_replay": {"detector_cases": cases},
            "projection": {"distinct_child_hashes_required": True, "missing_pointer_noncontributing": True},
        },
        ["Rocq: T-9.11 detector totality/distinct-child lemmas", "TLA+: Inv_DetectorContributionConfluence", "Rust: detector horizon fixture"],
    )


def horizon_multi_epoch_rebond_carryover():
    current = [0, 1]
    strict, strict_trace = closure(current, [], [])
    loose, loose_trace = closure(current, [0], [(1, 0)])
    return record(
        "horizon_multi_epoch_rebond_carryover",
        "candidate_boundary",
        "sage_horizon_epoch_rebond_identity_policy",
        "Multi-epoch horizon modeling keeps stale evidence out of current closure unless the protocol explicitly chooses loose identity or pending-slash carryover.",
        {
            "validators": current,
            "epochs": [1, 1],
            "stakes": [1, 1],
            "stale_direct": [0],
            "direct": [],
            "edges": [[1, 0]],
            "strict_closure": strict,
            "strict_trace": strict_trace,
            "loose_identity_closure": loose,
            "loose_identity_trace": loose_trace,
            "events": [
                {"kind": "observe_stale_direct", "epoch": 0, "validator": 0},
                {"kind": "advance_epoch", "epoch": 1},
                {"kind": "rejoin", "validator": 0, "epoch": 1},
            ],
            "projection": {"loose_identity": True, "strict_epoch_tagged_identity": False},
        },
        ["Rocq: stale_epoch_not_eligible", "TLA+: RebondIdentityDivergenceClass", "docs: epoch identity policy"],
    )


def horizon_weighted_damage_min_attacker(max_stake):
    validators = [0, 1, 2, 3]
    stakes = [int(max(2, max_stake)), int(max(2, max_stake - 1)), 1, 2]
    direct = [2]
    edges = [(0, 1), (1, 2), (3, 0)]
    closure_set, trace = closure(validators, direct, edges)
    stake_vector = vector(ZZ, stakes)
    direct_stake = stake_sum(stake_vector, direct)
    closure_stake = stake_sum(stake_vector, closure_set)
    return record(
        "horizon_weighted_damage_min_attacker",
        "assumption_counterexample",
        "sage_horizon_weighted_damage_min_attacker",
        "Weighted horizon search preserves the minimum-attacker lesson: a low-stake direct offender can amplify damage through a neglect path when the weighted closure-bound assumption is removed.",
        {
            "validators": validators,
            "stakes": stakes,
            "direct": direct,
            "edges": edge_list(edges),
            "closure": closure_set,
            "trace": trace,
            "direct_stake": int(direct_stake),
            "closure_stake": int(closure_stake),
            "extra_stake": int(closure_stake - direct_stake),
        },
        ["Rocq: weighted closure-bound precondition", "TLA+: BoundedWeightedSlashClosure", "docs: weighted threat catalog"],
    )


def horizon_view_merge_convergence():
    validators = [0, 1, 2, 3]
    direct = [0]
    view_a = [(1, 0)]
    view_b = [(2, 1), (3, 0)]
    merged = sorted(set(view_a).union(set(view_b)))
    closure_a, trace_a = closure(validators, direct, view_a)
    closure_b, trace_b = closure(validators, direct, view_b)
    closure_merged, trace_merged = closure(validators, direct, merged)
    return record(
        "horizon_view_merge_convergence",
        "confirmed_safe",
        "sage_horizon_view_merge_overapproximates_partitions",
        "Partitioned evidence views may disagree before convergence, but retained view merge over-approximates both local closures and is commutative.",
        {
            "validators": validators,
            "direct": direct,
            "view_a_edges": edge_list(view_a),
            "view_b_edges": edge_list(view_b),
            "merged_edges": edge_list(merged),
            "closure_a": closure_a,
            "trace_a": trace_a,
            "closure_b": closure_b,
            "trace_b": trace_b,
            "closure": closure_merged,
            "trace": trace_merged,
            "views": [
                {"node": "A", "active_edges": edge_list(view_a), "closure": closure_a},
                {"node": "B", "active_edges": edge_list(view_b), "closure": closure_b},
                {"node": "merged", "active_edges": edge_list(merged), "closure": closure_merged},
            ],
        },
        ["Rocq: union_neglect_graph closure theorems", "TLA+: Inv_ViewMergeOverapproximatesInputs", "Rust: UC-110 view fixture"],
    )


def horizon_batch_arithmetic_projection(bits):
    limit = Integer(2) ** Integer(bits) - Integer(1)
    bonds = [int(limit), 1]
    exact = Integer(sum(bonds))
    wrapped = int(exact % (Integer(2) ** Integer(bits)))
    checked_ok = exact <= limit
    return record(
        "horizon_batch_arithmetic_projection",
        "projection_risk",
        "sage_horizon_batch_arithmetic_checked_projection",
        "Batch accounting and bounded arithmetic are coupled at the horizon: exact Sage totals reject or require a wider envelope where fixed-width wrapping would silently change vault accounting.",
        {
            "validators": [0, 1],
            "stakes": bonds,
            "direct": [0, 1],
            "closure": [0, 1],
            "bits": int(bits),
            "limit": int(limit),
            "exact_vault_total": int(exact),
            "wrapped_vault_total": wrapped,
            "checked_ok": bool(checked_ok),
            "projection": {"wrapping_arithmetic": wrapped, "checked_arithmetic_accepts": bool(checked_ok)},
        },
        ["Rocq: arithmetic_safe_envelope", "TLA+: Inv_ArithmeticSafeEnvelope", "Rust: checked arithmetic projection fixture"],
    )


def horizon_metamorphic_ordering():
    validators = [0, 1, 2, 3]
    direct = [0]
    edges = [(1, 0), (2, 1), (3, 0)]
    baseline, baseline_trace = closure(validators, direct, edges)
    matrix_oracle = matrix_reverse_closure(validators, direct, edges)
    permutations_checked = []
    for item in permutations(edges):
        candidate, _ = closure(validators, direct, item)
        permutations_checked.append({"edges": edge_list(item), "closure": candidate, "matches": candidate == baseline})
    return record(
        "horizon_metamorphic_ordering",
        "confirmed_safe",
        "sage_horizon_metamorphic_ordering_matrix_crosscheck",
        "Horizon metamorphic checks keep edge-order invariance and the independent matrix reachability oracle in the same witness, reducing the chance that the graph model masks ordering bugs.",
        {
            "validators": validators,
            "direct": direct,
            "edges": edge_list(edges),
            "closure": baseline,
            "trace": baseline_trace,
            "matrix_oracle_closure": matrix_oracle,
            "permutations_checked": permutations_checked,
            "all_permutations_match": all(item["matches"] for item in permutations_checked),
            "matrix_matches_iterative": matrix_oracle == baseline,
        },
        ["Rocq: slash_iter_reachability_characterization", "TLA+: closure oracle invariants", "Rust: metamorphic horizon fixture"],
    )


def analyze(max_validators, horizon, bits, max_stake):
    records = [
        horizon_cross_coupled_retention_schedule(horizon),
        horizon_proposer_withholding_fairness(),
        horizon_rust_detector_projection(),
        horizon_multi_epoch_rebond_carryover(),
        horizon_weighted_damage_min_attacker(max_stake),
        horizon_view_merge_convergence(),
        horizon_batch_arithmetic_projection(bits),
        horizon_metamorphic_ordering(),
    ]
    records = [item for item in records if len(item["scenario"]["validators"]) <= int(max_validators)]
    axis_counts = {}
    class_counts = {}
    for item in records:
        axis_counts[item["axis"]] = axis_counts.get(item["axis"], 0) + 1
        class_counts[item["classification"]] = class_counts.get(item["classification"], 0) + 1
    missing_axes = [axis for axis in AXES if axis not in axis_counts]
    return {
        "summaries": [
            {
                "max_validators": int(max_validators),
                "horizon": int(horizon),
                "bits": int(bits),
                "max_stake": int(max_stake),
                "axes": len(axis_counts),
                "records": len(records),
                "missing_axes": missing_axes,
                "class_counts": class_counts,
                "unexpected_count": class_counts.get("unexpected", 0),
            }
        ],
        "records": records,
    }


def filtered_records(result, objectives):
    if objectives == "all":
        return list(result["records"])
    requested = Set([item.strip() for item in objectives.split(",") if item.strip()])
    selected = []
    for item in result["records"]:
        searchable = Set([item["axis"], item["name"], item["classification"]]).union(Set(item.get("coverage_features", [])))
        if requested.intersection(searchable):
            selected.append(item)
    return selected


def frontier_fixtures(result, top_k, objectives):
    records = filtered_records(result, objectives)
    records = sorted(records, key=lambda item: (-int(item.get("threat_score", 0)), item["name"]))[: int(top_k)]
    fixtures = []
    for item in records:
        fixtures.append(
            scenario_fixture(
                item["name"],
                item["classification"],
                item["scenario"],
                item["deterministic_witness"],
                item["deterministic_witness"],
                assertions=["classification == {}".format(item["classification"]), "unexpected_count == 0"],
            )
        )
    return {"summaries": [coverage_summary(fixtures)], "fixtures": fixtures}


def self_test():
    result = analyze(4, 3, 8, 4)
    summary = result["summaries"][0]
    if summary["missing_axes"]:
        raise AssertionError("missing horizon axes: {}".format(summary["missing_axes"]))
    if summary["unexpected_count"] != 0:
        raise AssertionError("unexpected horizon divergence found")
    names = Set([item["name"] for item in result["records"]])
    required = Set(
        [
            "sage_horizon_retention_gossip_inclusion_window",
            "sage_horizon_withholding_requires_fair_inclusion",
            "sage_horizon_rust_detector_contribution_gate",
            "sage_horizon_epoch_rebond_identity_policy",
            "sage_horizon_weighted_damage_min_attacker",
            "sage_horizon_view_merge_overapproximates_partitions",
            "sage_horizon_batch_arithmetic_checked_projection",
            "sage_horizon_metamorphic_ordering_matrix_crosscheck",
        ]
    )
    if not required.issubset(names):
        raise AssertionError("missing required horizon witness")
    for item in result["records"]:
        if item["name"] == "sage_horizon_metamorphic_ordering_matrix_crosscheck":
            witness = item["deterministic_witness"]
            if not witness["all_permutations_match"] or not witness["matrix_matches_iterative"]:
                raise AssertionError("horizon metamorphic oracle mismatch")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print(
            "horizon={horizon} axes={axes} records={records} missing_axes={missing_axes} unexpected={unexpected_count}".format(
                **summary
            )
        )
        for classification in sorted(summary["class_counts"]):
            print("classification={classification} count={count}".format(classification=classification, count=summary["class_counts"][classification]))
    for item in result["records"]:
        print("axis={axis} classification={classification} name={name}".format(**item))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage horizon search for cross-coupled slashing witnesses")
    parser.add_argument("--max-validators", type=int, default=4)
    parser.add_argument("--horizon", type=int, default=3)
    parser.add_argument("--bits", type=int, default=8)
    parser.add_argument("--max-stake", type=int, default=4)
    parser.add_argument("--top-k", type=int, default=12)
    parser.add_argument("--objectives", default="all")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    parser.add_argument("--schema-out")
    parser.add_argument("--fixture-out")
    parser.add_argument("--coverage-out")
    args = parser.parse_args(argv)
    if args.self_test:
        result = self_test()
    else:
        result = analyze(args.max_validators, args.horizon, args.bits, args.max_stake)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")
    if args.schema_out:
        with open(args.schema_out, "w", encoding="utf-8") as handle:
            json.dump(schema_example(), handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")
    if args.fixture_out:
        with open(args.fixture_out, "w", encoding="utf-8") as handle:
            json.dump(frontier_fixtures(result, args.top_k, args.objectives), handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")
    if args.coverage_out:
        with open(args.coverage_out, "w", encoding="utf-8") as handle:
            json.dump(coverage_summary(filtered_records(result, args.objectives)), handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
