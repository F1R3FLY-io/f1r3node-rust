import argparse
import json
import sys

from sage.all import DiGraph, Integer, Subsets


def possible_edges(evidence_n):
    return [(src, dst) for src in range(evidence_n) for dst in range(evidence_n) if src != dst]


def edge_sets(evidence_n, edge_limit):
    edges = possible_edges(evidence_n)
    limit = min(edge_limit, len(edges))
    for size in range(limit + 1):
        for subset in Subsets(range(len(edges)), size):
            yield [edges[i] for i in sorted(subset)]


def slash_closure(vertices, equivocators, edges):
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


def fixed_current_closure(current, equivocators, edges):
    filtered_edges = [(src, dst) for src, dst in edges if src in current and dst in current]
    return slash_closure(current, set(equivocators).intersection(current), filtered_edges)


def legacy_unfiltered_projection(current, evidence, equivocators, edges):
    return sorted(set(slash_closure(evidence, equivocators, edges)).intersection(current))


def analyze_case(current_n, evidence_n, equivocators, edges):
    current = set(range(current_n))
    evidence = set(range(evidence_n))
    fixed = fixed_current_closure(current, equivocators, edges)
    legacy = legacy_unfiltered_projection(current, evidence, equivocators, edges)
    return {
        "model": "sage_validator_set_boundary",
        "current_validators": sorted(current),
        "evidence_validators": sorted(evidence),
        "equivocators": sorted(equivocators),
        "edges": [list(edge) for edge in sorted(edges)],
        "fixed_current_closure": fixed,
        "legacy_unfiltered_current_projection": legacy,
        "property": "current_validator_filtering_preserves_boundary",
        "holds": fixed == legacy,
        "unexpected_current_slashed": sorted(set(legacy).difference(fixed)),
        "missed_current_slashed": sorted(set(fixed).difference(legacy)),
    }


def find_boundary_divergence(current_n, extra_validators, edge_limit, max_models):
    evidence_n = current_n + extra_validators
    checked = 0
    for edges in edge_sets(evidence_n, edge_limit):
        for equivocators in Subsets(range(evidence_n)):
            checked += 1
            if checked > max_models:
                raise SystemExit("refusing more than {} models".format(max_models))
            record = analyze_case(current_n, evidence_n, set(equivocators), edges)
            if not record["holds"]:
                return {"summaries": [{"checked": checked, "current_n": current_n, "evidence_n": evidence_n}], "witnesses": [record]}
    return {"summaries": [{"checked": checked, "current_n": current_n, "evidence_n": evidence_n}], "witnesses": []}


def self_test():
    result = find_boundary_divergence(3, 1, 1, 1_000_000)
    if not result["witnesses"]:
        raise AssertionError("validator boundary divergence not found")
    witness = result["witnesses"][0]
    if not witness["unexpected_current_slashed"]:
        raise AssertionError("boundary divergence did not slash a current validator unexpectedly")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("checked={checked} current_n={current_n} evidence_n={evidence_n}".format(**summary))
    if result["witnesses"]:
        first = result["witnesses"][0]
        print(
            "first_witness current={current_validators} evidence={evidence_validators} equivocators={equivocators} edges={edges} fixed={fixed_current_closure} legacy={legacy_unfiltered_current_projection} unexpected={unexpected_current_slashed}".format(
                **first
            )
        )


def main(argv):
    parser = argparse.ArgumentParser(description="Sage model for current validator-set boundary cases")
    parser.add_argument("--current-n", type=int, default=3)
    parser.add_argument("--extra-validators", type=int, default=1)
    parser.add_argument("--edge-limit", type=int, default=1)
    parser.add_argument("--max-models", type=int, default=1_000_000)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    result = self_test() if args.self_test else find_boundary_divergence(args.current_n, args.extra_validators, args.edge_limit, args.max_models)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
