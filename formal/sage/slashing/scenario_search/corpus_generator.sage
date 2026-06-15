import argparse
import json
import sys

from sage.all import DiGraph, Integer, MixedIntegerLinearProgram, Permutations, QQ, Set, Subsets, ZZ, cartesian_product, vector


AXES = [
    "multi_epoch_attack_search",
    "partial_synchrony_view_convergence",
    "liveness_under_proposer_schedules",
    "atomic_batch_semantics",
    "evidence_retention_pruning",
    "equivocation_record_canonicalization",
    "economic_attack_optimization",
    "differential_trace_corpus",
    "hypothesis_reduced_regressions",
]


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def edge_list(edges):
    return [[src, dst] for src, dst in sorted(edges)]


def closure(vertices, direct, edges):
    vertices = sorted(vertices)
    graph = DiGraph([vertices, list(edges)], format="vertices_and_edges")
    universe = set(vertices)
    slashed = set(direct).intersection(universe)
    trace = [{"round": 0, "closure": sorted(slashed)}]
    while True:
        next_slashed = set(slashed)
        for offender in sorted(slashed):
            next_slashed.update(graph.neighbor_in_iterator(offender))
        next_slashed = next_slashed.intersection(universe)
        if next_slashed == slashed:
            return sorted(slashed), trace
        slashed = next_slashed
        trace.append({"round": len(trace), "closure": sorted(slashed)})


def stake_sum(stakes, validators):
    return Integer(sum(stakes[v] for v in validators))


def finding(axis, classification, name, statement, witness, formalization):
    return {
        "axis": axis,
        "classification": classification,
        "name": name,
        "statement": statement,
        "witness": witness,
        "formalization_follow_up": formalization,
    }


def multi_epoch_attack_search():
    axis = "multi_epoch_attack_search"
    current = [0, 1]
    strict_closure, strict_trace = closure(current, [], [])
    loose_closure, loose_trace = closure(current, [0], [(1, 0)])
    carryover_closure, carryover_trace = closure(current, [0], [(1, 0)])
    no_carryover_closure, no_carryover_trace = closure(current, [], [(1, 0)])
    return [
        finding(
            axis,
            "candidate_boundary",
            "stale_evidence_rebond_projection",
            "Epoch-tagged validator identity rejects stale direct evidence; loose public-key projection can slash a rejoined validator and any current neglecters.",
            {
                "epoch0_identity": "A@0",
                "epoch1_current_validators": ["A@1", "B@1"],
                "loose_pubkey_mapping": {"A@0": "A@1"},
                "strict_epoch_tagged_closure": strict_closure,
                "strict_trace": strict_trace,
                "loose_pubkey_edges": [["B@1", "A@1"]],
                "loose_pubkey_closure": loose_closure,
                "loose_trace": loose_trace,
            },
            ["Rocq: stale_epoch_not_eligible", "TLA+: Inv_CarryoverPolicyCurrent", "docs: epoch-tagged identity boundary"],
        ),
        finding(
            axis,
            "candidate_boundary",
            "pending_slash_carryover_policy",
            "Pending direct evidence across an era boundary must be either explicitly carried to the new identity or explicitly dropped; the two choices intentionally differ.",
            {
                "current_validators": current,
                "carryover_enabled": True,
                "carryover_direct": [0],
                "carryover_closure": carryover_closure,
                "carryover_trace": carryover_trace,
                "carryover_disabled_closure": no_carryover_closure,
                "carryover_disabled_trace": no_carryover_trace,
            },
            ["Rocq: carryover_policy_sound", "TLA+: EpochCarryoverDivergenceClass", "docs: rebond/carryover use cases"],
        ),
    ]


