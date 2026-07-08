import argparse
import json
import os
import sys

from sage.all import Integer, QQ, Set, ZZ, vector

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


AXES = [
    "objective_pareto_frontier",
    "objective_novelty_coverage",
    "objective_threat_priority",
    "objective_rust_fixture_selection",
    "objective_projection_boundary",
    "objective_assumption_boundary",
]


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def closure(vertices, direct, edges):
    universe = set(int(v) for v in vertices)
    slashed = set(int(v) for v in direct).intersection(universe)
    trace = [{"round": 0, "closure": sorted(slashed)}]
    while True:
        next_slashed = set(slashed)
        for src, dst in edges:
            if int(dst) in slashed and int(src) in universe:
                next_slashed.add(int(src))
        next_slashed = next_slashed.intersection(universe)
        if next_slashed == slashed:
            return sorted(slashed), trace
        slashed = next_slashed
        trace.append({"round": len(trace), "closure": sorted(slashed)})


def stake_sum(stakes, validators):
    return Integer(sum(Integer(stakes[int(v)]) for v in validators))


def record(axis, classification, name, statement, scenario, witness, formalization, objective_tags):
    features = coverage_features(scenario, classification, witness)
    objective = objective_vector(classification, features, witness)
    return {
        "axis": axis,
        "classification": classification,
        "name": name,
        "statement": statement,
        "scenario": scenario,
        "deterministic_witness": witness,
        "coverage_features": features,
        "threat_score": threat_score(classification, features, witness),
        "objective_vector": [int(value) for value in objective],
        "objective_tags": objective_tags,
        "formalization_follow_up": formalization,
        "promotion_status": "classified_objective_frontier_witness",
    }


def severity(classification):
    return {
        "unexpected": 100,
        "projection_risk": 70,
        "assumption_counterexample": 55,
        "candidate_boundary": 35,
        "permitted_bug_fix": 20,
        "bisimilar": 0,
        "confirmed_safe": 0,
    }.get(classification, 10)


def objective_vector(classification, features, witness):
    witness_text = json.dumps(witness, sort_keys=True, default=json_default)
    closure_size = Integer(max([len(witness.get(key, [])) for key in ["closure", "exact_closure", "retained_closure", "loose_closure"]] + [0]))
    extra_stake = Integer(witness.get("extra_stake", 0))
    view_gap = Integer(len(witness.get("partial_view_gap", []))) + Integer(witness.get("view_gap", 0))
    retention_gap = Integer(1 if "retention" in witness_text or "pruned" in witness_text else 0)
    arithmetic_gap = Integer(witness.get("overflow_by", 0)) + Integer(witness.get("boundary_distance", 0))
    feature_count = Integer(len(features))
    return vector(ZZ, [Integer(severity(classification)), closure_size, extra_stake, view_gap, retention_gap, arithmetic_gap, feature_count])


def dominates(left, right):
    left_vector = vector(ZZ, left["objective_vector"])
    right_vector = vector(ZZ, right["objective_vector"])
    return all(left_vector[index] >= right_vector[index] for index in range(len(left_vector))) and any(
        left_vector[index] > right_vector[index] for index in range(len(left_vector))
    )


def pareto_frontier(records):
    frontier = []
    for candidate in records:
        if not any(dominates(other, candidate) for other in records if other["name"] != candidate["name"]):
            frontier.append(candidate)
    return sorted(frontier, key=lambda item: (-item["threat_score"], item["name"]))


