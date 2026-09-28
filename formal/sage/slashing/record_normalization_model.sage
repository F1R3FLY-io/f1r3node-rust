import argparse
import json
import sys

from sage.all import Permutations, Set


def normalize(records):
    normalized = {}
    for key, hashes in records:
        current = normalized.setdefault(tuple(key), Set([]))
        normalized[tuple(key)] = current.union(Set(hashes))
    return {str(key): sorted(value) for key, value in sorted(normalized.items())}


def canonical_slash_candidates(evidence):
    selected = {}
    for validator, epoch, invalid_hash in evidence:
        key = (validator, epoch)
        selected[key] = min(invalid_hash, selected.get(key, invalid_hash))
    return sorted((validator, epoch, invalid_hash) for (validator, epoch), invalid_hash in selected.items())


def analyze():
    records = [((0, 1), ["h1", "h2"]), ((0, 1), ["h2", "h3"]), ((1, 2), ["h4"])]
    expected = None
    failures = []
    checked = 0
    for order in Permutations(range(len(records))):
        checked += 1
        candidate = normalize([records[index] for index in order])
        if expected is None:
            expected = candidate
        elif candidate != expected:
            failures.append({"order": list(order), "candidate": candidate, "expected": expected})
            break
    duplicate = normalize([((0, 1), ["h1", "h1", "h2"])])
    slash_evidence = [("v1", 0, "h3"), ("v1", 0, "h1"), ("v1", 0, "h3"), ("v2", 0, "h2")]
    selected = canonical_slash_candidates(slash_evidence)
    target_keys = [(validator, epoch) for validator, epoch, _ in selected]
    return {
        "summaries": [{
            "checked": checked,
            "failures": len(failures),
            "duplicate_idempotent": duplicate == {"(0, 1)": ["h1", "h2"]},
            "slash_target_unique": len(target_keys) == len(set(target_keys)),
        }],
        "failures": failures,
        "normalized": expected,
        "canonical_slash_candidates": selected,
    }


def self_test():
    result = analyze()
    if result["failures"]:
        raise AssertionError("record normalization depended on insertion order")
    if not result["summaries"][0]["duplicate_idempotent"]:
        raise AssertionError("duplicate hashes changed normalized meaning")
    if not result["summaries"][0]["slash_target_unique"]:
        raise AssertionError("slash target normalization changed")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("checked={checked} failures={failures} duplicate_idempotent={duplicate_idempotent} slash_target_unique={slash_target_unique}".format(**summary))
    print("normalized={}".format(result["normalized"]))
    print("canonical_slash_candidates={}".format(result["canonical_slash_candidates"]))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage model for record normalization modulo order and duplicates")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else analyze()
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