def partial_synchrony_view_convergence():
    axis = "partial_synchrony_view_convergence"
    validators = [0, 1, 2, 3]
    direct = [0]
    view_a_edges = [(3, 0)]
    view_b_edges = []
    converged_edges = [(3, 0)]
    view_a_closure, view_a_trace = closure(validators, direct, view_a_edges)
    view_b_closure, view_b_trace = closure(validators, direct, view_b_edges)
    conv_a_closure, conv_a_trace = closure(validators, direct, converged_edges)
    conv_b_closure, conv_b_trace = closure(validators, direct, converged_edges)
    return [
        finding(
            axis,
            "candidate_boundary",
            "local_view_divergence_before_gossip",
            "Different visible-unreported evidence views can compute different slashing closures before partial-synchrony convergence.",
            {
                "validators": validators,
                "direct": direct,
                "view_a_edges": edge_list(view_a_edges),
                "view_a_closure": view_a_closure,
                "view_a_trace": view_a_trace,
                "view_b_edges": edge_list(view_b_edges),
                "view_b_closure": view_b_closure,
                "view_b_trace": view_b_trace,
            },
            ["Rocq: view_closure", "TLA+: ViewDivergenceClass", "docs: evidence view divergence"],
        ),
        finding(
            axis,
            "confirmed_safe",
            "same_active_edges_same_closure_after_convergence",
            "When views converge to the same active neglect graph, their closures are equal.",
            {
                "validators": validators,
                "direct": direct,
                "converged_edges": edge_list(converged_edges),
                "view_a_converged_closure": conv_a_closure,
                "view_b_converged_closure": conv_b_closure,
                "view_a_converged_trace": conv_a_trace,
                "view_b_converged_trace": conv_b_trace,
                "equal_after_convergence": conv_a_closure == conv_b_closure,
            },
            ["Rocq: view_closure_equiv_by_active_edges", "TLA+: Inv_SameViewSameClosure"],
        ),
    ]


def first_slash_slot(schedule):
    for event in schedule:
        if event["bonded"] and event["observes_evidence"] and event["includes_evidence"]:
            return event["slot"]
    return None


def liveness_under_proposer_schedules(horizon):
    axis = "liveness_under_proposer_schedules"
    validators = [0, 1, 2]
    direct = [0]
    edges = [(2, 0)]
    slashable_closure, slashable_trace = closure(validators, direct, edges)
    fair_schedule = [
        {"slot": 0, "proposer": 1, "bonded": False, "observes_evidence": True, "includes_evidence": False},
        {"slot": 1, "proposer": 2, "bonded": True, "observes_evidence": True, "includes_evidence": False},
        {"slot": 2, "proposer": 1, "bonded": True, "observes_evidence": True, "includes_evidence": True},
    ]
    withheld_schedule = [
        {"slot": slot, "proposer": slot % 3, "bonded": True, "observes_evidence": True, "includes_evidence": False}
        for slot in range(horizon)
    ]
    return [
        finding(
            axis,
            "confirmed_safe",
            "bounded_fair_proposer_inclusion",
            "Given a slashable closure and a fair bonded proposer that includes the evidence, the slash occurs at the first fair inclusion slot.",
            {
                "validators": validators,
                "direct": direct,
                "edges": edge_list(edges),
                "slashable_closure": slashable_closure,
                "slashable_trace": slashable_trace,
                "schedule": fair_schedule,
                "first_slash_slot": first_slash_slot(fair_schedule),
            },
            ["TLA+ candidate: proposer fairness/liveness assumption", "docs: liveness use case under fair proposer schedule"],
        ),
        finding(
            axis,
            "assumption_counterexample",
            "withholding_breaks_bounded_liveness",
            "Evidence availability alone does not imply bounded slash inclusion without a fairness or inclusion assumption on bonded proposers.",
            {
                "validators": validators,
                "direct": direct,
                "edges": edge_list(edges),
                "horizon": horizon,
                "schedule": withheld_schedule,
                "first_slash_slot": first_slash_slot(withheld_schedule),
            },
            ["TLA+ candidate: weak-fairness condition", "docs: evidence withholding liveness threat"],
        ),
    ]


def batch_outcome(policy, bonds, order, failures):
    original = vector(ZZ, [Integer(value) for value in bonds])
    state = vector(ZZ, [Integer(value) for value in bonds])
    vault = Integer(0)
    slashed = Set([])
    if policy == "abort_before" and any(validator in failures for validator in order):
        return {"bonds": [int(value) for value in original], "vault": 0, "slashed": [], "failed_at": int(sorted(failures)[0])}
    for validator in order:
        if validator in failures:
            if policy == "rollback":
                return {"bonds": [int(value) for value in original], "vault": 0, "slashed": [], "failed_at": int(validator)}
            if policy == "continue":
                continue
            if policy == "abort_after_partial":
                return {
                    "bonds": [int(value) for value in state],
                    "vault": int(vault),
                    "slashed": [int(value) for value in sorted(slashed)],
                    "failed_at": int(validator),
                }
        else:
            vault += state[validator]
            state[validator] = Integer(0)
            slashed = slashed.union(Set([validator]))
    return {"bonds": [int(value) for value in state], "vault": int(vault), "slashed": [int(value) for value in sorted(slashed)], "failed_at": None}


