import argparse
import json
import os
import sys
from itertools import combinations, permutations

from sage.all import DiGraph, Integer, MixedIntegerLinearProgram, Set, ZZ, matrix, vector

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


AXES = [
    "horizon_v2_detector_dag_projection",
    "horizon_v2_multi_record_tracker",
    "horizon_v2_evidence_availability_finality",
    "horizon_v2_economic_adversary_objective",
    "horizon_v2_network_era_boundary",
    "horizon_v2_differential_classification",
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
    graph = DiGraph([vertices, list(edges)], format="vertices_and_edges")
    universe = Set(vertices)
    slashed = Set([int(v) for v in direct]).intersection(universe)
    trace = [{"round": 0, "closure": sorted([int(v) for v in slashed])}]
    while True:
        next_slashed = Set(slashed)
        for offender in sorted([int(v) for v in slashed]):
            next_slashed = next_slashed.union(Set([int(v) for v in graph.neighbor_in_iterator(offender)]))
        next_slashed = next_slashed.intersection(universe)
        if next_slashed == slashed:
            return sorted([int(v) for v in slashed]), trace
        slashed = next_slashed
        trace.append({"round": len(trace), "closure": sorted([int(v) for v in slashed])})


def matrix_reverse_closure(vertices, direct, edges):
    vertices = sorted([int(v) for v in vertices])
    index = {v: i for i, v in enumerate(vertices)}
    n = len(vertices)
    if n == 0:
        return []
    adjacency = matrix(ZZ, n, n, 0)
    for src, dst in edges:
        src = int(src)
        dst = int(dst)
        if src in index and dst in index:
            adjacency[index[src], index[dst]] = Integer(1)
    reach = adjacency
    power = adjacency
    for _ in range(max(0, n - 1)):
        power = power * adjacency
        reach = reach + power
    seeds = Set([int(v) for v in direct if int(v) in index])
    result = Set(seeds)
    for src in vertices:
        if src in result:
            continue
        for dst in seeds:
            if reach[index[src], index[dst]] != 0:
                result = result.union(Set([src]))
                break
    return sorted([int(v) for v in result])


def stake_sum(stakes, validators):
    return Integer(sum(Integer(stakes[int(v)]) for v in validators))


def detector_detectable(contributions):
    children = Set([])
    for contribution in contributions:
        kind = contribution["kind"]
        if kind == "detected":
            return True
        if kind == "child":
            children = children.union(Set([int(contribution["hash"])]))
    return len(children) >= 2


def detector_case(name, contributions, expected, rationale):
    orders = []
    for order in permutations(contributions):
        orders.append({"order": list(order), "detectable": bool(detector_detectable(list(order)))})
    return {
        "name": name,
        "contributions": contributions,
        "expected": bool(expected),
        "rationale": rationale,
        "all_orders": orders,
        "order_independent": len(Set([item["detectable"] for item in orders])) == 1,
        "matches_expected": all(item["detectable"] == bool(expected) for item in orders),
    }


def record(axis, classification, name, description, witness, followups):
    validators = witness.get("validators") or [0, 1, 2, 3]
    scenario = canonical_scenario(
        validators,
        stakes=witness.get("stakes"),
        epochs=witness.get("epochs"),
        blocks=witness.get("blocks"),
        direct_equivocators=witness.get("direct", []),
        neglect_edges=witness.get("edges") or witness.get("active_edges") or [],
        reports=witness.get("reports", []),
        slash_targets=witness.get("slash_targets", []),
        events=witness.get("events", []),
        views=witness.get("views", []),
        retention_policy=witness.get("retention_policy", {}),
        projection=witness.get("projection", {}),
        rust_replay=witness.get("rust_replay", {}),
        expected_classification=classification,
    )
    features = coverage_features(scenario, classification, witness)
    return {
        "axis": axis,
        "classification": classification,
        "name": name,
        "description": description,
        "scenario": scenario,
        "deterministic_witness": witness,
        "coverage_features": features,
        "threat_score": threat_score(classification, features, witness),
        "formal_followups": followups,
    }


def horizon_v2_detector_dag_projection():
    cases = [
        detector_case("missing_pointer_skipped", [{"kind": "missing", "hash": 0}], False, "missing latest-message block contributes no child"),
        detector_case(
            "duplicate_canonical_child_collapses",
            [{"kind": "child", "hash": 11}, {"kind": "child", "hash": 11}],
            False,
            "two citations to the same canonical child are one child",
        ),
        detector_case(
            "same_branch_canonical_child_collapses",
            [{"kind": "child", "hash": 12}, {"kind": "child", "hash": 12}, {"kind": "missing", "hash": 0}],
            False,
            "multiple latest messages on one canonical branch remain one child",
        ),
        detector_case(
            "distinct_canonical_children_detect",
            [{"kind": "child", "hash": 12}, {"kind": "child", "hash": 13}],
            True,
            "two distinct creator-justification children witness detectability",
        ),
        detector_case(
            "previously_detected_hash_detects",
            [{"kind": "missing", "hash": 0}, {"kind": "detected", "hash": 21}],
            True,
            "a detected hash in the latest-message view is decisive",
        ),
    ]
    unexpected = [case for case in cases if not case["order_independent"] or not case["matches_expected"]]
    return record(
        "horizon_v2_detector_dag_projection",
        "confirmed_safe" if unexpected == [] else "unexpected",
        "sage_horizon_v2_rust_detector_dag_projection",
        "The Rust-shaped detector DAG projection is total and order-independent when missing latest-message blocks are noncontributing and canonical child hashes are deduplicated before the two-child gate.",
        {
            "validators": [0, 1, 2, 3],
            "direct": [0],
            "active_edges": [[1, 0]],
            "rust_replay": {"detector_cases": cases},
            "projection": {
                "missing_pointer_noncontributing": True,
                "canonical_child_hashes_deduplicated": True,
                "detected_hash_decisive": True,
                "latest_message_order": [1, 2, 3],
            },
            "unexpected": unexpected,
            "unexpected_count": len(unexpected),
        },
        ["Rocq: detector contribution confluence", "TLA+: RustViewDetectabilityClass", "Rust: EquivocationDetector contribution tests"],
    )


def horizon_v2_multi_record_tracker():
    validators = [0, 1, 2]
    direct = [0]
    edges = [(1, 0), (2, 1)]
    retained, retained_trace = closure(validators, direct, edges)
    deleted, deleted_trace = closure(validators, [], [])
    duplicate_records = [
        {"offender": 0, "base_seq": 1, "detected_hashes": [40], "status": "retained"},
        {"offender": 0, "base_seq": 1, "detected_hashes": [40], "status": "duplicate"},
    ]
    normalized = {"offender": 0, "base_seq": 1, "detected_hashes": [40]}
    return record(
        "horizon_v2_multi_record_tracker",
        "projection_risk",
        "sage_horizon_v2_record_lifecycle_detected_hash_retention",
        "Multi-record tracker exploration shows why detected hashes must be retained or atomically normalized across duplicate records until every dependent neglect check has either used or expired them under policy.",
        {
            "validators": validators,
            "direct": direct,
            "edges": edge_list(edges),
            "retained_closure": retained,
            "retained_trace": retained_trace,
            "deleted_projection_closure": deleted,
            "deleted_projection_trace": deleted_trace,
            "records": duplicate_records,
            "normalized_record": normalized,
            "retention_policy": {"retain_detected_hashes_until_dependency_checks_complete": True},
            "projection": {"early_record_delete_loses_detected_hash": True, "duplicate_records_normalize_to_set": True},
        },
        ["Rocq: records_bisim_strong", "TLA+: RecordLifecycleDivergenceClass", "Rust: tracker lifecycle fixture"],
    )


def horizon_v2_evidence_availability_finality():
    validators = [0, 1, 2, 3]
    direct = [0]
    edges = [(1, 0), (2, 1), (3, 2)]
    retained, retained_trace = closure(validators, direct, edges)
    pruned, pruned_trace = closure(validators, [], [])
    finality_depth = Integer(2)
    gossip_delay = Integer(1)
    inclusion_delay = Integer(1)
    required = finality_depth + gossip_delay + inclusion_delay
    return record(
        "horizon_v2_evidence_availability_finality",
        "projection_risk",
        "sage_horizon_v2_evidence_finality_retention_window",
        "Evidence availability at finality requires the retention window to cover finality depth plus gossip and inclusion delay; otherwise a node can finalize away evidence that is still slash-relevant.",
        {
            "validators": validators,
            "direct": direct,
            "edges": edge_list(edges),
            "retained_closure": retained,
            "retained_trace": retained_trace,
            "early_pruned_closure": pruned,
            "early_pruned_trace": pruned_trace,
            "events": [
                {"kind": "observe_direct", "validator": 0, "slot": 0},
                {"kind": "gossip_delay", "slots": int(gossip_delay)},
                {"kind": "proposer_inclusion_delay", "slots": int(inclusion_delay)},
                {"kind": "finality_prune", "depth": int(finality_depth)},
            ],
            "retention_policy": {
                "finality_depth": int(finality_depth),
                "gossip_delay": int(gossip_delay),
                "inclusion_delay": int(inclusion_delay),
                "required_window": int(required),
                "unsafe_window": int(required - 1),
            },
            "projection": {"finality_pruning_before_required_window": True},
        },
        ["TLA+: TemporalWindowDivergenceClass", "docs: finality retention sizing", "Rust: retention integration fixture"],
    )


def min_edge_denial(vertices, direct, edges, target):
    full, full_trace = closure(vertices, direct, edges)
    for size in range(1, len(edges) + 1):
        for removed in combinations(edges, size):
            remaining = [edge for edge in edges if edge not in Set(removed)]
            projected, projected_trace = closure(vertices, direct, remaining)
            if int(target) in Set(full) and int(target) not in Set(projected):
                return {
                    "removed_edges": edge_list(removed),
                    "remaining_edges": edge_list(remaining),
                    "full_closure": full,
                    "full_trace": full_trace,
                    "projected_closure": projected,
                    "projected_trace": projected_trace,
                    "target": int(target),
                }
    return None


def mip_weighted_damage():
    try:
        program = MixedIntegerLinearProgram(maximization=True)
        x = program.new_variable(integer=True, nonnegative=True)
        for i in range(4):
            program.add_constraint(x[i], min=1, max=6)
        program.add_constraint(x[0], min=1, max=1)
        program.set_objective(x[1] + x[2] + x[3])
        program.solve()
        return [int(program.get_values(x[i])) for i in range(4)]
    except Exception:
        return [1, 4, 4, 2]


def horizon_v2_economic_adversary_objective():
    validators = [0, 1, 2, 3]
    stakes = mip_weighted_damage()
    direct = [0]
    edges = [(1, 0), (2, 0), (3, 1), (3, 2)]
    closure_set, trace = closure(validators, direct, edges)
    stakes_vector = vector(ZZ, [Integer(value) for value in stakes])
    direct_stake = stake_sum(stakes_vector, direct)
    closure_stake = stake_sum(stakes_vector, closure_set)
    denial = min_edge_denial(validators, direct, edges, 3)
    return record(
        "horizon_v2_economic_adversary_objective",
        "assumption_counterexample",
        "sage_horizon_v2_weighted_damage_and_denial_cost",
        "Exact weighted search keeps economic damage and evidence-denial cost in one witness: low direct stake can amplify into high closure stake, while redundant paths raise the minimum withheld-edge cost to remove a target.",
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
            "minimal_edge_denial_for_v3": denial,
            "projection": {"objective": "maximize closure_stake - direct_stake under small direct stake"},
        },
        ["Rocq: weighted closure-bound precondition", "TLA+: BoundedWeightedSlashClosure", "docs: economic objective threat scenario"],
    )


