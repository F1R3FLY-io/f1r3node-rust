import argparse
import json
import sys

from sage.all import Integer, Subsets, vector, ZZ


def stake_sum(stakes, validators):
    return Integer(sum(stakes[v] for v in validators))


def quorum_threshold(total):
    return (Integer(2) * Integer(total)) // Integer(3) + Integer(1)


def analyze(stakes):
    n = len(stakes)
    validators = set(range(n))
    total = stake_sum(stakes, validators)
    threshold = quorum_threshold(total)
    failures = []
    checked = 0
    quorums = []
    for subset in Subsets(range(n)):
        subset = set(subset)
        if stake_sum(stakes, subset) >= threshold:
            quorums.append(subset)
    for i, left in enumerate(quorums):
        for right in quorums[i:]:
            checked += 1
            if left.isdisjoint(right):
                failures.append(
                    {
                        "model": "sage_weighted_quorum_intersection",
                        "stakes": [int(value) for value in stakes],
                        "threshold": int(threshold),
                        "left": sorted(left),
                        "right": sorted(right),
                        "left_stake": int(stake_sum(stakes, left)),
                        "right_stake": int(stake_sum(stakes, right)),
                        "property": "weighted_quorums_intersect",
                        "holds": False,
                    }
                )
                return {"summaries": [{"n": n, "quorums": len(quorums), "checked": checked}], "failures": failures}
    return {"summaries": [{"n": n, "quorums": len(quorums), "checked": checked}], "failures": failures}


def stake_vectors(n, max_stake):
    from sage.all import cartesian_product

    for values in cartesian_product([range(1, max_stake + 1) for _ in range(n)]):
        yield vector(ZZ, [Integer(value) for value in values])


def search(max_n, max_stake, max_models):
    checked = 0
    summaries = []
    for n in range(1, max_n + 1):
        summary = {"n": n, "cases": 0, "failures": 0}
        for stakes in stake_vectors(n, max_stake):
            checked += 1
            if checked > max_models:
                raise SystemExit("refusing more than {} models".format(max_models))
            result = analyze(stakes)
            summary["cases"] += 1
            if result["failures"]:
                summary["failures"] += 1
                return {"summaries": summaries + [summary], "failures": result["failures"]}
        summaries.append(summary)
    return {"summaries": summaries, "failures": []}


def self_test():
    result = search(5, 3, 1_000_000)
    if result["failures"]:
        raise AssertionError("weighted quorum intersection failed")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("n={n} cases={cases} failures={failures}".format(**summary))
    if result["failures"]:
        first = result["failures"][0]
        print("first_failure stakes={stakes} threshold={threshold} left={left} right={right}".format(**first))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage weighted quorum intersection model")
    parser.add_argument("--max-n", type=int, default=5)
    parser.add_argument("--max-stake", type=int, default=3)
    parser.add_argument("--max-models", type=int, default=1_000_000)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else search(args.max_n, args.max_stake, args.max_models)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