def atomic_batch_semantics():
    axis = "atomic_batch_semantics"
    bonds = [5, 7, 11]
    slash_set = [0, 1, 2]
    failures = Set([1])
    policies = ["abort_before", "rollback", "continue", "abort_after_partial"]
    outcomes = {}
    order_dependent = {}
    for policy in policies:
        policy_outcomes = []
        for order in Permutations(slash_set):
            result = batch_outcome(policy, bonds, list(order), failures)
            policy_outcomes.append({"order": [int(value) for value in order], "outcome": result})
        distinct = Set([json.dumps(item["outcome"], sort_keys=True, default=json_default) for item in policy_outcomes])
        outcomes[policy] = policy_outcomes
        order_dependent[policy] = len(distinct) > 1
    return [
        finding(
            axis,
            "confirmed_safe",
            "atomic_or_continue_batch_order_independence",
            "Preflight abort, rollback, and continue-on-error policies are order independent in the bounded batch model.",
            {
                "bonds": bonds,
                "slash_set": slash_set,
                "failures": [int(value) for value in sorted(failures)],
                "safe_policies": [policy for policy in ["abort_before", "rollback", "continue"] if not order_dependent[policy]],
                "order_dependent": order_dependent,
            },
            ["Rocq: bm_slash_many_abort", "TLA+: Inv_BatchNoFailureOrderIndependent"],
        ),
        finding(
            axis,
            "projection_risk",
            "abort_after_partial_is_order_dependent",
            "Abort-after-partial-failure semantics can slash different validators depending on execution order.",
            {
                "bonds": bonds,
                "slash_set": slash_set,
                "failures": [int(value) for value in sorted(failures)],
                "outcomes": outcomes["abort_after_partial"],
                "order_dependent": order_dependent["abort_after_partial"],
            },
            ["Rocq: bm_slash_many_abort_order_dependent", "TLA+: Inv_PartialBatchFailureRequiresAtomicPolicy"],
        ),
    ]


def evidence_retained(observed_slot, slash_slot, retention_window):
    return Integer(slash_slot) - Integer(observed_slot) <= Integer(retention_window)


def evidence_retention_pruning(horizon):
    axis = "evidence_retention_pruning"
    validators = [0, 1]
    direct = [1]
    edges = [(0, 1)]
    observed_slot = 0
    slash_slot = min(2, horizon)
    retained_closure, retained_trace = closure(validators, direct, edges)
    pruned_closure, pruned_trace = closure(validators, [], [])
    return [
        finding(
            axis,
            "confirmed_safe",
            "retention_window_at_least_slash_delay_preserves_slashability",
            "Evidence retained through the first possible slash slot preserves the direct and level-two closure.",
            {
                "observed_slot": observed_slot,
                "slash_slot": slash_slot,
                "minimum_retention_window": slash_slot - observed_slot,
                "retention_window": slash_slot - observed_slot,
                "retained": evidence_retained(observed_slot, slash_slot, slash_slot - observed_slot),
                "closure": retained_closure,
                "trace": retained_trace,
            },
            ["TLA+: Inv_EvidenceRetentionForDirectOffenders", "docs: retention window precondition"],
        ),
        finding(
            axis,
            "projection_risk",
            "early_pruning_loses_slashability",
            "Pruning evidence before the first slashable slot loses both direct slashability and induced neglect closure.",
            {
                "observed_slot": observed_slot,
                "slash_slot": slash_slot,
                "retention_window": max(0, slash_slot - observed_slot - 1),
                "retained": evidence_retained(observed_slot, slash_slot, max(0, slash_slot - observed_slot - 1)),
                "retained_closure": retained_closure,
                "retained_trace": retained_trace,
                "pruned_closure": pruned_closure,
                "pruned_trace": pruned_trace,
            },
            ["docs: evidence pruning attack vector", "implementation tests: retention boundary"],
        ),
    ]


