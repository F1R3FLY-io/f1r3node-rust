import argparse
import json
import sys

from sage.all import DiGraph, Integer, QQ, Subsets, ZZ, cartesian_product, vector


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def fault_bound(n):
    return (Integer(n) - Integer(1)) // Integer(3)


def stake_fault_bound(total):
    return (Integer(total) - Integer(1)) // Integer(3) if total > 0 else Integer(0)


def quorum_bound(n):
    return Integer(n) - fault_bound(n)


def possible_edges(n):
    return [(src, dst) for src in range(n) for dst in range(n) if src != dst]


def edge_sets(n, edge_limit):
    edges = possible_edges(n)
    for size in range(min(edge_limit, len(edges)) + 1):
        for subset in Subsets(range(len(edges)), size):
            yield [edges[i] for i in sorted(subset)]


def stake_vectors(n, max_stake):
    for values in cartesian_product([range(1, max_stake + 1) for _ in range(n)]):
        yield vector(ZZ, [Integer(value) for value in values])


def bounded_subsets(values, limit):
    values = list(values)
    for size in range(min(limit, len(values)) + 1):
        for subset in Subsets(range(len(values)), size):
            yield [values[i] for i in sorted(subset)]


def closure(n, equivocators, edges):
    graph = DiGraph([list(range(n)), list(edges)], format="vertices_and_edges")
    slashed = set(equivocators)
    rounds = [sorted(slashed)]
    while True:
        next_slashed = set(slashed)
        for offender in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(offender))
        if next_slashed == slashed:
            return sorted(slashed), rounds
        slashed = next_slashed
        rounds.append(sorted(slashed))


def stake_sum(stakes, validators):
    return Integer(sum(stakes[v] for v in validators))


def analyze_case(stakes, adversaries, equivocators, candidate_edges, visible_edges, reports):
    n = len(stakes)
    active_edges = sorted(set(visible_edges).difference(reports))
    partial_closure, rounds = closure(n, equivocators, active_edges)
    full_closure, full_rounds = closure(n, equivocators, candidate_edges)
    adversaries = set(adversaries)
    honest_slashed = sorted(set(partial_closure).difference(adversaries))
    gap = sorted(set(full_closure).difference(partial_closure))
    total_stake = stake_sum(stakes, range(n))
    slashed_stake = stake_sum(stakes, partial_closure)
    direct_stake = stake_sum(stakes, equivocators)
    honest_slashed_stake = stake_sum(stakes, honest_slashed)
    active_count = Integer(n) - Integer(len(partial_closure))
    active_stake = total_stake - slashed_stake
    fault = fault_bound(n)
    stake_fault = stake_fault_bound(total_stake)
    delay = Integer(max(len(rounds) - 1, 0))
    denominator = direct_stake if direct_stake > 0 else Integer(1)
    return {
        "model": "sage_adversarial_timing_game",
        "n": n,
        "stakes": [int(value) for value in stakes],
        "adversaries": sorted(adversaries),
        "equivocators": sorted(equivocators),
        "candidate_edges": [list(edge) for edge in sorted(candidate_edges)],
        "visible_edges": [list(edge) for edge in sorted(visible_edges)],
        "reports": [list(edge) for edge in sorted(reports)],
        "active_edges": [list(edge) for edge in active_edges],
        "partial_closure": partial_closure,
        "full_closure": full_closure,
        "rounds": rounds,
        "full_rounds": full_rounds,
        "honest_slashed": honest_slashed,
        "gap": gap,
        "fault_bound": int(fault),
        "stake_fault_bound": int(stake_fault),
        "active_count": int(active_count),
        "active_stake": int(active_stake),
        "total_stake": int(total_stake),
        "direct_adversarial_stake": int(direct_stake),
        "slashed_stake": int(slashed_stake),
        "honest_slashed_stake": int(honest_slashed_stake),
        "quorum_drop_count": active_count < quorum_bound(n),
        "quorum_drop_stake": active_stake < total_stake - stake_fault,
        "delay": int(delay),
        "damage_ratio": str(QQ(honest_slashed_stake) / QQ(denominator)),
    }


def better(score, current):
    return current is None or score > current[0]


