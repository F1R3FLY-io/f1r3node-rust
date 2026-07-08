import argparse
import json
import sys

from sage.all import Integer, Permutations, Set, ZZ, vector


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def aborting_batch(bonds, order, failures):
    bonds = vector(ZZ, [Integer(value) for value in bonds])
    vault = Integer(0)
    slashed = Set([])
    for validator in order:
        if validator in failures:
            return {"bonds": [int(value) for value in bonds], "vault": int(vault), "slashed": [int(v) for v in sorted(slashed)], "failed_at": int(validator)}
        vault += bonds[validator]
        bonds[validator] = Integer(0)
        slashed = slashed.union(Set([validator]))
    return {"bonds": [int(value) for value in bonds], "vault": int(vault), "slashed": [int(v) for v in sorted(slashed)], "failed_at": None}


def partial_failure_order_risk():
    bonds = [5, 7]
    slash_set = [0, 1]
    failures = Set([1])
    outcomes = []
    for order in Permutations(slash_set):
        outcomes.append({"order": list(order), "outcome": aborting_batch(bonds, list(order), failures)})
    distinct = Set([json.dumps(outcome["outcome"], sort_keys=True) for outcome in outcomes])
    return {
        "model": "sage_projection_partial_failure_order_risk",
        "bonds": bonds,
        "failures": [int(v) for v in sorted(failures)],
        "outcomes": outcomes,
        "order_dependent": len(distinct) > 1,
    }


def serialization_collision_risk():
    keys = [(1, 23), (12, 3)]
    naive = ["{}{}".format(v, seq) for v, seq in keys]
    canonical = ["{}:{}".format(v, seq) for v, seq in keys]
    return {
        "model": "sage_projection_serialization_collision_risk",
        "keys": [list(key) for key in keys],
        "naive": naive,
        "canonical": canonical,
        "naive_collision": len(Set(naive)) < len(naive),
        "canonical_collision": len(Set(canonical)) < len(canonical),
    }


def pruning_risk():
    retained = [0, 1]
    pruned = []
    return {
        "model": "sage_projection_evidence_pruning_risk",
        "direct": [1],
        "edge": [0, 1],
        "retained_closure": retained,
        "pruned_closure": pruned,
        "slashability_lost": retained != pruned,
    }


def arithmetic_projection_risk():
    high = Integer(2) ** Integer(64) - Integer(1)
    exact = high + Integer(1)
    return {
        "model": "sage_projection_arithmetic_overflow_risk",
        "bits": 64,
        "max": int(high),
        "exact_sum": int(exact),
        "checked_ok": False,
        "wrapping_value": 0,
        "saturating_value": int(high),
    }


def duplicate_record_projection_risk():
    records = [((0, 1), "h1"), ((0, 1), "h1"), ((0, 1), "h2")]
    normalized = {"(0, 1)": sorted(Set(["h1", "h2"]))}
    return {
        "model": "sage_projection_duplicate_record_risk",
        "records": [{"key": list(key), "hash": block_hash} for key, block_hash in records],
        "raw_record_count": len(records),
        "normalized": normalized,
        "duplicate_count": len(records) - sum(len(values) for values in normalized.values()),
        "state_equivalent_after_normalization": True,
    }


def analyze():
    risks = [
        partial_failure_order_risk(),
        serialization_collision_risk(),
        pruning_risk(),
        arithmetic_projection_risk(),
        duplicate_record_projection_risk(),
    ]
    return {"summaries": [{"risks": len(risks)}], "risks": risks}


def self_test():
    result = analyze()
    by_model = {risk["model"]: risk for risk in result["risks"]}
    if not by_model["sage_projection_partial_failure_order_risk"]["order_dependent"]:
        raise AssertionError("partial failure order risk missing")
    if not by_model["sage_projection_serialization_collision_risk"]["naive_collision"]:
        raise AssertionError("serialization collision risk missing")
    if not by_model["sage_projection_evidence_pruning_risk"]["slashability_lost"]:
        raise AssertionError("pruning risk missing")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("risks={risks}".format(**summary))
    for risk in result["risks"]:
        print("risk={model}".format(**risk))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage model for implementation projection risks")
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
