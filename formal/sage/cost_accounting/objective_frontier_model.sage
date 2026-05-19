import argparse
import json
import os
import sys

base_dir = os.path.dirname(os.path.abspath(sys.argv[0]))
load(os.path.join(base_dir, "scenario_schema.sage"))


def all_records():
    invalid_event = canonical_event("source", 0)
    trace_cap_events = [canonical_event("source", 1, path=[i]) for i in range(5)]
    replay_fields = {
        "cost": 7,
        "digest_present": True,
        "digest": "good",
        "event_count": 2,
        "signature": "sig-a",
        "activation": "cost_accounted",
    }
    settlement = {
        "deploys": [
            {"phlo_limit": 10, "phlo_price": 2, "token_cost": 3, "refund": 14},
            {"phlo_limit": 5, "phlo_price": 4, "token_cost": 5, "refund": 0},
        ],
        "total_refund": 14,
        "total_escrow": 40,
    }

    return [
        record(
            "objective_frontier",
            "confirmed_safe",
            "sage_frontier_invalid_event_rejection",
            "Invalid billable events are high-priority because accepting them would authenticate trace evidence without fuel.",
            canonical_scenario(
                "frontier_invalid_event",
                events=[invalid_event],
                initial_budget=10,
                threat_family="producer_routing",
                expected_invariants=["admitted-event validity"],
                promotion_target="rocq:uc_ca_069",
                expected_classification="confirmed_safe",
            ),
            {"invalid_event": invalid_event, "accepted": False, "trace_mutated": False},
            ["Rocq: admitted-event validity", "Rust: runtime_budget_admission fuzz"],
        ),
        record(
            "objective_frontier",
            "proof_or_model_strengthening",
            "sage_frontier_trace_cap_boundary",
            "Trace-cap behavior remains a separate frontier point because it combines resource exhaustion with replay evidence retention.",
            canonical_scenario(
                "frontier_trace_cap",
                events=trace_cap_events,
                initial_budget=10,
                projection={"max_trace_events": 4},
                threat_family="concurrency_schedule",
                expected_invariants=["trace-slot linearizability", "trace-cap preservation"],
                promotion_target="rocq:uc_ca_070",
                expected_classification="proof_or_model_strengthening",
            ),
            {"trace_cap": 4, "events": trace_cap_events, "accepted_count": 4, "rejected_count": 1},
            ["TLA+: RuntimeBudgetReplay", "Loom: trace-slot accounting"],
        ),
        record(
            "objective_frontier",
            "confirmed_safe",
            "sage_frontier_replay_authentication",
            "Replay-field mutation stays on the frontier because it is consensus-critical even when current tests pass.",
            canonical_scenario(
                "frontier_replay",
                replay_fields=replay_fields,
                threat_family="replay_authentication",
                expected_invariants=["replay field sensitivity"],
                promotion_target="tla:CostAccountingThreats",
                expected_classification="confirmed_safe",
            ),
            {"replay_fields": replay_fields, "mutated_fields": sorted(replay_fields.keys())},
            ["TLA+: CostAccountingThreats", "Rust: replay_payload_cost_fields fuzz"],
        ),
        record(
            "objective_frontier",
            "proof_or_model_strengthening",
            "sage_frontier_multi_deploy_settlement",
            "Multi-deploy settlement is retained as a frontier objective because it composes refund arithmetic with block aggregation.",
            canonical_scenario(
                "frontier_multi_deploy",
                settlement={"kind": "multi_deploy"},
                threat_family="settlement",
                expected_invariants=["multi-deploy settlement additivity"],
                promotion_target="rocq:uc_ca_072",
                expected_classification="proof_or_model_strengthening",
            ),
            settlement,
            ["Sage: settlement model", "Rust: generated frontier replay fixture"],
        ),
        record(
            "objective_frontier",
            "projection_risk",
            "sage_frontier_producer_routing_guard",
            "Producer-routing regressions are retained on the frontier because zero-capable producers can otherwise authenticate cost trace entries without work.",
            canonical_scenario(
                "frontier_producer_routing",
                events=[canonical_event("primitive", 0, descriptor="variable-work-empty")],
                initial_budget=10,
                threat_family="producer_routing",
                expected_invariants=["zero-weight rejection", "strict producer positivity"],
                rust_reproducer={
                    "test": "projection_risk_witnesses_have_guarded_safe_disposition",
                    "guard": "projection_risk_zero_weight_strict_route_rejects_before_trace_mutation",
                },
                promotion_target="rust:guard",
                expected_classification="projection_risk",
            ),
            {"producer": "variable-work", "routing": "strict", "accepted": False, "guarded": True},
            [
                "Sage: producer routing model",
                "Rust: projection_risk_witnesses_have_guarded_safe_disposition",
                "Sage guard: projection_risk_zero_weight_strict_route_rejects_before_trace_mutation",
            ],
        ),
        record(
            "objective_frontier",
            "confirmed_safe",
            "sage_frontier_slashing_composition",
            "Slashing composition remains on the frontier because cost-invalid evidence must not rewrite user fuel or settlement inputs.",
            canonical_scenario(
                "frontier_slashing_composition",
                settlement={"kind": "slash_after_evaluation"},
                threat_family="slashing_composition",
                expected_invariants=["slash preserves settlement inputs", "slash cannot add runtime fuel"],
                promotion_target="rocq:uc_ca_073",
                expected_classification="confirmed_safe",
            ),
            {"slashing": "post_evaluation", "settlement_inputs_preserved": True, "runtime_fuel_added": False},
            ["Sage: slashing composition model", "Rocq: uc_ca_073_slashing_composition_frontier"],
        ),
        record(
            "objective_frontier",
            "proof_or_model_strengthening",
            "sage_frontier_resource_exhaustion",
            "Resource-exhaustion bounds stay on the frontier because descriptor, path, and trace-window limits protect replay evidence memory.",
            canonical_scenario(
                "frontier_resource_exhaustion",
                events=[canonical_event("primitive", 1, descriptor="descriptor" * 80)],
                initial_budget=10,
                projection={"max_descriptor_bytes": 512, "max_source_path_components": 1024},
                threat_family="resource_exhaustion",
                expected_invariants=["reject before mutation"],
                promotion_target="rocq:uc_ca_074",
                expected_classification="proof_or_model_strengthening",
            ),
            {"descriptor": "oversized", "source_path": "bounded", "trace_mutated": False},
            ["Sage: resource exhaustion model", "Fuzz: cost_accounting_lifecycle_trace"],
        ),
    ]


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    records_all = all_records()
    frontier = pareto_frontier(records_all)
    output = {
        "records": records_all,
        "frontier": frontier,
        "coverage_summary": coverage_summary(records_all),
    }
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
