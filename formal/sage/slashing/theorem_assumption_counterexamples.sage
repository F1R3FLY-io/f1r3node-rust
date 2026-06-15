import argparse
import json
import sys

from sage.all import Integer, Set


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def counterexamples():
    return [
        {
            "model": "sage_assumption_counterexample_closure_bound",
            "dropped_assumption": "length closure <= F",
            "universe": [0, 1, 2, 3],
            "F": 1,
            "closure": [0, 1],
            "active_count": 2,
            "required_active_count": 3,
            "failure": "active_count < n - F",
        },
        {
            "model": "sage_assumption_counterexample_quorum_strictness",
            "dropped_assumption": "length active < 2 * Q",
            "active": [0, 1, 2, 3],
            "Q": 2,
            "q1": [0, 1],
            "q2": [2, 3],
            "intersection": [],
            "failure": "disjoint quorums exist when length active = 2 * Q",
        },
        {
            "model": "sage_assumption_counterexample_quorum_nodup",
            "dropped_assumption": "NoDup q1 and NoDup q2",
            "active": [0, 1],
            "Q": 2,
            "q1": [0, 0],
            "q2": [1, 1],
            "intersection": [],
            "failure": "duplicate votes inflate apparent quorum size",
        },
        {
            "model": "sage_assumption_counterexample_current_filter",
            "dropped_assumption": "direct offenders and neglect edges are current-validator filtered",
            "current": [0, 1, 2],
            "evidence_domain": [0, 1, 2, 3],
            "direct": [3],
            "edge": [0, 3],
            "filtered_closure": [],
            "unfiltered_current_projection": [0],
            "failure": "stale evidence slashes a current validator",
        },
        {
            "model": "sage_assumption_counterexample_report_suppression",
            "dropped_assumption": "reported evidence is removed from neglect edges",
            "direct": [0],
            "visible": [[1, 0]],
            "reported": [[1, 0]],
            "with_suppression": [0],
            "without_suppression": [0, 1],
            "failure": "validator is slashed despite having reported the offender",
        },
        {
            "model": "sage_assumption_counterexample_s0_subset_universe",
            "dropped_assumption": "incl s0 universe",
            "universe": [0, 1, 2],
            "s0": [3],
            "closure": [3],
            "failure": "closure contains a non-current validator",
        },
        {
            "model": "sage_assumption_counterexample_arithmetic_envelope",
            "dropped_assumption": "vault + |V| * maxStake <= limit",
            "limit": int(Integer(2) ** Integer(64) - Integer(1)),
            "vault": 0,
            "validator_count": 2,
            "max_stake": int(Integer(2) ** Integer(63)),
            "worst_sum": int(Integer(2) ** Integer(64)),
            "failure": "exact accounting exceeds u64 max by one",
        },
        {
            "model": "sage_assumption_counterexample_weighted_disjoint_bound",
            "dropped_assumption": "disjoint quorum stake sum <= active stake",
            "active_stake": 10,
            "q1_stake": 7,
            "q2_stake": 7,
            "q1": ["synthetic-heavy-copy-1"],
            "q2": ["synthetic-heavy-copy-2"],
            "intersection": [],
            "failure": "without a real disjoint-stake bound, arithmetic alone does not imply shared validator identity",
        },
    ]


def analyze():
    witnesses = counterexamples()
    names = Set([witness["model"] for witness in witnesses])
    return {"summaries": [{"witnesses": len(witnesses), "distinct": len(names)}], "witnesses": witnesses}


def self_test():
    result = analyze()
    if result["summaries"][0]["witnesses"] < 8:
        raise AssertionError("assumption counterexample set is incomplete")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("witnesses={witnesses} distinct={distinct}".format(**summary))
    for witness in result["witnesses"]:
        print("counterexample={model} dropped={dropped_assumption}".format(**witness))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage catalog of minimal counterexamples when theorem assumptions are removed")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else analyze()
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
