import argparse
import json
import sys

from sage.all import DiGraph


def closure(n, equivocators, edges):
    graph = DiGraph([list(range(n)), list(edges)], format="vertices_and_edges", loops=True, multiedges=True)
    slashed = set(equivocators)
    rounds = [sorted(slashed)]
    while True:
        next_slashed = set(slashed)
        for validator in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(validator))
        if next_slashed == slashed:
            return sorted(slashed), rounds
        slashed = next_slashed
        rounds.append(sorted(slashed))
        if len(rounds) > n + 1:
            raise AssertionError("closure did not converge")


def analyze_cases():
    duplicate_edges = [(0, 1), (0, 1), (0, 1)]
    duplicate_closure, duplicate_rounds = closure(2, {1}, duplicate_edges)
    dedupe_closure, _ = closure(2, {1}, [(0, 1)])
    disconnected_cycle_edges = [(0, 1), (1, 0)]
    disconnected_cycle_closure, _ = closure(3, {2}, disconnected_cycle_edges)
    cycle_to_offender_edges = [(0, 1), (1, 0), (1, 2)]
    cycle_to_offender_closure, cycle_rounds = closure(3, {2}, cycle_to_offender_edges)
    self_edge_only_closure, _ = closure(2, {1}, [(0, 0)])
    direct_self_edge_closure, _ = closure(2, {1}, [(1, 1)])
    records = [
        {
            "model": "sage_duplicate_edges",
            "edges": [list(edge) for edge in duplicate_edges],
            "closure": duplicate_closure,
            "deduplicated_closure": dedupe_closure,
            "property": "duplicate_edges_do_not_change_closure",
            "holds": duplicate_closure == dedupe_closure,
        },
        {
            "model": "sage_disconnected_cycle",
            "edges": [list(edge) for edge in disconnected_cycle_edges],
            "equivocators": [2],
            "closure": disconnected_cycle_closure,
            "property": "cycle_without_path_to_offender_is_not_slashed",
            "holds": disconnected_cycle_closure == [2],
        },
        {
            "model": "sage_cycle_to_offender",
            "edges": [list(edge) for edge in cycle_to_offender_edges],
            "equivocators": [2],
            "closure": cycle_to_offender_closure,
            "rounds": cycle_rounds,
            "property": "cycle_with_path_to_offender_is_slashed",
            "holds": cycle_to_offender_closure == [0, 1, 2],
        },
        {
            "model": "sage_self_edge_only",
            "edges": [[0, 0]],
            "equivocators": [1],
            "closure": self_edge_only_closure,
            "property": "self_edge_without_path_to_offender_is_not_slashed",
            "holds": self_edge_only_closure == [1],
        },
        {
            "model": "sage_direct_offender_self_edge",
            "edges": [[1, 1]],
            "equivocators": [1],
            "closure": direct_self_edge_closure,
            "property": "direct_offender_self_edge_is_idempotent",
            "holds": direct_self_edge_closure == [1],
        },
    ]
    return {"summaries": [{"cases": len(records), "failures": len([record for record in records if not record["holds"]])}], "records": records}


def self_test():
    result = analyze_cases()
    if any(not record["holds"] for record in result["records"]):
        raise AssertionError("graph edge-case invariant failed")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("cases={cases} failures={failures}".format(**summary))
    for record in result["records"]:
        print("case={model} property={property} holds={holds} closure={closure}".format(**record))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage model for duplicate, self-edge, and cyclic neglect graphs")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    result = self_test() if args.self_test else analyze_cases()
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
