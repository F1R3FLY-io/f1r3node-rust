import argparse
import itertools
import json
import os
import random
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


# Canonical sort order of BillableKind (rholang accounting/mod.rs::BillableKind),
# used to mirror the derived `Ord` on BillableTokenEvent. Only the relative
# order matters for schedule-independence (any total order makes the
# reconciliation a function of the input multiset).
_KIND_RANK = {"source": 0, "primitive": 1, "substitution": 2}


def _canonical_key(event):
    """The canonical Ord key on BillableTokenEvent:
    (deploy_id, source_path, redex_id, local_index, kind, weight). The runtime's
    `kind` field carries the primitive descriptor inside the Primitive variant,
    so we fold (kind_rank, primitive_descriptor) in at the kind position."""
    return (
        int(event["deploy"]),
        tuple(int(v) for v in event["path"]),
        int(event["redex_id"]),
        int(event["local_index"]),
        _KIND_RANK.get(str(event["kind"]), 99),
        str(event.get("primitive_descriptor", "")),
        int(event["weight"]),
    )


def reconcile(attempt_log, initial_budget, max_trace_events):
    """Faithful model of RuntimeBudget::reconcile()
    (rholang/src/rust/interpreter/accounting/mod.rs:455).

    Sorts the attempt log by the canonical Ord (multiplicity preserved),
    truncates to max_trace_events, then walks accumulating weight: the first
    event that would exceed the budget is the OOP boundary (consumed clamps to
    `initial`); otherwise consumed is the sum of committed weights.

    Returns (consumed, oop_present, committed_multiset) where committed_multiset
    is a sorted tuple of canonical keys (order-insensitive identity)."""
    ordered = sorted(attempt_log, key=_canonical_key)
    if len(ordered) > max_trace_events:
        ordered = ordered[:max_trace_events]
    consumed = 0
    committed = []
    oop_present = False
    for event in ordered:
        weight = int(event["weight"])
        if consumed + weight > initial_budget:
            consumed = initial_budget
            oop_present = True
            break
        consumed += weight
        committed.append(_canonical_key(event))
    committed_multiset = tuple(sorted(committed))
    return consumed, oop_present, committed_multiset


def _valid_events(attempt_log, max_weight):
    """Intrinsically-valid admitted events: positive, in-range weight. Mirrors
    the runtime admission gate before an event enters the attempt log."""
    return [e for e in attempt_log if 0 < int(e["weight"]) <= max_weight]


def semantic_comm_identity(consume, produces, peeks):
    semantic_produces = sorted(
        (
            produce["channel"],
            produce["datum_hash"],
            bool(produce["persistent"]),
            int(produce["repetition_count"]),
        )
        for produce in produces
    )
    return (
        consume["channels_hash"],
        consume["patterns_hash"],
        consume["continuation_hash"],
        bool(consume["persistent"]),
        tuple(semantic_produces),
        tuple(sorted(int(peek) for peek in peeks)),
    )


def execute_atomic_comms(order, requirements, budget):
    pending = set()
    committed = []
    rejected = []
    for command in order:
        available = pending | {command}
        ready = sorted(
            event
            for event, required in requirements.items()
            if event not in committed and required.issubset(available)
        )
        if not ready:
            pending.add(command)
            continue
        event = ready[0]
        before = frozenset(pending)
        if len(committed) >= budget:
            rejected.append((event, command, before, frozenset(pending)))
            continue
        pending = available - requirements[event]
        committed.append(event)
    return frozenset(pending), tuple(sorted(committed)), tuple(rejected)


