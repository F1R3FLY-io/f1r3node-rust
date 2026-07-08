import argparse
import json
import sys

from sage.all import DiGraph, Permutations, Set, Subsets


def closure(vertices, equivocators, edges):
    graph = DiGraph([sorted(vertices), list(edges)], format="vertices_and_edges")
    slashed = set(equivocators).intersection(vertices)
    while True:
        next_slashed = set(slashed)
        for validator in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(validator))
        next_slashed = next_slashed.intersection(vertices)
        if next_slashed == slashed:
            return sorted(slashed)
        slashed = next_slashed


def compare_current_filter(current_n, evidence_n, equivocators, edges):
    current = set(range(current_n))
    evidence = set(range(evidence_n))
    fixed_edges = [(src, dst) for src, dst in edges if src in current and dst in current]
    fixed = closure(current, set(equivocators).intersection(current), fixed_edges)
    legacy = sorted(set(closure(evidence, equivocators, edges)).intersection(current))
    in_current_state = (
        set(equivocators).issubset(current)
        and all(src in current and dst in current for src, dst in edges)
    )
    return {
        "model": "sage_differential_current_filter",
        "current_n": current_n,
        "evidence_n": evidence_n,
        "equivocators": sorted(equivocators),
        "edges": [list(edge) for edge in sorted(edges)],
        "fixed": fixed,
        "legacy": legacy,
        "in_current_state": in_current_state,
        "diverges": fixed != legacy,
        "classification": "candidate_boundary_filter" if fixed != legacy and not in_current_state else ("unexpected" if fixed != legacy else "bisimilar"),
    }


def tracker_operations(thread_count):
    return [{"op": op, "hash": "h{}".format(op)} for op in range(thread_count)]


def prefixed_schedules(operation_count):
    events = [(op, "read") for op in range(operation_count)] + [(op, "write") for op in range(operation_count)]
    for permutation in Permutations(range(len(events))):
        positions = {event_index: position for position, event_index in enumerate(permutation)}
        if all(positions[op] < positions[operation_count + op] for op in range(operation_count)):
            yield [events[index] for index in permutation]


def tracker_pre_fix(schedule, ops):
    stored = Set([])
    snapshots = {}
    for op_index, step in schedule:
        if step == "read":
            snapshots[op_index] = Set(stored)
        else:
            stored = Set(snapshots[op_index]).union(Set([ops[op_index]["hash"]]))
    return sorted(stored)


def compare_tracker(thread_count):
    ops = tracker_operations(thread_count)
    expected = sorted(Set([op["hash"] for op in ops]))
    for schedule in prefixed_schedules(thread_count):
        legacy = tracker_pre_fix(schedule, ops)
        if legacy != expected:
            return {
                "model": "sage_differential_tracker_atomicity",
                "thread_count": thread_count,
                "schedule": [{"op": op, "step": step} for op, step in schedule],
                "legacy": legacy,
                "fixed": expected,
                "diverges": True,
                "classification": "permitted_bug_fix_atomic_tracker",
            }
    return {
        "model": "sage_differential_tracker_atomicity",
        "thread_count": thread_count,
        "legacy": expected,
        "fixed": expected,
        "diverges": False,
        "classification": "bisimilar",
    }


def possible_edges(n):
    return [(src, dst) for src in range(n) for dst in range(n) if src != dst]


def search(current_n, extra_validators, edge_limit, max_models):
    evidence_n = current_n + extra_validators
    edges_all = possible_edges(evidence_n)
    checked = 0
    unexpected = []
    candidates = []
    permitted = []
    for edge_size in range(edge_limit + 1):
        for edge_subset in Subsets(range(len(edges_all)), edge_size):
            edges = [edges_all[i] for i in sorted(edge_subset)]
            for equivocators in Subsets(range(evidence_n)):
                checked += 1
                if checked > max_models:
                    raise SystemExit("refusing more than {} models".format(max_models))
                record = compare_current_filter(current_n, evidence_n, set(equivocators), edges)
                if record["classification"] == "candidate_boundary_filter" and len(candidates) < 4:
                    candidates.append(record)
                if record["classification"] == "unexpected":
                    unexpected.append(record)
                    return {"summaries": [{"checked": checked, "unexpected": len(unexpected), "candidates": len(candidates), "permitted": len(permitted)}], "unexpected": unexpected, "candidates": candidates, "permitted": permitted}
    tracker = compare_tracker(3)
    if tracker["classification"] == "permitted_bug_fix_atomic_tracker" and len(permitted) < 4:
        permitted.append(tracker)
    return {"summaries": [{"checked": checked, "unexpected": len(unexpected), "candidates": len(candidates), "permitted": len(permitted)}], "unexpected": unexpected, "candidates": candidates, "permitted": permitted}


def self_test():
    result = search(3, 1, 1, 1_000_000)
    if result["unexpected"]:
        raise AssertionError("unexpected bisimilarity divergence found")
    if not result["candidates"]:
        raise AssertionError("candidate boundary divergence not found")
    if not result["permitted"]:
        raise AssertionError("expected tracker bug-fix divergence not found")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("checked={checked} unexpected={unexpected} candidates={candidates} permitted={permitted}".format(**summary))
    if result["candidates"]:
        first = result["candidates"][0]
        print("candidate_divergence model={model} classification={classification}".format(**first))
    if result["permitted"]:
        first = result["permitted"][0]
        print("permitted_divergence model={model} classification={classification}".format(**first))
    if result["unexpected"]:
        first = result["unexpected"][0]
        print("unexpected_divergence model={model}".format(**first))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage differential model for bisimilarity except tagged bug fixes")
    parser.add_argument("--current-n", type=int, default=3)
    parser.add_argument("--extra-validators", type=int, default=1)
    parser.add_argument("--edge-limit", type=int, default=1)
    parser.add_argument("--max-models", type=int, default=1_000_000)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    result = self_test() if args.self_test else search(args.current_n, args.extra_validators, args.edge_limit, args.max_models)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