def search(max_n, max_stake, edge_limit, report_limit, max_adversaries, max_models):
    checked = 0
    best = {
        "honest_slashed_stake": None,
        "quorum_drop": None,
        "accountability_gap": None,
        "slash_delay": None,
        "damage_ratio": None,
    }
    start_n = 4 if max_n >= 4 else 2
    for n in range(start_n, max_n + 1):
        validators = list(range(n))
        for stakes in stake_vectors(n, max_stake):
            for adversaries in bounded_subsets(validators, max_adversaries):
                adversaries = set(adversaries)
                if not adversaries:
                    continue
                for equivocators in Subsets(sorted(adversaries)):
                    equivocators = set(equivocators)
                    if not equivocators:
                        continue
                    for candidate_edges in edge_sets(n, edge_limit):
                        candidate_edges = [tuple(edge) for edge in candidate_edges]
                        for visible_indexes in Subsets(range(len(candidate_edges))):
                            visible_edges = [candidate_edges[i] for i in sorted(visible_indexes)]
                            for reports in bounded_subsets(visible_edges, report_limit):
                                checked += 1
                                if checked > max_models:
                                    raise SystemExit("refusing more than {} models".format(max_models))
                                record = analyze_case(stakes, adversaries, equivocators, candidate_edges, visible_edges, reports)
                                scores = {
                                    "honest_slashed_stake": (
                                        Integer(record["honest_slashed_stake"]),
                                        Integer(len(record["honest_slashed"])),
                                        -Integer(len(record["candidate_edges"])),
                                    ),
                                    "quorum_drop": (
                                        Integer(1 if record["quorum_drop_count"] or record["quorum_drop_stake"] else 0),
                                        Integer(record["slashed_stake"]),
                                        Integer(len(record["partial_closure"])),
                                    ),
                                    "accountability_gap": (
                                        Integer(len(record["gap"])),
                                        stake_sum(stakes, record["gap"]),
                                        -Integer(len(record["visible_edges"])),
                                    ),
                                    "slash_delay": (
                                        Integer(record["delay"]),
                                        Integer(len(record["partial_closure"])),
                                        -Integer(len(record["candidate_edges"])),
                                    ),
                                    "damage_ratio": (
                                        QQ(record["damage_ratio"]),
                                        Integer(record["honest_slashed_stake"]),
                                        -Integer(record["direct_adversarial_stake"]),
                                    ),
                                }
                                for name, score in scores.items():
                                    if better(score, best[name]):
                                        best[name] = (score, record)
    objectives = {name: value[1] for name, value in best.items() if value is not None}
    return {"summaries": [{"checked": checked, "max_n": max_n}], "objectives": objectives}


def self_test():
    result = search(4, 2, 1, 1, 1, 500000)
    if result["objectives"]["honest_slashed_stake"]["honest_slashed_stake"] < 1:
        raise AssertionError("no honest-slashed witness found")
    if not result["objectives"]["quorum_drop"]["quorum_drop_count"]:
        raise AssertionError("no count quorum-drop witness found")
    if not result["objectives"]["accountability_gap"]["gap"]:
        raise AssertionError("no accountability-gap witness found")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("checked={checked} max_n={max_n}".format(**summary))
    for name in sorted(result["objectives"]):
        record = result["objectives"][name]
        print(
            "objective={name} n={n} stakes={stakes} adversaries={adversaries} equivocators={equivocators} active_edges={active_edges} closure={partial_closure} honest={honest_slashed} gap={gap} quorum_drop_count={quorum_drop_count} quorum_drop_stake={quorum_drop_stake} delay={delay} ratio={damage_ratio}".format(
                name=name,
                **record
            )
        )


def main(argv):
    parser = argparse.ArgumentParser(description="Sage adversarial timing game for slashing objectives")
    parser.add_argument("--max-n", type=int, default=4)
    parser.add_argument("--max-stake", type=int, default=3)
    parser.add_argument("--edge-limit", type=int, default=2)
    parser.add_argument("--report-limit", type=int, default=1)
    parser.add_argument("--max-adversaries", type=int, default=2)
    parser.add_argument("--max-models", type=int, default=1_000_000)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else search(args.max_n, args.max_stake, args.edge_limit, args.report_limit, args.max_adversaries, args.max_models)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