def assert_atomic_comm_semantics():
    requirements = {
        "binary": frozenset(["send-a", "receive-a"]),
        "join": frozenset(["send-b", "send-c", "join-bc"]),
    }
    commands = ["send-a", "receive-a", "send-b", "send-c", "join-bc", "unmatched"]
    terminal_states = set()
    permutations_checked = 0
    for order in itertools.permutations(commands):
        pending, committed, rejected = execute_atomic_comms(order, requirements, 2)
        assert not rejected
        assert committed == ("binary", "join")
        assert pending == frozenset(["unmatched"])
        assert len(committed) == 2
        terminal_states.add((pending, committed))
        permutations_checked += 1
    assert len(terminal_states) == 1

    rejection_orders_checked = 0
    for order in itertools.permutations(["send-a", "receive-a", "unmatched"]):
        pending, committed, rejected = execute_atomic_comms(
            order, {"binary": requirements["binary"]}, 0
        )
        assert committed == ()
        assert len(rejected) == 1
        _, trigger, before, after = rejected[0]
        assert before == after
        assert trigger not in after
        assert len(committed) == 0
        rejection_orders_checked += 1

    consume = {
        "channels_hash": "channels",
        "patterns_hash": "patterns",
        "continuation_hash": "continuation",
        "persistent": False,
    }
    producer_triggered = [
        {
            "channel": "a",
            "datum_hash": "datum",
            "persistent": False,
            "repetition_count": 1,
            "output_value": "scheduler-local-a",
            "failed": False,
        },
        {
            "channel": "b",
            "datum_hash": "datum-b",
            "persistent": True,
            "repetition_count": 1,
            "output_value": "scheduler-local-c",
            "failed": False,
        },
    ]
    consumer_triggered = [
        {
            "channel": "b",
            "datum_hash": "datum-b",
            "persistent": True,
            "repetition_count": 1,
            "output_value": "scheduler-local-d",
            "failed": True,
        },
        {
            "channel": "a",
            "datum_hash": "datum",
            "persistent": False,
            "repetition_count": 1,
            "output_value": "scheduler-local-b",
            "failed": True,
        }
    ]
    stable_identity = semantic_comm_identity(consume, producer_triggered, [])
    assert stable_identity == semantic_comm_identity(consume, consumer_triggered, [])
    changed_repetition = list(consumer_triggered)
    changed_repetition[0] = dict(changed_repetition[0], repetition_count=2)
    assert stable_identity != semantic_comm_identity(consume, changed_repetition, [])

    return {
        "arrival_permutations_checked": int(permutations_checked),
        "rejection_permutations_checked": int(rejection_orders_checked),
        "terminal_state_count": int(len(terminal_states)),
        "successful_binary_cost": 1,
        "successful_join_cost": 1,
        "unmatched_introduction_cost": 0,
        "rejection_preserves_state": True,
        "trigger_and_telemetry_independent_identity": True,
        "producer_order_independent_identity": True,
        "repetition_count_authenticated": True,
    }


