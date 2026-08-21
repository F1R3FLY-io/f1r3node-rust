import argparse
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


MAX_DESCRIPTOR_BYTES = 512
MAX_SOURCE_PATH_COMPONENTS = 1024
MAX_TRACE_EVENTS = 4


def records():
    descriptor_event = canonical_event(
        "primitive",
        1,
        descriptor="descriptor" * 80,
        path=[],
    )
    source_path_event = canonical_event(
        "source",
        1,
        descriptor="source_path",
        path=list(range(MAX_SOURCE_PATH_COMPONENTS + 1)),
    )
    trace_cap_events = [
        canonical_event("source", 1, descriptor="trace-cap", path=[i])
        for i in range(MAX_TRACE_EVENTS + 1)
    ]

    return [
        record(
            "resource_exhaustion",
            "confirmed_safe",
            "sage_resource_descriptor_bound_rejects_before_trace",
            "Primitive descriptors above the replay bound are rejected before trace mutation.",
            canonical_scenario(
                "resource_descriptor_bound",
                events=[descriptor_event],
                initial_budget=10,
                projection={"max_descriptor_bytes": MAX_DESCRIPTOR_BYTES},
                threat_family="resource_exhaustion",
                expected_invariants=["oversized_descriptor_rejected_before_mutation"],
                rust_reproducer={"fuzz": "cost_accounting_lifecycle_trace"},
                promotion_target="rust:fuzz",
                expected_classification="confirmed_safe",
            ),
            {"descriptor_bytes": len(descriptor_event["descriptor"]), "accepted": False, "trace_mutated": False},
            ["Rust: descriptor bound rejection tests", "Fuzz: cost_accounting_lifecycle_trace"],
        ),
        record(
            "resource_exhaustion",
            "confirmed_safe",
            "sage_resource_source_path_bound_rejects_before_trace",
            "Source paths above the replay bound are rejected before trace mutation.",
            canonical_scenario(
                "resource_source_path_bound",
                events=[source_path_event],
                initial_budget=10,
                projection={"max_source_path_components": MAX_SOURCE_PATH_COMPONENTS},
                threat_family="resource_exhaustion",
                expected_invariants=["oversized_source_path_rejected_before_mutation"],
                rust_reproducer={"fuzz": "runtime_budget_admission"},
                promotion_target="rust:fuzz",
                expected_classification="confirmed_safe",
            ),
            {"source_path_components": len(source_path_event["path"]), "accepted": False, "trace_mutated": False},
            ["Rust: source-path bound rejection tests", "Fuzz: runtime_budget_admission"],
        ),
        record(
            "resource_exhaustion",
            "proof_or_model_strengthening",
            "sage_resource_trace_cap_frontier",
            "Trace-cap exhaustion rejects the next event without consuming fuel or losing committed evidence.",
            canonical_scenario(
                "resource_trace_cap",
                events=trace_cap_events,
                initial_budget=10,
                projection={"max_trace_events": MAX_TRACE_EVENTS},
                threat_family="resource_exhaustion",
                expected_invariants=["trace_cap_rejection_preserves_budget"],
                promotion_target="rocq:uc_ca_074",
                expected_classification="proof_or_model_strengthening",
            ),
            {"trace_cap": MAX_TRACE_EVENTS, "accepted_count": MAX_TRACE_EVENTS, "rejected_count": 1},
            ["Rocq: uc_ca_074_resource_exhaustion_frontier", "TLA+: RuntimeBudgetReplay"],
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
