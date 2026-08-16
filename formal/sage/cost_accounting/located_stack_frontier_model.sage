import argparse
import itertools
import json
import sys


def transfer_stack(source, target, seen, event, cells):
    if event in seen or source < cells:
        return source, target, tuple(seen), False
    return source - cells, target + cells, tuple(sorted(seen + (event,))), True


def unsafe_idempotent_transfer(source, target, seen, event, cells):
    if event in seen:
        return source, target + cells, tuple(seen), True
    return transfer_stack(source, target, seen, event, cells)


def check_stack_transfers():
    traces = 0
    violations = []
    for source, target, cells, event, prior in itertools.product(
        range(9), range(5), range(1, 5), range(3), range(2)
    ):
        seen = (event,) if prior else ()
        result = transfer_stack(source, target, seen, event, cells)
        post_source, post_target, post_seen, accepted = result
        traces += 1
        if accepted:
            safe = (
                source >= cells
                and event not in seen
                and post_source == source - cells
                and post_target == target + cells
                and post_source + post_target == source + target
                and len(post_seen) == len(seen) + 1
            )
        else:
            safe = result[:3] == (source, target, seen)
        if not safe:
            violations.append(
                {
                    "source": source,
                    "target": target,
                    "cells": cells,
                    "event": event,
                    "seen": seen,
                    "result": result,
                }
            )

    unsafe_witness = None
    first = unsafe_idempotent_transfer(4, 0, (), 7, 2)
    second = unsafe_idempotent_transfer(first[0], first[1], first[2], 7, 2)
    if second[0] + second[1] != 4:
        unsafe_witness = {"first": first, "second": second}

    return {
        "traces": traces,
        "violations": violations,
        "duplicate_idempotence_counterexample": unsafe_witness,
        "passed": not violations and unsafe_witness is not None,
    }


def expand_frontier(backing, fee, demand):
    known = 1
    capacity = sum(backing[:known]) - fee
    retries = 0
    speculative_effects = 0
    observed = [capacity]
    while demand > capacity and known < len(backing):
        speculative_effects = 0
        known += 1
        capacity = sum(backing[:known]) - fee
        observed.append(capacity)
        retries += 1
    return {
        "accepted": demand <= capacity,
        "known": known,
        "capacity": capacity,
        "retries": retries,
        "speculative_effects": speculative_effects,
        "observed": observed,
        "replay_capacity": capacity,
    }


def check_frontier_expansion():
    traces = 0
    violations = []
    fixed_cap_witness = None
    unbacked_witness = None
    for length in range(2, 5):
        for backing in itertools.product(range(1, 5), repeat=length):
            for fee in range(backing[0]):
                total_capacity = sum(backing) - fee
                initial_capacity = backing[0] - fee
                for demand in range(total_capacity + 2):
                    result = expand_frontier(backing, fee, demand)
                    traces += 1
                    observed = result["observed"]
                    safe = (
                        result["capacity"] == sum(backing[: result["known"]]) - fee
                        and result["capacity"] <= total_capacity
                        and result["retries"] <= length - 1
                        and all(left < right for left, right in zip(observed, observed[1:]))
                        and result["speculative_effects"] == 0
                        and result["replay_capacity"] == result["capacity"]
                        and result["accepted"] == (demand <= total_capacity)
                    )
                    if not safe:
                        violations.append(
                            {
                                "backing": backing,
                                "fee": fee,
                                "demand": demand,
                                "result": result,
                            }
                        )
                    if (
                        fixed_cap_witness is None
                        and initial_capacity < demand <= total_capacity
                        and result["accepted"]
                    ):
                        fixed_cap_witness = {
                            "backing": backing,
                            "fee": fee,
                            "demand": demand,
                            "initial_capacity": initial_capacity,
                            "expanded_capacity": result["capacity"],
                        }
                    if unbacked_witness is None and result["known"] > 1:
                        unbacked_capacity = result["capacity"] + 1
                        if unbacked_capacity != sum(backing[: result["known"]]) - fee:
                            unbacked_witness = {
                                "backing": backing,
                                "fee": fee,
                                "authenticated": result["capacity"],
                                "unbacked": unbacked_capacity,
                            }

    return {
        "traces": traces,
        "violations": violations,
        "fixed_cap_counterexample": fixed_cap_witness,
        "unbacked_counterexample": unbacked_witness,
        "passed": (
            not violations
            and fixed_cap_witness is not None
            and unbacked_witness is not None
        ),
    }


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    stack = check_stack_transfers()
    frontier = check_frontier_expansion()
    output = {
        "stack": stack,
        "frontier": frontier,
        "overall_pass": stack["passed"] and frontier["passed"],
    }
    rendered = json.dumps(output, indent=2, sort_keys=True)
    if args.json_out:
        with open(args.json_out, "w") as handle:
            handle.write(rendered + "\n")
    else:
        print(rendered)


main(sys.argv[1:])
