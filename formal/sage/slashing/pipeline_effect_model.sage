import argparse
import json
import sys

from sage.all import Integer, Subsets, ZZ, cartesian_product, vector


def apply_slash(bonds, vault, already_slashed, slash_set):
    next_bonds = list(bonds)
    next_vault = Integer(vault)
    next_slashed = set(already_slashed)
    transferred = Integer(0)
    for validator in sorted(slash_set):
        if validator not in next_slashed:
            amount = Integer(next_bonds[validator])
            transferred += amount
            next_vault += amount
            next_bonds[validator] = Integer(0)
            next_slashed.add(validator)
    return vector(ZZ, next_bonds), next_vault, next_slashed, transferred


def check_case(bonds, slash_set):
    initial_vault = Integer(0)
    initial_slashed = set()
    after_bonds, after_vault, after_slashed, transferred = apply_slash(
        bonds, initial_vault, initial_slashed, slash_set
    )
    again_bonds, again_vault, again_slashed, transferred_again = apply_slash(
        after_bonds, after_vault, after_slashed, slash_set
    )
    total_before = Integer(sum(bonds)) + initial_vault
    total_after = Integer(sum(after_bonds)) + after_vault
    if total_before != total_after:
        return "total_accounting"
    if any(amount < 0 for amount in after_bonds):
        return "negative_bond"
    if any(after_bonds[validator] != 0 for validator in slash_set):
        return "slashed_bond_not_zero"
    if any(after_bonds[validator] != bonds[validator] for validator in range(len(bonds)) if validator not in slash_set):
        return "unslashed_bond_changed"
    if after_bonds != again_bonds or after_vault != again_vault or after_slashed != again_slashed:
        return "idempotence"
    if transferred_again != 0:
        return "second_slash_transferred"
    if transferred != sum(bonds[validator] for validator in slash_set):
        return "transfer_amount"
    return None


def bond_vectors(n, max_bond):
    ranges = [range(max_bond + 1) for _ in range(n)]
    for values in cartesian_product(ranges):
        yield vector(ZZ, [Integer(value) for value in values])


def run_analysis(max_validators, max_bond, max_failures):
    summaries = []
    failures = []
    for n in range(1, max_validators + 1):
        summary = {"n": n, "cases": 0, "failures": 0}
        for bonds in bond_vectors(n, max_bond):
            for slash_set in Subsets(range(n)):
                slash_set = set(slash_set)
                summary["cases"] += 1
                failure = check_case(bonds, slash_set)
                if failure is not None:
                    summary["failures"] += 1
                    if len(failures) < max_failures:
                        failures.append(
                            {
                                "model": "sage_vector_slash_pipeline_effect",
                                "n": n,
                                "bonds": [int(value) for value in bonds],
                                "slash_set": sorted(slash_set),
                                "property": failure,
                                "holds": False,
                            }
                        )
        summaries.append(summary)
    return {"summaries": summaries, "failures": failures}


def overflow_boundary(word_bits):
    if word_bits < 2:
        raise ValueError("word_bits must be at least 2")
    max_signed = (Integer(2) ** Integer(word_bits - 1)) - Integer(1)
    min_overflow = {
        "model": "sage_exact_arithmetic_fixed_width_boundary",
        "word_bits": word_bits,
        "signed_max": int(max_signed),
        "vault": int(max_signed),
        "bond": 1,
        "exact_sum": int(max_signed + 1),
        "property": "fixed_width_projection_safe",
        "holds": False,
        "interpretation": "exact Sage arithmetic is safe; any fixed-width signed projection needs an explicit bound or checked arithmetic",
    }
    safe_boundary = {
        "vault": int(max_signed - 1),
        "bond": 1,
        "exact_sum": int(max_signed),
        "holds": True,
    }
    return {"summaries": [{"word_bits": word_bits, "signed_max": int(max_signed)}], "failures": [min_overflow], "safe_boundary": safe_boundary}


def self_test():
    result = run_analysis(5, 3, 4)
    if result["failures"]:
        raise AssertionError("slash effect invariant failed")
    overflow = overflow_boundary(64)
    if overflow["failures"][0]["exact_sum"] != 2 ** 63:
        raise AssertionError("overflow boundary changed")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        if "n" in summary:
            print("n={n} cases={cases} failures={failures}".format(**summary))
        else:
            print("word_bits={word_bits} signed_max={signed_max}".format(**summary))
    if result["failures"]:
        first = result["failures"][0]
        if "n" in first:
            print("first_failure n={n} bonds={bonds} slash_set={slash_set} property={property}".format(**first))
        else:
            print("first_failure model={model} property={property}".format(**first))
    if "safe_boundary" in result:
        first = result["failures"][0]
        print(
            "overflow_boundary word_bits={word_bits} signed_max={signed_max} vault={vault} bond={bond} exact_sum={exact_sum}".format(
                **first
            )
        )


def main(argv):
    parser = argparse.ArgumentParser(description="Sage vector model for slashing pipeline effects")
    parser.add_argument("--max-validators", type=int, default=5)
    parser.add_argument("--max-bond", type=int, default=3)
    parser.add_argument("--max-failures", type=int, default=8)
    parser.add_argument("--overflow-boundary", action="store_true")
    parser.add_argument("--word-bits", type=int, default=64)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    if args.max_validators < 1:
        parser.error("--max-validators must be positive")
    if args.max_bond < 0:
        parser.error("--max-bond must be non-negative")
    if args.max_failures < 0:
        parser.error("--max-failures must be non-negative")

    if args.word_bits < 2:
        parser.error("--word-bits must be at least 2")

    if args.self_test:
        result = self_test()
    elif args.overflow_boundary:
        result = overflow_boundary(args.word_bits)
    else:
        result = run_analysis(args.max_validators, args.max_bond, args.max_failures)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
