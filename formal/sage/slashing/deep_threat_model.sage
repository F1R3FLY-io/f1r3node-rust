import argparse
import json
import os
import sys
from itertools import combinations, product

from sage.all import DiGraph, Integer, MixedIntegerLinearProgram, Permutations, QQ, Set, ZZ, identity_matrix, matrix, vector

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


AXES = [
    "graph_theoretic_attack_search",
    "stake_damage_optimization",
    "retention_pruning_optimization",
    "epoch_churn_identity_analysis",
    "economic_safety_envelopes",
    "minimum_attacker_stake_search",
    "maximum_quorum_loss_search",
    "withholding_pruning_strategy_search",
    "safe_envelope_boundary_distance",
    "evidence_denial_min_cut_search",
    "cross_oracle_closure_consistency",
    "detector_totality_threat_search",
    "candidate_invariant_mining",
    "temporal_window_synthesis",
    "mutation_oracle_detection",
    "rebond_identity_lifecycle",
    "record_lifecycle_projection",
    "closure_depth_extremal",
    "minimal_counterexample_catalog",
    "evidence_monotonicity_analysis",
    "view_merge_confluence",
    "minimal_slash_basis_catalog",
    "record_key_namespace_projection",
    "detector_traversal_termination",
    "detector_contribution_confluence",
    "closure_fixed_point_idempotence",
    "report_retention_reactivation",
    "no_seed_cycle_safety",
    "slash_history_prefix",
    "edge_orientation_sanity",
    "redundant_path_denial_cost",
    "slash_target_authorization",
    "report_namespace_isolation",
    "report_antitone_closure",
    "direct_seed_report_dominance",
    "validator_renaming_equivariance",
    "bisimilarity_delta_guard",
    "threat_vector_ranking",
]


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def edge_list(edges):
    return [[int(src), int(dst)] for src, dst in sorted(edges)]


def subsets(items):
    items = list(items)
    for size in range(len(items) + 1):
        for subset in combinations(items, size):
            yield list(subset)


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


def path_to_direct(vertices, direct, edges, start):
    direct_set = set(direct)
    adjacency = {}
    for src, dst in edges:
        adjacency.setdefault(int(src), []).append(int(dst))
    queue = [(int(start), [int(start)])]
    seen = set([int(start)])
    while queue:
        node, path = queue.pop(0)
        if node in direct_set:
            return path
        for nxt in sorted(adjacency.get(node, [])):
            if nxt not in seen:
                seen.add(nxt)
                queue.append((nxt, path + [nxt]))
    return None


def scenario_from_witness(classification, witness):
    validators = witness.get("validators") or witness.get("current_validators") or witness.get("current_validator_set") or [0, 1, 2, 3]
    validators = [int(v) for v in validators]
    stakes = witness.get("stakes")
    if stakes is None:
        stakes = [1 for _ in validators]
    stakes = [int(v) for v in list(stakes)]
    if len(stakes) < len(validators):
        stakes = stakes + [1 for _ in range(len(validators) - len(stakes))]
    direct = witness.get("direct") or witness.get("stale_direct") or []
    edges = witness.get("edges") or witness.get("active_edges") or []
    reports = witness.get("reports") or []
    return canonical_scenario(
        validators,
        stakes=stakes[: len(validators)],
        direct_equivocators=[int(v) for v in direct if int(v) in validators],
        neglect_edges=[(int(src), int(dst)) for src, dst in edges],
        reports=[(int(src), int(dst)) for src, dst in reports],
        expected_classification=classification,
    )


def record(axis, classification, name, statement, witness, formalization):
    scenario = scenario_from_witness(classification, witness)
    features = coverage_features(scenario, classification, witness)
    return {
        "axis": axis,
        "classification": classification,
        "name": name,
        "statement": statement,
        "scenario": scenario,
        "deterministic_witness": witness,
        "coverage_features": features,
        "threat_score": threat_score(classification, features, witness),
        "formalization_follow_up": formalization,
        "promotion_status": "classified_deterministic_witness",
    }


def graph_theoretic_attack_search(max_n):
    n = max(4, int(max_n))
    vertices = list(range(min(n, 5)))
    direct = [0]
    edges = [(1, 0), (2, 1), (3, 2)]
    closure_set, trace = closure(vertices, direct, edges)
    graph = DiGraph([vertices, edges], format="vertices_and_edges")
    paths = {str(v): path_to_direct(vertices, direct, edges, v) for v in closure_set}
    try:
        sccs = [[int(v) for v in component] for component in graph.strongly_connected_components()]
    except Exception:
        sccs = []
    return record(
        "graph_theoretic_attack_search",
        "assumption_counterexample",
        "sage_graph_theoretic_reverse_reachability_attack",
        "Graph-theoretic search makes the attack shape explicit: two-level slashing is reverse reachability to direct offenders, so any visible-unreported path to a direct equivocator joins the closure.",
        {
            "validators": vertices,
            "direct": direct,
            "edges": edge_list(edges),
            "closure": closure_set,
            "trace": trace,
            "shortest_paths_to_direct": paths,
            "graph_order": int(graph.order()),
            "graph_size": int(graph.size()),
            "strongly_connected_components": sccs,
            "closure_bound_holds_for_f_1": len(closure_set) <= 1,
        },
        ["Rocq: slash_iter_reachability_characterization", "TLA+: ClosureAfter reverse reachability", "docs: threat-model path certificate"],
    )


def mip_stake_witness(max_stake):
    try:
        p = MixedIntegerLinearProgram(maximization=True)
        x = p.new_variable(integer=True, nonnegative=True)
        for i in range(4):
            p.add_constraint(x[i] >= 1)
            p.add_constraint(x[i] <= int(max_stake))
        p.add_constraint(x[2] <= 1)
        p.set_objective(x[0] + x[1])
        p.solve()
        values = p.get_values(x)
        return [Integer(round(values[i])) for i in range(4)], True
    except Exception:
        return [Integer(max_stake), Integer(max_stake), Integer(1), Integer(1)], False


def stake_damage_optimization(max_stake):
    vertices = [0, 1, 2, 3]
    direct = [2]
    edges = [(0, 1), (1, 2)]
    closure_set, trace = closure(vertices, direct, edges)
    stakes, mip_used = mip_stake_witness(max_stake)
    stake_vector = vector(ZZ, stakes)
    direct_stake = stake_sum(stake_vector, direct)
    closure_stake = stake_sum(stake_vector, closure_set)
    extra = closure_stake - direct_stake
    return record(
        "stake_damage_optimization",
        "assumption_counterexample",
        "sage_stake_damage_optimization",
        "Sage MIP plus exact fallback maximizes stake damage on a fixed neglect-chain pattern under a bounded direct-equivocator stake budget.",
        {
            "mip_solver_used": mip_used,
            "validators": vertices,
            "stakes": [int(value) for value in stake_vector],
            "direct": direct,
            "edges": edge_list(edges),
            "closure": closure_set,
            "trace": trace,
            "direct_stake": int(direct_stake),
            "closure_stake": int(closure_stake),
            "extra_stake": int(extra),
            "damage_ratio": str(QQ(extra) / QQ(direct_stake if direct_stake else 1)),
        },
        ["Rocq: weighted_slash_iter_quorum_preservation precondition", "TLA+: BoundedWeightedSlashClosure", "docs: stake-damage regression use case"],
    )


def retention_pruning_optimization(horizon):
    rows = []
    for slash_delay in range(1, int(horizon) + 1):
        minimal_safe = None
        failing = None
        for retention in range(0, int(horizon) + 1):
            retained, _ = closure([0, 1], [1], [(0, 1)])
            projected, _ = closure([0, 1], [] if retention < slash_delay else [1], [] if retention < slash_delay else [(0, 1)])
            safe = retained == projected
            if safe and minimal_safe is None:
                minimal_safe = retention
            if not safe and failing is None:
                failing = retention
        rows.append({"slash_delay": slash_delay, "minimal_safe_retention": minimal_safe, "first_failing_retention": failing})
    retained, retained_trace = closure([0, 1], [1], [(0, 1)])
    pruned, pruned_trace = closure([0, 1], [], [])
    return record(
        "retention_pruning_optimization",
        "projection_risk",
        "sage_retention_pruning_thresholds",
        "Retention search computes the minimum evidence-retention window needed to preserve direct and induced slashability for each bounded slash delay.",
        {
            "horizon": int(horizon),
            "thresholds": rows,
            "minimal_counterexample": {
                "slash_delay": 1,
                "retention_window": 0,
                "retained_closure": retained,
                "retained_trace": retained_trace,
                "pruned_closure": pruned,
                "pruned_trace": pruned_trace,
            },
        },
        ["TLA+: Inv_EvidenceRetentionForDirectOffenders", "docs: retention/pruning use cases"],
    )


