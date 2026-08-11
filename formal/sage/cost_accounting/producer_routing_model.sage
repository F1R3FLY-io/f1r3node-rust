import argparse
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


def records():
    positive_event = canonical_event("primitive", 1, descriptor="standalone-send")
    zero_work_event = canonical_event("primitive", 0, descriptor="variable-work-empty")
    substitution_event = canonical_event("substitution", 1, descriptor="standalone-substitution")

    return [
        record(
            "producer_routing",
            "confirmed_safe",
            "sage_producer_standalone_billable_is_positive",
            "Standalone billable producer routes emit positive bounded events.",
            canonical_scenario(
                "producer_positive_standalone",
                events=[positive_event],
                initial_budget=4,
                threat_family="producer_routing",
                expected_invariants=["admitted_success_has_positive_bounded_weight"],
                rust_reproducer={"test": "cost_accounting_frontier_generated_fixtures_are_classified"},
                promotion_target="rocq:uc_ca_069",
                expected_classification="confirmed_safe",
            ),
            {"producer": "strict", "routing": "billable", "accepted": True, "weight": 1},
            ["Rocq: uc_ca_069_producer_routing_search_frontier", "Rust: producer-routing guard"],
        ),
        record(
            "producer_routing",
            "confirmed_safe",
            "sage_producer_zero_work_stays_nonbillable",
            "Zero-capable variable-work producers must not emit authenticated trace evidence for no work.",
            canonical_scenario(
                "producer_zero_work_nonbillable",
                events=[],
                initial_budget=4,
                threat_family="producer_routing",
                expected_invariants=["nonbillable_frame_preserves_trace"],
                rust_reproducer={
                    "test": "projection_risk_witnesses_have_guarded_safe_disposition",
                    "guard": "projection_risk_zero_weight_strict_route_rejects_before_trace_mutation",
                },
                promotion_target="rust:guard",
                expected_classification="confirmed_safe",
            ),
            {"producer": "variable-work", "routing": "nonbillable", "trace_mutated": False},
            [
                "Rocq: rb_nonbillable_frame_preserves_trace",
                "Rust: projection_risk_witnesses_have_guarded_safe_disposition",
                "Sage guard: projection_risk_zero_weight_strict_route_rejects_before_trace_mutation",
            ],
        ),
        record(
            "producer_routing",
            "projection_risk",
            "sage_producer_zero_strict_route_is_guarded",
            "A zero-weight event on a strict billable route is a projection risk and must be guarded before implementation changes.",
            canonical_scenario(
                "producer_zero_strict_projection",
                events=[zero_work_event],
                initial_budget=4,
                threat_family="producer_routing",
                expected_invariants=["zero_weight_rejected_before_mutation"],
                rust_reproducer={
                    "test": "projection_risk_witnesses_have_guarded_safe_disposition",
                    "guard": "projection_risk_zero_weight_strict_route_rejects_before_trace_mutation",
                },
                promotion_target="rust:guard",
                expected_classification="projection_risk",
            ),
            {"producer": "strict", "routing": "billable", "accepted": False, "reason": "zero_weight"},
            [
                "Rocq: rb_zero_weight_admission_rejection_preserves_trace",
                "Rust: projection_risk_witnesses_have_guarded_safe_disposition",
                "Sage guard: projection_risk_zero_weight_strict_route_rejects_before_trace_mutation",
            ],
        ),
        record(
            "producer_routing",
            "confirmed_safe",
            "sage_substitution_standalone_has_floor",
            "Standalone substitution work emits a positive event floor while empty substitutions remain nonbillable.",
            canonical_scenario(
                "producer_substitution_floor",
                events=[substitution_event],
                initial_budget=4,
                threat_family="producer_routing",
                expected_invariants=["admitted_success_has_positive_bounded_weight"],
                promotion_target="rocq:uc_ca_069",
                expected_classification="confirmed_safe",
            ),
            {"producer": "substitution", "routing": "billable", "weight": 1},
            ["Rocq: uc_ca_069_producer_routing_search_frontier", "Rust: substitution producer tests"],
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
