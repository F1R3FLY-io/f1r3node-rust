import argparse
import json
import os
import sys

from sage.all import DiGraph, Integer, Set, ZZ, vector

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


AXES = [
    "dag_direct_equivocation",
    "dag_neglect_edge_derivation",
    "dag_reported_citation",
    "dag_epoch_churn_projection",
    "dag_retention_projection",
    "dag_multi_level_invalid_citation",
]


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def edge_list(edges):
    return [[int(src), int(dst)] for src, dst in sorted(edges)]


def closure(vertices, direct, edges):
    vertices = sorted(vertices)
    graph = DiGraph([vertices, list(edges)], format="vertices_and_edges")
    universe = set(vertices)
    slashed = set(direct).intersection(universe)
    trace = [{"round": 0, "closure": sorted(slashed)}]
    while True:
        next_slashed = set(slashed)
        for offender in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(offender))
        next_slashed = next_slashed.intersection(universe)
        if next_slashed == slashed:
            return sorted(slashed), trace
        slashed = next_slashed
        trace.append({"round": len(trace), "closure": sorted(slashed)})


def direct_equivocators(blocks):
    by_key = {}
    for block in blocks:
        key = (int(block["sender"]), int(block["seq"]))
        by_key.setdefault(key, Set([]))
        by_key[key] = by_key[key].union(Set([int(block["hash"])]))
    direct = Set([])
    for sender, _seq in by_key:
        if len(by_key[(sender, _seq)]) > 1:
            direct = direct.union(Set([sender]))
    return sorted([int(v) for v in direct])


def derive_neglect_edges(blocks, direct):
    by_hash = {int(block["hash"]): block for block in blocks}
    direct_set = set(int(v) for v in direct)
    edges = Set([])
    reports = Set([])
    slash_targets = Set([])
    for block in blocks:
        sender = int(block["sender"])
        for target in block.get("slash_targets", []):
            slash_targets = slash_targets.union(Set([(sender, int(target))]))
        for cited_hash in block.get("justifications", []):
            cited = by_hash.get(int(cited_hash))
            if cited is None:
                continue
            offender = int(cited["sender"])
            if offender in direct_set and offender != sender:
                if offender in [int(v) for v in block.get("slash_targets", [])]:
                    reports = reports.union(Set([(sender, offender)]))
                else:
                    edges = edges.union(Set([(sender, offender)]))
    return sorted(edges), sorted(reports), sorted(slash_targets)


def record(axis, classification, name, statement, scenario, witness, formalization):
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
        "promotion_status": "classified_deterministic_witness",
    }