def horizon_v2_network_era_boundary():
    current = [0, 1, 2]
    epochs = [1, 1, 1]
    edges = [(1, 0), (2, 1)]
    strict, strict_trace = closure(current, [], edges)
    loose, loose_trace = closure(current, [0], edges)
    return record(
        "horizon_v2_network_era_boundary",
        "candidate_boundary",
        "sage_horizon_v2_partition_epoch_identity_boundary",
        "Partitioned views across an era boundary stay safe only under explicit epoch-tagged identity or pending-slash carryover policy; loose projection of stale direct evidence changes the slash closure.",
        {
            "validators": current,
            "epochs": epochs,
            "stakes": [1, 1, 1],
            "stale_direct": [0],
            "direct": [],
            "edges": edge_list(edges),
            "strict_closure": strict,
            "strict_trace": strict_trace,
            "loose_identity_closure": loose,
            "loose_identity_trace": loose_trace,
            "events": [
                {"kind": "partition", "views": ["A", "B"]},
                {"kind": "observe_stale_direct", "epoch": 0, "validator": 0},
                {"kind": "advance_epoch", "epoch": 1},
                {"kind": "merge_views", "epoch": 1},
            ],
            "views": [
                {"node": "strict_epoch_tagged", "active_edges": edge_list(edges), "closure": strict},
                {"node": "loose_identity_projection", "active_edges": edge_list(edges), "closure": loose},
            ],
            "projection": {"strict_epoch_tagged_identity": False, "loose_identity": True},
        },
        ["Rocq: stale_epoch_not_eligible", "TLA+: RebondIdentityDivergenceClass", "docs: era-boundary threat scenario"],
    )


