import argparse
import json
import sys

from sage.all import DiGraph, Integer, Subsets, ZZ, cartesian_product, vector


def stake_fault_bound(total_stake):
    if total_stake <= 0:
        return Integer(0)
    return (Integer(total_stake) - Integer(1)) // Integer(3)


def possible_edges(n):
    return [(src, dst) for src in range(n) for dst in range(n) if src != dst]


def edge_sets(n, edge_limit):
    edges = possible_edges(n)
    limit = len(edges) if edge_limit is None else min(edge_limit, len(edges))
    for size in range(limit + 1):
        for subset in Subsets(range(len(edges)), size):
            yield [edges[i] for i in sorted(subset)]


def stake_vectors(n, max_stake):
    for values in cartesian_product([range(max_stake + 1) for _ in range(n)]):
        if sum(values) > 0:
            yield vector(ZZ, [Integer(value) for value in values])


def slash_closure(n, equivocators, edges):
    graph = DiGraph([list(range(n)), list(edges)], format="vertices_and_edges")
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


def stake_sum(stakes, validators):
    return Integer(sum(stakes[v] for v in validators))


def analyze_case(stakes, equivocators, edges):
    n = len(stakes)
    total = Integer(sum(stakes))
    fault = stake_fault_bound(total)
    closure, rounds = slash_closure(n, equivocators, edges)
    direct_stake = stake_sum(stakes, equivocators)
    slashed_stake = stake_sum(stakes, closure)
    return {
        "model": "sage_damage_optimizer",
        "n": n,
        "stakes": [int(value) for value in stakes],
        "total_stake": int(total),
        "stake_fault_bound": int(fault),
        "equivocators": sorted(equivocators),
        "direct_equivocator_stake": int(direct_stake),
        "edges": [list(edge) for edge in sorted(edges)],
        "closure": closure,
        "rounds": rounds,
        "closure_size": len(closure),
        "slashed_stake": int(slashed_stake),
        "extra_slashed_validators": len(closure) - len(equivocators),
        "extra_slashed_stake": int(slashed_stake - direct_stake),
        "depth": len(rounds) - 1,
    }


def optimize(max_n, max_stake, edge_limit, max_models):
    checked = 0
    best = None
    for n in range(1, max_n + 1):
        for stakes in stake_vectors(n, max_stake):
            fault = stake_fault_bound(sum(stakes))
            if fault < 1:
                continue
            for edges in edge_sets(n, edge_limit):
                for equivocators in Subsets(range(n)):
                    checked += 1
                    if checked > max_models:
                        raise SystemExit("refusing more than {} models".format(max_models))
                    equivocators = set(equivocators)
                    if not equivocators:
                        continue
                    direct_stake = stake_sum(stakes, equivocators)
                    if direct_stake > fault:
                        continue
                    record = analyze_case(stakes, equivocators, edges)
                    score = (
                        record["extra_slashed_stake"],
                        record["extra_slashed_validators"],
                        record["depth"],
                        -len(record["edges"]),
                        record["slashed_stake"],
                    )
                    if best is None or score > best[0]:
                        best = (score, record)
    return {"summaries": [{"checked": checked, "max_n": max_n}], "witnesses": [] if best is None else [best[1]]}


def self_test():
    result = optimize(4, 2, 2, 1_000_000)
    if not result["witnesses"]:
        raise AssertionError("damage witness not found")
    if result["witnesses"][0]["extra_slashed_stake"] < 1:
        raise AssertionError("damage optimizer found no amplified damage")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("checked={checked} max_n={max_n}".format(**summary))
    if result["witnesses"]:
        first = result["witnesses"][0]
        print(
            "best n={n} stakes={stakes} fault={stake_fault_bound} equivocators={equivocators} direct_stake={direct_equivocator_stake} edges={edges} closure={closure} extra_stake={extra_slashed_stake} extra_validators={extra_slashed_validators} depth={depth}".format(
                **first
            )
        )


def main(argv):
    parser = argparse.ArgumentParser(description="Sage optimizer for slashing damage witnesses")
    parser.add_argument("--max-n", type=int, default=5)
    parser.add_argument("--max-stake", type=int, default=3)
    parser.add_argument("--edge-limit", type=int, default=3)
    parser.add_argument("--max-models", type=int, default=1_000_000)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    result = self_test() if args.self_test else optimize(args.max_n, args.max_stake, args.edge_limit, args.max_models)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