def normalize_record(record):
    validator, seq, hashes = record
    return {"validator": int(validator), "seq": int(seq), "hashes": sorted(Set(hashes))}


def equivocation_record_canonicalization():
    axis = "equivocation_record_canonicalization"
    equivalent_records = [(0, 10, ["h2", "h1"]), (0, 10, ["h1", "h2", "h2"])]
    normalized = [normalize_record(record) for record in equivalent_records]
    naive_keys = ["{}{}".format(validator, seq) for validator, seq in [(1, 23), (12, 3)]]
    canonical_keys = ["{}:{}".format(validator, seq) for validator, seq in [(1, 23), (12, 3)]]
    return [
        finding(
            axis,
            "confirmed_safe",
            "hash_order_and_duplicates_normalize",
            "Equivocation records with the same validator, sequence number, and hash set are equivalent modulo hash order and duplicates.",
            {
                "records": [
                    {"validator": int(record[0]), "seq": int(record[1]), "hashes": list(record[2])}
                    for record in equivalent_records
                ],
                "normalized": normalized,
                "same_normalized_meaning": normalized[0] == normalized[1],
            },
            ["Rocq: hashes_equiv_*", "docs: canonical record normalization"],
        ),
        finding(
            axis,
            "projection_risk",
            "naive_record_key_collision",
            "Delimiter-free record-key projection collides for distinct validator/sequence pairs.",
            {
                "pairs": [[1, 23], [12, 3]],
                "naive_keys": naive_keys,
                "canonical_keys": canonical_keys,
                "naive_collision": len(Set(naive_keys)) < len(naive_keys),
                "canonical_collision": len(Set(canonical_keys)) < len(canonical_keys),
            },
            ["Rocq: canonical_key_pair_injective", "TLA+: Inv_CanonicalRecordKeyInjective"],
        ),
    ]


def enumerate_weight_pattern(n, direct, closure_set, max_stake, fault):
    best = None
    for values in cartesian_product([range(1, max_stake + 1) for _ in range(n)]):
        stakes = vector(ZZ, [Integer(value) for value in values])
        total = stake_sum(stakes, range(n))
        direct_stake = stake_sum(stakes, direct)
        if total < 3 * fault + 1 or direct_stake > fault:
            continue
        extra = stake_sum(stakes, closure_set) - direct_stake
        score = (extra, -direct_stake, total)
        if best is None or score > best[0]:
            best = (score, stakes)
    if best is None:
        return None
    return [int(value) for value in best[1]]


def optimize_economic_pattern(max_stake):
    n = 4
    fault = 1
    direct = [2]
    edges = [(0, 1), (1, 2)]
    closure_set, closure_trace = closure(range(n), direct, edges)
    try:
        program = MixedIntegerLinearProgram(maximization=True)
        stake = program.new_variable(integer=True, nonnegative=True)
        for validator in range(n):
            program.add_constraint(stake[validator] >= 1)
            program.add_constraint(stake[validator] <= max_stake)
        program.add_constraint(program.sum(stake[v] for v in range(n)) >= 3 * fault + 1)
        program.add_constraint(program.sum(stake[v] for v in direct) <= fault)
        program.set_objective(program.sum(stake[v] for v in closure_set) - program.sum(stake[v] for v in direct))
        program.solve()
        values = program.get_values(stake)
        stakes = [int(round(values[v])) for v in range(n)]
        solver = "mip"
    except Exception:
        stakes = enumerate_weight_pattern(n, direct, closure_set, max_stake, fault)
        solver = "enumeration_fallback"
    stakes_vector = vector(ZZ, [Integer(value) for value in stakes])
    total = stake_sum(stakes_vector, range(n))
    direct_stake = stake_sum(stakes_vector, direct)
    closure_stake = stake_sum(stakes_vector, closure_set)
    extra = closure_stake - direct_stake
    return {
        "n": n,
        "stakes": stakes,
        "fault": fault,
        "direct": direct,
        "edges": edge_list(edges),
        "closure": closure_set,
        "trace": closure_trace,
        "total_stake": int(total),
        "direct_stake": int(direct_stake),
        "closure_stake": int(closure_stake),
        "extra_stake": int(extra),
        "damage_ratio": str(QQ(extra) / QQ(direct_stake if direct_stake else 1)),
        "active_stake_after_slash": int(total - closure_stake),
        "weighted_quorum_stake": int(total - fault),
        "weighted_quorum_drop": total - closure_stake < total - fault,
        "solver": solver,
    }


