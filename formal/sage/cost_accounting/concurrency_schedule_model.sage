import argparse
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


def records():
    oop_race = [
        canonical_event("source", 3, descriptor="branch-a", path=[0]),
        canonical_event("source", 3, descriptor="branch-b", path=[1]),
    ]
    success_then_finalize = [
        canonical_event("source", 1, descriptor="worker-a", path=[0]),
        canonical_event("source", 1, descriptor="worker-b", path=[1]),
    ]
    invalid_event = canonical_event("primitive", 0, descriptor="invalid-worker")

    return [
        record(
            "concurrency_schedule",
            "proof_or_model_strengthening",
            "sage_concurrency_repeated_oop_boundary_is_single",
            "Racing OOP branches retain one authenticated boundary event and do not leak trace slots.",
            canonical_scenario(
                "concurrency_repeated_oop",
                events=oop_race,
                initial_budget=4,
                concurrency={"racing_oop": True},
                threat_family="concurrency_schedule",
                expected_invariants=["oop_count_le_one", "oop_trace_entries_at_most_one"],
                rust_reproducer={"test": "loom_cost_trace_slots::trace_slots_stay_bounded_under_repeated_oop_race"},
                promotion_target="rocq:uc_ca_070",
                expected_classification="proof_or_model_strengthening",
            ),
            {"oop": "single_boundary", "slot_leak": False, "event_count_max": 1},
            ["Rocq: uc_ca_070_trace_slot_linearizability_frontier", "Loom: trace slots"],
        ),
        record(
            "concurrency_schedule",
            "proof_or_model_strengthening",
            "sage_concurrency_finalization_requires_worker_completion",
            "Finalization completeness is a scheduling frontier: finalized evidence must be read after worker trace append completion.",
            canonical_scenario(
                "concurrency_finalization_completion",
                events=success_then_finalize,
                initial_budget=4,
                concurrency={"finalization_after_workers": True},
                threat_family="concurrency_schedule",
                expected_invariants=["cost_trace_event_count_success_and_oop"],
                rust_reproducer={"test": "finalization_after_workers_observes_complete_trace_count"},
                promotion_target="tla:RuntimeBudgetReplay",
                expected_classification="proof_or_model_strengthening",
            ),
            {"finalization": "after_workers", "event_count": 2, "missing_append": False},
            ["Rocq: uc_ca_041_concurrent_finalization_trace_completeness", "TLA+: RuntimeBudgetReplay"],
        ),
        record(
            "concurrency_schedule",
            "confirmed_safe",
            "sage_concurrency_invalid_admission_releases_no_slot",
            "Invalid admission under concurrency leaves consumed fuel, trace count, and slot count unchanged.",
            canonical_scenario(
                "concurrency_invalid_admission",
                events=[invalid_event],
                initial_budget=4,
                concurrency={"invalid_worker": True},
                threat_family="concurrency_schedule",
                expected_invariants=["zero_weight_rejected_before_mutation"],
                rust_reproducer={"test": "loom_cost_trace_slots::invalid_admission_does_not_reserve_trace_slot"},
                promotion_target="rust:loom",
                expected_classification="confirmed_safe",
            ),
            {"slot_count": 0, "consumed": 0, "trace_count": 0},
            ["Rocq: rb_zero_weight_admission_rejection_preserves_trace", "Loom: invalid admission"],
        ),
        record(
            "concurrency_schedule",
            "proof_or_model_strengthening",
            "sage_concurrency_reconciliation_is_schedule_independent",
            "Option E: post-hoc canonical reconciliation produces the same (committed_set, oop_event, consumed_units) triple under any concurrent attempt-log permutation. The Rust runtime races lock-free CAS attempts; the consensus-relevant values come from the canonical walk, not from CAS race winners.",
            canonical_scenario(
                "concurrency_reconciliation_schedule_independent",
                events=oop_race,
                initial_budget=4,
                concurrency={"option_e_reconciliation": True},
                threat_family="concurrency_schedule",
                expected_invariants=[
                    "rb_reconcile_consumed_invariant_under_permutation",
                    "rb_reconcile_oop_occurrence_invariant_under_permutation",
                ],
                rust_reproducer={
                    "test": "loom_runtime_budget_reconciliation::reconcile_canonical_oop_is_higher_rank_event_under_any_schedule",
                },
                promotion_target="rocq:rb_reconcile_consumed_invariant_under_permutation",
                expected_classification="proof_or_model_strengthening",
            ),
            {
                "consumed_units": "min(initial, sum_weights)",
                "schedule_invariant": True,
                "canonical_oop_identity": "rank_minimum_overflow",
            },
            [
                "Rocq: rb_reconcile_consumed_eq_min_initial_or_sum",
                "Rocq: rb_reconcile_oop_occurrence_invariant_under_permutation",
                "TLA+: RuntimeBudgetReplay.ReconciledDigestIsPureFunctionOfEventsAndInitial",
                "Loom: reconcile_canonical_oop_is_higher_rank_event_under_any_schedule",
            ],
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
