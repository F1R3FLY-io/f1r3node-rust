import argparse
import json
import sys

from sage.all import Permutations, Set, vector, ZZ, Integer


def execute_order(bonds, order):
    bonds = vector(ZZ, [Integer(value) for value in bonds])
    vault = Integer(0)
    slashed = Set([])
    for validator in order:
        if validator not in slashed:
            vault += bonds[validator]
            bonds[validator] = Integer(0)
            slashed = slashed.union(Set([validator]))
    return {"bonds": [int(value) for value in bonds], "vault": int(vault), "slashed": sorted(slashed)}


def analyze(bonds, slash_set):
    expected = None
    failures = []
    checked = 0
    for order in Permutations(list(slash_set)):
        checked += 1
        result = execute_order(bonds, order)
        if expected is None:
            expected = result
        elif result != expected:
            failures.append({"order": list(order), "result": result, "expected": expected})
            break
    return {"summaries": [{"validators": len(bonds), "slash_count": len(slash_set), "checked": checked, "failures": len(failures)}], "failures": failures, "expected": expected}


def self_test():
    result = analyze([5, 7, 11, 13], {0, 1, 2, 3})
    if result["failures"]:
        raise AssertionError("slash order changed final state")
    if result["expected"]["vault"] != 36:
        raise AssertionError("vault total changed")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("validators={validators} slash_count={slash_count} checked={checked} failures={failures}".format(**summary))
    print("expected={}".format(result["expected"]))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage model for batch slash order independence")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else analyze([5, 7, 11, 13], {0, 1, 2, 3})
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
