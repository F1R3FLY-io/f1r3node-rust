import argparse
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


MAX_WEIGHT = 2**63 - 1
MAX_TRACE_EVENTS = 4


def simulate_budget(initial_budget, events, max_trace_events=MAX_TRACE_EVENTS):
    consumed = 0
    trace = []
    oop = None
    rejected = []
    for index, event in enumerate(events):
        weight = int(event["weight"])
        if weight <= 0 or weight > MAX_WEIGHT:
            rejected.append({"index": index, "reason": "invalid_weight", "event": event})
            continue
        if len(trace) + (0 if oop is None else 1) >= max_trace_events:
            rejected.append({"index": index, "reason": "trace_cap", "event": event})
            continue
        if consumed + weight > initial_budget:
            consumed = initial_budget
            if oop is None:
                oop = event
            else:
                rejected.append({"index": index, "reason": "oop_already_recorded", "event": event})
            continue
        consumed += weight
        trace.append(event)
    return {
        "consumed": int(consumed),
        "remaining": int(max(0, initial_budget - consumed)),
        "trace": trace,
        "oop": oop,
        "rejected": rejected,
        "trace_count": len(trace) + (0 if oop is None else 1),
    }


def records():
    zero = canonical_event("source", 0)
    oversized = canonical_event("primitive", MAX_WEIGHT + 1, descriptor="oversized")
    trace_cap_events = [canonical_event("source", 1, path=[i]) for i in range(MAX_TRACE_EVENTS + 1)]
    oop_events = [canonical_event("source", 3, path=[0]), canonical_event("source", 3, path=[1])]

    zero_result = simulate_budget(10, [zero])
    oversized_result = simulate_budget(10, [oversized])
    trace_cap_result = simulate_budget(10, trace_cap_events)
    oop_result = simulate_budget(4, oop_events)

    return [
        record(
            "budget_admission",
            "confirmed_safe",
            "sage_cost_zero_weight_rejected_before_mutation",
            "Zero-weight billable events are rejected without changing fuel or trace evidence.",
            canonical_scenario("zero_weight", events=[zero], initial_budget=10, expected_classification="confirmed_safe"),
            zero_result,
            ["Rocq: rb_zero_weight_admission_rejection_preserves_trace", "Rust: zero_weight_billable_event_is_rejected_without_trace_entry"],
        ),
        record(
            "budget_admission",
            "confirmed_safe",
            "sage_cost_oversized_weight_rejected_before_mutation",
            "Weights outside the runtime i64 settlement range are rejected before trace mutation.",
            canonical_scenario("oversized_weight", events=[oversized], initial_budget=10, expected_classification="confirmed_safe"),
            oversized_result,
            ["Rocq: rb_oversized_weight_admission_rejection_preserves_trace", "Rust: oversized event rejection tests"],
        ),
        record(
            "budget_admission",
            "confirmed_safe",
            "sage_cost_trace_cap_rejects_before_mutation",
            "A full trace window rejects the next event while preserving the already committed trace.",
            canonical_scenario("trace_cap", events=trace_cap_events, initial_budget=10, projection={"max_trace_events": MAX_TRACE_EVENTS}, expected_classification="confirmed_safe"),
            trace_cap_result,
            ["Rocq: rb_trace_cap_rejection_preserves_trace", "TLA+: TraceWithinRetentionBound", "Rust: runtime_budget_admission fuzz"],
        ),
        record(
            "budget_admission",
            "proof_or_model_strengthening",
            "sage_cost_repeated_oop_does_not_duplicate_boundary",
            "Repeated OOP attempts after the first boundary must not duplicate cost-trace evidence.",
            canonical_scenario("repeated_oop", events=oop_events, initial_budget=4, concurrency={"racing_oop": True}, expected_classification="proof_or_model_strengthening"),
            oop_result,
            ["Loom: trace-slot release shadow model", "Rust: cost_accounting_lifecycle_trace fuzz"],
        ),
    ]


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    output = {"records": records()}
    output["coverage_summary"] = coverage_summary(output["records"])
    text = json.dumps(output, indent=2, sort_keys=True, default=schema_json_default)
    if args.json_out:
        with open(args.json_out, "w") as handle:
            handle.write(text + "\n")
    else:
        print(text)


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