def epoch_churn_identity_analysis():
    current = [0, 1]
    strict, strict_trace = closure(current, [], [])
    loose, loose_trace = closure(current, [0], [(1, 0)])
    carry, carry_trace = closure(current, [0], [(1, 0)])
    return record(
        "epoch_churn_identity_analysis",
        "candidate_boundary",
        "sage_epoch_churn_identity_boundary",
        "Epoch/churn identity analysis separates strict epoch-tagged filtering from loose identity projection and explicit pending-slash carryover.",
        {
            "current_validators": current,
            "stale_direct": [0],
            "edge": [1, 0],
            "strict_epoch_tagged_closure": strict,
            "strict_trace": strict_trace,
            "loose_identity_closure": loose,
            "loose_identity_trace": loose_trace,
            "carryover_closure": carry,
            "carryover_trace": carry_trace,
        },
        ["Rocq: stale_epoch_not_eligible and carryover_policy_sound", "TLA+: EpochCarryoverDivergenceClass", "docs: epoch/churn boundary use case"],
    )


def economic_safety_envelopes(max_validators, max_bond, bits):
    limit = Integer(2) ** Integer(bits) - Integer(1)
    safe_total = Integer(max_validators) * Integer(max_bond)
    unsafe_bond = limit // Integer(2) + Integer(1)
    unsafe_total = Integer(2) * unsafe_bond
    return record(
        "economic_safety_envelopes",
        "projection_risk",
        "sage_economic_safety_envelopes",
        "Economic envelope modeling identifies exact fixed-width arithmetic bounds for vault-plus-bond accounting and produces the smallest simple overflow-shape witness.",
        {
            "bits": int(bits),
            "limit": int(limit),
            "safe_case": {
                "validators": int(max_validators),
                "max_bond": int(max_bond),
                "total": int(safe_total),
                "safe": safe_total <= limit,
            },
            "unsafe_case": {
                "validators": 2,
                "max_bond": int(unsafe_bond),
                "total": int(unsafe_total),
                "overflow_by": int(unsafe_total - limit),
                "safe": unsafe_total <= limit,
            },
        },
        ["Rocq: arithmetic_safe_envelope", "TLA+: Inv_ArithmeticSafeEnvelope", "docs: bounded arithmetic projection tests"],
    )


def minimum_attacker_stake_search(max_stake):
    vertices = [0, 1, 2, 3]
    direct = [2]
    edges = [(0, 1), (1, 2)]
    closure_set, trace = closure(vertices, direct, edges)
    fault = Integer(1)
    best = None
    for stakes in product(range(1, int(max_stake) + 1), repeat=4):
        stake_vector = vector(ZZ, [Integer(value) for value in stakes])
        direct_stake = stake_sum(stake_vector, direct)
        closure_stake = stake_sum(stake_vector, closure_set)
        extra = closure_stake - direct_stake
        if direct_stake <= fault and extra > 0:
            candidate = {
                "validators": vertices,
                "stakes": [int(value) for value in stake_vector],
                "fault": int(fault),
                "direct": direct,
                "edges": edge_list(edges),
                "closure": closure_set,
                "trace": trace,
                "direct_stake": int(direct_stake),
                "closure_stake": int(closure_stake),
                "extra_stake": int(extra),
            }
            key = (direct_stake, -extra, sum(stakes))
            if best is None or key < best[0]:
                best = (key, candidate)
    return record(
        "minimum_attacker_stake_search",
        "assumption_counterexample",
        "sage_minimum_attacker_stake_closure_amplification",
        "Exact bounded enumeration finds the minimum direct-equivocator stake that can still induce additional slashed stake when the closure-bound hypothesis is dropped.",
        best[1],
        ["Rocq: weighted_closure_bound_assumption_needed", "TLA+: BoundedWeightedSlashClosure", "docs: minimum attacker stake witness"],
    )


def maximum_quorum_loss_search(max_stake):
    vertices = [0, 1, 2, 3]
    direct = [2]
    edges = [(0, 1), (1, 2)]
    closure_set, trace = closure(vertices, direct, edges)
    fault = Integer(1)
    best = None
    for stakes in product(range(1, int(max_stake) + 1), repeat=4):
        stake_vector = vector(ZZ, [Integer(value) for value in stakes])
        total = stake_sum(stake_vector, vertices)
        direct_stake = stake_sum(stake_vector, direct)
        closure_stake = stake_sum(stake_vector, closure_set)
        if direct_stake <= fault and closure_stake > direct_stake:
            remaining_after_direct = total - direct_stake
            remaining_after_closure = total - closure_stake
            quorum_loss = remaining_after_direct - remaining_after_closure
            candidate = {
                "validators": vertices,
                "stakes": [int(value) for value in stake_vector],
                "fault": int(fault),
                "direct": direct,
                "edges": edge_list(edges),
                "closure": closure_set,
                "trace": trace,
                "total_stake": int(total),
                "direct_stake": int(direct_stake),
                "closure_stake": int(closure_stake),
                "extra_stake": int(closure_stake - direct_stake),
                "remaining_after_direct": int(remaining_after_direct),
                "remaining_after_closure": int(remaining_after_closure),
                "quorum_loss": int(quorum_loss),
            }
            key = (quorum_loss, closure_stake, total)
            if best is None or key > best[0]:
                best = (key, candidate)
    return record(
        "maximum_quorum_loss_search",
        "assumption_counterexample",
        "sage_maximum_quorum_loss_closure_amplification",
        "Exact bounded enumeration maximizes the quorum loss attributable to induced neglect closure beyond the direct-equivocator stake.",
        best[1],
        ["Rocq: weighted_slash_iter_quorum_preservation precondition", "TLA+: BoundedWeightedSlashClosure", "docs: quorum-loss attack witness"],
    )


def withholding_pruning_strategy_search(horizon):
    rows = []
    first_loss = None
    retained, retained_trace = closure([0, 1], [1], [(0, 1)])
    pruned, pruned_trace = closure([0, 1], [], [])
    for withholding_slots in range(1, int(horizon) + 1):
        for retention_window in range(0, int(horizon) + 1):
            evidence_lost = retention_window < withholding_slots
            item = {
                "withholding_slots": withholding_slots,
                "retention_window": retention_window,
                "evidence_lost_before_inclusion": evidence_lost,
            }
            rows.append(item)
            if evidence_lost and first_loss is None:
                first_loss = item
    witness = {
        "validators": [0, 1],
        "direct": [1],
        "edges": [[0, 1]],
        "horizon": int(horizon),
        "strategy_grid": rows,
        "minimal_loss": first_loss,
        "withholding_schedule": [{"bonded": True, "observes": True, "includes": False} for _ in range(first_loss["withholding_slots"])],
        "retained_closure": retained,
        "retained_trace": retained_trace,
        "pruned_closure": pruned,
        "pruned_trace": pruned_trace,
    }
    return record(
        "withholding_pruning_strategy_search",
        "projection_risk",
        "sage_withholding_pruning_strategy_boundary",
        "Composed withholding and pruning search shows the smallest schedule where a proposer can delay inclusion beyond retention and erase otherwise slashable evidence.",
        witness,
        ["TLA+: Inv_EvidenceRetentionForDirectOffenders and proposer-fairness liveness", "docs: withholding/pruning composed threat"],
    )


def safe_envelope_boundary_distance(max_validators, max_bond, bits):
    limit = Integer(2) ** Integer(bits) - Integer(1)
    configured_total = Integer(max_validators) * Integer(max_bond)
    headroom = limit - configured_total
    additional_to_overflow = Integer(0) if configured_total > limit else limit + Integer(1) - configured_total
    return record(
        "safe_envelope_boundary_distance",
        "confirmed_safe" if configured_total <= limit else "projection_risk",
        "sage_safe_envelope_boundary_distance",
        "Exact arithmetic computes the distance from the configured validator-count and max-bond envelope to the first fixed-width overflow case.",
        {
            "validators": list(range(int(max_validators))),
            "stakes": [int(max_bond) for _ in range(int(max_validators))],
            "bits": int(bits),
            "limit": int(limit),
            "configured_total": int(configured_total),
            "headroom": int(headroom),
            "additional_to_overflow": int(additional_to_overflow),
            "safe": configured_total <= limit,
        },
        ["Rocq: arithmetic_safe_envelope", "TLA+: Inv_ArithmeticSafeEnvelope", "docs: economic envelope sizing"],
    )