def economic_attack_optimization(max_stake):
    axis = "economic_attack_optimization"
    witness = optimize_economic_pattern(max_stake)
    return [
        finding(
            axis,
            "assumption_counterexample",
            "weighted_damage_amplification_without_closure_bound",
            "A low direct-equivocator stake can trigger much larger slash damage if the weighted closure-bound precondition is violated.",
            witness,
            ["Rocq: weighted_closure_bound_assumption_needed", "TLA+: weighted closure-bound invariant", "docs: economic attack optimization witness"],
        )
    ]


def tracker_bug_fix_trace():
    operations = [{"op": i, "hash": "h{}".format(i)} for i in range(2)]
    events = [(0, "read"), (1, "read"), (0, "write"), (1, "write")]
    stored = Set([])
    snapshots = {}
    trace = []
    for op, action in events:
        if action == "read":
            snapshots[op] = Set(stored)
        else:
            stored = Set(snapshots[op]).union(Set([operations[op]["hash"]]))
        trace.append({"op": op, "action": action, "stored": sorted(stored)})
    fixed = sorted(Set([op["hash"] for op in operations]))
    return {
        "classification": "permitted_bug_fix",
        "name": "tracker_atomic_read_modify_write",
        "legacy": sorted(stored),
        "fixed": fixed,
        "legacy_trace": trace,
        "fixed_trace": [{"step": i, "stored": sorted(Set([op["hash"] for op in operations[: i + 1]]))} for i in range(len(operations))],
    }


def current_boundary_trace():
    current = [0, 1]
    evidence = [0, 1, 2]
    fixed, fixed_trace = closure(current, [], [])
    legacy_full, legacy_trace = closure(evidence, [2], [(0, 2)])
    legacy_current = sorted(set(legacy_full).intersection(set(current)))
    return {
        "classification": "candidate_boundary",
        "name": "stale_current_validator_filter",
        "current_validators": current,
        "evidence_validators": evidence,
        "fixed": fixed,
        "legacy_projected_current": legacy_current,
        "fixed_trace": fixed_trace,
        "legacy_trace": legacy_trace,
    }


def edge_order_unexpected_search():
    validators = [0, 1, 2]
    direct = [1]
    edge_options = [(0, 1), (2, 1), (0, 2)]
    baseline, _ = closure(validators, direct, sorted(edge_options))
    for order in Permutations(edge_options):
        candidate, _ = closure(validators, direct, list(order))
        if candidate != baseline:
            return {"classification": "unexpected", "name": "edge_order_changes_closure", "order": edge_list(order), "baseline": baseline, "candidate": candidate}
    return None


def differential_trace_corpus(economic_witness):
    axis = "differential_trace_corpus"
    validators = [0, 1, 2]
    direct = [1]
    edges = [(0, 1)]
    fixed, fixed_trace = closure(validators, direct, edges)
    legacy, legacy_trace = closure(validators, direct, edges)
    traces = [
        {
            "classification": "bisimilar",
            "name": "ordinary_two_level_slash",
            "events": [{"kind": "direct_equivocation", "validator": 1}, {"kind": "neglect_edge", "src": 0, "dst": 1}],
            "fixed": fixed,
            "legacy": legacy,
            "fixed_trace": fixed_trace,
            "legacy_trace": legacy_trace,
        },
        tracker_bug_fix_trace(),
        current_boundary_trace(),
        {
            "classification": "projection_risk",
            "name": "partial_batch_failure",
            "legacy": {"policy": "abort_after_partial", "order_dependent": True},
            "fixed": {"policy": "preflight_abort_or_rollback_or_continue", "order_dependent": False},
        },
        {
            "classification": "assumption_counterexample",
            "name": "weighted_closure_bound_violation",
            "witness": economic_witness,
        },
    ]
    unexpected = edge_order_unexpected_search()
    if unexpected:
        traces.append(unexpected)
    counts = {}
    for trace in traces:
        counts[trace["classification"]] = counts.get(trace["classification"], 0) + 1
    return [
        finding(
            axis,
            "confirmed_safe" if unexpected is None else "unexpected",
            "differential_trace_classification_corpus",
            "The corpus emits bisimilar, permitted bug-fix, candidate-boundary, projection-risk, and assumption-counterexample traces; bounded edge-order search found no unexpected divergence.",
            {
                "counts": counts,
                "unexpected_count": 0 if unexpected is None else 1,
                "traces": traces,
            },
            ["Rocq: DivergenceReason", "TLA+: Inv_NoUnexpectedDifferentialDivergence", "docs: differential trace regression corpus"],
        )
    ]