def representative_records(max_stake, bits):
    retained, retained_trace = closure([0, 1], [1], [(0, 1)])
    pruned, pruned_trace = closure([0, 1], [], [])
    retention_scenario = canonical_scenario([0, 1], direct_equivocators=[1], neglect_edges=[(0, 1)], expected_classification="projection_risk")
    retention = record(
        "objective_projection_boundary",
        "projection_risk",
        "sage_objective_retention_minimal_projection",
        "The objective frontier keeps the smallest retention/pruning witness because it has low scenario size and high projection severity.",
        retention_scenario,
        {
            "slash_delay": 1,
            "retention_window": 0,
            "retained_closure": retained,
            "retained_trace": retained_trace,
            "pruned_closure": pruned,
            "pruned_trace": pruned_trace,
        },
        ["TLA+: Inv_EvidenceRetentionForDirectOffenders", "docs: projection-risk catalog"],
        ["retention", "projection"],
    )

    chain_closure, chain_trace = closure([0, 1, 2, 3], [0], [(1, 0), (2, 1)])
    chain_scenario = canonical_scenario([0, 1, 2, 3], direct_equivocators=[0], neglect_edges=[(1, 0), (2, 1)], expected_classification="assumption_counterexample")
    chain = record(
        "objective_assumption_boundary",
        "assumption_counterexample",
        "sage_objective_multilevel_reverse_reachability",
        "The frontier preserves the minimal two-edge reverse-reachability chain because dropping the bounded-closure hypothesis admits extra offenders.",
        chain_scenario,
        {"direct": [0], "edges": [[1, 0], [2, 1]], "closure": chain_closure, "trace": chain_trace, "closure_bound_holds_for_f_1": len(chain_closure) <= 1},
        ["Rocq: slash_iter_reachability_characterization", "TLA+: ClosureAfter reverse reachability"],
        ["closure", "assumption"],
    )

    stake_bound = Integer(max_stake)
    stakes = [int(stake_bound), int(stake_bound), 1, 1]
    damage_closure, damage_trace = closure([0, 1, 2, 3], [2], [(0, 1), (1, 2)])
    direct_stake = stake_sum(stakes, [2])
    closure_stake = stake_sum(stakes, damage_closure)
    damage_scenario = canonical_scenario([0, 1, 2, 3], stakes=stakes, direct_equivocators=[2], neglect_edges=[(0, 1), (1, 2)], expected_classification="assumption_counterexample")
    damage = record(
        "objective_threat_priority",
        "assumption_counterexample",
        "sage_objective_weighted_damage_priority",
        "Weighted objective ranking prioritizes chains where a low-stake direct offender can induce high extra stake into the closure if the closure-bound hypothesis is absent.",
        damage_scenario,
        {
            "stakes": stakes,
            "direct": [2],
            "edges": [[0, 1], [1, 2]],
            "closure": damage_closure,
            "trace": damage_trace,
            "direct_stake": int(direct_stake),
            "closure_stake": int(closure_stake),
            "extra_stake": int(closure_stake - direct_stake),
            "damage_ratio": str(QQ(closure_stake - direct_stake) / QQ(direct_stake)),
        },
        ["Rocq: weighted_closure_bound_assumption_needed", "TLA+: BoundedWeightedSlashClosure"],
        ["stake", "closure", "assumption"],
    )

    strict, strict_trace = closure([0, 1], [], [])
    loose, loose_trace = closure([0, 1], [0], [(1, 0)])
    epoch_scenario = canonical_scenario([0, 1], epochs=[0, 1], direct_equivocators=[0], neglect_edges=[(1, 0)], expected_classification="candidate_boundary")
    epoch = record(
        "objective_novelty_coverage",
        "candidate_boundary",
        "sage_objective_epoch_identity_boundary",
        "Novelty scoring keeps stale-evidence epoch identity projection as a separate feature family from pruning and weighted damage.",
        epoch_scenario,
        {"strict_closure": strict, "strict_trace": strict_trace, "loose_closure": loose, "loose_trace": loose_trace, "view_gap": len(set(loose).difference(set(strict)))},
        ["Rocq: stale_epoch_not_eligible", "TLA+: EpochCarryoverDivergenceClass"],
        ["epoch", "identity", "projection"],
    )

    limit = Integer(2) ** Integer(bits) - Integer(1)
    overflow_by = Integer(1)
    overflow_value = limit + overflow_by
    arithmetic_scenario = canonical_scenario([0, 1], stakes=[int(limit), 1], expected_classification="projection_risk")
    arithmetic = record(
        "objective_projection_boundary",
        "projection_risk",
        "sage_objective_arithmetic_boundary_distance",
        "Arithmetic objective scoring keeps exact boundary distance so bounded-integer replay tests can target the smallest overflowing projection.",
        arithmetic_scenario,
        {
            "bits": int(bits),
            "limit": int(limit),
            "safe_value": int(limit),
            "overflow_value": int(overflow_value),
            "overflow_by": int(overflow_by),
            "boundary_distance": int(overflow_by),
            "wrapped": int(overflow_value % (Integer(2) ** Integer(bits))),
        },
        ["Rocq: arithmetic_safe_envelope", "TLA+: Inv_ArithmeticSafeEnvelope"],
        ["arithmetic", "projection"],
    )

    reported, reported_trace = closure([0, 1], [0], [])
    reported_scenario = canonical_scenario([0, 1], direct_equivocators=[0], reports=[(1, 0)], slash_targets=[(1, 0)], expected_classification="bisimilar")
    report = record(
        "objective_rust_fixture_selection",
        "bisimilar",
        "sage_objective_report_suppression_fixture",
        "Fixture selection retains a bisimilar reported-citation case so replay suites check that explicit reports suppress neglect edges.",
        reported_scenario,
        {"direct": [0], "reports": [[1, 0]], "closure": reported, "trace": reported_trace},
        ["Rocq: reported_edge_not_active", "TLA+: Inv_ReportsSuppressNeglectEdges"],
        ["report", "bisimilar"],
    )

    return [retention, chain, damage, epoch, arithmetic, report]