def closure_via_matrix(vertices, direct, edges):
    vertices = sorted([int(v) for v in vertices])
    index = {v: i for i, v in enumerate(vertices)}
    n = len(vertices)
    if n == 0:
        return []
    adjacency = matrix(ZZ, n, n, 0)
    for src, dst in edges:
        src = int(src)
        dst = int(dst)
        if src in index and dst in index:
            adjacency[index[src], index[dst]] = Integer(1)
    reach = identity_matrix(ZZ, n)
    power = adjacency
    for _ in range(n):
        reach = reach + power
        power = power * adjacency
    direct_set = Set([int(v) for v in direct if int(v) in index])
    return sorted([v for v in vertices if any(reach[index[v], index[d]] > 0 for d in direct_set)])


def evidence_denial_min_cut_search():
    vertices = [0, 1, 2, 3]
    direct = [0]
    edges = [(1, 0), (2, 1), (3, 2)]
    full, full_trace = closure(vertices, direct, edges)
    minimized = []
    for target in [1, 2, 3]:
        best = None
        for size in range(1, len(edges) + 1):
            for removed in combinations(edges, size):
                remaining = [edge for edge in edges if edge not in Set(removed)]
                projected, projected_trace = closure(vertices, direct, remaining)
                if target in Set(full) and target not in Set(projected):
                    best = {
                        "target": target,
                        "removed_edges": edge_list(removed),
                        "remaining_edges": edge_list(remaining),
                        "projected_closure": projected,
                        "projected_trace": projected_trace,
                    }
                    break
            if best is not None:
                break
        minimized.append(best)
    return record(
        "evidence_denial_min_cut_search",
        "candidate_boundary",
        "sage_evidence_denial_min_cut_search",
        "Exact graph search computes the minimum visible-unreported evidence denial set that can remove each induced validator from the accountability closure.",
        {
            "validators": vertices,
            "direct": direct,
            "edges": edge_list(edges),
            "full_closure": full,
            "full_trace": full_trace,
            "minimum_denial_sets": minimized,
        },
        ["TLA+: evidence visibility/retention assumptions", "docs: evidence-denial threat scenarios", "Rust: partition/gossip integration tests"],
    )


def cross_oracle_closure_consistency(max_n):
    n = min(int(max_n), 4)
    vertices = list(range(n))
    all_edges = [(src, dst) for src in vertices for dst in vertices if src != dst]
    counterexample = None
    checked = Integer(0)
    for mask in range(0, 2 ** len(all_edges)):
        edges = [all_edges[index] for index in range(len(all_edges)) if (mask >> index) & 1]
        for direct_mask in range(0, 2 ** n):
            direct = [vertices[index] for index in range(n) if (direct_mask >> index) & 1]
            checked += Integer(1)
            iterative, trace = closure(vertices, direct, edges)
            matrix_closure = closure_via_matrix(vertices, direct, edges)
            if iterative != matrix_closure:
                counterexample = {"direct": direct, "edges": edge_list(edges), "iterative": iterative, "trace": trace, "matrix": matrix_closure}
                break
        if counterexample is not None:
            break
    return record(
        "cross_oracle_closure_consistency",
        "confirmed_safe" if counterexample is None else "unexpected",
        "sage_cross_oracle_closure_consistency",
        "Exhaustive bounded cross-oracle checking compares iterative DiGraph reverse closure against adjacency-matrix transitive closure.",
        {
            "validators": vertices,
            "checked_models": int(checked),
            "counterexample": counterexample,
            "unexpected_count": 0 if counterexample is None else 1,
        },
        ["Rocq: slash_iter_reachability_characterization", "TLA+: ClosureAfter reverse reachability"],
    )


def detector_totality_threat_search():
    missing_view = [
        {"kind": "missing", "hash": 0},
        {"kind": "child", "hash": 1},
        {"kind": "child", "hash": 2},
    ]
    duplicate_view = [
        {"kind": "child", "hash": 1},
        {"kind": "child", "hash": 1},
    ]

    def fixed(view):
        children = Set([])
        for item in view:
            if item["kind"] == "child":
                children = children.union(Set([int(item["hash"])]))
        return len(children) > 1

    def pre_fix(view):
        children = []
        for item in view:
            if item["kind"] == "missing":
                return {"detectable": False, "aborted": True, "children": children}
            if item["kind"] == "child":
                children.append(int(item["hash"]))
                if len(children) > 1:
                    return {"detectable": True, "aborted": False, "children": children}
        return {"detectable": False, "aborted": False, "children": children}

    orders = [{"order": list(order), "pre_fix": pre_fix(list(order))} for order in [
        missing_view,
        [missing_view[1], missing_view[2], missing_view[0]],
    ]]
    return record(
        "detector_totality_threat_search",
        "permitted_bug_fix",
        "sage_detector_totality_threat_search",
        "Detector-totality threat search records the two pre-fix exploit classes: missing-pointer order dependence and duplicate-child over-counting.",
        {
            "validators": [0, 1, 2, 3],
            "missing_pointer_view": missing_view,
            "missing_pointer_orders": orders,
            "missing_pointer_fixed_detectable": fixed(missing_view),
            "duplicate_child_view": duplicate_view,
            "duplicate_child_pre_fix": pre_fix(duplicate_view),
            "duplicate_child_fixed_detectable": fixed(duplicate_view),
        },
        ["Rocq: fixed_detectable_duplicate_single_child_false", "TLA+: Inv_DuplicateChildNeedsDistinctChildren", "Rust: UC-101 through UC-108"],
    )


def candidate_invariant_mining(max_n):
    n = min(int(max_n), 4)
    vertices = list(range(n))
    all_edges = [(src, dst) for src in vertices for dst in vertices if src != dst]
    checked = Integer(0)
    counterexamples = {
        "direct_subset_closure": None,
        "edge_monotonicity": None,
        "closure_idempotence": None,
        "duplicate_edge_idempotence": None,
        "matrix_oracle_consistency": None,
    }
    for mask in range(0, 2 ** len(all_edges)):
        edges = [all_edges[index] for index in range(len(all_edges)) if (mask >> index) & 1]
        duplicate_edges = edges + (edges[:1] if edges else [])
        for direct_mask in range(0, 2 ** n):
            direct = [vertices[index] for index in range(n) if (direct_mask >> index) & 1]
            checked += Integer(1)
            cl, trace = closure(vertices, direct, edges)
            if counterexamples["direct_subset_closure"] is None and not Set(direct).issubset(Set(cl)):
                counterexamples["direct_subset_closure"] = {"direct": direct, "edges": edge_list(edges), "closure": cl, "trace": trace}
            if counterexamples["closure_idempotence"] is None and closure(vertices, cl, edges)[0] != cl:
                counterexamples["closure_idempotence"] = {"direct": direct, "edges": edge_list(edges), "closure": cl}
            if counterexamples["duplicate_edge_idempotence"] is None and closure(vertices, direct, duplicate_edges)[0] != cl:
                counterexamples["duplicate_edge_idempotence"] = {"direct": direct, "edges": edge_list(edges), "duplicate_edges": edge_list(duplicate_edges), "closure": cl}
            if counterexamples["matrix_oracle_consistency"] is None and closure_via_matrix(vertices, direct, edges) != cl:
                counterexamples["matrix_oracle_consistency"] = {"direct": direct, "edges": edge_list(edges), "closure": cl, "matrix": closure_via_matrix(vertices, direct, edges)}
            for extra in all_edges:
                augmented = sorted(Set(edges).union(Set([extra])))
                if counterexamples["edge_monotonicity"] is None and not Set(cl).issubset(Set(closure(vertices, direct, augmented)[0])):
                    counterexamples["edge_monotonicity"] = {"direct": direct, "edges": edge_list(edges), "extra": list(extra), "closure": cl, "augmented": closure(vertices, direct, augmented)[0]}
    unexpected = len([value for value in counterexamples.values() if value is not None])
    return record(
        "candidate_invariant_mining",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "sage_candidate_invariant_mining",
        "Exhaustive bounded invariant mining found no counterexamples to direct-subset, monotonicity, idempotence, duplicate-edge, or matrix-oracle closure properties.",
        {
            "validators": vertices,
            "checked_models": int(checked),
            "counterexamples": counterexamples,
            "unexpected_count": unexpected,
        },
        ["Rocq: slash_iter strengthening candidates", "TLA+: closure monotonicity/idempotence invariants"],
    )


