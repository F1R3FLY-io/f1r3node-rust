import argparse
import json
import sys

from sage.all import Integer


def envelope(max_validators, max_bond, initial_vault, bits, signed):
    max_value = (Integer(2) ** Integer(bits - 1) - 1) if signed else (Integer(2) ** Integer(bits) - 1)
    worst_sum = Integer(initial_vault) + Integer(max_validators) * Integer(max_bond)
    return {
        "model": "sage_arithmetic_safe_envelope",
        "max_validators": max_validators,
        "max_bond": max_bond,
        "initial_vault": int(initial_vault),
        "bits": bits,
        "signed": signed,
        "max_value": int(max_value),
        "worst_sum": int(worst_sum),
        "safe": worst_sum <= max_value,
        "max_safe_bond": int((max_value - Integer(initial_vault)) // Integer(max_validators)) if max_validators > 0 and initial_vault <= max_value else None,
    }


def self_test():
    result = envelope(100, 10, 0, 64, False)
    if not result["safe"]:
        raise AssertionError("small envelope unexpectedly unsafe")
    unsafe = envelope(2, 2 ** 63, 0, 64, False)
    if unsafe["safe"]:
        raise AssertionError("overflow envelope unexpectedly safe")
    return {"summaries": [result, unsafe]}


def print_summary(result):
    for summary in result["summaries"]:
        print("validators={max_validators} max_bond={max_bond} vault={initial_vault} bits={bits} signed={signed} worst_sum={worst_sum} max_value={max_value} safe={safe} max_safe_bond={max_safe_bond}".format(**summary))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage arithmetic safe-envelope model")
    parser.add_argument("--max-validators", type=int, default=100)
    parser.add_argument("--max-bond", type=int, default=10)
    parser.add_argument("--initial-vault", type=int, default=0)
    parser.add_argument("--bits", type=int, default=64)
    parser.add_argument("--signed", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else {"summaries": [envelope(args.max_validators, args.max_bond, args.initial_vault, args.bits, args.signed)]}
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