def select_records(records, objectives):
    if objectives == "all":
        return records
    requested = Set([item.strip() for item in objectives.split(",") if item.strip()])
    selected = []
    for item in records:
        tags = Set(item.get("objective_tags", []))
        if requested.intersection(tags) or item["axis"] in requested:
            selected.append(item)
    return selected


def analyze(max_stake, bits, top_k, objectives):
    records = select_records(representative_records(max_stake, bits), objectives)
    frontier = pareto_frontier(records)
    if objectives == "all":
        records.append(
            record(
                "objective_pareto_frontier",
                "confirmed_safe",
                "sage_objective_pareto_frontier_summary",
                "The objective model computes the nondominated scenario frontier before fixture selection, making threat prioritization auditable instead of implicit.",
                canonical_scenario([0], expected_classification="confirmed_safe"),
                {"frontier_names": [item["name"] for item in frontier], "frontier_count": len(frontier), "dominance_relation": "componentwise_ge_with_one_strict"},
                ["Sage: objective_vector and dominates", "docs: threat-vector ranking"],
                ["frontier", "ranking"],
            )
        )
    ranked = sorted(records, key=lambda item: (-item["threat_score"], item["name"]))
    selected = ranked[: int(top_k)]
    axis_counts = {}
    class_counts = {}
    for item in records:
        axis_counts[item["axis"]] = axis_counts.get(item["axis"], 0) + 1
        class_counts[item["classification"]] = class_counts.get(item["classification"], 0) + 1
    missing_axes = [axis for axis in AXES if axis not in axis_counts] if objectives == "all" else []
    return {
        "summaries": [
            {
                "max_stake": int(max_stake),
                "bits": int(bits),
                "top_k": int(top_k),
                "objectives": objectives,
                "axes": len(axis_counts),
                "records": len(records),
                "frontier_count": len(frontier),
                "selected_count": len(selected),
                "missing_axes": missing_axes,
                "class_counts": class_counts,
                "unexpected_count": class_counts.get("unexpected", 0),
            }
        ],
        "records": records,
        "pareto_frontier": [{"name": item["name"], "classification": item["classification"], "objective_vector": item["objective_vector"], "threat_score": item["threat_score"]} for item in frontier],
        "selected": [{"name": item["name"], "classification": item["classification"], "threat_score": item["threat_score"], "objective_tags": item["objective_tags"]} for item in selected],
    }


def fixture_output(result):
    fixtures = []
    selected_names = Set([item["name"] for item in result["selected"]])
    for item in result["records"]:
        if item["name"] in selected_names:
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
    result = analyze(4, 8, 6, "all")
    summary = result["summaries"][0]
    if summary["missing_axes"]:
        raise AssertionError("missing objective axes: {}".format(summary["missing_axes"]))
    if summary["unexpected_count"] != 0:
        raise AssertionError("unexpected objective classification")
    if summary["frontier_count"] == 0:
        raise AssertionError("empty objective frontier")
    if len(fixture_output(result)["fixtures"]) != summary["selected_count"]:
        raise AssertionError("fixture selection mismatch")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("axes={axes} records={records} frontier={frontier_count} selected={selected_count} missing_axes={missing_axes} unexpected={unexpected_count}".format(**summary))
        for classification in sorted(summary["class_counts"]):
            print("classification={classification} count={count}".format(classification=classification, count=summary["class_counts"][classification]))
    for item in result["selected"]:
        print("selected={name} classification={classification} threat_score={threat_score}".format(**item))


def main(argv):
    parser = argparse.ArgumentParser(description="Objective-guided Sage frontier model for slashing")
    parser.add_argument("--max-stake", type=int, default=4)
    parser.add_argument("--bits", type=int, default=8)
    parser.add_argument("--top-k", type=int, default=6)
    parser.add_argument("--objectives", default="all")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    parser.add_argument("--schema-out")
    parser.add_argument("--fixture-out")
    parser.add_argument("--coverage-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else analyze(args.max_stake, args.bits, args.top_k, args.objectives)
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
            json.dump(fixture_output(result), handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")
    if args.coverage_out:
        with open(args.coverage_out, "w", encoding="utf-8") as handle:
            json.dump(coverage_summary(result["records"]), handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
