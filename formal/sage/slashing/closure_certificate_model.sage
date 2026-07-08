import argparse
import json
import sys

from sage.all import DiGraph, Integer


def closure_rounds(n, equivocators, edges):
    graph = DiGraph([list(range(n)), list(edges)], format="vertices_and_edges")
    slashed = set(equivocators)
    rounds = [sorted(slashed)]
    while True:
        next_slashed = set(slashed)
        for offender in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(offender))
        if next_slashed == slashed:
            return graph, rounds
        slashed = next_slashed
        rounds.append(sorted(slashed))


def first_rounds(rounds):
    result = {}
    for index, validators in enumerate(rounds):
        for validator in validators:
            result.setdefault(validator, index)
    return result


def analyze(n, equivocators, edges):
    graph, rounds = closure_rounds(n, equivocators, edges)
    first = first_rounds(rounds)
    certificates = []
    failures = []
    for validator, round_index in sorted(first.items()):
        if validator in equivocators:
            expected = Integer(0)
            path = [validator]
        else:
            paths = []
            for offender in sorted(equivocators):
                candidate = graph.shortest_path(validator, offender)
                if candidate:
                    paths.append(candidate)
            path = min(paths, key=lambda value: (len(value), value)) if paths else []
            expected = Integer(len(path) - 1) if path else None
        certificates.append({"validator": validator, "first_round": round_index, "shortest_distance": None if expected is None else int(expected), "path": path})
        if expected is None or Integer(round_index) != expected:
            failures.append(certificates[-1])
    fixed_point_round = len(rounds) - 1
    stable_after_n = fixed_point_round <= n
    return {
        "summaries": [{"n": n, "fixed_point_round": fixed_point_round, "stable_after_n": stable_after_n, "failures": len(failures)}],
        "certificates": certificates,
        "failures": failures,
    }


def chain_case(n):
    edges = [(i, i + 1) for i in range(n - 1)]
    return analyze(n, {n - 1}, edges)


def self_test():
    result = chain_case(6)
    if result["summaries"][0]["fixed_point_round"] != 5:
        raise AssertionError("chain fixed-point round changed")
    if result["failures"]:
        raise AssertionError("first slash round did not match shortest distance")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("n={n} fixed_point_round={fixed_point_round} stable_after_n={stable_after_n} failures={failures}".format(**summary))
    for item in result["certificates"][:8]:
        print("validator={validator} first_round={first_round} shortest_distance={shortest_distance} path={path}".format(**item))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage closure certificates for fixed-point depth and first slash round")
    parser.add_argument("--n", type=int, default=6)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else chain_case(args.n)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