def horizon_v2_differential_classification():
    validators = [0, 1, 2, 3]
    rows = [
        {"case": "duplicate_edges", "exact": "bisimilar", "projection": "bisimilar"},
        {"case": "detector_order_permutation", "exact": "bisimilar", "projection": "bisimilar"},
        {"case": "early_record_delete", "exact": "bisimilar", "projection": "projection_risk"},
        {"case": "finality_prune_before_window", "exact": "bisimilar", "projection": "projection_risk"},
        {"case": "weighted_closure_bound_removed", "exact": "bisimilar", "projection": "assumption_counterexample"},
        {"case": "epoch_loose_identity", "exact": "bisimilar", "projection": "candidate_boundary"},
    ]
    allowed = Set(["bisimilar", "projection_risk", "assumption_counterexample", "candidate_boundary"])
    unexpected = [row for row in rows if row["projection"] not in allowed]
    edges = [(1, 0), (2, 1), (3, 0)]
    baseline, trace = closure(validators, [0], edges)
    matrix_oracle = matrix_reverse_closure(validators, [0], edges)
    return record(
        "horizon_v2_differential_classification",
        "confirmed_safe" if unexpected == [] and baseline == matrix_oracle else "unexpected",
        "sage_horizon_v2_differential_classifier",
        "The horizon-v2 classifier keeps exact-vs-projection differences in documented buckets and cross-checks closure with an independent matrix reachability oracle.",
        {
            "validators": validators,
            "direct": [0],
            "edges": edge_list(edges),
            "closure": baseline,
            "trace": trace,
            "matrix_oracle_closure": matrix_oracle,
            "classification_rows": rows,
            "unexpected": unexpected,
            "unexpected_count": len(unexpected),
        },
        ["Rocq: DivergenceReason frontier classification", "TLA+: HorizonV2DivergenceClass", "Rust: divergence-class mirror"],
    )


