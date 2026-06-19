import argparse
import json
import sys

from sage.all import DiGraph, Permutations, Set


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def closure(vertices, equivocators, edges):
    graph = DiGraph([sorted(vertices), list(edges)], format="vertices_and_edges")
    slashed = set(equivocators).intersection(vertices)
    trace = [{"step": 0, "closure": sorted(slashed)}]
    while True:
        next_slashed = set(slashed)
        for offender in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(offender))
        next_slashed = next_slashed.intersection(vertices)
        if next_slashed == slashed:
            return sorted(slashed), trace
        slashed = next_slashed
        trace.append({"step": len(trace), "closure": sorted(slashed)})


def bisimilar_trace():
    current = Set([0, 1, 2])
    direct = Set([1])
    edges = [(0, 1)]
    fixed, fixed_trace = closure(current, direct, edges)
    legacy, legacy_trace = closure(current, direct, edges)
    return {
        "classification": "bisimilar",
        "model": "sage_differential_trace_bisimilar",
        "events": [{"kind": "direct_equivocation", "validator": 1}, {"kind": "neglect_edge", "src": 0, "dst": 1}],
        "fixed": fixed,
        "legacy": legacy,
        "fixed_trace": fixed_trace,
        "legacy_trace": legacy_trace,
    }


def candidate_boundary_trace():
    current = Set([0, 1, 2])
    evidence = Set([0, 1, 2, 3])
    direct = Set([3])
    edges = [(0, 3)]
    fixed, fixed_trace = closure(current, direct.intersection(current), [])
    legacy_full, legacy_trace = closure(evidence, direct, edges)
    legacy = sorted(set(legacy_full).intersection(set(current)))
    return {
        "classification": "candidate_boundary_filter",
        "model": "sage_differential_trace_candidate_boundary",
        "events": [{"kind": "stale_direct_equivocation", "validator": 3}, {"kind": "current_validator_cites_stale", "src": 0, "dst": 3}],
        "fixed": fixed,
        "legacy": legacy,
        "fixed_trace": fixed_trace,
        "legacy_trace": legacy_trace,
    }


def tracker_trace():
    ops = [{"op": i, "hash": "h{}".format(i)} for i in range(2)]
    events = [(0, "read"), (1, "read"), (0, "write"), (1, "write")]
    stored = Set([])
    snapshots = {}
    trace = []
    for op, action in events:
        if action == "read":
            snapshots[op] = Set(stored)
        else:
            stored = Set(snapshots[op]).union(Set([ops[op]["hash"]]))
        trace.append({"op": op, "action": action, "stored": sorted(stored)})
    fixed = sorted(Set([op["hash"] for op in ops]))
    return {
        "classification": "permitted_bug_fix_atomic_tracker",
        "model": "sage_differential_trace_tracker_atomicity",
        "events": [{"op": op, "action": action} for op, action in events],
        "legacy": sorted(stored),
        "fixed": fixed,
        "legacy_trace": trace,
        "fixed_trace": [{"step": i, "stored": sorted(Set([op["hash"] for op in ops[: i + 1]]))} for i in range(len(ops))],
    }


def unexpected_search():
    vertices = Set([0, 1, 2])
    direct = Set([1])
    edge_options = [(0, 1), (2, 1), (0, 2)]
    for order in Permutations(edge_options):
        fixed, _ = closure(vertices, direct, list(order))
        legacy, _ = closure(vertices, direct, sorted(edge_options))
        if fixed != legacy:
            return {
                "classification": "unexpected",
                "model": "sage_differential_trace_unexpected_edge_order",
                "events": [list(edge) for edge in order],
                "fixed": fixed,
                "legacy": legacy,
            }
    return None


def analyze():
    traces = [bisimilar_trace(), candidate_boundary_trace(), tracker_trace()]
    unexpected = unexpected_search()
    if unexpected:
        traces.append(unexpected)
    return {"summaries": [{"traces": len(traces), "unexpected": 1 if unexpected else 0}], "traces": traces}


def self_test():
    result = analyze()
    classes = {trace["classification"] for trace in result["traces"]}
    required = {"bisimilar", "candidate_boundary_filter", "permitted_bug_fix_atomic_tracker"}
    if not required.issubset(classes):
        raise AssertionError("missing differential trace class")
    if result["summaries"][0]["unexpected"] != 0:
        raise AssertionError("unexpected differential trace found")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("traces={traces} unexpected={unexpected}".format(**summary))
    for trace in result["traces"]:
        print("trace={model} classification={classification}".format(**trace))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage differential trace generator for bisimilar, bug-fix, and boundary classes")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else analyze()
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
