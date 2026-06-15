import argparse
import json
import sys

from sage.all import DiGraph, Integer, Subsets, binomial


def fault_bound(n):
    return int((Integer(n) - Integer(1)) // Integer(3))


def possible_edges(n):
    return [(src, dst) for src in range(n) for dst in range(n) if src != dst]


def edge_sets(n, edge_limit):
    edges = possible_edges(n)
    edge_indices = range(len(edges))
    limit = len(edges) if edge_limit is None else edge_limit
    for size in range(limit + 1):
        for subset in Subsets(edge_indices, size):
            yield [edges[i] for i in sorted(subset)]


def neglect_graph(n, edges):
    return DiGraph([list(range(n)), list(edges)], format="vertices_and_edges")


def slash_closure(n, equivocators, edges):
    graph = neglect_graph(n, edges)
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
            raise AssertionError("closure did not converge within n rounds")


def closure_levels(rounds):
    levels = {}
    previous = set()
    by_round = []
    for round_index, validators in enumerate(rounds):
        current = set(validators)
        added = sorted(current.difference(previous))
        by_round.append(added)
        for validator in added:
            levels[validator] = round_index
        previous = current
    return {
        "levels": [{"validator": validator, "round": levels[validator]} for validator in sorted(levels)],
        "new_by_round": by_round,
        "depth": max(levels.values()) if levels else 0,
    }


def closure_via_transitive_closure(n, equivocators, edges):
    graph = neglect_graph(n, edges)
    transitive = graph.transitive_closure()
    closure = set(equivocators)
    for validator in range(n):
        if validator in closure:
            continue
        if any(transitive.has_edge(validator, offender) for offender in equivocators):
            closure.add(validator)
    return sorted(closure)


def witness_paths(n, equivocators, edges, closure):
    graph = neglect_graph(n, edges)
    paths = []
    for validator in sorted(set(closure).difference(equivocators)):
        candidates = []
        for offender in sorted(equivocators):
            path = graph.shortest_path(validator, offender)
            if path:
                candidates.append(path)
        if candidates:
            path = min(candidates, key=lambda value: (len(value), value))
            paths.append({"validator": validator, "offender": path[-1], "path": path, "distance": len(path) - 1})
    return paths


def analyze_case(n, equivocators, edges, include_details=False):
    f = fault_bound(n)
    closure, rounds = slash_closure(n, equivocators, edges)
    tc_closure = closure_via_transitive_closure(n, equivocators, edges)
    if closure != tc_closure:
        raise AssertionError("iterative closure disagrees with Sage transitive closure")
    active = n - len(closure)
    quorum_bound = n - f
    record = {
        "model": "sage_digraph_two_level_slash_closure",
        "n": n,
        "f": f,
        "equivocators": sorted(equivocators),
        "edges": [list(edge) for edge in sorted(edges)],
        "closure": closure,
        "rounds": rounds,
        "active": active,
        "quorum_bound": quorum_bound,
        "property": "active_set_above_quorum",
        "holds": active >= quorum_bound,
        "sage_checks": {
            "transitive_closure_agrees": True,
            "vertices": n,
            "edges": len(edges),
        },
    }
    if include_details:
        record["reachability_characterization"] = "closure is exactly validators with a directed neglect path to a direct equivocator"
        record["closure_levels"] = closure_levels(rounds)
        record["witness_paths"] = witness_paths(n, set(equivocators), edges, closure)
        record["theorem_precondition"] = {
            "closure_size_at_most_f": len(closure) <= f,
            "direct_equivocators_at_most_f": len(equivocators) <= f,
        }
    return record


def protocol_relevant_quorum_drop(record):
    return (
        not record["holds"]
        and record["f"] >= 1
        and len(record["equivocators"]) <= record["f"]
        and len(record["closure"]) > record["f"]
    )


def quorum_drop_case(n, equivocators, edges):
    return protocol_relevant_quorum_drop(analyze_case(n, equivocators, edges))


def minimality(n, equivocators, edges):
    edge_minimal = True
    removable_edges = []
    for edge in edges:
        smaller = [candidate for candidate in edges if candidate != edge]
        if quorum_drop_case(n, set(equivocators), smaller):
            edge_minimal = False
            removable_edges.append(list(edge))
    equivocator_minimal = True
    removable_equivocators = []
    for offender in sorted(equivocators):
        smaller = set(equivocators)
        smaller.remove(offender)
        if quorum_drop_case(n, smaller, edges):
            equivocator_minimal = False
            removable_equivocators.append(offender)
    return {
        "edge_minimal": edge_minimal,
        "equivocator_minimal": equivocator_minimal,
        "removable_edges_preserving_drop": removable_edges,
        "removable_equivocators_preserving_drop": removable_equivocators,
    }


def detailed_case(n, equivocators, edges):
    record = analyze_case(n, set(equivocators), edges, include_details=True)
    if protocol_relevant_quorum_drop(record):
        record["minimality"] = minimality(n, set(equivocators), edges)
    return record


def check_record(record):
    n = record["n"]
    f = record["f"]
    closure = set(record["closure"])
    rounds = [set(values) for values in record["rounds"]]
    if not closure.issubset(set(range(n))):
        raise AssertionError("closure contains a non-validator")
    if len(rounds) > n + 1:
        raise AssertionError("closure took too many rounds")
    for before, after in zip(rounds, rounds[1:]):
        if not before.issubset(after):
            raise AssertionError("closure is not monotone")
    if len(closure) <= f and not record["holds"]:
        raise AssertionError("bounded closure did not preserve quorum")


def model_count(n, edge_limit):
    edge_count = len(possible_edges(n))
    initial_sets = Integer(2) ** Integer(n)
    if edge_limit is None:
        graphs = Integer(2) ** Integer(edge_count)
    else:
        graphs = sum(binomial(edge_count, k) for k in range(edge_limit + 1))
    return int(initial_sets * graphs)


def run_analysis(max_n, edge_limit, max_models, max_witnesses):
    summaries = []
    witnesses = []
    for n in range(1, max_n + 1):
        estimated = model_count(n, edge_limit)
        if estimated > max_models:
            raise SystemExit(
                "refusing {} cases for n={}; raise --max-models or lower --max-n/--edge-limit".format(
                    estimated, n
                )
            )
        summary = {
            "n": n,
            "f": fault_bound(n),
            "cases": 0,
            "quorum_failures": 0,
            "max_closure": 0,
            "max_depth": 0,
            "max_amplification": 0,
        }
        for edges in edge_sets(n, edge_limit):
            for equivocators in Subsets(range(n)):
                record = analyze_case(n, set(equivocators), edges)
                check_record(record)
                summary["cases"] += 1
                summary["max_closure"] = max(summary["max_closure"], len(record["closure"]))
                summary["max_depth"] = max(summary["max_depth"], len(record["rounds"]) - 1)
                if len(record["equivocators"]) <= record["f"]:
                    summary["max_amplification"] = max(
                        summary["max_amplification"],
                        len(record["closure"]) - len(record["equivocators"]),
                    )
                if not record["holds"]:
                    summary["quorum_failures"] += 1
                    if protocol_relevant_quorum_drop(record) and len(witnesses) < max_witnesses:
                        witnesses.append(detailed_case(n, set(equivocators), edges))
        summaries.append(summary)
    return {"summaries": summaries, "witnesses": witnesses}


def known_counterexample():
    return detailed_case(4, {0}, [(3, 0)])


def minimal_quorum_drop(max_n, edge_limit, max_models):
    checked = 0
    for n in range(1, max_n + 1):
        estimated = model_count(n, edge_limit)
        if estimated > max_models:
            raise SystemExit(
                "refusing {} cases for n={}; raise --max-models or lower --max-n/--edge-limit".format(
                    estimated, n
                )
            )
        f = fault_bound(n)
        if f < 1:
            continue
        edges = possible_edges(n)
        edge_limit_n = len(edges) if edge_limit is None else min(edge_limit, len(edges))
        for equiv_size in range(1, f + 1):
            for edge_size in range(edge_limit_n + 1):
                for edge_indices in Subsets(range(len(edges)), edge_size):
                    candidate_edges = [edges[i] for i in sorted(edge_indices)]
                    for equivocators in Subsets(range(n), equiv_size):
                        checked += 1
                        equivocators = set(equivocators)
                        if quorum_drop_case(n, equivocators, candidate_edges):
                            record = detailed_case(n, equivocators, candidate_edges)
                            return {"summaries": [{"checked": checked, "max_n": max_n}], "witnesses": [record]}
    return {"summaries": [{"checked": checked, "max_n": max_n}], "witnesses": []}


def max_amplification(max_n, edge_limit, max_models):
    best = None
    checked = 0
    for n in range(1, max_n + 1):
        estimated = model_count(n, edge_limit)
        if estimated > max_models:
            raise SystemExit(
                "refusing {} cases for n={}; raise --max-models or lower --max-n/--edge-limit".format(
                    estimated, n
                )
            )
        f = fault_bound(n)
        if f < 1:
            continue
        for edges in edge_sets(n, edge_limit):
            for equivocators in Subsets(range(n)):
                equivocators = set(equivocators)
                if not equivocators or len(equivocators) > f:
                    continue
                checked += 1
                record = analyze_case(n, equivocators, edges)
                amplification = len(record["closure"]) - len(record["equivocators"])
                depth = len(record["rounds"]) - 1
                score = (amplification, depth, -len(edges), -n)
                if best is None or score > best[0]:
                    detailed = detailed_case(n, equivocators, edges)
                    detailed["amplification"] = {
                        "extra_slashed": amplification,
                        "depth": depth,
                        "direct_equivocators": len(equivocators),
                        "closure_size": len(record["closure"]),
                    }
                    best = (score, detailed)
    witnesses = [] if best is None else [best[1]]
    return {"summaries": [{"checked": checked, "max_n": max_n}], "witnesses": witnesses}


def self_test():
    witness = known_counterexample()
    if witness["closure"] != [0, 3]:
        raise AssertionError("known closure witness changed")
    if witness["holds"]:
        raise AssertionError("known quorum failure was not detected")
    result = run_analysis(4, None, 1_000_000, 8)
    if not any(item["n"] == 4 and item["quorum_failures"] > 0 for item in result["summaries"]):
        raise AssertionError("n=4 quorum failure was not found")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        if "n" in summary:
            print(
                "n={n} F={f} cases={cases} max_closure={max_closure} max_depth={max_depth} max_amplification={max_amplification} quorum_failures={quorum_failures}".format(
                    **summary
                )
            )
        else:
            print("checked={checked} max_n={max_n}".format(**summary))
    if result["witnesses"]:
        first = result["witnesses"][0]
        print(
            "first_witness n={n} F={f} equivocators={equivocators} edges={edges} closure={closure} active={active} quorum_bound={quorum_bound}".format(
                **first
            )
        )
        if "amplification" in first:
            print("amplification={}".format(first["amplification"]))
        if "witness_paths" in first:
            print("witness_paths={}".format(first["witness_paths"]))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage DiGraph model for two-level slashing closure")
    parser.add_argument("--max-n", type=int, default=4)
    parser.add_argument("--edge-limit", type=int, default=None)
    parser.add_argument("--max-models", type=int, default=1_000_000)
    parser.add_argument("--max-witnesses", type=int, default=16)
    parser.add_argument("--known-counterexample", action="store_true")
    parser.add_argument("--minimal-quorum-drop", action="store_true")
    parser.add_argument("--max-amplification", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    if args.max_n < 1:
        parser.error("--max-n must be positive")
    if args.edge_limit is not None and args.edge_limit < 0:
        parser.error("--edge-limit must be non-negative")
    if args.max_models < 1:
        parser.error("--max-models must be positive")
    if args.max_witnesses < 0:
        parser.error("--max-witnesses must be non-negative")

    if args.known_counterexample:
        result = {"summaries": [], "witnesses": [known_counterexample()]}
    elif args.minimal_quorum_drop:
        result = minimal_quorum_drop(args.max_n, args.edge_limit, args.max_models)
    elif args.max_amplification:
        result = max_amplification(args.max_n, args.edge_limit, args.max_models)
    elif args.self_test:
        result = self_test()
    else:
        result = run_analysis(args.max_n, args.edge_limit, args.max_models, args.max_witnesses)

    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