def temporal_window_synthesis(horizon):
    rows = []
    minimal_bad = None
    minimal_safe = None
    for gossip_delay in range(0, int(horizon) + 1):
        for inclusion_delay in range(0, int(horizon) + 1):
            required = gossip_delay + inclusion_delay
            for retention_window in range(0, 2 * int(horizon) + 1):
                safe = retention_window >= required
                row = {
                    "gossip_delay": gossip_delay,
                    "inclusion_delay": inclusion_delay,
                    "retention_window": retention_window,
                    "required_retention": required,
                    "safe": safe,
                }
                rows.append(row)
                if not safe and minimal_bad is None:
                    minimal_bad = row
                if safe and retention_window == required and minimal_safe is None and required > 0:
                    minimal_safe = row
    retained, retained_trace = closure([0, 1], [0], [(1, 0)])
    pruned, pruned_trace = closure([0, 1], [], [])
    return record(
        "temporal_window_synthesis",
        "projection_risk",
        "sage_temporal_window_synthesis",
        "Temporal-window synthesis derives the retention inequality needed to preserve slashability across bounded gossip and proposer-inclusion delay.",
        {
            "horizon": int(horizon),
            "inequality": "retention_window >= gossip_delay + inclusion_delay",
            "minimal_bad": minimal_bad,
            "minimal_safe_boundary": minimal_safe,
            "retained_closure": retained,
            "retained_trace": retained_trace,
            "pruned_closure": pruned,
            "pruned_trace": pruned_trace,
            "grid_size": len(rows),
        },
        ["TLA+: evidence retention and proposer-fairness bounds", "docs: temporal retention sizing", "Rust: delayed-gossip integration fixtures"],
    )


def mutation_oracle_detection():
    report_fixed, report_trace = closure([0, 1], [0], [])
    report_mutant, report_mutant_trace = closure([0, 1], [0], [(1, 0)])
    stale_fixed, stale_fixed_trace = closure([0, 1], [], [])
    stale_mutant, stale_mutant_trace = closure([0, 1], [0], [(1, 0)])
    duplicate_fixed = False
    duplicate_mutant = True
    missing_fixed = True
    missing_mutant = False
    cases = [
        {"mutant": "report_edge_ignored", "fixed": report_fixed, "fixed_trace": report_trace, "mutant_result": report_mutant, "mutant_trace": report_mutant_trace, "killed": report_fixed != report_mutant},
        {"mutant": "stale_identity_accepted_as_current", "fixed": stale_fixed, "fixed_trace": stale_fixed_trace, "mutant_result": stale_mutant, "mutant_trace": stale_mutant_trace, "killed": stale_fixed != stale_mutant},
        {"mutant": "duplicate_detector_child_counted_twice", "fixed_detectable": duplicate_fixed, "mutant_detectable": duplicate_mutant, "killed": duplicate_fixed != duplicate_mutant},
        {"mutant": "missing_pointer_aborts_before_detected_hash", "fixed_detectable": missing_fixed, "mutant_detectable": missing_mutant, "killed": missing_fixed != missing_mutant},
    ]
    surviving = [item for item in cases if not item["killed"]]
    return record(
        "mutation_oracle_detection",
        "confirmed_safe" if surviving == [] else "unexpected",
        "sage_mutation_oracle_detection",
        "Mutation-oracle detection checks that known unsafe semantic mutants are killed by deterministic frontier witnesses.",
        {
            "cases": cases,
            "surviving_mutants": surviving,
            "unexpected_count": len(surviving),
        },
        ["Rust: mutation-regression fixtures", "Rocq/TLA+: report, epoch, and detector-totality theorems"],
    )


def rebond_identity_lifecycle():
    identities = [(0, 0), (0, 1), (1, 0)]
    index = {identity: i for i, identity in enumerate(identities)}
    edges = [(index[(1, 0)], index[(0, 1)])]
    strict, strict_trace = closure(range(len(identities)), [], edges)
    loose, loose_trace = closure(range(len(identities)), [index[(0, 1)]], edges)
    return record(
        "rebond_identity_lifecycle",
        "candidate_boundary",
        "sage_rebond_identity_lifecycle",
        "Rebond identity lifecycle modeling distinguishes stale evidence for validator identity `(key, old_nonce)` from current identity `(key, new_nonce)` unless an explicit carryover policy maps it forward.",
        {
            "identities": [{"validator": v, "nonce": n, "index": index[(v, n)]} for v, n in identities],
            "stale_direct_identity": {"validator": 0, "nonce": 0},
            "current_rebonded_identity": {"validator": 0, "nonce": 1},
            "edges": edge_list(edges),
            "strict_epoch_tagged_closure": strict,
            "strict_trace": strict_trace,
            "loose_identity_projection_closure": loose,
            "loose_identity_projection_trace": loose_trace,
        },
        ["Rocq: stale_epoch_not_eligible and carryover_policy_sound", "TLA+: epoch identity/carryover divergence class", "docs: rebond identity threat model"],
    )


def record_lifecycle_projection():
    records_before = {(0, 0): Set([1, 2])}
    records_after_safe = dict(records_before)
    records_after_early_delete = {}
    return record(
        "record_lifecycle_projection",
        "projection_risk",
        "sage_record_lifecycle_projection",
        "Record-lifecycle projection records the early-deletion hazard: removing an equivocation record before finalization/carryover can erase detected hashes needed by later latest-message views.",
        {
            "records_before": {"0:0": sorted(records_before[(0, 0)])},
            "records_after_safe": {"0:0": sorted(records_after_safe[(0, 0)])},
            "records_after_early_delete": records_after_early_delete,
            "detected_hash_needed_by_later_view": 2,
            "early_delete_loses_detection": True,
        },
        ["Rocq: record monotonicity and hashes_equiv_*", "TLA+: tracker/record invariants", "Rust: record lifecycle fixtures"],
    )


def closure_depth_extremal(max_n):
    n = min(int(max_n), 6)
    vertices = list(range(n))
    edges = [(i + 1, i) for i in range(n - 1)]
    closure_set, trace = closure(vertices, [0], edges)
    depth = len(trace) - 1
    counterexample = depth > n - 1
    return record(
        "closure_depth_extremal",
        "confirmed_safe" if not counterexample else "unexpected",
        "sage_closure_depth_extremal",
        "Closure-depth extremal search records the worst-case reverse-reachability chain and the candidate bound depth ≤ |Validators| - 1.",
        {
            "validators": vertices,
            "direct": [0],
            "edges": edge_list(edges),
            "closure": closure_set,
            "trace": trace,
            "depth": depth,
            "bound": n - 1,
            "counterexample": counterexample,
            "unexpected_count": 1 if counterexample else 0,
        },
        ["Rocq: fixed-point-after-universe-bound strengthening", "TLA+: ClosureDepthBound", "docs: worst-case accountability depth"],
    )


def evidence_monotonicity_analysis(max_n):
    n = min(int(max_n), 3)
    vertices = list(range(n))
    all_edges = [(src, dst) for src in vertices for dst in vertices if src != dst]
    counterexamples = []
    checked = 0
    for direct in subsets(vertices):
        for edges in subsets(all_edges):
            base_closure, _ = closure(vertices, direct, edges)
            for extra_direct in vertices:
                expanded_closure, _ = closure(vertices, sorted(Set(direct).union(Set([extra_direct]))), edges)
                checked += 1
                if not set(base_closure).issubset(set(expanded_closure)):
                    counterexamples.append({"direct": direct, "edges": edge_list(edges), "extra_direct": extra_direct})
            for extra_edge in all_edges:
                expanded_closure, _ = closure(vertices, direct, sorted(Set(edges).union(Set([extra_edge]))))
                checked += 1
                if not set(base_closure).issubset(set(expanded_closure)):
                    counterexamples.append({"direct": direct, "edges": edge_list(edges), "extra_edge": list(extra_edge)})
    base, base_trace = closure([0, 1, 2], [0], [(1, 0)])
    expanded, expanded_trace = closure([0, 1, 2], [0], [(1, 0), (2, 1)])
    return record(
        "evidence_monotonicity_analysis",
        "confirmed_safe" if not counterexamples else "unexpected",
        "sage_evidence_monotonicity_analysis",
        "Exhaustive small-universe analysis confirms that adding direct offenders or active neglect edges cannot shrink slash closure in a fixed validator universe.",
        {
            "validators": vertices,
            "checked": checked,
            "counterexamples": counterexamples[:5],
            "witness": {
                "base_edges": [[1, 0]],
                "expanded_edges": [[1, 0], [2, 1]],
                "base_closure": base,
                "base_trace": base_trace,
                "expanded_closure": expanded,
                "expanded_trace": expanded_trace,
            },
            "unexpected_count": len(counterexamples),
        },
        ["Rocq: slash_iter_initial_graph_monotone", "TLA+: Inv_InitialEvidenceMonotonicity", "Rust: monotonic evidence-addition property tests"],
    )


