import argparse
import json
import sys

from sage.all import DiGraph, Integer, Subsets


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def possible_edges(n):
    return [(src, dst) for src in range(n) for dst in range(n) if src != dst]


def closure(n, equivocators, edges):
    graph = DiGraph([list(range(n)), list(edges)], format="vertices_and_edges")
    slashed = set(equivocators)
    while True:
        next_slashed = set(slashed)
        for offender in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(offender))
        if next_slashed == slashed:
            return sorted(slashed)
        slashed = next_slashed


def edge_sets(n, edge_limit):
    edges = possible_edges(n)
    for size in range(1, min(edge_limit, len(edges)) + 1):
        for subset in Subsets(range(len(edges)), size):
            yield [edges[i] for i in sorted(subset)]


def active_at(edges, visibility_time, report_time, time):
    active = []
    for edge in edges:
        vt = visibility_time[edge]
        rt = report_time.get(edge)
        if vt <= time and (rt is None or time < rt):
            active.append(edge)
    return active


def find_view_divergence(n, edge_limit):
    equivocators = {0}
    for edges in edge_sets(n, edge_limit):
        for a_visible_indexes in Subsets(range(len(edges))):
            a_visible = [edges[i] for i in sorted(a_visible_indexes)]
            for b_visible_indexes in Subsets(range(len(edges))):
                b_visible = [edges[i] for i in sorted(b_visible_indexes)]
                a_closure = closure(n, equivocators, a_visible)
                b_closure = closure(n, equivocators, b_visible)
                if a_closure != b_closure:
                    return {
                        "model": "sage_evidence_timing_view_divergence",
                        "n": n,
                        "equivocators": sorted(equivocators),
                        "edges": [list(edge) for edge in edges],
                        "view_a_edges": [list(edge) for edge in a_visible],
                        "view_b_edges": [list(edge) for edge in b_visible],
                        "view_a_closure": a_closure,
                        "view_b_closure": b_closure,
                        "only_a": sorted(set(a_closure).difference(b_closure)),
                        "only_b": sorted(set(b_closure).difference(a_closure)),
                    }
    return None


def find_report_nonmonotone(n, horizon):
    equivocators = {0}
    for edge in possible_edges(n):
        if edge[1] != 0:
            continue
        for visible_time in range(horizon):
            for report_time in range(visible_time + 1, horizon + 1):
                visibility = {edge: Integer(visible_time)}
                reports = {edge: Integer(report_time)}
                records = []
                previous = set()
                for time in range(horizon + 1):
                    active_edges = active_at([edge], visibility, reports, time)
                    current = set(closure(n, equivocators, active_edges))
                    records.append({"time": time, "active_edges": [list(e) for e in active_edges], "closure": sorted(current), "monotone": previous.issubset(current)})
                    if not previous.issubset(current):
                        return {
                            "model": "sage_evidence_timing_report_nonmonotone",
                            "n": n,
                            "equivocators": sorted(equivocators),
                            "edge": list(edge),
                            "visible_time": int(visible_time),
                            "report_time": int(report_time),
                            "records": records,
                        }
                    previous = current
    return None


def find_delay(n, horizon):
    equivocators = {0}
    edge = (1, 0)
    visibility = {edge: Integer(horizon)}
    reports = {edge: None}
    first_extra = None
    records = []
    for time in range(horizon + 1):
        active_edges = active_at([edge], visibility, reports, time)
        current = closure(n, equivocators, active_edges)
        if 1 in current and first_extra is None:
            first_extra = time
        records.append({"time": time, "active_edges": [list(e) for e in active_edges], "closure": current})
    return {
        "model": "sage_evidence_timing_slash_delay",
        "n": n,
        "equivocators": sorted(equivocators),
        "edge": list(edge),
        "first_extra_slash_time": first_extra,
        "records": records,
    }


def analyze(n, edge_limit, horizon):
    witnesses = []
    view = find_view_divergence(n, edge_limit)
    nonmonotone = find_report_nonmonotone(n, horizon)
    delay = find_delay(n, horizon)
    if view:
        witnesses.append(view)
    if nonmonotone:
        witnesses.append(nonmonotone)
    witnesses.append(delay)
    return {"summaries": [{"n": n, "witnesses": len(witnesses)}], "witnesses": witnesses}


def self_test():
    result = analyze(4, 1, 3)
    models = {witness["model"] for witness in result["witnesses"]}
    required = {
        "sage_evidence_timing_view_divergence",
        "sage_evidence_timing_report_nonmonotone",
        "sage_evidence_timing_slash_delay",
    }
    if not required.issubset(models):
        raise AssertionError("missing timing witnesses")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("n={n} witnesses={witnesses}".format(**summary))
    for witness in result["witnesses"]:
        print("witness={model}".format(**witness))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage search for evidence timing, view divergence, reports, and slash delay")
    parser.add_argument("--n", type=int, default=4)
    parser.add_argument("--edge-limit", type=int, default=1)
    parser.add_argument("--horizon", type=int, default=3)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else analyze(args.n, args.edge_limit, args.horizon)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