def assert_schedule_independence(seed=20260527, trials=400, max_forks=6,
                                 max_weight_cap=2 ** 63 - 1):
    """Bounded-exhaustive check of the consensus-quantity claims:

      1. `consumed` (= min(initial, Σ valid weights) when the cap does not bite)
         is SCHEDULE-INDEPENDENT — identical under every permutation of the
         attempt log.
      2. The OOP verdict is SCHEDULE-INDEPENDENT.
      3. When NOT OOP and the cap does not bite, the committed multiset is
         COMPLETE — exactly the multiset of all intrinsically-valid events.

    For each random fork/weight/budget config the attempt log is reconciled
    under EVERY permutation (exhaustive for small logs) and the resulting
    (consumed, oop, committed_multiset) triple is asserted invariant. Raises
    AssertionError on any violation, so `sage` exits non-zero on failure."""
    # Sage's preparser turns integer literals into Sage Integer objects;
    # random.Random and the stdlib randint bounds want plain Python ints.
    seed = int(seed)
    trials = int(trials)
    max_forks = int(max_forks)
    max_weight_cap = int(max_weight_cap)
    rng = random.Random(seed)
    # MAX_COST_TRACE_EVENTS is 1,000,000 in production. To exercise BOTH the
    # uncapped path (completeness holds) and the cap-binding backstop path
    # (completeness is correctly NOT claimed) within a bounded check, we draw a
    # small cap per trial and branch the assertions on whether it bites.
    checked = 0
    cap_bound_trials = 0
    oop_trials = 0
    complete_trials = 0
    repeated_key_trials = 0

    for trial in range(trials):
        fork_count = rng.randint(1, max_forks)
        max_trace_events = rng.randint(1, max_forks)
        # Mix in some zero / oversized weights to exercise the admission gate,
        # and deliberately reuse (deploy, path, redex, local, kind) across forks
        # so the attempt log contains equal canonical keys (a loop re-attempting
        # the same logical event) — the multiplicity-preservation path.
        events = []
        reuse_slot = rng.random() < 0.5
        for fork in range(fork_count):
            weight_roll = rng.random()
            if weight_roll < 0.12:
                weight = 0                       # invalid: zero weight
            elif weight_roll < 0.18:
                weight = max_weight_cap + 1      # invalid: oversized weight
            else:
                weight = rng.randint(1, 5)       # valid billable weight
            slot = 0 if reuse_slot and fork % 2 == 0 else fork
            events.append(
                canonical_event(
                    "source" if rng.random() < 0.7 else "primitive",
                    weight,
                    descriptor="fork-{}".format(slot),
                    path=[slot],
                    redex_id=slot,
                    local_index=0,
                )
            )

        valid = _valid_events(events, max_weight_cap)
        valid_keys = tuple(sorted(_canonical_key(e) for e in valid))
        sum_valid = sum(int(e["weight"]) for e in valid)
        # Choose a budget that lands on either side of the Σ threshold so both
        # the OOP and non-OOP verdicts are exercised across trials.
        if rng.random() < 0.5 and sum_valid > 0:
            initial_budget = rng.randint(0, max(0, sum_valid - 1))   # likely OOP
        else:
            initial_budget = sum_valid + rng.randint(0, 4)           # likely non-OOP

        cap_truncates = len(valid) > max_trace_events
        if cap_truncates:
            cap_bound_trials += 1
        if len(valid) != len(set(_canonical_key(e) for e in valid)):
            repeated_key_trials += 1

        # The reference answer: reconcile the canonically-ordered (identity)
        # attempt log. NOTE: only VALID events enter the attempt log in the
        # runtime, so we reconcile over `valid`.
        ref = reconcile(valid, initial_budget, max_trace_events)
        ref_consumed, ref_oop, ref_committed = ref

        # Enumerate permutations exhaustively when small; sample otherwise.
        n = len(valid)
        if n <= 6:
            perms = list(itertools.permutations(valid))
        else:
            perms = [tuple(valid)]
            for _ in range(120):
                shuffled = list(valid)
                rng.shuffle(shuffled)
                perms.append(tuple(shuffled))

        for perm in perms:
            consumed_p, oop_p, committed_p = reconcile(
                list(perm), initial_budget, max_trace_events
            )
            # (1) consumed schedule-independent.
            assert consumed_p == ref_consumed, (
                "consumed schedule-dependent: trial={} perm-consumed={} ref={} "
                "budget={} valid_weights={}".format(
                    trial, consumed_p, ref_consumed, initial_budget,
                    [int(e["weight"]) for e in valid],
                )
            )
            # (2) OOP verdict schedule-independent.
            assert oop_p == ref_oop, (
                "OOP verdict schedule-dependent: trial={} perm-oop={} ref={} "
                "budget={} valid_weights={}".format(
                    trial, oop_p, ref_oop, initial_budget,
                    [int(e["weight"]) for e in valid],
                )
            )
            # (3) committed multiset schedule-independent (always; it is a pure
            #     function of the input multiset under the canonical sort).
            assert committed_p == ref_committed, (
                "committed multiset schedule-dependent: trial={} budget={}".format(
                    trial, initial_budget
                )
            )
            checked += 1

        # consumed == min(initial, Σ valid weights) when the cap does not bite.
        if not cap_truncates:
            assert ref_consumed == min(initial_budget, sum_valid), (
                "consumed != min(initial, Σ): trial={} consumed={} initial={} "
                "sum_valid={}".format(trial, ref_consumed, initial_budget, sum_valid)
            )
        # OOP verdict == (Σ valid weights > initial) when the cap does not bite.
        if not cap_truncates:
            assert ref_oop == (sum_valid > initial_budget), (
                "OOP verdict != (Σ > initial): trial={} oop={} sum_valid={} "
                "initial={}".format(trial, ref_oop, sum_valid, initial_budget)
            )
            if ref_oop:
                oop_trials += 1

        # Non-OOP + uncapped => committed multiset is the COMPLETE valid set.
        if not ref_oop and not cap_truncates:
            assert ref_committed == valid_keys, (
                "non-OOP committed multiset incomplete: trial={} committed={} "
                "valid={}".format(trial, ref_committed, valid_keys)
            )
            complete_trials += 1

    # Sanity: the bounded search must actually exercise each regime, otherwise
    # the check is vacuous.
    assert oop_trials > 0, "no OOP trials were generated; widen the search"
    assert complete_trials > 0, "no complete-commit trials; widen the search"
    assert cap_bound_trials > 0, "no cap-binding trials; widen the search"
    assert repeated_key_trials > 0, "no repeated-canonical-key trials; widen search"

    return {
        "trials": int(trials),
        "permutation_reconciliations_checked": int(checked),
        "oop_trials": int(oop_trials),
        "complete_commit_trials": int(complete_trials),
        "cap_binding_trials": int(cap_bound_trials),
        "repeated_canonical_key_trials": int(repeated_key_trials),
        "consumed_schedule_independent": True,
        "oop_verdict_schedule_independent": True,
        "non_oop_committed_multiset_complete": True,
        "consumed_equals_min_initial_sum": True,
    }