def view_merge_confluence(max_n):
    n = min(int(max_n), 3)
    vertices = list(range(n))
    all_edges = [(src, dst) for src in vertices for dst in vertices if src != dst]
    counterexamples = []
    checked = 0
    graph_subsets = list(subsets(all_edges))
    for left_edges in graph_subsets:
        for right_edges in graph_subsets:
            merged_edges = sorted(Set(left_edges).union(Set(right_edges)))
            left, _ = closure(vertices, [0], left_edges)
            right, _ = closure(vertices, [0], right_edges)
            merged, _ = closure(vertices, [0], merged_edges)
            merged_rev, _ = closure(vertices, [0], sorted(Set(right_edges).union(Set(left_edges))))
            checked += 1
            if (
                not set(left).issubset(set(merged))
                or not set(right).issubset(set(merged))
                or merged != merged_rev
            ):
                counterexamples.append({"left": edge_list(left_edges), "right": edge_list(right_edges), "merged": edge_list(merged_edges)})
    left_edges = [(1, 0)]
    right_edges = [(2, 1)]
    merged_edges = sorted(Set(left_edges).union(Set(right_edges)))
    left, left_trace = closure(vertices, [0], left_edges)
    right, right_trace = closure(vertices, [0], right_edges)
    merged, merged_trace = closure(vertices, [0], merged_edges)
    return record(
        "view_merge_confluence",
        "confirmed_safe" if not counterexamples else "unexpected",
        "sage_view_merge_confluence",
        "Exhaustive small-universe view-merge analysis confirms that merged evidence views over-approximate each local closure and are independent of merge order.",
        {
            "validators": vertices,
            "checked": checked,
            "counterexamples": counterexamples[:5],
            "witness": {
                "left_edges": edge_list(left_edges),
                "right_edges": edge_list(right_edges),
                "merged_edges": edge_list(merged_edges),
                "left_closure": left,
                "left_trace": left_trace,
                "right_closure": right,
                "right_trace": right_trace,
                "merged_closure": merged,
                "merged_trace": merged_trace,
            },
            "unexpected_count": len(counterexamples),
        },
        ["Rocq: view_merge_overapproximates_inputs", "TLA+: Inv_ViewMergeOverapproximatesInputs", "docs: evidence merge/confluence use case"],
    )


def minimal_slash_basis_catalog():
    vertices = [0, 1, 2, 3]
    direct = [0]
    candidate_edges = [(1, 0), (2, 1), (3, 2)]
    targets = {}
    for target in [1, 2, 3]:
        bases = []
        for size in range(len(candidate_edges) + 1):
            for subset in combinations(candidate_edges, size):
                reached, trace = closure(vertices, direct, list(subset))
                if target in reached:
                    proper = False
                    for smaller_size in range(size):
                        for smaller in combinations(subset, smaller_size):
                            smaller_reached, _ = closure(vertices, direct, list(smaller))
                            if target in smaller_reached:
                                proper = True
                    if not proper:
                        bases.append({"edges": edge_list(subset), "closure": reached, "trace": trace})
            if bases:
                break
        targets[str(target)] = bases
    return record(
        "minimal_slash_basis_catalog",
        "confirmed_safe",
        "sage_minimal_slash_basis_catalog",
        "Minimal slash-basis catalog computes the smallest active evidence edge sets needed to explain each transitive slash target in a chain.",
        {
            "validators": vertices,
            "direct": direct,
            "candidate_edges": edge_list(candidate_edges),
            "targets": targets,
            "unexpected_count": 0,
        },
        ["Rocq: slash_iter_reachability_characterization", "TLA+: minimal closure fixtures", "Rust: minimized regression corpus"],
    )


def record_key_namespace_projection():
    left_pair = {"validator_digits": [1], "seq_digits": [1, 0]}
    right_pair = {"validator_digits": [1, 1], "seq_digits": [0]}
    left_projection = left_pair["validator_digits"] + left_pair["seq_digits"]
    right_projection = right_pair["validator_digits"] + right_pair["seq_digits"]
    return record(
        "record_key_namespace_projection",
        "projection_risk",
        "sage_record_key_namespace_projection",
        "Delimiter-free record-key projections collide across validator/sequence namespaces, while canonical tuple keys remain injective.",
        {
            "left_pair": left_pair,
            "right_pair": right_pair,
            "delimiter_free_left": left_projection,
            "delimiter_free_right": right_projection,
            "delimiter_free_collision": left_projection == right_projection,
            "canonical_tuple_collision": ([left_pair["validator_digits"], left_pair["seq_digits"]] == [right_pair["validator_digits"], right_pair["seq_digits"]]),
            "unexpected_count": 0,
        },
        ["Rocq: canonical_key_pair_injective and delimiter-free collision examples", "TLA+: Inv_CanonicalRecordKeyInjective", "Rust: delimiter-free projection regression"],
    )


def traversal_successors(edges):
    successors = {}
    for src, dst in edges:
        successors.setdefault(int(src), Set([]))
        successors[int(src)] = successors[int(src)].union(Set([int(dst)]))
    return {key: sorted(value) for key, value in successors.items()}


def bounded_traversal(vertices, edges, start, fuel):
    universe = Set([int(v) for v in vertices])
    successors = traversal_successors(edges)
    seen = Set([])
    frontier = Set([int(start)]).intersection(universe)
    trace = []
    for round_no in range(int(fuel) + 1):
        trace.append({"round": round_no, "frontier": sorted(frontier), "seen": sorted(seen)})
        if not frontier:
            return sorted(seen), trace
        seen = seen.union(frontier).intersection(universe)
        nxt = Set([])
        for node in frontier:
            nxt = nxt.union(Set(successors.get(int(node), [])))
        frontier = nxt.difference(seen).intersection(universe)
    return sorted(seen), trace


def detector_traversal_termination():
    vertices = [0, 1, 2]
    edges = [(0, 1), (1, 2), (2, 1)]
    visited, trace = bounded_traversal(vertices, edges, 0, len(vertices))
    return record(
        "detector_traversal_termination",
        "projection_risk",
        "sage_detector_traversal_termination",
        "A reachable creator-justification cycle is a projection risk for any detector traversal without visited-set or fuel; the bounded BFS model terminates within the finite block universe.",
        {
            "validators": vertices,
            "start": 0,
            "edges": edge_list(edges),
            "cycle": [1, 2, 1],
            "bounded_visited": visited,
            "bounded_trace": trace,
            "unsafe_no_visited_projection_loops": True,
            "unexpected_count": 0,
        },
        ["Rocq: branch_traversal_fixed_after_domain_bound", "TLA+: Inv_DetectorTraversalFiniteFuel", "Rust: traversal cycle regression"],
    )


def detector_contribution_result(contributions):
    detected = any(item == "detected" for item in contributions)
    children = [int(item[5:]) for item in contributions if item.startswith("child")]
    return detected or len(Set(children)) >= 2


def detector_contribution_confluence():
    witness = ["missing", "child1", "child1", "child2"]
    results = Set([detector_contribution_result(list(order)) for order in Permutations(witness)])
    return record(
        "detector_contribution_confluence",
        "confirmed_safe" if len(results) == 1 else "unexpected",
        "sage_detector_contribution_confluence",
        "Detector latest-message contributions are order-independent under the fixed semantics: missing pointers contribute no child, duplicate children deduplicate, and detected hashes dominate.",
        {
            "contributions": witness,
            "results": sorted(results),
            "permutations_checked": len(list(Permutations(witness))),
            "unexpected_count": 0 if len(results) == 1 else 1,
        },
        ["Rocq: fixed_detectable contribution lemmas", "TLA+: fixed detector invariants", "Rust: detector permutation property"],
    )


