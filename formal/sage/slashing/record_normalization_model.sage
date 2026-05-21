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


def recoverable_rejected_slash_hashes(rejected, own_hashes):
    own = set(own_hashes)
    seen = set()
    out = []
    for invalid_hash, _issuer in sorted(rejected):
        if invalid_hash in own or invalid_hash in seen:
            continue
        seen.add(invalid_hash)
        out.append(invalid_hash)
    return out


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
    rejected = [("h3", "issuer_b"), ("h1", "issuer_a"), ("h3", "issuer_c"), ("h2", "issuer_d")]
    recovered = recoverable_rejected_slash_hashes(rejected, {"h1"})
    return {
        "summaries": [{
            "checked": checked,
            "failures": len(failures),
            "duplicate_idempotent": duplicate == {"(0, 1)": ["h1", "h2"]},
            "rejected_slash_recovery": recovered == ["h2", "h3"],
        }],
        "failures": failures,
        "normalized": expected,
        "recoverable_rejected_slashes": recovered,
    }


def self_test():
    result = analyze()
    if result["failures"]:
        raise AssertionError("record normalization depended on insertion order")
    if not result["summaries"][0]["duplicate_idempotent"]:
        raise AssertionError("duplicate hashes changed normalized meaning")
    if not result["summaries"][0]["rejected_slash_recovery"]:
        raise AssertionError("rejected slash recovery normalization changed")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("checked={checked} failures={failures} duplicate_idempotent={duplicate_idempotent} rejected_slash_recovery={rejected_slash_recovery}".format(**summary))
    print("normalized={}".format(result["normalized"]))
    print("recoverable_rejected_slashes={}".format(result["recoverable_rejected_slashes"]))


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
