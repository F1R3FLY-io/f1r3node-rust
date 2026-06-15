import argparse
import json
import sys

from sage.all import DiGraph, Set


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def closure(vertices, equivocators, edges):
    graph = DiGraph([list(vertices), list(edges)], format="vertices_and_edges")
    slashed = set(equivocators).intersection(vertices)
    while True:
        next_slashed = set(slashed)
        for offender in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(offender))
        next_slashed = next_slashed.intersection(vertices)
        if next_slashed == slashed:
            return sorted(slashed)
        slashed = next_slashed


def stale_direct_current_neglecter():
    current = Set([0, 1, 2])
    evidence = Set([0, 1, 2, 3])
    direct = Set([3])
    edges = [(0, 3)]
    filtered = closure(current, direct.intersection(current), [])
    legacy = sorted(set(closure(evidence, direct, edges)).intersection(set(current)))
    return {
        "model": "sage_epoch_churn_stale_direct_current_neglecter",
        "current": sorted(current),
        "evidence": sorted(evidence),
        "direct": sorted(direct),
        "edges": [list(edge) for edge in edges],
        "filtered_current_closure": filtered,
        "unfiltered_current_projection": legacy,
        "candidate_divergence": filtered != legacy,
    }


def loose_pubkey_rejoin():
    strict_current = Set(["A@1", "B@1"])
    strict_evidence = Set(["A@0", "B@1"])
    strict_direct = Set(["A@0"])
    strict_edges = [("B@1", "A@0")]
    loose_vertices = Set(["A", "B"])
    loose_direct = Set(["A"])
    loose_edges = [("B", "A")]
    strict_filtered = closure(strict_current, strict_direct.intersection(strict_current), [])
    strict_unfiltered = sorted(set(closure(strict_evidence.union(strict_current), strict_direct, strict_edges)).intersection(set(strict_current)))
    loose = closure(loose_vertices, loose_direct, loose_edges)
    return {
        "model": "sage_epoch_churn_loose_pubkey_rejoin",
        "strict_current": sorted(strict_current),
        "strict_evidence": sorted(strict_evidence),
        "strict_direct": sorted(strict_direct),
        "strict_edges": [list(edge) for edge in strict_edges],
        "strict_filtered_closure": strict_filtered,
        "strict_unfiltered_current_projection": strict_unfiltered,
        "loose_pubkey_closure": loose,
        "requires_identity_epoch_tag": "A" in loose and "A@1" not in strict_filtered,
    }


def pending_slash_rebond_policy():
    no_carry_current = Set(["A@1", "C@1"])
    carry_current = Set(["A@1", "C@1"])
    stale_direct = Set(["A@0"])
    carry_mapped_direct = Set(["A@1"])
    no_carry = closure(no_carry_current, stale_direct.intersection(no_carry_current), [])
    carry = closure(carry_current, carry_mapped_direct, [])
    return {
        "model": "sage_epoch_churn_pending_slash_rebond_policy",
        "stale_direct": sorted(stale_direct),
        "no_carry_current": sorted(no_carry_current),
        "no_carry_closure": no_carry,
        "carry_mapped_direct": sorted(carry_mapped_direct),
        "carry_closure": carry,
        "policy_boundary": no_carry != carry,
    }


def analyze():
    witnesses = [stale_direct_current_neglecter(), loose_pubkey_rejoin(), pending_slash_rebond_policy()]
    return {"summaries": [{"witnesses": len(witnesses)}], "witnesses": witnesses}


def self_test():
    result = analyze()
    if not result["witnesses"][0]["candidate_divergence"]:
        raise AssertionError("stale-current divergence missing")
    if not result["witnesses"][1]["requires_identity_epoch_tag"]:
        raise AssertionError("identity epoch-tag witness missing")
    if not result["witnesses"][2]["policy_boundary"]:
        raise AssertionError("rebond policy boundary missing")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("witnesses={witnesses}".format(**summary))
    for witness in result["witnesses"]:
        print("witness={model}".format(**witness))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage model for epoch churn and validator identity boundary attacks")
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