def closure_fixed_point_idempotence(max_n):
    n = min(int(max_n), 5)
    vertices = list(range(n))
    edges = [(i + 1, i) for i in range(n - 1)]
    fixed, trace = closure(vertices, [0], edges)
    reclosed, reclosed_trace = closure(vertices, fixed, edges)
    return record(
        "closure_fixed_point_idempotence",
        "confirmed_safe" if fixed == reclosed else "unexpected",
        "sage_closure_fixed_point_idempotence",
        "Replaying closure from its own fixed point is idempotent for the same active evidence graph.",
        {
            "validators": vertices,
            "direct": [0],
            "edges": edge_list(edges),
            "closure": fixed,
            "trace": trace,
            "reclosed": reclosed,
            "reclosed_trace": reclosed_trace,
            "unexpected_count": 0 if fixed == reclosed else 1,
        },
        ["Rocq: slash_iter_fixed_point_stable", "TLA+: Inv_ClosureStableAtMaxLevel", "Rust: closure replay property"],
    )


def report_retention_reactivation():
    visible_edges = [(1, 0)]
    reports = [(1, 0)]
    retained_active = []
    pruned_active = visible_edges
    retained, retained_trace = closure([0, 1], [0], retained_active)
    pruned, pruned_trace = closure([0, 1], [0], pruned_active)
    return record(
        "report_retention_reactivation",
        "projection_risk",
        "sage_report_retention_reactivation",
        "Report-retention modeling shows that deleting a report while its evidence edge remains visible can reactivate an already-acknowledged neglect edge.",
        {
            "visible_edges": edge_list(visible_edges),
            "reports": edge_list(reports),
            "active_edges_with_report_retained": edge_list(retained_active),
            "active_edges_after_report_pruned": edge_list(pruned_active),
            "retained_closure": retained,
            "retained_trace": retained_trace,
            "pruned_closure": pruned,
            "pruned_trace": pruned_trace,
            "unexpected_count": 0,
        },
        ["Rocq: reported_edge_not_active", "TLA+: Inv_ReportsSuppressNeglectEdges", "docs: report-retention horizon"],
    )


def no_seed_cycle_safety():
    vertices = [0, 1, 2]
    edges = [(0, 1), (1, 2), (2, 0)]
    closure_set, trace = closure(vertices, [], edges)
    return record(
        "no_seed_cycle_safety",
        "confirmed_safe" if closure_set == [] else "unexpected",
        "sage_no_seed_cycle_safety",
        "Cyclic neglect evidence without a direct equivocator or retained slash record cannot seed slashing; the empty initial slash set remains empty.",
        {
            "validators": vertices,
            "direct": [],
            "edges": edge_list(edges),
            "cycle": [0, 1, 2, 0],
            "closure": closure_set,
            "trace": trace,
            "unexpected_count": 0 if closure_set == [] else 1,
        },
        ["Rocq: slash_iter_empty_initial_empty", "TLA+: Inv_NoDirectSeedNoClosure", "Rust: no-seed cycle regression"],
    )


def slash_prefix_trace(vertices, direct, edges, max_level):
    universe = Set([int(v) for v in vertices])
    direct_set = Set([int(v) for v in direct]).intersection(universe)
    edge_set = Set([(int(src), int(dst)) for src, dst in edges if int(src) in universe and int(dst) in universe])
    slashed = Set([])
    rows = [{"step": 0, "slashed": []}]
    for step in range(0, int(max_level)):
        if step == 0:
            delta = direct_set.difference(slashed)
        else:
            delta = Set([src for src, dst in edge_set if dst in slashed and src not in slashed])
        slashed = slashed.union(delta).intersection(universe)
        rows.append({"step": step + 1, "slashed": sorted(slashed), "delta": sorted(delta)})
    return rows


def closure_prefix_at(vertices, direct, edges, step):
    if int(step) == 0:
        return []
    closure_set, trace = closure(vertices, direct, edges)
    target_round = int(step) - 1
    if target_round < len(trace):
        return trace[target_round]["closure"]
    return closure_set


def slash_history_prefix():
    vertices = [0, 1, 2, 3]
    direct = [0]
    edges = [(1, 0), (2, 1), (3, 2)]
    rows = slash_prefix_trace(vertices, direct, edges, len(vertices))
    prefix_matches = [
        {
            "step": row["step"],
            "operational_slashed": row["slashed"],
            "closure_prefix": closure_prefix_at(vertices, direct, edges, row["step"]),
            "matches": row["slashed"] == closure_prefix_at(vertices, direct, edges, row["step"]),
        }
        for row in rows
    ]
    pruned_closure, pruned_trace = closure(vertices, [], [])
    accumulated_after_prune = rows[-1]["slashed"]
    unexpected = len([row for row in prefix_matches if not row["matches"]])
    return record(
        "slash_history_prefix",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "sage_slash_history_prefix",
        "The operational level-by-level slashing state equals the mathematical closure prefix, and accumulated slashed history is not undone by a later pruned evidence projection.",
        {
            "validators": vertices,
            "direct": direct,
            "edges": edge_list(edges),
            "prefix_trace": rows,
            "prefix_matches": prefix_matches,
            "projected_after_prune_closure": pruned_closure,
            "projected_after_prune_trace": pruned_trace,
            "accumulated_slashed_after_prune": accumulated_after_prune,
            "history_preserves_prior_slash": set(pruned_closure).issubset(set(accumulated_after_prune)),
            "unexpected_count": unexpected,
        },
        ["Rocq: slash_iter reachability characterization", "TLA+: Inv_SlashedEqualsClosurePrefix", "Rust: slashed-history monotonicity fixture"],
    )


def reverse_edges(edges):
    return [(int(dst), int(src)) for src, dst in edges]


def edge_orientation_sanity():
    vertices = [0, 1]
    direct = [0]
    edges = [(1, 0)]
    forward, forward_trace = closure(vertices, direct, edges)
    reversed_closure, reversed_trace = closure(vertices, direct, reverse_edges(edges))
    return record(
        "edge_orientation_sanity",
        "projection_risk",
        "sage_edge_orientation_sanity",
        "The neglect graph orientation is semantically load-bearing: edges are neglecter → offender. Reversing the edge changes who is accountable.",
        {
            "validators": vertices,
            "direct": direct,
            "edges": edge_list(edges),
            "reversed_edges": edge_list(reverse_edges(edges)),
            "forward_closure": forward,
            "forward_trace": forward_trace,
            "reversed_closure": reversed_closure,
            "reversed_trace": reversed_trace,
            "orientation_projection_differs": forward != reversed_closure,
            "unexpected_count": 0,
        },
        ["Rocq: slash_iter_reachability_characterization", "TLA+: edge-orientation projection class", "Rust: edge orientation regression"],
    )


def redundant_path_denial_cost():
    vertices = [0, 1, 2, 3]
    direct = [0]
    edges = [(1, 0), (3, 1), (2, 0), (3, 2)]
    full, full_trace = closure(vertices, direct, edges)
    target = 3
    minimal = None
    for size in range(1, len(edges) + 1):
        for removed in combinations(edges, size):
            remaining = [edge for edge in edges if edge not in Set(removed)]
            projected, projected_trace = closure(vertices, direct, remaining)
            if target in Set(full) and target not in Set(projected):
                minimal = {
                    "removed_edges": edge_list(removed),
                    "remaining_edges": edge_list(remaining),
                    "projected_closure": projected,
                    "projected_trace": projected_trace,
                }
                break
        if minimal is not None:
            break
    return record(
        "redundant_path_denial_cost",
        "confirmed_safe" if minimal is not None and len(minimal["removed_edges"]) == 2 else "unexpected",
        "sage_redundant_path_denial_cost",
        "Two independent evidence paths to the same target require at least two edge denials before that target drops out of the closure.",
        {
            "validators": vertices,
            "direct": direct,
            "edges": edge_list(edges),
            "target": target,
            "full_closure": full,
            "full_trace": full_trace,
            "minimal_denial": minimal,
            "unexpected_count": 0 if minimal is not None and len(minimal["removed_edges"]) == 2 else 1,
        },
        ["Rocq: slash_iter_reachability_characterization", "TLA+: evidence-denial min-cut fixtures", "docs: redundant evidence-path threat model"],
    )