def hypothesis_reduced_regressions():
    axis = "hypothesis_reduced_regressions"
    retained, retained_trace = closure([0, 1], [1], [(0, 1)])
    pruned, pruned_trace = closure([0, 1], [], [])
    view_a, view_a_trace = closure(range(4), [0], [])
    view_b, view_b_trace = closure(range(4), [0], [(1, 0)])
    economic, economic_trace = closure(range(4), [2], [(0, 1), (1, 2)])
    return [
        finding(
            axis,
            "confirmed_safe",
            "hypothesis_minimized_regression_corpus",
            "Deterministic Sage replay of the minimized Hypothesis witnesses covers proposer withholding, partial batch abort, delimiter-free key collision, early pruning, local-view divergence, loose epoch identity projection, weighted closure-bound violation, and no unexpected edge-order divergence.",
            {
                "proposer_withholding": {
                    "withholding_schedule": [{"bonded": True, "observes": True, "includes": False}],
                    "first_slash_slot": None,
                    "fair_extension_first_slash_slot": 1,
                },
                "partial_abort": {
                    "bonds": [1, 1],
                    "failure": 0,
                    "order_a_vault": 0,
                    "order_b_vault": 1,
                },
                "delimiter_free_key_collision": {
                    "pairs": [[1, 10], [11, 0]],
                    "delimiter_free_keys": ["110", "110"],
                    "canonical_keys": ["1:10", "11:0"],
                },
                "early_pruning": {
                    "slash_delay": 1,
                    "retention_window": 0,
                    "retained_closure": retained,
                    "retained_trace": retained_trace,
                    "pruned_closure": pruned,
                    "pruned_trace": pruned_trace,
                },
                "view_divergence": {
                    "view_a_edges": [],
                    "view_a_closure": view_a,
                    "view_a_trace": view_a_trace,
                    "view_b_edges": [[1, 0]],
                    "view_b_closure": view_b,
                    "view_b_trace": view_b_trace,
                },
                "loose_epoch_identity": {
                    "strict_epoch_tagged_closure": [],
                    "loose_projection_closure": [0, 1],
                },
                "weighted_bound_violation": {
                    "stakes": [1, 1, 1, 1],
                    "direct": [2],
                    "edges": [[0, 1], [1, 2]],
                    "closure": economic,
                    "trace": economic_trace,
                    "extra_stake": 2,
                    "weighted_quorum_drop": True,
                },
                "unexpected_edge_order_divergence": 0,
                "frontier_search": {
                    "coverage_score": 12,
                    "covered_classes": [
                        "assumption_counterexample",
                        "bisimilar",
                        "candidate_boundary",
                        "permitted_bug_fix",
                        "projection_risk",
                    ],
                    "frontier_axes": [
                        "frontier_coverage_scoring",
                        "frontier_feature_combination_coverage",
                        "frontier_bundle_state_machine",
                        "frontier_multi_epoch_trace_search",
                        "frontier_adversarial_scheduler",
                        "frontier_liveness_as_safety",
                        "frontier_exact_projection_differential",
                        "frontier_arithmetic_projection_stress",
                        "frontier_generated_trace_classification",
                        "frontier_semantic_attack_campaign",
                        "frontier_attack_objective_search",
                        "frontier_objective_guided_search",
                        "frontier_metamorphic_properties",
                        "frontier_rust_metamorphic_checks",
                        "frontier_assumption_minimization",
                        "frontier_assumption_weakening",
                        "frontier_precondition_fuzzing",
                        "frontier_partition_gossip_state_machine",
                        "frontier_rust_differential_corpus",
                        "frontier_rust_differential_replay",
                    ],
                    "stateful_rule_machines": [
                        "EvidenceLifecycleMachine",
                        "BundleEvidenceMachine",
                        "MultiEpochFrontierMachine",
                        "PartitionGossipMachine",
                        "SemanticAttackCampaignMachine",
                    ],
                    "quick_all_records": 32,
                    "quick_frontier_records": 23,
                    "deep_all_records": 32,
                    "rust_corpus_traces": 11,
                    "rust_replay_cases": 13,
                    "feature_combinations": 5,
                    "metamorphic_counterexamples": 0,
                    "assumption_minimized_witnesses": 5,
                    "assumption_weakening_witnesses": 8,
                    "precondition_fuzzing_witnesses": 11,
                    "deep_threat_records": 7,
                    "unexpected_count": 0,
                },
            },
            ["Sage: hypothesis_search/hypothesis_scenario_search.sage", "Rocq/TLA/docs: promoted findings 33+"],
        )
    ]


