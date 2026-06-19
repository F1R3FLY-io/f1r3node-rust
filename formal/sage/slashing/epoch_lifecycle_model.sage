import argparse
import json
import sys

from sage.all import DiGraph, Subsets


def closure(current, evidence_epoch, current_epoch, equivocators, edges):
    current = set(current)
    eligible = {v for v in equivocators if v in current and evidence_epoch.get(v) == current_epoch}
    filtered_edges = [(src, dst) for src, dst in edges if src in current and dst in current and evidence_epoch.get(dst) == current_epoch]
    graph = DiGraph([sorted(current), filtered_edges], format="vertices_and_edges")
    slashed = set(eligible)
    while True:
        next_slashed = set(slashed)
        for offender in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(offender))
        next_slashed = next_slashed.intersection(current)
        if next_slashed == slashed:
            return sorted(slashed)
        slashed = next_slashed


def analyze():
    current = {0, 1, 2}
    evidence_epoch = {0: 0, 1: 1, 2: 1, 3: 0}
    current_epoch = 1
    stale_equivocators = {3}
    fresh_equivocators = {1}
    stale_edges = [(0, 3)]
    fresh_edges = [(0, 1)]
    stale = closure(current, evidence_epoch, current_epoch, stale_equivocators, stale_edges)
    fresh = closure(current, evidence_epoch, current_epoch, fresh_equivocators, fresh_edges)
    records = [
        {"case": "stale_offender_filtered", "closure": stale, "holds": stale == []},
        {"case": "fresh_current_offender_propagates", "closure": fresh, "holds": fresh == [0, 1]},
    ]
    return {"summaries": [{"cases": len(records), "failures": len([r for r in records if not r["holds"]])}], "records": records}


def self_test():
    result = analyze()
    if any(not record["holds"] for record in result["records"]):
        raise AssertionError("epoch lifecycle invariant failed")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("cases={cases} failures={failures}".format(**summary))
    for record in result["records"]:
        print("case={case} closure={closure} holds={holds}".format(**record))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage model for epoch/current-validator lifecycle filtering")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else analyze()
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