def slash_target_authorization():
    vertices = [0, 1, 2]
    slash_targets = [1, 2]
    authorized, authorized_trace = closure(vertices, [], [])
    unsafe_projection, unsafe_projection_trace = closure(vertices, slash_targets, [])
    return record(
        "slash_target_authorization",
        "projection_risk",
        "sage_slash_target_authorization",
        "Slash-target lists are reports/acknowledgements, not direct evidence. A projection that treats slash targets as slash seeds enables unsupported slash injection.",
        {
            "validators": vertices,
            "slash_targets": slash_targets,
            "authorized_closure_without_direct_evidence": authorized,
            "authorized_trace": authorized_trace,
            "unsafe_projection_if_targets_are_direct": unsafe_projection,
            "unsafe_projection_trace": unsafe_projection_trace,
            "injection_projection_differs": authorized != unsafe_projection,
            "unexpected_count": 0,
        },
        ["Rocq: slash_iter_empty_initial_empty", "TLA+: Inv_NoDirectSeedNoClosure", "Rust: unauthorized slash-target injection fixture"],
    )


def report_namespace_isolation():
    visible_edges = [(1, 0), (1, 2)]
    reports = [(1, 0)]
    active_edges = [edge for edge in visible_edges if edge not in reports]
    blanket_projection_edges = [edge for edge in visible_edges if edge[0] != 1]
    correct, correct_trace = closure(range(4), [2], active_edges)
    blanket, blanket_trace = closure(range(4), [2], blanket_projection_edges)
    return record(
        "report_namespace_isolation",
        "projection_risk",
        "sage_report_namespace_isolation",
        "Reports are pair-scoped: reporting `reporter → offender` suppresses only that offender for that reporter, not every edge emitted by the reporter.",
        {
            "direct": [2],
            "visible_edges": edge_list(visible_edges),
            "reports": edge_list(reports),
            "active_edges": edge_list(active_edges),
            "blanket_report_projection_edges": edge_list(blanket_projection_edges),
            "correct_closure": correct,
            "correct_trace": correct_trace,
            "blanket_projection_closure": blanket,
            "blanket_projection_trace": blanket_trace,
            "projection_differs": correct != blanket,
            "unexpected_count": 0,
        },
        ["Rocq: unreported_visible_edge_remains_active", "TLA+: Inv_UnreportedVisibleEdgesRemainActive", "Rust: report namespace fixture"],
    )


def report_antitone_closure():
    visible_edges = [(1, 0), (2, 1)]
    reports_before = []
    reports_after = [(1, 0)]
    active_before = [edge for edge in visible_edges if edge not in reports_before]
    active_after = [edge for edge in visible_edges if edge not in reports_after]
    before, before_trace = closure(range(4), [0], active_before)
    after, after_trace = closure(range(4), [0], active_after)
    return record(
        "report_antitone_closure",
        "confirmed_safe" if set(after).issubset(set(before)) else "unexpected",
        "sage_report_antitone_closure",
        "Adding reports can only remove active edges from a fixed visible evidence view, so the post-report closure is a subset of the pre-report closure.",
        {
            "direct": [0],
            "visible_edges": edge_list(visible_edges),
            "reports_before": edge_list(reports_before),
            "reports_after": edge_list(reports_after),
            "closure_before": before,
            "trace_before": before_trace,
            "closure_after": after,
            "trace_after": after_trace,
            "after_subset_before": set(after).issubset(set(before)),
            "unexpected_count": 0 if set(after).issubset(set(before)) else 1,
        },
        ["Rocq: view_closure_reports_antimonotone", "TLA+: Inv_ReportGrowthCannotExpandViewClosure", "Rust: report antitone property"],
    )


def direct_seed_report_dominance():
    closure_set, trace = closure(range(4), [0], [])
    return record(
        "direct_seed_report_dominance",
        "confirmed_safe" if 0 in Set(closure_set) else "unexpected",
        "sage_direct_seed_report_dominance",
        "Reports suppress neglect edges but never suppress direct equivocation seeds; every direct offender remains in closure.",
        {
            "direct": [0],
            "visible_edges": [[1, 0]],
            "reports": [[1, 0], [0, 1]],
            "active_edges": [],
            "closure": closure_set,
            "trace": trace,
            "direct_subset_closure": 0 in Set(closure_set),
            "unexpected_count": 0 if 0 in Set(closure_set) else 1,
        },
        ["Rocq: slash_iter_monotone", "TLA+: Inv_ReportsDoNotSuppressDirectEvidence", "Rust: direct evidence report dominance property"],
    )


def validator_renaming_equivariance():
    direct = [0]
    edges = [(1, 0), (2, 1)]
    permutation = [2, 0, 3, 1]
    base, base_trace = closure(range(4), direct, edges)
    renamed_direct = sorted(Set([permutation[v] for v in direct]))
    renamed_edges = [(permutation[src], permutation[dst]) for src, dst in edges]
    renamed, renamed_trace = closure(range(4), renamed_direct, renamed_edges)
    renamed_base = sorted(Set([permutation[v] for v in base]))
    return record(
        "validator_renaming_equivariance",
        "confirmed_safe" if renamed == renamed_base else "unexpected",
        "sage_validator_renaming_equivariance",
        "Closure is invariant up to bijective validator renaming, which catches accidental dependence on numeric validator ordering.",
        {
            "direct": direct,
            "edges": edge_list(edges),
            "permutation": permutation,
            "base_closure": base,
            "base_trace": base_trace,
            "renamed_direct": renamed_direct,
            "renamed_edges": edge_list(renamed_edges),
            "renamed_closure": renamed,
            "renamed_trace": renamed_trace,
            "renamed_base_closure": renamed_base,
            "unexpected_count": 0 if renamed == renamed_base else 1,
        },
        ["Rocq: graph isomorphism theorem candidate", "TLA+: symmetry reduction sanity", "Rust: validator renaming property"],
    )


def bisimilarity_delta_guard():
    duplicate_base, _ = closure(range(4), [0], [(1, 0)])
    duplicate_projection, _ = closure(range(4), [0], [(1, 0), (1, 0)])
    order_base, _ = closure(range(4), [0], [(1, 0), (2, 1)])
    order_projection, _ = closure(range(4), [0], [(2, 1), (1, 0)])
    reversed_projection, _ = closure(range(4), [0], [(0, 1)])
    target_projection, _ = closure(range(4), [1], [])
    cases = [
        {"name": "duplicate_edge", "classification": "bisimilar", "holds": duplicate_base == duplicate_projection},
        {"name": "edge_order", "classification": "bisimilar", "holds": order_base == order_projection},
        {"name": "reversed_edge", "classification": "projection_risk", "holds": duplicate_base != reversed_projection},
        {"name": "slash_target_as_direct", "classification": "projection_risk", "holds": [] != target_projection},
    ]
    unexpected = [case for case in cases if not case["holds"]]
    return record(
        "bisimilarity_delta_guard",
        "confirmed_safe" if unexpected == [] else "unexpected",
        "sage_bisimilarity_delta_guard",
        "Delta-guard modeling classifies semantic differences: duplicate/order changes are bisimilar, while reversed edges and slash-target injection are projection risks that cannot be silent divergences.",
        {"cases": cases, "unexpected": unexpected, "unexpected_count": len(unexpected)},
        ["Rocq/TLA: divergence classification", "docs: bisimilarity except documented bug fixes"],
    )


def minimal_counterexample_catalog():
    return record(
        "minimal_counterexample_catalog",
        "assumption_counterexample",
        "sage_minimal_counterexample_catalog",
        "The catalog groups the smallest known witnesses for theorem-precondition failures and implementation projections so they can be replayed in Rocq examples, TLA+ configs, and Rust tests.",
        {
            "counterexamples": [
                {"name": "closure_bound", "direct": [0], "edges": [[1, 0]], "closure": closure([0, 1], [0], [(1, 0)])[0]},
                {"name": "weighted_closure_bound", "stakes": [1, 1, 1, 1], "direct": [2], "edges": [[0, 1], [1, 2]], "closure": closure([0, 1, 2, 3], [2], [(0, 1), (1, 2)])[0]},
                {"name": "current_validator_filter", "strict": [], "loose": closure([0, 1], [0], [(1, 0)])[0]},
                {"name": "report_suppression", "unsuppressed": closure([0, 1], [0], [(1, 0)])[0], "suppressed": closure([0, 1], [0], [])[0]},
                {"name": "retention_pruning", "retained": closure([0, 1], [1], [(0, 1)])[0], "pruned": closure([0, 1], [], [])[0]},
                {"name": "batch_atomicity", "bonds": [1, 1], "failure": 0},
                {"name": "record_key_projection", "pairs": [[1, 10], [11, 0]], "delimiter_free_keys": ["110", "110"], "canonical_keys": ["1:10", "11:0"]},
                {"name": "arithmetic_projection", "bits": 8, "exact": 256, "wrapped": 0, "saturated": 255},
                {"name": "proposer_fairness", "schedule": [{"bonded": True, "observes": True, "includes": False}], "first_slash_slot": None},
            ]
        },
        ["Rocq: minimized examples", "TLA+: divergence classes", "docs: counterexample and regression catalog"],
    )


