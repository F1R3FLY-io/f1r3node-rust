import argparse
import json
import sys

from sage.all import DiGraph, Integer


def closure(n, equivocators, visible, reports):
    edges = sorted(set(visible).difference(reports))
    graph = DiGraph([list(range(n)), edges], format="vertices_and_edges")
    slashed = set(equivocators)
    while True:
        next_slashed = set(slashed)
        for offender in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(offender))
        if next_slashed == slashed:
            return sorted(slashed), edges
        slashed = next_slashed


def analyze(n):
    equivocators = {0}
    stages = [
        {"time": 0, "visible": set(), "reports": set()},
        {"time": 1, "visible": {(1, 0)}, "reports": set()},
        {"time": 2, "visible": {(1, 0), (2, 0)}, "reports": {(1, 0)}},
        {"time": 3, "visible": {(1, 0), (2, 0), (3, 0)}, "reports": {(1, 0)}},
    ]
    records = []
    previous = set()
    for stage in stages:
        slashed, edges = closure(n, equivocators, stage["visible"], stage["reports"])
        current = set(slashed)
        records.append(
            {
                "time": stage["time"],
                "edges": [list(edge) for edge in edges],
                "closure": slashed,
                "monotone": previous.issubset(current),
                "all_edges_visible": set(edges).issubset(stage["visible"]),
                "all_edges_unreported": set(edges).isdisjoint(stage["reports"]),
            }
        )
        previous = current
    failures = [record for record in records if not record["all_edges_visible"] or not record["all_edges_unreported"]]
    return {"summaries": [{"n": n, "stages": len(stages), "failures": len(failures)}], "records": records, "failures": failures}


def self_test():
    result = analyze(4)
    if result["failures"]:
        raise AssertionError("propagation edge invariant failed")
    if result["records"][-1]["closure"] != [0, 2, 3]:
        raise AssertionError("visibility/report closure changed")
    if result["records"][2]["monotone"]:
        raise AssertionError("report removal should make accountability closure shrink in this witness")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("n={n} stages={stages} failures={failures}".format(**summary))
    for record in result["records"]:
        print("time={time} edges={edges} closure={closure} monotone={monotone} visible={all_edges_visible} unreported={all_edges_unreported}".format(**record))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage model for evidence propagation and withholding over time")
    parser.add_argument("--n", type=int, default=4)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else analyze(args.n)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