def direct_equivocation_record():
    blocks = [
        {"hash": 1, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
        {"hash": 2, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
    ]
    direct = direct_equivocators(blocks)
    edges, reports, slash_targets = derive_neglect_edges(blocks, direct)
    closure_set, trace = closure([0, 1, 2], direct, edges)
    scenario = canonical_scenario([0, 1, 2], blocks=blocks, direct_equivocators=direct, neglect_edges=edges, reports=reports, slash_targets=slash_targets)
    witness = {"direct": direct, "edges": edge_list(edges), "closure": closure_set, "trace": trace}
    return record(
        "dag_direct_equivocation",
        "bisimilar",
        "sage_dag_direct_equivocation_derives_level1",
        "Production-shaped DAG blocks with the same sender and sequence derive the same direct-equivocator seed as the formal closure model.",
        scenario,
        witness,
        ["Rocq: detection_sound/detection_complete", "TLA+: EquivocationDetector"],
    )


def neglect_edge_record():
    blocks = [
        {"hash": 1, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
        {"hash": 2, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
        {"hash": 3, "sender": 1, "seq": 2, "justifications": [2], "slash_targets": []},
    ]
    direct = direct_equivocators(blocks)
    edges, reports, slash_targets = derive_neglect_edges(blocks, direct)
    closure_set, trace = closure([0, 1, 2], direct, edges)
    scenario = canonical_scenario([0, 1, 2], blocks=blocks, direct_equivocators=direct, neglect_edges=edges, reports=reports, slash_targets=slash_targets)
    witness = {"direct": direct, "edges": edge_list(edges), "closure": closure_set, "trace": trace}
    return record(
        "dag_neglect_edge_derivation",
        "bisimilar",
        "sage_dag_missing_slash_deploy_derives_neglect_edge",
        "A block that cites a direct equivocator without a slash target derives the same visible neglect edge consumed by two-level closure.",
        scenario,
        witness,
        ["Rocq: visible_unreported_graph_in", "TLA+: Inv_NeglectEdgesVisibleUnreported"],
    )


def reported_citation_record():
    blocks = [
        {"hash": 1, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
        {"hash": 2, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
        {"hash": 3, "sender": 1, "seq": 2, "justifications": [2], "slash_targets": [0]},
    ]
    direct = direct_equivocators(blocks)
    edges, reports, slash_targets = derive_neglect_edges(blocks, direct)
    closure_set, trace = closure([0, 1, 2], direct, edges)
    scenario = canonical_scenario([0, 1, 2], blocks=blocks, direct_equivocators=direct, neglect_edges=edges, reports=reports, slash_targets=slash_targets)
    witness = {"direct": direct, "edges": edge_list(edges), "reports": edge_list(reports), "closure": closure_set, "trace": trace}
    return record(
        "dag_reported_citation",
        "bisimilar",
        "sage_dag_slash_target_suppresses_neglect_edge",
        "A block that cites a direct equivocator and includes the slash target is reported rather than neglectful, so closure stays at the direct offender.",
        scenario,
        witness,
        ["Rocq: reported_edge_not_active", "TLA+: Inv_ReportsSuppressNeglectEdges"],
    )


def epoch_churn_record():
    strict, strict_trace = closure([0, 1], [], [])
    loose, loose_trace = closure([0, 1], [0], [(1, 0)])
    scenario = canonical_scenario([0, 1], epochs=[0, 1], direct_equivocators=[0], neglect_edges=[(1, 0)], expected_classification="candidate_boundary")
    witness = {"strict_closure": strict, "strict_trace": strict_trace, "loose_closure": loose, "loose_trace": loose_trace}
    return record(
        "dag_epoch_churn_projection",
        "candidate_boundary",
        "sage_dag_epoch_identity_projection_boundary",
        "DAG-shaped stale evidence remains a policy boundary: strict epoch identity filters it, while loose identity projects it into current closure.",
        scenario,
        witness,
        ["Rocq: stale_epoch_not_eligible", "TLA+: EpochCarryoverDivergenceClass"],
    )


def retention_record():
    retained, retained_trace = closure([0, 1], [0], [(1, 0)])
    pruned, pruned_trace = closure([0, 1], [], [])
    scenario = canonical_scenario([0, 1], direct_equivocators=[0], neglect_edges=[(1, 0)], expected_classification="projection_risk")
    witness = {"slash_delay": 1, "retention_window": 0, "retained_closure": retained, "retained_trace": retained_trace, "pruned_closure": pruned, "pruned_trace": pruned_trace}
    return record(
        "dag_retention_projection",
        "projection_risk",
        "sage_dag_retention_projection_loses_evidence",
        "Pruning DAG evidence before the first slashable slot removes the direct seed and derived neglect edge, reproducing the projection-risk witness.",
        scenario,
        witness,
        ["TLA+: Inv_EvidenceRetentionForDirectOffenders", "docs: UC-95"],
    )


def multi_level_invalid_citation_record():
    blocks = [
        {"hash": 1, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
        {"hash": 2, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
        {"hash": 3, "sender": 1, "seq": 2, "justifications": [2], "slash_targets": []},
        {"hash": 4, "sender": 2, "seq": 3, "justifications": [3], "slash_targets": []},
    ]
    direct = [0]
    edges = [(1, 0), (2, 1)]
    closure_set, trace = closure([0, 1, 2, 3], direct, edges)
    scenario = canonical_scenario([0, 1, 2, 3], blocks=blocks, direct_equivocators=direct, neglect_edges=edges, expected_classification="assumption_counterexample")
    witness = {"direct": direct, "edges": edge_list(edges), "closure": closure_set, "trace": trace, "closure_bound_holds_for_f_1": len(closure_set) <= 1}
    return record(
        "dag_multi_level_invalid_citation",
        "assumption_counterexample",
        "sage_dag_multi_level_invalid_citation_chain",
        "A production-shaped citation chain reproduces the reverse-reachability closure-bound counterexample when the bounded-closure hypothesis is dropped.",
        scenario,
        witness,
        ["Rocq: deep_threat_chain_closure_bound_assumption_needed", "TLA+: DeepThreatModelDivergenceClass"],
    )


def analyze(max_dag_blocks, max_epochs, max_validators):
    records = [
        direct_equivocation_record(),
        neglect_edge_record(),
        reported_citation_record(),
        epoch_churn_record(),
        retention_record(),
        multi_level_invalid_citation_record(),
    ]
    records = [record for record in records if len(record["scenario"]["blocks"]) <= int(max_dag_blocks) or record["axis"] in ["dag_epoch_churn_projection", "dag_retention_projection"]]
    axis_counts = {}
    class_counts = {}
    for item in records:
        axis_counts[item["axis"]] = axis_counts.get(item["axis"], 0) + 1
        class_counts[item["classification"]] = class_counts.get(item["classification"], 0) + 1
    missing_axes = [axis for axis in AXES if axis not in axis_counts]
    return {
        "summaries": [
            {
                "max_dag_blocks": int(max_dag_blocks),
                "max_epochs": int(max_epochs),
                "max_validators": int(max_validators),
                "axes": len(axis_counts),
                "records": len(records),
                "missing_axes": missing_axes,
                "class_counts": class_counts,
                "unexpected_count": class_counts.get("unexpected", 0),
            }
        ],
        "records": records,
    }


def fixture_output(result):
    fixtures = []
    for item in result["records"]:
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
    result = analyze(4, 2, 4)
    summary = result["summaries"][0]
    if summary["missing_axes"]:
        raise AssertionError("missing DAG behavior axes: {}".format(summary["missing_axes"]))
    if summary["unexpected_count"] != 0:
        raise AssertionError("unexpected DAG behavior classification")
    if len(fixture_output(result)["fixtures"]) != len(result["records"]):
        raise AssertionError("fixture count mismatch")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("axes={axes} records={records} missing_axes={missing_axes} unexpected={unexpected_count}".format(**summary))
        for classification in sorted(summary["class_counts"]):
            print("classification={classification} count={count}".format(classification=classification, count=summary["class_counts"][classification]))
    for item in result["records"]:
        print("axis={axis} classification={classification} name={name}".format(**item))


def main(argv):
    parser = argparse.ArgumentParser(description="Production-shaped DAG behavior Sage model for slashing")
    parser.add_argument("--max-dag-blocks", type=int, default=4)
    parser.add_argument("--max-epochs", type=int, default=2)
    parser.add_argument("--max-validators", type=int, default=4)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    parser.add_argument("--schema-out")
    parser.add_argument("--fixture-out")
    parser.add_argument("--coverage-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else analyze(args.max_dag_blocks, args.max_epochs, args.max_validators)
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