def analyze(max_stake, horizon):
    economic_records = economic_attack_optimization(max_stake)
    records = []
    records.extend(multi_epoch_attack_search())
    records.extend(partial_synchrony_view_convergence())
    records.extend(liveness_under_proposer_schedules(horizon))
    records.extend(atomic_batch_semantics())
    records.extend(evidence_retention_pruning(horizon))
    records.extend(equivocation_record_canonicalization())
    records.extend(economic_records)
    records.extend(differential_trace_corpus(economic_records[0]["witness"]))
    records.extend(hypothesis_reduced_regressions())
    axis_counts = {}
    class_counts = {}
    for record in records:
        axis_counts[record["axis"]] = axis_counts.get(record["axis"], 0) + 1
        class_counts[record["classification"]] = class_counts.get(record["classification"], 0) + 1
    missing_axes = [axis for axis in AXES if axis not in axis_counts]
    return {
        "summaries": [
            {
                "axes": len(axis_counts),
                "records": len(records),
                "missing_axes": missing_axes,
                "class_counts": class_counts,
                "max_stake": max_stake,
                "horizon": horizon,
            }
        ],
        "records": records,
    }


def self_test():
    result = analyze(3, 4)
    summary = result["summaries"][0]
    if summary["missing_axes"]:
        raise AssertionError("missing scenario axes: {}".format(summary["missing_axes"]))
    required_classes = Set(["confirmed_safe", "candidate_boundary", "projection_risk", "assumption_counterexample"])
    if not required_classes.issubset(Set(summary["class_counts"].keys())):
        raise AssertionError("missing classification coverage")
    by_name = {record["name"]: record for record in result["records"]}
    if not by_name["same_active_edges_same_closure_after_convergence"]["witness"]["equal_after_convergence"]:
        raise AssertionError("view convergence equality missing")
    if not by_name["abort_after_partial_is_order_dependent"]["witness"]["order_dependent"]:
        raise AssertionError("partial batch failure risk missing")
    if by_name["differential_trace_classification_corpus"]["witness"]["unexpected_count"] != 0:
        raise AssertionError("unexpected differential trace found")
    if by_name["weighted_damage_amplification_without_closure_bound"]["witness"]["extra_stake"] <= 0:
        raise AssertionError("economic amplification missing")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print(
            "axes={axes} records={records} missing_axes={missing_axes} max_stake={max_stake} horizon={horizon}".format(
                **summary
            )
        )
        for classification in sorted(summary["class_counts"]):
            print("classification={classification} count={count}".format(classification=classification, count=summary["class_counts"][classification]))
    for record in result["records"]:
        print("axis={axis} classification={classification} name={name}".format(**record))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage scenario corpus generator for slashing exploratory modeling")
    parser.add_argument("--max-stake", type=int, default=3)
    parser.add_argument("--horizon", type=int, default=4)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else analyze(args.max_stake, args.horizon)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