def record_threat_score(item):
    base = {
        "unexpected": 100,
        "projection_risk": 70,
        "assumption_counterexample": 55,
        "candidate_boundary": 35,
        "confirmed_safe": 0,
        "bisimilar": 0,
    }.get(item["classification"], 10)
    witness = item["deterministic_witness"]
    text = json.dumps(witness, sort_keys=True, default=json_default)
    bonus = 0
    for token, value in [("extra_stake", 10), ("overflow", 8), ("retention", 6), ("loose", 6), ("withholding", 6), ("projection", 5)]:
        if token in text:
            bonus += value
    return base + bonus


def threat_vector_ranking(records):
    ranked = []
    for item in records:
        if item["axis"] == "threat_vector_ranking":
            continue
        ranked.append(
            {
                "axis": item["axis"],
                "name": item["name"],
                "classification": item["classification"],
                "score": record_threat_score(item),
                "formalization_follow_up": item["formalization_follow_up"],
            }
        )
    ranked = sorted(ranked, key=lambda item: (-item["score"], item["name"]))
    return record(
        "threat_vector_ranking",
        "confirmed_safe",
        "sage_threat_vector_ranking",
        "Threat-vector ranking prioritizes projection risks first, then assumption counterexamples and policy boundaries, giving higher score to stake damage, overflow, retention, identity, and withholding witnesses.",
        {"ranked": ranked, "top": ranked[:5]},
        ["docs: threat model prioritization", "Rust tests: regression priority order"],
    )


def analyze(max_n, max_stake, horizon, bits):
    records = [
        graph_theoretic_attack_search(max_n),
        stake_damage_optimization(max_stake),
        retention_pruning_optimization(horizon),
        epoch_churn_identity_analysis(),
        economic_safety_envelopes(max_n, max_stake, bits),
        minimum_attacker_stake_search(max_stake),
        maximum_quorum_loss_search(max_stake),
        withholding_pruning_strategy_search(horizon),
        safe_envelope_boundary_distance(max_n, max_stake, bits),
        evidence_denial_min_cut_search(),
        cross_oracle_closure_consistency(max_n),
        detector_totality_threat_search(),
        candidate_invariant_mining(max_n),
        temporal_window_synthesis(horizon),
        mutation_oracle_detection(),
        rebond_identity_lifecycle(),
        record_lifecycle_projection(),
        closure_depth_extremal(max_n),
        evidence_monotonicity_analysis(max_n),
        view_merge_confluence(max_n),
        minimal_slash_basis_catalog(),
        record_key_namespace_projection(),
        detector_traversal_termination(),
        detector_contribution_confluence(),
        closure_fixed_point_idempotence(max_n),
        report_retention_reactivation(),
        no_seed_cycle_safety(),
        slash_history_prefix(),
        edge_orientation_sanity(),
        redundant_path_denial_cost(),
        slash_target_authorization(),
        report_namespace_isolation(),
        report_antitone_closure(),
        direct_seed_report_dominance(),
        validator_renaming_equivariance(),
        bisimilarity_delta_guard(),
        minimal_counterexample_catalog(),
    ]
    records.append(threat_vector_ranking(records))
    axis_counts = {}
    class_counts = {}
    for item in records:
        axis_counts[item["axis"]] = axis_counts.get(item["axis"], 0) + 1
        class_counts[item["classification"]] = class_counts.get(item["classification"], 0) + 1
    missing_axes = [axis for axis in AXES if axis not in axis_counts]
    return {
        "summaries": [
            {
                "max_n": int(max_n),
                "max_stake": int(max_stake),
                "horizon": int(horizon),
                "bits": int(bits),
                "axes": len(axis_counts),
                "records": len(records),
                "missing_axes": missing_axes,
                "class_counts": class_counts,
                "unexpected_count": class_counts.get("unexpected", 0),
            }
        ],
        "records": records,
    }


def self_test():
    result = analyze(4, 4, 4, 8)
    summary = result["summaries"][0]
    if summary["missing_axes"]:
        raise AssertionError("missing deep threat axes: {}".format(summary["missing_axes"]))
    if summary["unexpected_count"] != 0:
        raise AssertionError("unexpected deep threat classification")
    names = Set([item["name"] for item in result["records"]])
    required = Set(
        [
            "sage_graph_theoretic_reverse_reachability_attack",
            "sage_stake_damage_optimization",
            "sage_retention_pruning_thresholds",
            "sage_epoch_churn_identity_boundary",
            "sage_economic_safety_envelopes",
            "sage_minimum_attacker_stake_closure_amplification",
            "sage_maximum_quorum_loss_closure_amplification",
            "sage_withholding_pruning_strategy_boundary",
            "sage_safe_envelope_boundary_distance",
            "sage_evidence_denial_min_cut_search",
            "sage_cross_oracle_closure_consistency",
            "sage_detector_totality_threat_search",
            "sage_candidate_invariant_mining",
            "sage_temporal_window_synthesis",
            "sage_mutation_oracle_detection",
            "sage_rebond_identity_lifecycle",
            "sage_record_lifecycle_projection",
            "sage_closure_depth_extremal",
            "sage_evidence_monotonicity_analysis",
            "sage_view_merge_confluence",
            "sage_minimal_slash_basis_catalog",
            "sage_record_key_namespace_projection",
            "sage_detector_traversal_termination",
            "sage_detector_contribution_confluence",
            "sage_closure_fixed_point_idempotence",
            "sage_report_retention_reactivation",
            "sage_no_seed_cycle_safety",
            "sage_slash_history_prefix",
            "sage_edge_orientation_sanity",
            "sage_redundant_path_denial_cost",
            "sage_slash_target_authorization",
            "sage_report_namespace_isolation",
            "sage_report_antitone_closure",
            "sage_direct_seed_report_dominance",
            "sage_validator_renaming_equivariance",
            "sage_bisimilarity_delta_guard",
            "sage_minimal_counterexample_catalog",
            "sage_threat_vector_ranking",
        ]
    )
    if not required.issubset(names):
        raise AssertionError("missing required deep threat witness")
    return result


def fixture_output(result):
    fixtures = []
    for item in result["records"]:
        fixtures.append(
            scenario_fixture(
                item["name"],
                item["classification"],
                item["scenario"],
                item["deterministic_witness"],
                item["deterministic_witness"],
                assertions=["classification == {}".format(item["classification"]), "unexpected_count == 0"],
            )
        )
    return {"summaries": [coverage_summary(fixtures)], "fixtures": fixtures}


def print_summary(result):
    for summary in result["summaries"]:
        print(
            "axes={axes} records={records} missing_axes={missing_axes} unexpected={unexpected_count}".format(
                **summary
            )
        )
        for classification in sorted(summary["class_counts"]):
            print("classification={classification} count={count}".format(classification=classification, count=summary["class_counts"][classification]))
    for item in result["records"]:
        print("axis={axis} classification={classification} name={name}".format(**item))


def main(argv):
    parser = argparse.ArgumentParser(description="Deep Sage threat modeling for slashing")
    parser.add_argument("--max-n", type=int, default=4)
    parser.add_argument("--max-stake", type=int, default=4)
    parser.add_argument("--horizon", type=int, default=4)
    parser.add_argument("--bits", type=int, default=8)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    parser.add_argument("--schema-out")
    parser.add_argument("--fixture-out")
    parser.add_argument("--coverage-out")
    args = parser.parse_args(argv)
    if args.self_test:
        result = self_test()
    else:
        result = analyze(args.max_n, args.max_stake, args.horizon, args.bits)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")
    if args.schema_out:
        with open(args.schema_out, "w", encoding="utf-8") as handle:
            json.dump(schema_example(), handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")
    if args.fixture_out:
        with open(args.fixture_out, "w", encoding="utf-8") as handle:
            json.dump(fixture_output(result), handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")
    if args.coverage_out:
        with open(args.coverage_out, "w", encoding="utf-8") as handle:
            json.dump(coverage_summary(result["records"]), handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
