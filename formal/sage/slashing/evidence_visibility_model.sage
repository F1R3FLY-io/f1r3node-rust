import argparse
import json
import sys

from sage.all import DiGraph, Subsets


def possible_pairs(n):
    return [(viewer, offender) for viewer in range(n) for offender in range(n) if viewer != offender]


def pair_sets(n, limit):
    pairs = possible_pairs(n)
    limit = min(limit, len(pairs))
    for size in range(limit + 1):
        for subset in Subsets(range(len(pairs)), size):
            yield [pairs[i] for i in sorted(subset)]


def closure(n, equivocators, neglect_edges):
    graph = DiGraph([list(range(n)), list(neglect_edges)], format="vertices_and_edges")
    slashed = set(equivocators)
    while True:
        next_slashed = set(slashed)
        for validator in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(validator))
        if next_slashed == slashed:
            return sorted(slashed)
        slashed = next_slashed


def neglect_edges_from_visibility(visibility, reports):
    return sorted(set(visibility).difference(reports))


def full_visibility(n, equivocators):
    return [(viewer, offender) for viewer in range(n) for offender in equivocators if viewer != offender]


def analyze_case(n, equivocators, visibility, reports):
    partial_edges = neglect_edges_from_visibility(visibility, reports)
    full_edges = neglect_edges_from_visibility(full_visibility(n, equivocators), reports)
    partial = closure(n, equivocators, partial_edges)
    full = closure(n, equivocators, full_edges)
    return {
        "model": "sage_evidence_visibility_withholding",
        "n": n,
        "equivocators": sorted(equivocators),
        "visibility": [list(edge) for edge in sorted(visibility)],
        "reports": [list(edge) for edge in sorted(reports)],
        "partial_neglect_edges": [list(edge) for edge in partial_edges],
        "full_visibility_neglect_edges": [list(edge) for edge in full_edges],
        "partial_closure": partial,
        "full_visibility_closure": full,
        "withholding_gap": sorted(set(full).difference(partial)),
        "property": "partial_visibility_matches_full_visibility_accountability",
        "holds": partial == full,
    }


def find_withholding_gap(n, visibility_limit, report_limit, max_models):
    checked = 0
    best = None
    for equivocators in Subsets(range(n)):
        equivocators = set(equivocators)
        if not equivocators:
            continue
        for visibility in pair_sets(n, visibility_limit):
            visibility = set(visibility)
            for reports in pair_sets(n, report_limit):
                checked += 1
                if checked > max_models:
                    raise SystemExit("refusing more than {} models".format(max_models))
                reports = set(reports).intersection(visibility)
                record = analyze_case(n, equivocators, visibility, reports)
                score = (len(record["withholding_gap"]), -len(record["visibility"]), -len(record["reports"]))
                if record["withholding_gap"] and (best is None or score > best[0]):
                    best = (score, record)
    return {"summaries": [{"checked": checked, "n": n}], "witnesses": [] if best is None else [best[1]]}


def self_test():
    result = find_withholding_gap(4, 1, 0, 1_000_000)
    if not result["witnesses"]:
        raise AssertionError("withholding gap not found")
    if len(result["witnesses"][0]["withholding_gap"]) < 1:
        raise AssertionError("withholding witness has no gap")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("checked={checked} n={n}".format(**summary))
    if result["witnesses"]:
        first = result["witnesses"][0]
        print(
            "best n={n} equivocators={equivocators} visibility={visibility} reports={reports} partial={partial_closure} full={full_visibility_closure} gap={withholding_gap}".format(
                **first
            )
        )


def main(argv):
    parser = argparse.ArgumentParser(description="Sage model for evidence visibility and withholding")
    parser.add_argument("--n", type=int, default=4)
    parser.add_argument("--visibility-limit", type=int, default=2)
    parser.add_argument("--report-limit", type=int, default=1)
    parser.add_argument("--max-models", type=int, default=1_000_000)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    result = self_test() if args.self_test else find_withholding_gap(args.n, args.visibility_limit, args.report_limit, args.max_models)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