def records():
    schedule_independence_witness = assert_schedule_independence()
    atomic_comm_witness = assert_atomic_comm_semantics()

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
            "Racing OOP branches retain one committed boundary event and do not leak trace slots.",
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
            "Historical low-level reservation diagnostic: post-hoc canonical reconciliation produces the same (committed_set, oop_event, consumed_units) triple under any concurrent attempt-log permutation. Native consensus cost is established separately from successful atomic COMM observations; this record does not define Rholang cost semantics.",
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
                "TLA+: RuntimeBudgetReplay.ConsumedAndVerdictScheduleIndependent",
                "Loom: reconcile_canonical_oop_is_higher_rank_event_under_any_schedule",
            ],
        ),
        record(
            "concurrency_schedule",
            "proof_or_model_strengthening",
            "sage_concurrency_consumed_and_verdict_schedule_independent_bounded_exhaustive",
            "Bounded-exhaustive low-level reservation diagnostic over random fork/weight/budget configs: "
            "`consumed` (= min(initial, Σ valid weights)) and the OOP "
            "verdict are schedule-independent under EVERY permutation of the attempt "
            "log, and the non-OOP committed multiset is complete. This is the property "
            "that survives the dropping of the per-operation cost_trace_digest from "
            "consensus: the digest's per-op order is schedule-dependent under OOP, but "
            "the reconciled total_cost and verdict are not. {} permutation "
            "reconciliations checked across {} configs ({} OOP, {} complete-commit, {} "
            "cap-binding, {} with repeated canonical keys).".format(
                schedule_independence_witness["permutation_reconciliations_checked"],
                schedule_independence_witness["trials"],
                schedule_independence_witness["oop_trials"],
                schedule_independence_witness["complete_commit_trials"],
                schedule_independence_witness["cap_binding_trials"],
                schedule_independence_witness["repeated_canonical_key_trials"],
            ),
            canonical_scenario(
                "concurrency_consumed_verdict_schedule_independent",
                events=success_then_finalize,
                initial_budget=4,
                concurrency={
                    "bounded_exhaustive_permutations": True,
                    "drops_per_op_digest_from_consensus": True,
                },
                threat_family="concurrency_schedule",
                expected_invariants=[
                    "rb_reconcile_consumed_invariant_under_permutation",
                    "rb_reconcile_oop_occurrence_invariant_under_permutation",
                    "consumed_equals_min_initial_or_sum",
                    "non_oop_committed_multiset_complete",
                ],
                rust_reproducer={
                    "test": "loom_runtime_budget_reconciliation::reconcile_canonical_oop_is_higher_rank_event_under_any_schedule",
                },
                promotion_target="tla:RuntimeBudgetReplay.ConsumedAndVerdictScheduleIndependent",
                expected_classification="proof_or_model_strengthening",
            ),
            schedule_independence_witness,
            [
                "Rocq: rb_reconcile_consumed_eq_min_initial_or_sum",
                "Rocq: ca_cost_deterministic",
                "TLA+: RuntimeBudgetReplay.ConsumedAndVerdictScheduleIndependent",
                "TLA+: RuntimeBudgetReplay.NonOopCommittedMultisetComplete",
                "TLA+: RuntimeBudgetReplay.TotalCostMatchesClampedSum",
                "Loom: reconcile_canonical_oop_is_higher_rank_event_under_any_schedule",
            ],
        ),
        record(
            "concurrency_schedule",
            "confirmed_safe",
            "sage_atomic_comm_arrival_order_is_semantically_irrelevant",
            "Every permutation of two independent matches plus one unmatched introduction commits the same binary and join COMMs, charges one unit per COMM, and leaves only the unmatched command resident.",
            canonical_scenario(
                "atomic_comm_arrival_order",
                events=success_then_finalize,
                initial_budget=2,
                concurrency={"all_arrival_permutations": True},
                threat_family="concurrency_schedule",
                expected_invariants=[
                    "unmatched_introductions_are_free",
                    "successful_comm_costs_exactly_one",
                    "join_arity_does_not_multiply_cost",
                    "terminal_rspace_is_schedule_independent",
                ],
                rust_reproducer={"test": "observes_exactly_once_for_either_trigger_side"},
                promotion_target="tla:AtomicCommAccounting",
                expected_classification="confirmed_safe",
            ),
            atomic_comm_witness,
            [
                "Rocq: committed_comm_costs_exactly_one",
                "Rocq: join_arity_does_not_multiply_cost",
                "TLA+: AtomicCommAccounting.TerminalRSpaceIsScheduleIndependent",
                "Rust: observes_exactly_once_for_either_trigger_side",
            ],
        ),
        record(
            "concurrency_schedule",
            "confirmed_safe",
            "sage_atomic_comm_identity_excludes_scheduler_telemetry",
            "Producer order, producer telemetry, and trigger direction do not change the semantic COMM identity, while repetition count remains authenticated.",
            canonical_scenario(
                "atomic_comm_semantic_identity",
                events=success_then_finalize,
                initial_budget=2,
                concurrency={
                    "opposite_trigger_paths": True,
                    "reversed_producer_order": True,
                    "different_telemetry": True,
                },
                threat_family="concurrency_schedule",
                expected_invariants=[
                    "trigger_side_does_not_change_cost",
                    "producer_order_does_not_change_identity",
                    "scheduler_telemetry_not_authenticated",
                    "repetition_count_authenticated",
                ],
                rust_reproducer={"test": "cost_identity_ignores_produce_telemetry"},
                promotion_target="rocq:trigger_side_does_not_change_cost",
                expected_classification="confirmed_safe",
            ),
            {
                "trigger_and_telemetry_independent_identity": atomic_comm_witness["trigger_and_telemetry_independent_identity"],
                "producer_order_independent_identity": atomic_comm_witness["producer_order_independent_identity"],
                "repetition_count_authenticated": atomic_comm_witness["repetition_count_authenticated"],
            },
            [
                "Rocq: trigger_side_does_not_change_cost",
                "Rust: cost_identity_ignores_produce_telemetry",
                "Rust: cost_identity_canonicalizes_producer_order",
                "Rust: cost_identity_commits_repetition_count",
            ],
        ),
        record(
            "concurrency_schedule",
            "confirmed_safe",
            "sage_atomic_comm_rejection_precedes_state_mutation",
            "Every trigger order at zero budget rejects before removing or storing RSpace data and emits no committed COMM cost.",
            canonical_scenario(
                "atomic_comm_rejection",
                events=[invalid_event],
                initial_budget=0,
                concurrency={"all_trigger_orders": True},
                threat_family="concurrency_schedule",
                expected_invariants=["rejected_comm_is_atomic", "rejected_comm_costs_zero"],
                rust_reproducer={"test": "replay_observer_rejection_preserves_rspace_and_trace"},
                promotion_target="tla:AtomicCommRejection",
                expected_classification="confirmed_safe",
            ),
            {
                "orders_checked": atomic_comm_witness["rejection_permutations_checked"],
                "rejection_preserves_state": atomic_comm_witness["rejection_preserves_state"],
                "committed_cost": 0,
            },
            [
                "Rocq: rejected_comm_is_atomic",
                "TLA+: AtomicCommRejection.RejectedCommIsAtomic",
                "Rust: replay_observer_rejection_preserves_rspace_and_trace",
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
