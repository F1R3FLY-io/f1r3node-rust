import argparse
import json
import sys

from sage.all import DiGraph, Integer, Subsets, ZZ, cartesian_product, vector


def count_fault_bound(n):
    return int((Integer(n) - Integer(1)) // Integer(3))


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
    ranges = [range(max_stake + 1) for _ in range(n)]
    for values in cartesian_product(ranges):
        if sum(values) > 0:
            yield vector(ZZ, [Integer(value) for value in values])


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
            raise AssertionError("closure did not converge")


def closure_stake(stakes, closure):
    return Integer(sum(stakes[v] for v in closure))


def analyze_case(stakes, equivocators, edges):
    n = len(stakes)
    total = Integer(sum(stakes))
    fault = stake_fault_bound(total)
    quorum = total - fault
    closure, rounds = slash_closure(n, equivocators, edges)
    slashed_stake = closure_stake(stakes, closure)
    direct_stake = closure_stake(stakes, equivocators)
    active_stake = total - slashed_stake
    return {
        "model": "sage_weighted_two_level_slash_closure",
        "n": n,
        "stakes": [int(value) for value in stakes],
        "total_stake": int(total),
        "stake_fault_bound": int(fault),
        "stake_quorum_bound": int(quorum),
        "equivocators": sorted(equivocators),
        "direct_equivocator_stake": int(direct_stake),
        "edges": [list(edge) for edge in sorted(edges)],
        "closure": closure,
        "rounds": rounds,
        "slashed_stake": int(slashed_stake),
        "active_stake": int(active_stake),
        "property": "active_stake_above_weighted_quorum",
        "holds": active_stake >= quorum,
        "precondition": {
            "direct_equivocator_stake_at_most_fault": direct_stake <= fault,
            "closure_stake_at_most_fault": slashed_stake <= fault,
        },
    }


def weighted_quorum_drop(record):
    return (
        not record["holds"]
        and record["stake_fault_bound"] >= 1
        and record["direct_equivocator_stake"] <= record["stake_fault_bound"]
        and record["slashed_stake"] > record["stake_fault_bound"]
    )


def minimal_weighted_quorum_drop(max_n, max_stake, edge_limit, max_models):
    checked = 0
    for n in range(1, max_n + 1):
        for stakes in stake_vectors(n, max_stake):
            fault = stake_fault_bound(sum(stakes))
            if fault < 1:
                continue
            for edge_size in range((0 if edge_limit is None else edge_limit) + 1):
                if edge_limit is None and edge_size > len(possible_edges(n)):
                    break
                edge_limit_here = edge_size
                for edges in edge_sets(n, edge_limit_here):
                    if len(edges) != edge_size:
                        continue
                    for equivocators in Subsets(range(n)):
                        checked += 1
                        if checked > max_models:
                            raise SystemExit("refusing more than {} models".format(max_models))
                        equivocators = set(equivocators)
                        if not equivocators:
                            continue
                        record = analyze_case(stakes, equivocators, edges)
                        if weighted_quorum_drop(record):
                            return {"summaries": [{"checked": checked, "max_n": max_n}], "witnesses": [record]}
    return {"summaries": [{"checked": checked, "max_n": max_n}], "witnesses": []}


def run_analysis(max_n, max_stake, edge_limit, max_models, max_witnesses):
    summaries = []
    witnesses = []
    checked = 0
    for n in range(1, max_n + 1):
        summary = {
            "n": n,
            "cases": 0,
            "weighted_quorum_failures": 0,
            "max_slashed_stake": 0,
            "max_active_stake_loss": 0,
        }
        for stakes in stake_vectors(n, max_stake):
            for edges in edge_sets(n, edge_limit):
                for equivocators in Subsets(range(n)):
                    checked += 1
                    if checked > max_models:
                        raise SystemExit("refusing more than {} models".format(max_models))
                    record = analyze_case(stakes, set(equivocators), edges)
                    summary["cases"] += 1
                    summary["max_slashed_stake"] = max(summary["max_slashed_stake"], record["slashed_stake"])
                    summary["max_active_stake_loss"] = max(
                        summary["max_active_stake_loss"],
                        record["slashed_stake"] - record["direct_equivocator_stake"],
                    )
                    if weighted_quorum_drop(record):
                        summary["weighted_quorum_failures"] += 1
                        if len(witnesses) < max_witnesses:
                            witnesses.append(record)
        summaries.append(summary)
    return {"summaries": summaries, "witnesses": witnesses}


def self_test():
    result = minimal_weighted_quorum_drop(4, 1, 1, 1_000_000)
    if not result["witnesses"]:
        raise AssertionError("weighted quorum drop witness not found")
    witness = result["witnesses"][0]
    if witness["slashed_stake"] <= witness["stake_fault_bound"]:
        raise AssertionError("weighted witness does not exceed fault bound")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        if "n" in summary:
            print(
                "n={n} cases={cases} weighted_quorum_failures={weighted_quorum_failures} max_slashed_stake={max_slashed_stake} max_active_stake_loss={max_active_stake_loss}".format(
                    **summary
                )
            )
        else:
            print("checked={checked} max_n={max_n}".format(**summary))
    if result["witnesses"]:
        first = result["witnesses"][0]
        print(
            "first_witness n={n} stakes={stakes} fault={stake_fault_bound} equivocators={equivocators} direct_stake={direct_equivocator_stake} edges={edges} closure={closure} slashed_stake={slashed_stake} active_stake={active_stake} quorum={stake_quorum_bound}".format(
                **first
            )
        )


def main(argv):
    parser = argparse.ArgumentParser(description="Sage weighted stake model for two-level slashing closure")
    parser.add_argument("--max-n", type=int, default=4)
    parser.add_argument("--max-stake", type=int, default=2)
    parser.add_argument("--edge-limit", type=int, default=2)
    parser.add_argument("--max-models", type=int, default=1_000_000)
    parser.add_argument("--max-witnesses", type=int, default=8)
    parser.add_argument("--minimal-quorum-drop", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    if args.self_test:
        result = self_test()
    elif args.minimal_quorum_drop:
        result = minimal_weighted_quorum_drop(args.max_n, args.max_stake, args.edge_limit, args.max_models)
    else:
        result = run_analysis(args.max_n, args.max_stake, args.edge_limit, args.max_models, args.max_witnesses)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