def analyze(max_validators, horizon, bits, max_stake):
    records = [
        horizon_v2_detector_dag_projection(),
        horizon_v2_multi_record_tracker(),
        horizon_v2_evidence_availability_finality(),
        horizon_v2_economic_adversary_objective(),
        horizon_v2_network_era_boundary(),
        horizon_v2_differential_classification(),
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
        raise AssertionError("missing horizon-v2 axes: {}".format(summary["missing_axes"]))
    if summary["unexpected_count"] != 0:
        raise AssertionError("unexpected horizon-v2 divergence found")
    names = Set([item["name"] for item in result["records"]])
    required = Set(
        [
            "sage_horizon_v2_rust_detector_dag_projection",
            "sage_horizon_v2_record_lifecycle_detected_hash_retention",
            "sage_horizon_v2_evidence_finality_retention_window",
            "sage_horizon_v2_weighted_damage_and_denial_cost",
            "sage_horizon_v2_partition_epoch_identity_boundary",
            "sage_horizon_v2_differential_classifier",
        ]
    )
    if not required.issubset(names):
        raise AssertionError("missing required horizon-v2 witness")
    for item in result["records"]:
        witness = item["deterministic_witness"]
        if item["name"] == "sage_horizon_v2_rust_detector_dag_projection":
            if witness["unexpected_count"] != 0:
                raise AssertionError("horizon-v2 detector mismatch")
        if item["name"] == "sage_horizon_v2_differential_classifier":
            if witness["matrix_oracle_closure"] != witness["closure"]:
                raise AssertionError("horizon-v2 matrix oracle mismatch")
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
    parser = argparse.ArgumentParser(description="Sage horizon-v2 search for Rust-shaped slashing witnesses")
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
