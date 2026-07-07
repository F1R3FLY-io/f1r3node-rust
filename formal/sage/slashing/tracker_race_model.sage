import argparse
import json
import sys

from sage.all import DiGraph, Permutations, Set


def operations(thread_count, hash_count):
    if thread_count < 1:
        raise ValueError("thread_count must be positive")
    if hash_count < 1:
        raise ValueError("hash_count must be positive")
    return [{"op": op, "thread": op, "hash": "h{}".format(op % hash_count)} for op in range(thread_count)]


def prefixed_schedules(operation_count):
    events = [(op, "read") for op in range(operation_count)] + [(op, "write") for op in range(operation_count)]
    indices = list(range(len(events)))
    order_graph = DiGraph(
        [
            indices,
            [(op, operation_count + op) for op in range(operation_count)],
        ],
        format="vertices_and_edges",
    )
    for permutation in Permutations(indices):
        positions = {event_index: position for position, event_index in enumerate(permutation)}
        if all(positions[src] < positions[dst] for src, dst in order_graph.edges(sort=True, labels=False)):
            yield [events[index] for index in permutation]


def atomic_schedules(operation_count):
    for order in Permutations(range(operation_count)):
        yield [(op, "atomic") for op in order]


def run_pre_fix(schedule, ops):
    stored = Set([])
    snapshots = {}
    trace = []
    for op_index, step in schedule:
        op = ops[op_index]
        if step == "read":
            snapshots[op_index] = Set(stored)
        elif step == "write":
            stored = Set(snapshots[op_index])
            stored = stored.union(Set([op["hash"]]))
        else:
            raise ValueError(step)
        trace.append(
            {
                "op": op_index,
                "thread": op["thread"],
                "hash": op["hash"],
                "step": step,
                "stored": sorted(stored),
            }
        )
    return stored, trace


def run_atomic(schedule, ops):
    stored = Set([])
    trace = []
    for op_index, step in schedule:
        op = ops[op_index]
        if step != "atomic":
            raise ValueError(step)
        stored = stored.union(Set([op["hash"]]))
        trace.append(
            {
                "op": op_index,
                "thread": op["thread"],
                "hash": op["hash"],
                "step": step,
                "stored": sorted(stored),
            }
        )
    return stored, trace


def find_pre_fix_witness(thread_count, hash_count):
    ops = operations(thread_count, hash_count)
    expected = Set([op["hash"] for op in ops])
    for schedule in prefixed_schedules(len(ops)):
        stored, trace = run_pre_fix(schedule, ops)
        if stored != expected:
            return {
                "model": "sage_poset_equivocation_tracker_pre_fix",
                "operation_count": len(ops),
                "operations": ops,
                "schedule": [{"op": op, "step": step} for op, step in schedule],
                "trace": trace,
                "expected": sorted(expected),
                "final": sorted(stored),
                "lost": sorted(expected.difference(stored)),
                "property": "no_lost_hash_update",
                "holds": False,
                "sage_checks": {"schedule_linear_extensions": True},
            }
    return None


def worst_pre_fix_loss(thread_count, hash_count):
    ops = operations(thread_count, hash_count)
    expected = Set([op["hash"] for op in ops])
    best = None
    checked = 0
    for schedule in prefixed_schedules(len(ops)):
        stored, trace = run_pre_fix(schedule, ops)
        checked += 1
        lost = expected.difference(stored)
        score = (len(lost), -len(stored))
        if best is None or score > best[0]:
            best = (
                score,
                {
                    "model": "sage_poset_equivocation_tracker_worst_pre_fix_loss",
                    "operation_count": len(ops),
                    "operations": ops,
                    "schedule": [{"op": op, "step": step} for op, step in schedule],
                    "trace": trace,
                    "expected": sorted(expected),
                    "final": sorted(stored),
                    "lost": sorted(lost),
                    "lost_count": len(lost),
                    "property": "maximal_lost_hash_update",
                    "holds": len(lost) == 0,
                    "checked": checked,
                },
            )
    return None if best is None else best[1]


def check_atomic(thread_count, hash_count):
    ops = operations(thread_count, hash_count)
    expected = Set([op["hash"] for op in ops])
    checked = 0
    for schedule in atomic_schedules(len(ops)):
        stored, trace = run_atomic(schedule, ops)
        checked += 1
        if stored != expected:
            return {
                "model": "sage_permutation_equivocation_tracker_atomic",
                "operation_count": len(ops),
                "operations": ops,
                "schedule": [{"op": op, "step": step} for op, step in schedule],
                "trace": trace,
                "expected": sorted(expected),
                "final": sorted(stored),
                "lost": sorted(expected.difference(stored)),
                "property": "no_lost_hash_update",
                "holds": False,
                "checked": checked,
            }
    return {
        "model": "sage_permutation_equivocation_tracker_atomic",
        "operation_count": len(ops),
        "operations": ops,
        "property": "no_lost_hash_update",
        "holds": True,
        "checked": checked,
    }


def self_test():
    witness = find_pre_fix_witness(2, 2)
    if witness is None:
        raise AssertionError("pre-fix tracker race witness was not found")
    if witness["lost"] == []:
        raise AssertionError("pre-fix witness did not lose an update")
    atomic = check_atomic(4, 4)
    if not atomic["holds"]:
        raise AssertionError("atomic tracker model lost an update")
    worst = worst_pre_fix_loss(3, 3)
    if worst["lost_count"] < 2:
        raise AssertionError("worst pre-fix loss did not find multi-loss schedule")
    return {"pre_fix_witness": witness, "atomic": atomic}


def print_summary(result):
    witness = result.get("pre_fix_witness")
    atomic = result.get("atomic")
    if witness is None:
        print("pre_fix no lost-update witness found")
    else:
        print(
            "pre_fix witness operations={operation_count} expected={expected} final={final} lost={lost}".format(
                **witness
            )
        )
    if atomic is not None:
        print("atomic holds={holds} operations={operation_count} checked={checked}".format(**atomic))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage schedule model for equivocation tracker races")
    parser.add_argument("--threads", type=int, default=2)
    parser.add_argument("--hashes", type=int, default=2)
    parser.add_argument("--worst-loss", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    if args.threads < 1:
        parser.error("--threads must be positive")
    if args.hashes < 1:
        parser.error("--hashes must be positive")

    if args.self_test:
        result = self_test()
    elif args.worst_loss:
        result = {"pre_fix_witness": worst_pre_fix_loss(args.threads, args.hashes), "atomic": check_atomic(args.threads, args.hashes)}
    else:
        result = {"pre_fix_witness": find_pre_fix_witness(args.threads, args.hashes), "atomic": check_atomic(args.threads, args.hashes)}
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
