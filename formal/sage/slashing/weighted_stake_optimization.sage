import argparse
import json
import sys

from sage.all import DiGraph, Integer, MixedIntegerLinearProgram, QQ, Subsets, ZZ, cartesian_product, vector


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def possible_edges(n):
    return [(src, dst) for src in range(n) for dst in range(n) if src != dst]


def edge_sets(n, edge_limit):
    edges = possible_edges(n)
    for size in range(min(edge_limit, len(edges)) + 1):
        for subset in Subsets(range(len(edges)), size):
            yield [edges[i] for i in sorted(subset)]


def closure(n, direct, edges):
    graph = DiGraph([list(range(n)), list(edges)], format="vertices_and_edges")
    slashed = set(direct)
    while True:
        next_slashed = set(slashed)
        for offender in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(offender))
        if next_slashed == slashed:
            return sorted(slashed)
        slashed = next_slashed


def stake_sum(stakes, validators):
    return Integer(sum(stakes[v] for v in validators))


def enumerate_pattern(n, direct, closure_set, max_stake, fault):
    best = None
    for values in cartesian_product([range(1, max_stake + 1) for _ in range(n)]):
        stakes = vector(ZZ, [Integer(value) for value in values])
        total = stake_sum(stakes, range(n))
        if total < 3 * fault + 1:
            continue
        direct_stake = stake_sum(stakes, direct)
        if direct_stake > fault:
            continue
        closure_stake = stake_sum(stakes, closure_set)
        extra = closure_stake - direct_stake
        score = (extra, closure_stake, -direct_stake, total)
        if best is None or score > best[0]:
            best = (score, stakes)
    if best is None:
        return None
    return [int(value) for value in best[1]]


def mip_pattern(n, direct, closure_set, max_stake, fault):
    try:
        program = MixedIntegerLinearProgram(maximization=True)
        stake = program.new_variable(integer=True, nonnegative=True)
        for validator in range(n):
            program.add_constraint(stake[validator] >= 1)
            program.add_constraint(stake[validator] <= max_stake)
        program.add_constraint(program.sum(stake[v] for v in range(n)) >= 3 * fault + 1)
        program.add_constraint(program.sum(stake[v] for v in direct) <= fault)
        program.set_objective(program.sum(stake[v] for v in closure_set) - program.sum(stake[v] for v in direct))
        program.solve()
        values = program.get_values(stake)
        return [int(round(values[v])) for v in range(n)], "mip"
    except Exception:
        fallback = enumerate_pattern(n, direct, closure_set, max_stake, fault)
        return fallback, "enumeration_fallback"


def optimize(max_n, max_stake, edge_limit, max_models):
    checked = 0
    best = None
    start_n = 4 if max_n >= 4 else 2
    for n in range(start_n, max_n + 1):
        for fault in range(1, max(2, n)):
            for edges in edge_sets(n, edge_limit):
                for direct in Subsets(range(n)):
                    checked += 1
                    if checked > max_models:
                        raise SystemExit("refusing more than {} models".format(max_models))
                    direct = sorted(direct)
                    if not direct:
                        continue
                    closure_set = closure(n, direct, edges)
                    if len(closure_set) <= len(direct):
                        continue
                    result, solver = mip_pattern(n, direct, closure_set, max_stake, fault)
                    if result is None:
                        continue
                    stakes = vector(ZZ, [Integer(value) for value in result])
                    total = stake_sum(stakes, range(n))
                    direct_stake = stake_sum(stakes, direct)
                    closure_stake = stake_sum(stakes, closure_set)
                    active_stake = total - closure_stake
                    quorum_stake = total - Integer(fault)
                    extra = closure_stake - direct_stake
                    ratio = QQ(extra) / QQ(direct_stake if direct_stake > 0 else 1)
                    record = {
                        "model": "sage_weighted_stake_optimization",
                        "n": n,
                        "stakes": [int(value) for value in stakes],
                        "fault": int(fault),
                        "direct": direct,
                        "edges": [list(edge) for edge in sorted(edges)],
                        "closure": closure_set,
                        "total_stake": int(total),
                        "direct_stake": int(direct_stake),
                        "closure_stake": int(closure_stake),
                        "extra_stake": int(extra),
                        "active_stake": int(active_stake),
                        "quorum_stake": int(quorum_stake),
                        "weighted_quorum_drop": active_stake < quorum_stake,
                        "damage_ratio": str(ratio),
                        "solver": solver,
                    }
                    score = (extra, ratio, Integer(1 if record["weighted_quorum_drop"] else 0), -Integer(len(edges)))
                    if best is None or score > best[0]:
                        best = (score, record)
    return {"summaries": [{"checked": checked, "max_n": max_n}], "witnesses": [] if best is None else [best[1]]}


def self_test():
    result = optimize(4, 3, 2, 500000)
    if not result["witnesses"]:
        raise AssertionError("weighted optimization witness missing")
    if result["witnesses"][0]["extra_stake"] <= 0:
        raise AssertionError("weighted optimization found no amplification")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("checked={checked} max_n={max_n}".format(**summary))
    for witness in result["witnesses"]:
        print(
            "best n={n} stakes={stakes} fault={fault} direct={direct} edges={edges} closure={closure} extra_stake={extra_stake} ratio={damage_ratio} quorum_drop={weighted_quorum_drop} solver={solver}".format(
                **witness
            )
        )


def main(argv):
    parser = argparse.ArgumentParser(description="Sage weighted-stake optimizer using MILP with exact-enumeration fallback")
    parser.add_argument("--max-n", type=int, default=4)
    parser.add_argument("--max-stake", type=int, default=4)
    parser.add_argument("--edge-limit", type=int, default=2)
    parser.add_argument("--max-models", type=int, default=1_000_000)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else optimize(args.max_n, args.max_stake, args.edge_limit, args.max_models)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
