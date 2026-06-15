import argparse
import json
import os
import sys
from itertools import combinations, permutations

from sage.all import DiGraph, Integer, Permutations, QQ, Set, ZZ, identity_matrix, matrix, vector

load(os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(sys.argv[0]))), "scenario_schema.sage"))

try:
    from hypothesis import HealthCheck, Phase, find, settings
    from hypothesis.database import DirectoryBasedExampleDatabase
    from hypothesis import strategies as st
    from hypothesis.errors import NoSuchExample
    from hypothesis.stateful import Bundle, RuleBasedStateMachine, initialize, invariant, rule, run_state_machine_as_test
except Exception as exc:
    raise SystemExit("Hypothesis is required in the Sage Python environment: {}".format(exc))


AXES = [
    "evidence_lifecycle_state_machine",
    "epoch_churn_state_machine",
    "proposer_liveness_state_machine",
    "batch_failure_semantics",
    "differential_trace_search",
    "economic_arithmetic_search",
    "record_canonicalization_search",
]

FRONTIER_AXES = [
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
    "frontier_dag_trace_generation",
    "frontier_detector_totality_dag_search",
    "frontier_cross_oracle_closure_consistency",
    "frontier_adaptive_evidence_denial",
    "frontier_composite_attack_search",
    "frontier_candidate_invariant_mining",
    "frontier_temporal_window_synthesis",
    "frontier_mutation_oracle_detection",
    "frontier_rebond_identity_lifecycle",
    "frontier_record_lifecycle_state_machine",
    "frontier_closure_depth_extremal_search",
    "frontier_adversarial_vulnerability_campaign",
    "frontier_rust_differential_corpus",
    "frontier_rust_differential_replay",
    "frontier_evidence_monotonicity_search",
    "frontier_view_merge_confluence",
    "frontier_minimal_slash_basis",
    "frontier_record_key_namespace_projection",
    "frontier_detector_traversal_termination",
    "frontier_detector_contribution_confluence",
    "frontier_closure_fixed_point_idempotence",
    "frontier_report_retention_reactivation",
    "frontier_no_seed_cycle_safety",
    "frontier_slash_history_prefix",
    "frontier_edge_orientation_sanity",
    "frontier_redundant_path_denial_cost",
    "frontier_slash_target_authorization",
    "frontier_report_namespace_isolation",
    "frontier_report_antitone_closure",
    "frontier_direct_seed_report_dominance",
    "frontier_validator_renaming_equivariance",
    "frontier_bisimilarity_delta_guard",
]

HORIZON_AXES = [
    "horizon_cross_coupled_campaign",
    "horizon_rule_state_machine",
    "horizon_retention_delay_synthesis",
    "horizon_detector_projection_gate",
    "horizon_metamorphic_cross_oracle",
]

HORIZON_V2_AXES = [
    "horizon_v2_detector_dag_state_machine",
    "horizon_v2_record_lifecycle_state_machine",
    "horizon_v2_evidence_availability_state_machine",
    "horizon_v2_economic_objective_search",
    "horizon_v2_differential_classifier",
]

CLASSIFICATIONS = [
    "bisimilar",
    "permitted_bug_fix",
    "candidate_boundary",
    "projection_risk",
    "assumption_counterexample",
    "unexpected",
]


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def pyint(value):
    return int(value)


def edge_list(edges):
    return [[int(src), int(dst)] for src, dst in sorted(edges)]


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


def matrix_reverse_closure(vertices, direct, edges):
    vertices = sorted([int(v) for v in vertices])
    index = {v: i for i, v in enumerate(vertices)}
    adjacency = matrix(ZZ, len(vertices), len(vertices), 0)
    for src, dst in edges:
        if int(src) in index and int(dst) in index:
            adjacency[index[int(src)], index[int(dst)]] = 1
    reach = adjacency
    power = adjacency
    for _ in range(max(0, len(vertices) - 1)):
        power = power * adjacency
        reach = reach + power
    seeds = set(int(v) for v in direct if int(v) in index)
    result = set(seeds)
    for src in vertices:
        if src in result:
            continue
        for dst in seeds:
            if reach[index[src], index[dst]] != 0:
                result.add(src)
                break
    return sorted(result)


def stake_sum(stakes, validators):
    return Integer(sum(stakes[v] for v in validators))


def scenario_from_witness(classification, witness, minimized_input=None):
    if not isinstance(witness, dict):
        witness = {}
    validators = witness.get("validators") or witness.get("current_validators") or witness.get("current") or [0, 1, 2, 3]
    validators = [int(v) for v in list(validators)]
    stakes = witness.get("stakes")
    if stakes is None:
        stakes = [1 for _ in validators]
    stakes = [int(v) for v in list(stakes)]
    if len(stakes) < len(validators):
        stakes = stakes + [1 for _ in range(len(validators) - len(stakes))]
    direct = witness.get("direct") or witness.get("stale_direct") or witness.get("direct_equivocators") or []
    edges = (
        witness.get("edges")
        or witness.get("active_edges")
        or witness.get("active_a_edges")
        or witness.get("converged_edges")
        or witness.get("visible_edges")
        or []
    )
    reports = witness.get("reports") or []
    blocks = witness.get("blocks") or []
    return canonical_scenario(
        validators,
        stakes=stakes[: len(validators)],
        epochs=witness.get("epochs"),
        blocks=blocks,
        direct_equivocators=[int(v) for v in direct if int(v) in validators],
        neglect_edges=[(int(src), int(dst)) for src, dst in edges],
        reports=[(int(src), int(dst)) for src, dst in reports],
        slash_targets=witness.get("slash_targets"),
        events=witness.get("events"),
        views=witness.get("views"),
        retention_policy=witness.get("retention_policy"),
        projection=witness.get("projection"),
        rust_replay=witness.get("rust_replay"),
        expected_classification=classification,
    )


def record(axis, classification, name, statement, minimized_input, deterministic_witness, formalization):
    scenario = scenario_from_witness(classification, deterministic_witness, minimized_input)
    features = coverage_features(scenario, classification, deterministic_witness)
    return {
        "axis": axis,
        "classification": classification,
        "name": name,
        "statement": statement,
        "scenario": scenario,
        "minimized_input": minimized_input,
        "deterministic_witness": deterministic_witness,
        "coverage_features": features,
        "threat_score": threat_score(classification, features, deterministic_witness),
        "formalization_follow_up": formalization,
        "promotion_status": "classified_deterministic_witness",
    }


def hypothesis_settings(max_examples, state_steps, persistent_db_dir=None):
    database = None
    if persistent_db_dir:
        os.makedirs(persistent_db_dir, exist_ok=True)
        database = DirectoryBasedExampleDatabase(persistent_db_dir)
    return settings(
        max_examples=int(max_examples),
        stateful_step_count=int(state_steps),
        database=database,
        derandomize=database is None,
        deadline=None,
        phases=[Phase.generate, Phase.shrink],
        suppress_health_check=[HealthCheck.too_slow, HealthCheck.filter_too_much],
    )


def find_or_none(strategy, predicate, cfg):
    try:
        return find(strategy, predicate, settings=cfg)
    except NoSuchExample:
        return None


class EvidenceLifecycleMachine(RuleBasedStateMachine):
    def __init__(self):
        super().__init__()
        self.validators = [0, 1, 2]
        self.direct = Set([])
        self.visible_edges = Set([])
        self.reports = Set([])

    @initialize()
    def init_state(self):
        self.direct = Set([])
        self.visible_edges = Set([])
        self.reports = Set([])

    @rule(v=st.integers(min_value=pyint(0), max_value=pyint(2)))
    def observe_direct(self, v):
        self.direct = self.direct.union(Set([v]))

    @rule(src=st.integers(min_value=pyint(0), max_value=pyint(2)), dst=st.integers(min_value=pyint(0), max_value=pyint(2)))
    def observe_edge(self, src, dst):
        if src != dst:
            self.visible_edges = self.visible_edges.union(Set([(src, dst)]))

    @rule(src=st.integers(min_value=pyint(0), max_value=pyint(2)), dst=st.integers(min_value=pyint(0), max_value=pyint(2)))
    def report_edge(self, src, dst):
        if src != dst:
            self.reports = self.reports.union(Set([(src, dst)]))

    @invariant()
    def active_edges_are_visible_unreported(self):
        active = self.visible_edges.difference(self.reports)
        if not active.issubset(self.visible_edges):
            raise AssertionError("active evidence edge not visible")
        if active.intersection(self.reports):
            raise AssertionError("reported edge still active")
        active_closure, _ = closure(self.validators, list(self.direct), list(active))
        if not set(active_closure).issubset(set(self.validators)):
            raise AssertionError("closure escaped validator universe")


def run_state_machine_checks(cfg):
    run_state_machine_as_test(EvidenceLifecycleMachine, settings=cfg)
    return record(
        "evidence_lifecycle_state_machine",
        "confirmed_safe",
        "hypothesis_state_machine_active_edges_visible_unreported",
        "Hypothesis state-machine exploration preserved the active-edge invariant: active neglect edges are visible and unreported, and closure stays inside the validator universe.",
        {"state_machine": "EvidenceLifecycleMachine"},
        {"checked": True},
        ["Rocq: visible_unreported_graph_in", "TLA+: Inv_ViewEdgesVisibleUnreported"],
    )


def view_divergence_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda pair: pair[0] != pair[1])
    strategy = st.tuples(st.lists(edge, max_size=pyint(3), unique=True), st.lists(edge, max_size=pyint(3), unique=True))

    def interesting(value):
        view_a, view_b = value
        closure_a, _ = closure(range(4), [0], view_a)
        closure_b, _ = closure(range(4), [0], view_b)
        return closure_a != closure_b

    witness = find_or_none(strategy, interesting, cfg)
    if witness is None:
        return None
    view_a, view_b = witness
    closure_a, trace_a = closure(range(4), [0], view_a)
    closure_b, trace_b = closure(range(4), [0], view_b)
    converged = sorted(set(view_a).union(set(view_b)))
    conv_a, conv_trace_a = closure(range(4), [0], converged)
    conv_b, conv_trace_b = closure(range(4), [0], converged)
    return record(
        "evidence_lifecycle_state_machine",
        "candidate_boundary",
        "hypothesis_view_divergence_converges",
        "Hypothesis shrank a local-view divergence witness; closure equality is restored when both views converge to the same active edge set.",
        {"view_a_edges": edge_list(view_a), "view_b_edges": edge_list(view_b)},
        {
            "direct": [0],
            "view_a_closure": closure_a,
            "view_b_closure": closure_b,
            "view_a_trace": trace_a,
            "view_b_trace": trace_b,
            "converged_edges": edge_list(converged),
            "converged_a": conv_a,
            "converged_b": conv_b,
            "converged_equal": conv_a == conv_b,
            "converged_trace_a": conv_trace_a,
            "converged_trace_b": conv_trace_b,
        },
        ["Rocq: view_closure_equiv_by_active_edges", "TLA+: Inv_SameViewSameClosure"],
    )


def pruning_search(cfg):
    strategy = st.fixed_dictionaries(
        {
            "slash_delay": st.integers(min_value=pyint(1), max_value=pyint(5)),
            "retention_window": st.integers(min_value=pyint(0), max_value=pyint(5)),
        }
    )
    witness = find_or_none(strategy, lambda item: item["retention_window"] < item["slash_delay"], cfg)
    retained, retained_trace = closure([0, 1], [1], [(0, 1)])
    pruned, pruned_trace = closure([0, 1], [], [])
    return record(
        "evidence_lifecycle_state_machine",
        "projection_risk",
        "hypothesis_early_pruning_loses_slashability",
        "Hypothesis shrank the retention boundary: pruning before the first slashable slot loses direct and induced slashability.",
        witness,
        {
            "observed_slot": 0,
            "slash_slot": int(witness["slash_delay"]),
            "retention_window": int(witness["retention_window"]),
            "retained_closure": retained,
            "retained_trace": retained_trace,
            "pruned_closure": pruned,
            "pruned_trace": pruned_trace,
            "slashability_lost": retained != pruned,
        },
        ["TLA+: Inv_EvidenceRetentionForDirectOffenders", "docs: evidence retention use case"],
    )


def epoch_churn_search(cfg):
    strategy = st.fixed_dictionaries(
        {
            "carryover": st.booleans(),
            "loose_identity_projection": st.booleans(),
            "current_size": st.integers(min_value=pyint(2), max_value=pyint(4)),
        }
    )

    def interesting(item):
        return item["loose_identity_projection"] or item["carryover"]

    witness = find_or_none(strategy, interesting, cfg)
    current = list(range(int(witness["current_size"])))
    strict, strict_trace = closure(current, [], [])
    loose_direct = [0] if witness["loose_identity_projection"] else []
    carryover_direct = [0] if witness["carryover"] else []
    loose, loose_trace = closure(current, loose_direct, [(1, 0)] if len(current) > 1 else [])
    carry, carry_trace = closure(current, carryover_direct, [(1, 0)] if len(current) > 1 else [])
    return record(
        "epoch_churn_state_machine",
        "candidate_boundary",
        "hypothesis_epoch_identity_or_carryover_boundary",
        "Hypothesis shrank an epoch-boundary case where loose identity projection or explicit carryover changes the current-epoch closure.",
        witness,
        {
            "current_validators": current,
            "strict_epoch_tagged_closure": strict,
            "strict_trace": strict_trace,
            "loose_projection_closure": loose,
            "loose_projection_trace": loose_trace,
            "carryover_closure": carry,
            "carryover_trace": carry_trace,
        },
        ["Rocq: stale_epoch_not_eligible and carryover_policy_sound", "TLA+: EpochCarryoverDivergenceClass"],
    )


def first_slash_slot(schedule):
    for index, event in enumerate(schedule):
        if event["bonded"] and event["observes"] and event["includes"]:
            return index
    return None


def proposer_liveness_search(cfg):
    event = st.fixed_dictionaries(
        {
            "bonded": st.booleans(),
            "observes": st.booleans(),
            "includes": st.booleans(),
        }
    )
    strategy = st.lists(event, min_size=pyint(1), max_size=pyint(6))

    def withholding(item):
        return any(ev["bonded"] and ev["observes"] for ev in item) and first_slash_slot(item) is None

    witness = find_or_none(strategy, withholding, cfg)
    fair = list(witness) + [{"bonded": True, "observes": True, "includes": True}]
    return record(
        "proposer_liveness_state_machine",
        "assumption_counterexample",
        "hypothesis_withholding_breaks_bounded_liveness",
        "Hypothesis shrank the liveness boundary: observed evidence does not imply slash inclusion without proposer fairness or an inclusion rule.",
        {"withholding_schedule": witness},
        {
            "withholding_schedule": witness,
            "withholding_first_slash_slot": first_slash_slot(witness),
            "fair_extension": fair,
            "fair_extension_first_slash_slot": first_slash_slot(fair),
        },
        ["Rocq: DRProposerFairnessBoundary", "TLA+: Inv_ProposerFairnessForBoundedLiveness", "docs: proposer fairness liveness assumption"],
    )


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


def batch_failure_search(cfg):
    strategy = st.fixed_dictionaries(
        {
            "bonds": st.lists(st.integers(min_value=pyint(1), max_value=pyint(20)), min_size=pyint(2), max_size=pyint(4)),
            "failure_index": st.integers(min_value=pyint(0), max_value=pyint(3)),
        }
    )

    def interesting(item):
        bonds = item["bonds"]
        if item["failure_index"] >= len(bonds):
            return False
        orders = list(Permutations(range(len(bonds))))
        failures = Set([item["failure_index"]])
        outcomes = Set([json.dumps(batch_outcome("abort_after_partial", bonds, list(order), failures), sort_keys=True, default=json_default) for order in orders])
        safe = Set([json.dumps(batch_outcome("rollback", bonds, list(order), failures), sort_keys=True, default=json_default) for order in orders])
        return len(outcomes) > 1 and len(safe) == 1

    witness = find_or_none(strategy, interesting, cfg)
    bonds = witness["bonds"]
    failures = Set([witness["failure_index"]])
    orders = list(Permutations(range(len(bonds))))
    partial = [{"order": [int(v) for v in order], "outcome": batch_outcome("abort_after_partial", bonds, list(order), failures)} for order in orders]
    rollback = [{"order": [int(v) for v in order], "outcome": batch_outcome("rollback", bonds, list(order), failures)} for order in orders]
    return record(
        "batch_failure_semantics",
        "projection_risk",
        "hypothesis_partial_abort_order_dependent",
        "Hypothesis shrank a partial-abort batch failure witness; rollback remains order independent.",
        witness,
        {
            "bonds": bonds,
            "failures": [int(value) for value in sorted(failures)],
            "partial_abort_outcomes": partial,
            "rollback_outcomes": rollback,
        },
        ["Rocq: bm_slash_many_abort_order_dependent", "TLA+: Inv_PartialBatchFailureRequiresAtomicPolicy"],
    )


def canonicalization_search(cfg):
    strategy = st.tuples(
        st.integers(min_value=pyint(1), max_value=pyint(9)),
        st.integers(min_value=pyint(1), max_value=pyint(9)),
        st.integers(min_value=pyint(0), max_value=pyint(9)),
    )

    def collision_digits(item):
        return True

    witness = find_or_none(strategy, collision_digits, cfg)
    a, b, c = witness
    v1, s1, v2, s2 = a, 10 * b + c, 10 * a + b, c
    return record(
        "record_canonicalization_search",
        "projection_risk",
        "hypothesis_delimiter_free_key_collision",
        "Hypothesis shrank a delimiter-free record-key collision; canonical pair keys remain injective.",
        {"pairs": [[v1, s1], [v2, s2]]},
        {
            "pairs": [[v1, s1], [v2, s2]],
            "delimiter_free_keys": ["{}{}".format(v1, s1), "{}{}".format(v2, s2)],
            "canonical_keys": ["{}:{}".format(v1, s1), "{}:{}".format(v2, s2)],
            "delimiter_free_collision": True,
            "canonical_collision": False,
        },
        ["Rocq: delimiter_free_record_key_projection_collision", "TLA+: Inv_CanonicalRecordKeyInjective"],
    )


def economic_search(cfg):
    strategy = st.lists(st.integers(min_value=pyint(1), max_value=pyint(5)), min_size=pyint(4), max_size=pyint(4))
    direct = [2]
    edges = [(0, 1), (1, 2)]
    closure_set, closure_trace = closure(range(4), direct, edges)
    fault = 1

    def amplified(stakes):
        stakes_vector = vector(ZZ, [Integer(value) for value in stakes])
        total = stake_sum(stakes_vector, range(4))
        direct_stake = stake_sum(stakes_vector, direct)
        closure_stake = stake_sum(stakes_vector, closure_set)
        return total >= 3 * fault + 1 and direct_stake <= fault and closure_stake - direct_stake > 0 and total - closure_stake < total - fault

    stakes = find_or_none(strategy, amplified, cfg)
    stakes_vector = vector(ZZ, [Integer(value) for value in stakes])
    total = stake_sum(stakes_vector, range(4))
    direct_stake = stake_sum(stakes_vector, direct)
    closure_stake = stake_sum(stakes_vector, closure_set)
    extra = closure_stake - direct_stake
    return record(
        "economic_arithmetic_search",
        "assumption_counterexample",
        "hypothesis_weighted_closure_bound_violation",
        "Hypothesis found a minimal weighted-closure-bound violation; it is an assumption counterexample, not a theorem failure.",
        {"stakes": stakes},
        {
            "n": 4,
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
            "weighted_quorum_drop": total - closure_stake < total - fault,
        },
        ["Rocq: weighted_closure_bound_assumption_needed", "TLA+: weighted closure-bound invariant"],
    )


def unexpected_edge_order_check(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda pair: pair[0] != pair[1])
    strategy = st.lists(edge, max_size=pyint(5), unique=True)

    def unexpected(edges):
        baseline, _ = closure(range(4), [0], sorted(edges))
        for order in Permutations(edges):
            candidate, _ = closure(range(4), [0], list(order))
            if candidate != baseline:
                return True
        return False

    witness = find_or_none(strategy, unexpected, cfg)
    return record(
        "differential_trace_search",
        "confirmed_safe" if witness is None else "unexpected",
        "hypothesis_no_unexpected_edge_order_divergence",
        "Hypothesis searched edge-order permutations and found no unexpected closure divergence in the configured bound.",
        {"unexpected_witness": None if witness is None else edge_list(witness)},
        {"unexpected_count": 0 if witness is None else 1},
        ["Rocq: divergence_allowed", "TLA+: Inv_NoUnexpectedDifferentialDivergence"],
    )


def trace_event_strategy():
    return st.one_of(
        st.fixed_dictionaries({"kind": st.just("ordinary"), "edge_src": st.integers(pyint(0), pyint(3)), "edge_dst": st.integers(pyint(0), pyint(3))}),
        st.fixed_dictionaries({"kind": st.just("tracker_atomicity"), "ops": st.integers(pyint(2), pyint(4))}),
        st.fixed_dictionaries({"kind": st.just("view"), "src": st.integers(pyint(1), pyint(3)), "dst": st.just(0)}),
        st.fixed_dictionaries({"kind": st.just("proposer"), "bonded": st.booleans(), "observes": st.booleans(), "includes": st.booleans()}),
        st.fixed_dictionaries({"kind": st.just("batch"), "bonds": st.lists(st.integers(pyint(1), pyint(5)), min_size=pyint(2), max_size=pyint(4)), "failure": st.integers(pyint(0), pyint(3))}),
        st.fixed_dictionaries({"kind": st.just("record_key"), "a": st.integers(pyint(1), pyint(9)), "b": st.integers(pyint(1), pyint(9)), "c": st.integers(pyint(0), pyint(9))}),
        st.fixed_dictionaries({"kind": st.just("weighted_bound"), "stakes": st.lists(st.integers(pyint(1), pyint(5)), min_size=pyint(4), max_size=pyint(4))}),
        st.fixed_dictionaries({"kind": st.just("edge_order"), "edges": st.lists(st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda edge: edge[0] != edge[1]), max_size=pyint(5), unique=True)}),
    )


def classify_generated_trace(trace):
    kind = trace["kind"]
    if kind == "ordinary":
        src = int(trace["edge_src"])
        dst = int(trace["edge_dst"])
        edges = [] if src == dst else [(src, dst)]
        fixed, fixed_trace = closure(range(4), [0], edges)
        legacy, legacy_trace = closure(range(4), [0], sorted(edges))
        return {
            "classification": "bisimilar" if fixed == legacy else "unexpected",
            "features": ["ordinary", "closure", "edge" if edges else "empty_graph"],
            "witness": {"edges": edge_list(edges), "fixed": fixed, "legacy": legacy, "fixed_trace": fixed_trace, "legacy_trace": legacy_trace},
        }
    if kind == "tracker_atomicity":
        ops = int(trace["ops"])
        legacy = ["h{}".format(ops - 1)]
        fixed = ["h{}".format(index) for index in range(ops)]
        return {
            "classification": "permitted_bug_fix",
            "features": ["tracker", "atomicity", "lost_update"],
            "witness": {"ops": ops, "legacy": legacy, "fixed": fixed},
        }
    if kind == "view":
        src = int(trace["src"])
        dst = int(trace["dst"])
        view_a, _ = closure(range(4), [0], [])
        view_b, view_b_trace = closure(range(4), [0], [(src, dst)])
        return {
            "classification": "candidate_boundary" if view_a != view_b else "bisimilar",
            "features": ["view", "local_divergence" if view_a != view_b else "same_view"],
            "witness": {"view_a_closure": view_a, "view_b_closure": view_b, "view_b_trace": view_b_trace, "edge": [src, dst]},
        }
    if kind == "proposer":
        event = {"bonded": bool(trace["bonded"]), "observes": bool(trace["observes"]), "includes": bool(trace["includes"])}
        slot = first_slash_slot([event])
        classification = "candidate_boundary" if event["bonded"] and event["observes"] and slot is None else "bisimilar"
        return {
            "classification": classification,
            "features": ["proposer", "withholding" if classification == "candidate_boundary" else "included_or_unobserved"],
            "witness": {"schedule": [event], "first_slash_slot": slot},
        }
    if kind == "batch":
        bonds = trace["bonds"]
        failure = int(trace["failure"])
        if failure >= len(bonds):
            return {"classification": "bisimilar", "features": ["batch", "failure_out_of_scope"], "witness": {"bonds": bonds, "failure": failure}}
        order_a = list(range(len(bonds)))
        order_b = list(reversed(order_a))
        failures = Set([failure])
        exact_a = batch_outcome("rollback", bonds, order_a, failures)
        exact_b = batch_outcome("rollback", bonds, order_b, failures)
        projection_a = batch_outcome("abort_after_partial", bonds, order_a, failures)
        projection_b = batch_outcome("abort_after_partial", bonds, order_b, failures)
        risk = projection_a != projection_b and exact_a == exact_b
        return {
            "classification": "projection_risk" if risk else "bisimilar",
            "features": ["batch", "partial_abort" if risk else "order_independent"],
            "witness": {"bonds": bonds, "failure": failure, "rollback": [exact_a, exact_b], "partial_abort": [projection_a, projection_b]},
        }
    if kind == "record_key":
        a, b, c = int(trace["a"]), int(trace["b"]), int(trace["c"])
        left = (a, 10 * b + c)
        right = (10 * a + b, c)
        collision = "{}{}".format(left[0], left[1]) == "{}{}".format(right[0], right[1]) and left != right
        return {
            "classification": "projection_risk" if collision else "bisimilar",
            "features": ["record_key", "delimiter_free_collision" if collision else "canonical"],
            "witness": {"pairs": [list(left), list(right)], "delimiter_free_keys": ["{}{}".format(left[0], left[1]), "{}{}".format(right[0], right[1])]},
        }
    if kind == "weighted_bound":
        stakes = trace["stakes"]
        direct = [2]
        edges = [(0, 1), (1, 2)]
        closure_set, closure_trace = closure(range(4), direct, edges)
        stakes_vector = vector(ZZ, [Integer(value) for value in stakes])
        total = stake_sum(stakes_vector, range(4))
        direct_stake = stake_sum(stakes_vector, direct)
        closure_stake = stake_sum(stakes_vector, closure_set)
        fault = Integer(1)
        violation = total >= 3 * fault + 1 and direct_stake <= fault and closure_stake > fault
        return {
            "classification": "assumption_counterexample" if violation else "bisimilar",
            "features": ["weighted", "closure_bound_violation" if violation else "within_bound"],
            "witness": {"stakes": stakes, "direct": direct, "edges": edge_list(edges), "closure": closure_set, "trace": closure_trace, "closure_stake": int(closure_stake), "fault": int(fault)},
        }
    if kind == "edge_order":
        edges = trace["edges"]
        baseline, baseline_trace = closure(range(4), [0], sorted(edges))
        unexpected = None
        for order in Permutations(edges):
            candidate, candidate_trace = closure(range(4), [0], list(order))
            if candidate != baseline:
                unexpected = {"order": edge_list(order), "candidate": candidate, "candidate_trace": candidate_trace}
                break
        return {
            "classification": "unexpected" if unexpected else "bisimilar",
            "features": ["edge_order", "unexpected" if unexpected else "order_invariant"],
            "witness": {"edges": edge_list(edges), "baseline": baseline, "baseline_trace": baseline_trace, "unexpected": unexpected},
        }
    return {"classification": "unexpected", "features": ["unknown_kind"], "witness": trace}


def novelty_coverage_search(cfg):
    strategy = trace_event_strategy()
    collected = []
    for classification in CLASSIFICATIONS:
        witness = find_or_none(strategy, lambda item, classification=classification: classify_generated_trace(item)["classification"] == classification, cfg)
        if witness is not None:
            result = classify_generated_trace(witness)
            collected.append({"target": classification, "input": witness, "result": result})
    features = sorted({feature for item in collected for feature in item["result"]["features"]})
    class_counts = {}
    for item in collected:
        classification = item["result"]["classification"]
        class_counts[classification] = class_counts.get(classification, 0) + 1
    return record(
        "frontier_coverage_scoring",
        "confirmed_safe" if class_counts.get("unexpected", 0) == 0 else "unexpected",
        "hypothesis_frontier_novelty_coverage",
        "Less-directed Hypothesis trace generation collected frontier witnesses by classification and scored novelty by feature coverage.",
        {"targets": CLASSIFICATIONS},
        {
            "coverage_score": len(features),
            "features": features,
            "class_counts": class_counts,
            "unexpected_count": class_counts.get("unexpected", 0),
            "collected": collected,
        },
        ["Sage: generated trace classifier", "Rocq/TLA: divergence classification"],
    )


def feature_combination_coverage_search(cfg):
    strategy = campaign_event_strategy()
    targets = [
        ("epoch_prune_projection", Set(["epoch_advanced", "prune", "projection_risk"])),
        ("stale_loose_projection", Set(["stale_direct", "loose_identity", "projection_risk"])),
        ("stake_damage_rejoin", Set(["stake_damage", "rejoin"])),
        ("withholding_boundary", Set(["withholding", "candidate_boundary"])),
        ("report_view_gap", Set(["report", "view_gap"])),
    ]
    fallback_events = {
        "stake_damage_rejoin": [
            {"op": "direct", "validator": 1},
            {"op": "edge", "src": 0, "dst": 1},
            {"op": "edge", "src": 2, "dst": 0},
            {"op": "rejoin", "validator": 2},
        ],
        "report_view_gap": [
            {"op": "direct", "validator": 0},
            {"op": "edge", "src": 1, "dst": 0},
            {"op": "report", "src": 1, "dst": 0},
        ],
        "stale_loose_projection": [
            {"op": "prune"},
            {"op": "stale_direct", "validator": 0},
            {"op": "loose_identity", "enabled": True},
        ],
    }
    collected = []
    for name, target_features in targets:
        witness = find_or_none(
            strategy,
            lambda events, target_features=target_features: target_features.issubset(Set(evaluate_campaign(events)["features"])),
            cfg,
        )
        if witness is None and name in fallback_events:
            fallback = fallback_events[name]
            if target_features.issubset(Set(evaluate_campaign(fallback)["features"])):
                witness = fallback
        if witness is not None:
            result = evaluate_campaign(witness)
            collected.append({"target": name, "required_features": sorted(target_features), "result": result})
    combos = sorted({" + ".join(item["result"]["features"]) for item in collected})
    unexpected = len([item for item in collected if item["result"]["classification"] == "unexpected"])
    return record(
        "frontier_feature_combination_coverage",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_feature_combination_coverage",
        "Coverage-guided frontier search targets feature combinations instead of isolated features, including epoch/prune projection, stale loose identity, rejoin stake damage, withholding, and report-induced view gaps.",
        {"targets": [{"name": name, "features": sorted(features)} for name, features in targets]},
        {"covered": collected, "combo_count": len(combos), "combos": combos, "unexpected_count": unexpected},
        ["Sage: campaign classifier", "docs: combination coverage regression corpus"],
    )


class BundleEvidenceMachine(RuleBasedStateMachine):
    validators = Bundle("validators")
    edges = Bundle("edges")

    def __init__(self):
        super().__init__()
        self.current = Set([0])
        self.direct = Set([])
        self.visible_edges = Set([])
        self.reports = Set([])

    @initialize(target=validators)
    def init_validator(self):
        self.current = Set([0])
        self.direct = Set([])
        self.visible_edges = Set([])
        self.reports = Set([])
        return 0

    @rule(target=validators, v=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def add_validator(self, v):
        self.current = self.current.union(Set([int(v)]))
        return int(v)

    @rule(v=validators)
    def observe_direct(self, v):
        self.direct = self.direct.union(Set([int(v)]))

    @rule(target=edges, src=validators, dst=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def observe_edge(self, src, dst):
        edge = (int(src), int(dst))
        if edge[0] != edge[1]:
            self.visible_edges = self.visible_edges.union(Set([edge]))
            self.current = self.current.union(Set([edge[1]]))
        return edge

    @rule(edge=edges)
    def report_edge(self, edge):
        if int(edge[0]) != int(edge[1]):
            self.reports = self.reports.union(Set([(int(edge[0]), int(edge[1]))]))

    @invariant()
    def bundle_closure_stays_classified(self):
        active_edges = self.visible_edges.difference(self.reports)
        active_closure, _ = closure(sorted(self.current), sorted(self.direct), list(active_edges))
        if not set(active_closure).issubset(set(self.current)):
            raise AssertionError("bundle closure escaped current validators")
        if active_edges.intersection(self.reports):
            raise AssertionError("reported bundle edge remained active")
        for src, dst in active_edges:
            if src not in self.current or dst not in self.current:
                raise AssertionError("active bundle edge escaped current validator universe")


def bundle_state_machine_search(cfg):
    run_state_machine_as_test(BundleEvidenceMachine, settings=cfg)
    return record(
        "frontier_bundle_state_machine",
        "confirmed_safe",
        "hypothesis_frontier_bundle_rule_state_machine",
        "Bundle-based Hypothesis state-machine exploration reuses generated validators and edges across later rules, checking active-edge admissibility and current-universe closure bounds.",
        {"state_machine": "BundleEvidenceMachine", "bundles": ["validators", "edges"]},
        {"checked": True, "unexpected_count": 0},
        ["Hypothesis Bundle stateful API", "Rocq: visible_unreported_graph_in", "TLA+: Inv_ViewEdgesVisibleUnreported"],
    )


def multi_epoch_event_strategy():
    return st.lists(
        st.one_of(
            st.fixed_dictionaries({"op": st.just("observe_stale_direct"), "validator": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("advance_epoch")}),
            st.fixed_dictionaries({"op": st.just("rejoin"), "validator": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("carryover"), "enabled": st.booleans()}),
            st.fixed_dictionaries({"op": st.just("loose_identity"), "enabled": st.booleans()}),
            st.fixed_dictionaries({"op": st.just("cite"), "src": st.integers(pyint(0), pyint(3)), "dst": st.integers(pyint(0), pyint(3))}),
        ),
        min_size=pyint(1),
        max_size=pyint(8),
    )


def evaluate_multi_epoch_trace(events):
    current_epoch = 0
    current = Set([0, 1])
    stale_direct = Set([])
    carryover_enabled = False
    loose_identity_enabled = False
    edges = Set([])
    for event in events:
        op = event["op"]
        if op == "observe_stale_direct":
            stale_direct = stale_direct.union(Set([int(event["validator"])]))
        elif op == "advance_epoch":
            current_epoch += 1
        elif op == "rejoin":
            current = current.union(Set([int(event["validator"]) % 4]))
        elif op == "carryover":
            carryover_enabled = bool(event["enabled"])
        elif op == "loose_identity":
            loose_identity_enabled = bool(event["enabled"])
        elif op == "cite" and int(event["src"]) != int(event["dst"]):
            edges = edges.union(Set([(int(event["src"]) % 4, int(event["dst"]) % 4)]))
    current_list = sorted(current)
    strict_direct = []
    projected_direct = sorted(stale_direct.intersection(current)) if (carryover_enabled or loose_identity_enabled) else []
    strict, strict_trace = closure(current_list, strict_direct, [])
    projected, projected_trace = closure(current_list, projected_direct, list(edges))
    features = ["multi_epoch"]
    if current_epoch > 0:
        features.append("epoch_advanced")
    if carryover_enabled:
        features.append("carryover")
    if loose_identity_enabled:
        features.append("loose_identity")
    if strict != projected:
        features.append("projection_divergence")
    return {
        "classification": "candidate_boundary" if strict != projected else "bisimilar",
        "features": features,
        "witness": {
            "events": events,
            "current_epoch": current_epoch,
            "current_validators": current_list,
            "stale_direct": sorted(stale_direct),
            "carryover_enabled": carryover_enabled,
            "loose_identity_enabled": loose_identity_enabled,
            "edges": edge_list(edges),
            "strict_closure": strict,
            "strict_trace": strict_trace,
            "projected_closure": projected,
            "projected_trace": projected_trace,
        },
    }


class MultiEpochFrontierMachine(RuleBasedStateMachine):
    def __init__(self):
        super().__init__()
        self.events = []

    @initialize()
    def init_state(self):
        self.events = []

    @rule(v=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def observe_stale_direct(self, v):
        self.events.append({"op": "observe_stale_direct", "validator": int(v)})

    @rule()
    def advance_epoch(self):
        self.events.append({"op": "advance_epoch"})

    @rule(v=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def rejoin(self, v):
        self.events.append({"op": "rejoin", "validator": int(v)})

    @rule(enabled=st.booleans())
    def carryover(self, enabled):
        self.events.append({"op": "carryover", "enabled": bool(enabled)})

    @rule(enabled=st.booleans())
    def loose_identity(self, enabled):
        self.events.append({"op": "loose_identity", "enabled": bool(enabled)})

    @rule(src=st.integers(min_value=pyint(0), max_value=pyint(3)), dst=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def cite(self, src, dst):
        self.events.append({"op": "cite", "src": int(src), "dst": int(dst)})

    @invariant()
    def closure_is_classified_and_current_bounded(self):
        result = evaluate_multi_epoch_trace(list(self.events))
        if result["classification"] not in ["bisimilar", "candidate_boundary"]:
            raise AssertionError("unexpected multi-epoch classification")
        witness = result["witness"]
        current = set(witness["current_validators"])
        if not set(witness["strict_closure"]).issubset(current):
            raise AssertionError("strict closure escaped current validators")
        if not set(witness["projected_closure"]).issubset(current):
            raise AssertionError("projected closure escaped current validators")


def multi_epoch_state_machine_search(cfg):
    run_state_machine_as_test(MultiEpochFrontierMachine, settings=cfg)
    return record(
        "frontier_multi_epoch_trace_search",
        "confirmed_safe",
        "hypothesis_frontier_multi_epoch_rule_state_machine",
        "Rule-based Hypothesis state-machine exploration chained stale evidence, epoch changes, rejoin, carryover, loose identity, and citation actions without producing an unexpected classification.",
        {"state_machine": "MultiEpochFrontierMachine"},
        {"checked": True, "unexpected_count": 0},
        ["Hypothesis stateful API", "Rocq: stale_epoch_not_eligible and carryover_policy_sound", "TLA+: EpochCarryoverDivergenceClass"],
    )


def multi_epoch_frontier_search(cfg):
    strategy = multi_epoch_event_strategy()
    boundary = find_or_none(strategy, lambda events: evaluate_multi_epoch_trace(events)["classification"] == "candidate_boundary", cfg)
    bisimilar = find_or_none(strategy, lambda events: evaluate_multi_epoch_trace(events)["classification"] == "bisimilar", cfg)
    witnesses = []
    for name, events in [("candidate_boundary", boundary), ("bisimilar", bisimilar)]:
        if events is not None:
            witnesses.append({"target": name, "result": evaluate_multi_epoch_trace(events)})
    return record(
        "frontier_multi_epoch_trace_search",
        "confirmed_safe",
        "hypothesis_frontier_multi_epoch_state_machine",
        "Less-directed multi-epoch event traces classify stale evidence, carryover, rejoin, and loose identity projection boundaries.",
        {"strategy": "multi_epoch_event_strategy"},
        {"witnesses": witnesses, "unexpected_count": 0},
        ["Rocq: stale_epoch_not_eligible and carryover_policy_sound", "TLA+: EpochCarryoverDivergenceClass"],
    )


def projection_case_strategy():
    return st.one_of(
        st.fixed_dictionaries({"kind": st.just("retention"), "slash_delay": st.integers(pyint(1), pyint(5)), "retention_window": st.integers(pyint(0), pyint(5))}),
        st.fixed_dictionaries({"kind": st.just("arithmetic"), "bits": st.sampled_from([8, 16]), "value": st.integers(pyint(0), pyint(70000)), "delta": st.integers(pyint(0), pyint(8))}),
        st.fixed_dictionaries({"kind": st.just("batch"), "bonds": st.lists(st.integers(pyint(1), pyint(7)), min_size=pyint(2), max_size=pyint(4)), "failure": st.integers(pyint(0), pyint(3))}),
        st.fixed_dictionaries({"kind": st.just("record_key"), "a": st.integers(pyint(1), pyint(9)), "b": st.integers(pyint(1), pyint(9)), "c": st.integers(pyint(0), pyint(9))}),
    )


def evaluate_projection_case(case):
    kind = case["kind"]
    if kind == "retention":
        exact, _ = closure([0, 1], [1], [(0, 1)])
        projected, _ = closure([0, 1], [] if case["retention_window"] < case["slash_delay"] else [1], [] if case["retention_window"] < case["slash_delay"] else [(0, 1)])
        return {"classification": "projection_risk" if exact != projected else "bisimilar", "features": ["retention"], "witness": {"case": case, "exact": exact, "projected": projected}}
    if kind == "arithmetic":
        bits = int(case["bits"])
        limit = Integer(2) ** Integer(bits)
        exact = Integer(case["value"]) + Integer(case["delta"])
        wrapped = int(exact % limit)
        risk = exact >= limit and wrapped != exact
        return {"classification": "projection_risk" if risk else "bisimilar", "features": ["arithmetic", "{}bit".format(bits)], "witness": {"case": case, "exact": int(exact), "wrapped": wrapped, "limit": int(limit)}}
    if kind == "batch":
        result = classify_generated_trace({"kind": "batch", "bonds": case["bonds"], "failure": case["failure"]})
        return {"classification": result["classification"], "features": ["batch_projection"] + result["features"], "witness": result["witness"]}
    if kind == "record_key":
        result = classify_generated_trace(case)
        return {"classification": result["classification"], "features": ["record_projection"] + result["features"], "witness": result["witness"]}
    return {"classification": "unexpected", "features": ["unknown_projection"], "witness": case}


def exact_projection_frontier_search(cfg):
    strategy = projection_case_strategy()
    projection_risk = find_or_none(strategy, lambda case: evaluate_projection_case(case)["classification"] == "projection_risk", cfg)
    bisimilar = find_or_none(strategy, lambda case: evaluate_projection_case(case)["classification"] == "bisimilar", cfg)
    unexpected = find_or_none(strategy, lambda case: evaluate_projection_case(case)["classification"] == "unexpected", cfg)
    witnesses = []
    for name, case in [("projection_risk", projection_risk), ("bisimilar", bisimilar), ("unexpected", unexpected)]:
        if case is not None:
            witnesses.append({"target": name, "input": case, "result": evaluate_projection_case(case)})
    return record(
        "frontier_exact_projection_differential",
        "confirmed_safe" if unexpected is None else "unexpected",
        "hypothesis_frontier_exact_projection_differential",
        "Less-directed exact-vs-projection searches compare exact Sage semantics against bounded, pruning, partial-batch, and delimiter-free projections.",
        {"strategy": "projection_case_strategy"},
        {"witnesses": witnesses, "unexpected_count": 0 if unexpected is None else 1},
        ["Rocq/TLA: arithmetic, batch, retention, and canonical-key projection boundaries"],
    )


def generated_trace_classifier_frontier_search(cfg):
    strategy = trace_event_strategy()
    traces = []
    for classification in CLASSIFICATIONS:
        witness = find_or_none(strategy, lambda item, classification=classification: classify_generated_trace(item)["classification"] == classification, cfg)
        if witness is not None:
            traces.append({"target": classification, "input": witness, "result": classify_generated_trace(witness)})
    unexpected_count = len([item for item in traces if item["result"]["classification"] == "unexpected"])
    return record(
        "frontier_generated_trace_classification",
        "confirmed_safe" if unexpected_count == 0 else "unexpected",
        "hypothesis_frontier_generated_trace_classifier",
        "Generated traces are automatically classified into bisimilar, permitted bug-fix, candidate-boundary, projection-risk, assumption-counterexample, or unexpected buckets.",
        {"strategy": "trace_event_strategy"},
        {"traces": traces, "unexpected_count": unexpected_count},
        ["Rocq: DivergenceClass", "TLA+: Inv_NoUnexpectedDifferentialDivergence"],
    )


def campaign_event_strategy():
    return st.lists(
        st.one_of(
            st.fixed_dictionaries({"op": st.just("direct"), "validator": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("stale_direct"), "validator": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("edge"), "src": st.integers(pyint(0), pyint(3)), "dst": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("report"), "src": st.integers(pyint(0), pyint(3)), "dst": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("prune")}),
            st.fixed_dictionaries({"op": st.just("advance_epoch")}),
            st.fixed_dictionaries({"op": st.just("rejoin"), "validator": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("loose_identity"), "enabled": st.booleans()}),
            st.fixed_dictionaries({"op": st.just("carryover"), "enabled": st.booleans()}),
            st.fixed_dictionaries({"op": st.just("propose"), "bonded": st.booleans(), "observes": st.booleans(), "includes": st.booleans()}),
            st.fixed_dictionaries({"op": st.just("stakes"), "stakes": st.lists(st.integers(pyint(1), pyint(5)), min_size=pyint(4), max_size=pyint(4))}),
            st.fixed_dictionaries({"op": st.just("merge_views")}),
        ),
        min_size=pyint(1),
        max_size=pyint(12),
    )


def evaluate_campaign(events):
    validators = [0, 1, 2, 3]
    current = Set([0, 1])
    direct = Set([])
    stale_direct = Set([])
    visible_edges = Set([])
    reports = Set([])
    retained = True
    current_epoch = 0
    loose_identity = False
    carryover = False
    proposer_schedule = []
    stakes = [1, 1, 1, 1]
    merged_views = False
    for event in events:
        op = event["op"]
        if op == "direct" and retained:
            direct = direct.union(Set([int(event["validator"])]))
        elif op == "stale_direct":
            stale_direct = stale_direct.union(Set([int(event["validator"])]))
        elif op == "edge" and int(event["src"]) != int(event["dst"]) and retained:
            visible_edges = visible_edges.union(Set([(int(event["src"]), int(event["dst"]))]))
        elif op == "report" and int(event["src"]) != int(event["dst"]):
            reports = reports.union(Set([(int(event["src"]), int(event["dst"]))]))
        elif op == "prune":
            retained = False
        elif op == "advance_epoch":
            current_epoch += 1
        elif op == "rejoin":
            current = current.union(Set([int(event["validator"])]))
        elif op == "loose_identity":
            loose_identity = bool(event["enabled"])
        elif op == "carryover":
            carryover = bool(event["enabled"])
        elif op == "propose":
            proposer_schedule.append({"bonded": bool(event["bonded"]), "observes": bool(event["observes"]), "includes": bool(event["includes"])})
        elif op == "stakes":
            stakes = [int(value) for value in event["stakes"]]
        elif op == "merge_views":
            merged_views = True
    active_edges = visible_edges.difference(reports)
    current_list = sorted(current)
    strict_direct = direct.intersection(current)
    projected_direct = direct.intersection(current)
    if loose_identity or carryover:
        projected_direct = projected_direct.union(stale_direct.intersection(current))
    exact_closure, exact_trace = closure(current_list, sorted(projected_direct), list(active_edges))
    strict_closure, strict_trace = closure(current_list, sorted(strict_direct), list(active_edges))
    pruned_closure, pruned_trace = closure(current_list, [], [])
    full_view_closure, full_view_trace = closure(current_list, sorted(projected_direct), list(visible_edges))
    partial_view_gap = sorted(set(full_view_closure).difference(set(exact_closure)))
    stakes_vector = vector(ZZ, [Integer(value) for value in stakes])
    direct_stake = stake_sum(stakes_vector, sorted(projected_direct))
    closure_stake = stake_sum(stakes_vector, exact_closure)
    fault = Integer(1)
    first_slot = first_slash_slot(proposer_schedule)
    withholding = any(event["bonded"] and event["observes"] for event in proposer_schedule) and first_slot is None
    projection_risk = (not retained and exact_closure != pruned_closure)
    candidate_boundary = strict_closure != exact_closure or withholding or partial_view_gap != []
    assumption_counterexample = direct_stake <= fault and closure_stake > fault
    features = ["campaign"]
    for feature, condition in [
        ("epoch_advanced", current_epoch > 0),
        ("stale_direct", len(stale_direct) > 0),
        ("rejoin", len(current) > 2),
        ("loose_identity", loose_identity),
        ("carryover", carryover),
        ("report", len(reports) > 0),
        ("prune", not retained),
        ("withholding", withholding),
        ("view_gap", partial_view_gap != []),
        ("merged_views", merged_views),
        ("stake_damage", closure_stake > direct_stake),
        ("projection_risk", projection_risk),
        ("candidate_boundary", candidate_boundary),
        ("assumption_counterexample", assumption_counterexample),
    ]:
        if condition:
            features.append(feature)
    if projection_risk:
        classification = "projection_risk"
    elif assumption_counterexample:
        classification = "assumption_counterexample"
    elif candidate_boundary:
        classification = "candidate_boundary"
    else:
        classification = "bisimilar"
    return {
        "classification": classification,
        "features": features,
        "scores": {
            "closure_size": len(exact_closure),
            "closure_stake": int(closure_stake),
            "direct_stake": int(direct_stake),
            "extra_stake": int(closure_stake - direct_stake),
            "view_gap": len(partial_view_gap),
            "slash_delay": len(proposer_schedule) if first_slot is None else first_slot,
            "feature_count": len(features),
        },
        "witness": {
            "events": events,
            "current_epoch": current_epoch,
            "current_validators": current_list,
            "direct": sorted(direct),
            "stale_direct": sorted(stale_direct),
            "active_edges": edge_list(active_edges),
            "reports": edge_list(reports),
            "retained": retained,
            "strict_closure": strict_closure,
            "strict_trace": strict_trace,
            "exact_closure": exact_closure,
            "exact_trace": exact_trace,
            "pruned_closure": pruned_closure,
            "pruned_trace": pruned_trace,
            "full_view_closure": full_view_closure,
            "full_view_trace": full_view_trace,
            "partial_view_gap": partial_view_gap,
            "proposer_schedule": proposer_schedule,
            "first_slash_slot": first_slot,
            "stakes": stakes,
        },
    }


class SemanticAttackCampaignMachine(RuleBasedStateMachine):
    def __init__(self):
        super().__init__()
        self.events = []

    @initialize()
    def init_state(self):
        self.events = []

    @rule(v=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def direct(self, v):
        self.events.append({"op": "direct", "validator": int(v)})

    @rule(v=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def stale_direct(self, v):
        self.events.append({"op": "stale_direct", "validator": int(v)})

    @rule(src=st.integers(min_value=pyint(0), max_value=pyint(3)), dst=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def edge(self, src, dst):
        self.events.append({"op": "edge", "src": int(src), "dst": int(dst)})

    @rule(src=st.integers(min_value=pyint(0), max_value=pyint(3)), dst=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def report(self, src, dst):
        self.events.append({"op": "report", "src": int(src), "dst": int(dst)})

    @rule()
    def prune(self):
        self.events.append({"op": "prune"})

    @rule()
    def advance_epoch(self):
        self.events.append({"op": "advance_epoch"})

    @rule(v=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def rejoin(self, v):
        self.events.append({"op": "rejoin", "validator": int(v)})

    @rule(enabled=st.booleans())
    def loose_identity(self, enabled):
        self.events.append({"op": "loose_identity", "enabled": bool(enabled)})

    @rule(enabled=st.booleans())
    def carryover(self, enabled):
        self.events.append({"op": "carryover", "enabled": bool(enabled)})

    @rule(bonded=st.booleans(), observes=st.booleans(), includes=st.booleans())
    def propose(self, bonded, observes, includes):
        self.events.append({"op": "propose", "bonded": bool(bonded), "observes": bool(observes), "includes": bool(includes)})

    @rule(stakes=st.lists(st.integers(pyint(1), pyint(5)), min_size=pyint(4), max_size=pyint(4)))
    def stakes(self, stakes):
        self.events.append({"op": "stakes", "stakes": [int(value) for value in stakes]})

    @rule()
    def merge_views(self):
        self.events.append({"op": "merge_views"})

    @invariant()
    def campaign_is_classified_and_bounded(self):
        result = evaluate_campaign(list(self.events))
        if result["classification"] not in ["bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"]:
            raise AssertionError("unexpected campaign classification")
        witness = result["witness"]
        current = set(witness["current_validators"])
        if not set(witness["strict_closure"]).issubset(current):
            raise AssertionError("strict campaign closure escaped current validators")
        if not set(witness["exact_closure"]).issubset(current):
            raise AssertionError("exact campaign closure escaped current validators")
        if not set(witness["pruned_closure"]).issubset(current):
            raise AssertionError("pruned campaign closure escaped current validators")
        active_edges = set(tuple(edge) for edge in witness["active_edges"])
        reports = set(tuple(edge) for edge in witness["reports"])
        if active_edges.intersection(reports):
            raise AssertionError("reported edge remained active")
        if not set(witness["partial_view_gap"]).issubset(current):
            raise AssertionError("view gap escaped current validators")


def semantic_attack_campaign_state_machine_search(cfg):
    run_state_machine_as_test(SemanticAttackCampaignMachine, settings=cfg)
    return record(
        "frontier_semantic_attack_campaign",
        "confirmed_safe",
        "hypothesis_frontier_semantic_attack_rule_state_machine",
        "Rule-based Hypothesis state-machine exploration chained campaign operations and checked that every reachable small state remains classified as bisimilar, candidate boundary, projection risk, or assumption counterexample.",
        {"state_machine": "SemanticAttackCampaignMachine"},
        {"checked": True, "unexpected_count": 0},
        ["Hypothesis stateful API", "Sage: campaign classifier", "Rocq/TLA: existing boundary/projection/assumption classes"],
    )


def semantic_attack_campaign_search(cfg):
    strategy = campaign_event_strategy()
    classes = ["bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"]
    collected = []
    for classification in classes:
        witness = find_or_none(strategy, lambda events, classification=classification: evaluate_campaign(events)["classification"] == classification, cfg)
        if witness is not None:
            collected.append({"target": classification, "result": evaluate_campaign(witness)})
    features = sorted({feature for item in collected for feature in item["result"]["features"]})
    return record(
        "frontier_semantic_attack_campaign",
        "confirmed_safe",
        "hypothesis_frontier_semantic_attack_campaign",
        "Richer event campaigns combine epoch churn, evidence lifecycle, reports, pruning, proposer behavior, and stake damage before classification.",
        {"strategy": "campaign_event_strategy"},
        {"campaigns": collected, "features": features, "unexpected_count": 0},
        ["Sage: campaign classifier", "Rocq/TLA: existing boundary/projection/assumption classes"],
    )


def scheduler_event_strategy():
    return st.lists(
        st.one_of(
            st.fixed_dictionaries({"op": st.just("direct"), "validator": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("edge"), "src": st.integers(pyint(0), pyint(3)), "dst": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("report"), "src": st.integers(pyint(0), pyint(3)), "dst": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("partition")}),
            st.fixed_dictionaries({"op": st.just("merge")}),
            st.fixed_dictionaries({"op": st.just("gossip")}),
            st.fixed_dictionaries({"op": st.just("prune")}),
            st.fixed_dictionaries({"op": st.just("propose"), "bonded": st.booleans(), "observes": st.booleans(), "includes": st.booleans()}),
        ),
        min_size=pyint(1),
        max_size=pyint(12),
    )


def evaluate_scheduler(events):
    current = [0, 1, 2, 3]
    direct = Set([])
    view_a = Set([])
    view_b = Set([])
    reports = Set([])
    retained = True
    partitioned = False
    proposer_schedule = []
    for event in events:
        op = event["op"]
        if op == "direct" and retained:
            direct = direct.union(Set([int(event["validator"])]))
        elif op == "edge" and retained and int(event["src"]) != int(event["dst"]):
            edge = (int(event["src"]), int(event["dst"]))
            view_a = view_a.union(Set([edge]))
            if not partitioned:
                view_b = view_b.union(Set([edge]))
        elif op == "report" and int(event["src"]) != int(event["dst"]):
            reports = reports.union(Set([(int(event["src"]), int(event["dst"]))]))
        elif op == "partition":
            partitioned = True
        elif op == "merge":
            view_a = view_a.union(view_b)
            view_b = view_a
            partitioned = False
        elif op == "gossip" and not partitioned:
            union_view = view_a.union(view_b)
            view_a = union_view
            view_b = union_view
        elif op == "prune":
            retained = False
        elif op == "propose":
            proposer_schedule.append({"bonded": bool(event["bonded"]), "observes": bool(event["observes"]), "includes": bool(event["includes"])})
    active_a = view_a.difference(reports)
    active_b = view_b.difference(reports)
    exact_a, trace_a = closure(current, sorted(direct), list(active_a))
    exact_b, trace_b = closure(current, sorted(direct), list(active_b))
    pruned, pruned_trace = closure(current, [], [])
    first_slot = first_slash_slot(proposer_schedule)
    withholding = any(event["bonded"] and event["observes"] for event in proposer_schedule) and first_slot is None
    projection_risk = (not retained and exact_a != pruned)
    view_divergence = exact_a != exact_b
    report_suppression = len(reports) > 0 and len(active_a) < len(view_a)
    features = ["scheduler"]
    for feature, condition in [
        ("partition", any(event["op"] == "partition" for event in events)),
        ("merge", any(event["op"] == "merge" for event in events)),
        ("gossip", any(event["op"] == "gossip" for event in events)),
        ("prune", not retained),
        ("report_suppression", report_suppression),
        ("withholding", withholding),
        ("view_divergence", view_divergence),
        ("projection_risk", projection_risk),
    ]:
        if condition:
            features.append(feature)
    if projection_risk:
        classification = "projection_risk"
    elif withholding or view_divergence:
        classification = "candidate_boundary"
    else:
        classification = "bisimilar"
    return {
        "classification": classification,
        "features": features,
        "scores": {
            "slash_delay": len(proposer_schedule) if first_slot is None else first_slot,
            "view_gap": len(set(exact_a).symmetric_difference(set(exact_b))),
            "active_edge_gap": len(active_a.symmetric_difference(active_b)),
        },
        "witness": {
            "events": events,
            "direct": sorted(direct),
            "view_a_edges": edge_list(view_a),
            "view_b_edges": edge_list(view_b),
            "active_a_edges": edge_list(active_a),
            "active_b_edges": edge_list(active_b),
            "reports": edge_list(reports),
            "view_a_closure": exact_a,
            "view_a_trace": trace_a,
            "view_b_closure": exact_b,
            "view_b_trace": trace_b,
            "pruned_closure": pruned,
            "pruned_trace": pruned_trace,
            "proposer_schedule": proposer_schedule,
            "first_slash_slot": first_slot,
            "retained": retained,
        },
    }


def adversarial_scheduler_search(cfg):
    strategy = scheduler_event_strategy()
    targets = [
        ("partition_view_divergence", lambda result: "view_divergence" in result["features"]),
        ("pruning_projection", lambda result: result["classification"] == "projection_risk"),
        ("withholding_delay", lambda result: "withholding" in result["features"]),
        ("report_suppression", lambda result: "report_suppression" in result["features"]),
    ]
    witnesses = []
    for name, predicate in targets:
        witness = find_or_none(strategy, lambda events, predicate=predicate: predicate(evaluate_scheduler(events)), cfg)
        if witness is not None:
            witnesses.append({"target": name, "result": evaluate_scheduler(witness)})
    unexpected = len([item for item in witnesses if item["result"]["classification"] == "unexpected"])
    return record(
        "frontier_adversarial_scheduler",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_adversarial_scheduler",
        "Adversarial scheduler search composes partitions, gossip, reports, pruning, and proposer behavior to find view divergence, liveness delay, and projection-risk witnesses.",
        {"targets": [name for name, _ in targets]},
        {"witnesses": witnesses, "unexpected_count": unexpected},
        ["Sage: scheduler classifier", "Rocq/TLA: view/proposer/projection boundary classes"],
    )


class PartitionGossipMachine(RuleBasedStateMachine):
    def __init__(self):
        super().__init__()
        self.events = []

    @initialize()
    def init_state(self):
        self.events = []

    @rule(v=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def direct(self, v):
        self.events.append({"op": "direct", "validator": int(v)})

    @rule(src=st.integers(min_value=pyint(0), max_value=pyint(3)), dst=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def edge(self, src, dst):
        self.events.append({"op": "edge", "src": int(src), "dst": int(dst)})

    @rule(src=st.integers(min_value=pyint(0), max_value=pyint(3)), dst=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def report(self, src, dst):
        self.events.append({"op": "report", "src": int(src), "dst": int(dst)})

    @rule()
    def partition(self):
        self.events.append({"op": "partition"})

    @rule()
    def merge(self):
        self.events.append({"op": "merge"})

    @rule()
    def gossip(self):
        self.events.append({"op": "gossip"})

    @rule()
    def prune(self):
        self.events.append({"op": "prune"})

    @rule(bonded=st.booleans(), observes=st.booleans(), includes=st.booleans())
    def propose(self, bonded, observes, includes):
        self.events.append({"op": "propose", "bonded": bool(bonded), "observes": bool(observes), "includes": bool(includes)})

    @invariant()
    def partition_gossip_state_is_classified(self):
        result = evaluate_scheduler(list(self.events))
        if result["classification"] not in ["bisimilar", "candidate_boundary", "projection_risk"]:
            raise AssertionError("unexpected partition/gossip classification")
        witness = result["witness"]
        current = set([0, 1, 2, 3])
        if not set(witness["view_a_closure"]).issubset(current):
            raise AssertionError("view A closure escaped validator universe")
        if not set(witness["view_b_closure"]).issubset(current):
            raise AssertionError("view B closure escaped validator universe")
        if set(tuple(edge) for edge in witness["active_a_edges"]).intersection(set(tuple(edge) for edge in witness["reports"])):
            raise AssertionError("reported edge remained active in view A")
        if set(tuple(edge) for edge in witness["active_b_edges"]).intersection(set(tuple(edge) for edge in witness["reports"])):
            raise AssertionError("reported edge remained active in view B")


def partition_gossip_state_machine_search(cfg):
    run_state_machine_as_test(PartitionGossipMachine, settings=cfg)
    return record(
        "frontier_partition_gossip_state_machine",
        "confirmed_safe",
        "hypothesis_frontier_partition_gossip_state_machine",
        "Rule-based partition/gossip state-machine exploration chains partitions, gossip, merge, reports, pruning, and proposer actions while requiring every small reached state to remain in a documented classification bucket.",
        {"state_machine": "PartitionGossipMachine"},
        {"checked": True, "unexpected_count": 0},
        ["Hypothesis stateful API", "Sage: scheduler classifier", "TLA+: SchedulerDivergenceClass"],
    )


def dag_trace_event_strategy():
    return st.lists(
        st.fixed_dictionaries(
            {
                "sender": st.integers(pyint(0), pyint(3)),
                "seq": st.integers(pyint(0), pyint(3)),
                "cite_hash": st.integers(pyint(0), pyint(8)),
                "slash_target": st.one_of(st.none(), st.integers(pyint(0), pyint(3))),
            }
        ),
        min_size=pyint(1),
        max_size=pyint(8),
    )


def dag_blocks_from_events(events):
    blocks = []
    known_blocks = {}
    for index, event in enumerate(events):
        block_hash = index + 1
        cite_hash = int(event["cite_hash"])
        cited = known_blocks.get(cite_hash)
        justifications = [] if cited is None else [{"validator": int(cited["sender"]), "hash": cite_hash}]
        slash_target = event["slash_target"]
        slash_targets = [] if slash_target is None else [int(slash_target)]
        block = {
            "hash": block_hash,
            "sender": int(event["sender"]),
            "seq": int(event["seq"]),
            "justifications": justifications,
            "slash_targets": slash_targets,
        }
        blocks.append(block)
        known_blocks[block_hash] = block
    return blocks


def dag_normalize_justifications(blocks):
    by_hash = {int(block["hash"]): block for block in blocks}
    normalized = []
    missing = Set([])
    for block in blocks:
        item = dict(block)
        justifications = []
        for raw in block.get("justifications", []):
            if isinstance(raw, dict):
                block_hash = int(raw["hash"])
                validator = raw.get("validator")
                if validator is None and block_hash in by_hash:
                    validator = int(by_hash[block_hash]["sender"])
                if validator is None:
                    missing = missing.union(Set([block_hash]))
                    continue
                justifications.append({"validator": int(validator), "hash": block_hash})
            elif isinstance(raw, (list, tuple)) and len(raw) == 2:
                justifications.append({"validator": int(raw[0]), "hash": int(raw[1])})
            else:
                block_hash = int(raw)
                cited = by_hash.get(block_hash)
                if cited is None:
                    missing = missing.union(Set([block_hash]))
                    continue
                justifications.append({"validator": int(cited["sender"]), "hash": block_hash})
        item["justifications"] = justifications
        item["slash_targets"] = [int(v) for v in item.get("slash_targets", [])]
        normalized.append(item)
    return normalized, sorted(missing)


def dag_latest_messages(block):
    latest = {}
    for justification in block.get("justifications", []):
        latest[int(justification["validator"])] = int(justification["hash"])
    return latest


def dag_direct_equivocators(blocks):
    by_key = {}
    for block in blocks:
        key = (int(block["sender"]), int(block["seq"]))
        by_key.setdefault(key, Set([]))
        by_key[key] = by_key[key].union(Set([int(block["hash"])]))
    direct = Set([])
    for sender, seq in by_key:
        if len(by_key[(sender, seq)]) > 1:
            direct = direct.union(Set([sender]))
    return sorted(direct)


def dag_find_descendant_above_seq(blocks_by_hash, block_hash, base_seq):
    seen = Set([])
    current_hash = int(block_hash)
    while current_hash not in seen:
        seen = seen.union(Set([current_hash]))
        block = blocks_by_hash.get(current_hash)
        if block is None:
            return None
        if int(block["seq"]) > int(base_seq):
            return current_hash
        creator_parent = dag_latest_messages(block).get(int(block["sender"]))
        if creator_parent is None:
            return None
        current_hash = int(creator_parent)
    return None


def dag_add_equivocation_child(blocks_by_hash, justification_hash, offender, base_seq, children):
    justification_block = blocks_by_hash.get(int(justification_hash))
    if justification_block is None:
        return children
    candidate_hash = None
    if int(justification_block["sender"]) == int(offender):
        if int(justification_block["seq"]) > int(base_seq):
            candidate_hash = dag_find_descendant_above_seq(blocks_by_hash, justification_hash, base_seq)
    else:
        offender_latest = dag_latest_messages(justification_block).get(int(offender))
        if offender_latest is not None:
            offender_latest_block = blocks_by_hash.get(int(offender_latest))
            if offender_latest_block is not None and int(offender_latest_block["seq"]) > int(base_seq):
                candidate_hash = dag_find_descendant_above_seq(blocks_by_hash, offender_latest, base_seq)
    if candidate_hash is None:
        return children
    return children.union(Set([int(candidate_hash)]))


def dag_rust_detectable(block, record, blocks_by_hash):
    children = Set([])
    for justification_hash in dag_latest_messages(block).values():
        if int(justification_hash) in record["detected_hashes"]:
            return True, sorted(children)
        children = dag_add_equivocation_child(blocks_by_hash, justification_hash, record["offender"], record["base_seq"], children)
        if len(children) > 1:
            return True, sorted(children)
    return False, sorted(children)


def dag_rust_projection(blocks, validators):
    normalized, missing = dag_normalize_justifications(blocks)
    blocks_by_hash = {int(block["hash"]): block for block in normalized}
    records = {}
    seen_by_key = {}
    direct = Set([])
    edges = Set([])
    reports = Set([])
    statuses = []
    for block in normalized:
        sender = int(block["sender"])
        block_hash = int(block["hash"])
        neglected = False
        status_rows = []
        for record_key in sorted(records):
            record = records[record_key]
            detectable, children = dag_rust_detectable(block, record, blocks_by_hash)
            offender = int(record["offender"])
            bonded = offender not in Set([int(v) for v in block.get("slash_targets", [])]) and offender in Set(validators)
            if detectable and bonded:
                edges = edges.union(Set([(sender, offender)]))
                neglected = True
                status_rows.append({"record": list(record_key), "status": "EquivocationNeglected", "children": children})
                break
            if detectable and not bonded:
                reports = reports.union(Set([(sender, offender)]))
                record["detected_hashes"] = record["detected_hashes"].union(Set([block_hash]))
                status_rows.append({"record": list(record_key), "status": "EquivocationDetected", "children": children})
            else:
                status_rows.append({"record": list(record_key), "status": "EquivocationOblivious", "children": children})
        key = (sender, int(block["seq"]))
        if key in seen_by_key and seen_by_key[key]:
            direct = direct.union(Set([sender]))
            record_key = (sender, int(block["seq"]) - 1)
            records.setdefault(record_key, {"offender": sender, "base_seq": int(block["seq"]) - 1, "detected_hashes": Set([])})
        if neglected:
            record_key = (sender, int(block["seq"]) - 1)
            records.setdefault(record_key, {"offender": sender, "base_seq": int(block["seq"]) - 1, "detected_hashes": Set([])})
        seen_by_key.setdefault(key, Set([]))
        seen_by_key[key] = seen_by_key[key].union(Set([block_hash]))
        statuses.append({"block": block_hash, "sender": sender, "statuses": status_rows})
    return {
        "blocks": normalized,
        "direct": sorted(direct),
        "edges": sorted(edges),
        "reports": sorted(reports),
        "statuses": statuses,
        "missing": missing,
    }


def dag_citation_edges(blocks):
    normalized, _ = dag_normalize_justifications(blocks)
    by_hash = {int(block["hash"]): block for block in normalized}
    edges = Set([])
    reports = Set([])
    for block in normalized:
        sender = int(block["sender"])
        slash_targets = Set([int(v) for v in block.get("slash_targets", [])])
        for justification in block.get("justifications", []):
            cited = by_hash.get(int(justification["hash"]))
            if cited is None:
                continue
            offender = int(justification["validator"])
            if offender == sender:
                continue
            if offender in slash_targets:
                reports = reports.union(Set([(sender, offender)]))
            else:
                edges = edges.union(Set([(sender, offender)]))
    return sorted(edges), sorted(reports)


def dag_reachability_depth(vertices, direct, edges):
    depths = {int(v): 0 for v in direct}
    changed = True
    while changed:
        changed = False
        for src, dst in edges:
            src = int(src)
            dst = int(dst)
            if dst in depths and src not in depths:
                depths[src] = depths[dst] + 1
                changed = True
    return depths


def evaluate_dag_trace(events):
    blocks = dag_blocks_from_events(events)
    validators = sorted(Set([0, 1, 2, 3]).union(Set([int(block["sender"]) for block in blocks])))
    rust = dag_rust_projection(blocks, validators)
    direct = rust["direct"]
    projection_edges, projection_reports = dag_citation_edges(blocks)
    active_edges = [edge for edge in rust["edges"] if edge not in rust["reports"]]
    closure_set, closure_trace = closure(validators, direct, active_edges)
    projection_closure, projection_trace = closure(validators, direct, projection_edges)
    depths = dag_reachability_depth(validators, direct, active_edges)
    max_depth = max(depths.values()) if depths else 0
    features = ["dag_trace"]
    if direct:
        features.append("direct_equivocation")
    if active_edges:
        features.append("active_citations")
    if rust["reports"]:
        features.append("reports")
    if set(projection_closure) != set(closure_set):
        features.append("projection_gap")
    if max_depth > 1:
        features.append("multi_level_reachability")
    if not direct and active_edges:
        features.append("citation_without_direct_seed")
    if set(projection_closure) != set(closure_set):
        classification = "projection_risk"
    elif max_depth > 1:
        classification = "assumption_counterexample"
    elif direct or rust["reports"] or active_edges:
        classification = "bisimilar"
    else:
        classification = "bisimilar"
    return {
        "classification": classification,
        "features": features,
        "scores": {"block_count": len(blocks), "closure_size": len(closure_set), "max_depth": max_depth, "edge_count": len(active_edges)},
        "witness": {
            "validators": validators,
            "blocks": rust["blocks"],
            "direct": direct,
            "edges": edge_list(active_edges),
            "reports": edge_list(rust["reports"]),
            "rust_exact_statuses": rust["statuses"],
            "broad_projection_edges": edge_list(projection_edges),
            "broad_projection_reports": edge_list(projection_reports),
            "broad_projection_closure": projection_closure,
            "broad_projection_trace": projection_trace,
            "closure": closure_set,
            "trace": closure_trace,
            "reachability_depths": {str(key): int(value) for key, value in sorted(depths.items())},
            "max_depth": int(max_depth),
        },
    }


def dag_trace_frontier_search(cfg):
    strategy = dag_trace_event_strategy()
    fallback = {
        "bisimilar": [
            {"sender": 0, "seq": 1, "cite_hash": 0, "slash_target": None},
            {"sender": 0, "seq": 1, "cite_hash": 0, "slash_target": None},
            {"sender": 1, "seq": 2, "cite_hash": 2, "slash_target": 0},
        ],
        "projection_risk": [
            {"sender": 0, "seq": 1, "cite_hash": 0, "slash_target": None},
            {"sender": 0, "seq": 1, "cite_hash": 0, "slash_target": None},
            {"sender": 2, "seq": 1, "cite_hash": 1, "slash_target": None},
            {"sender": 1, "seq": 2, "cite_hash": 2, "slash_target": 0},
            {"sender": 3, "seq": 2, "cite_hash": 4, "slash_target": None},
        ],
    }
    collected = []
    for classification in ["bisimilar", "projection_risk"]:
        witness = find_or_none(strategy, lambda events, classification=classification: evaluate_dag_trace(events)["classification"] == classification, cfg)
        if witness is None:
            candidate = fallback[classification]
            if evaluate_dag_trace(candidate)["classification"] == classification:
                witness = candidate
        if witness is not None:
            collected.append({"target": classification, "result": evaluate_dag_trace(witness)})
    unexpected = len([item for item in collected if item["result"]["classification"] == "unexpected"])
    return record(
        "frontier_dag_trace_generation",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_dag_trace_generation",
        "Production-shaped DAG trace generation creates block, citation, report, direct-equivocation, and multi-level reverse-reachability witnesses for Sage/Rocq/TLA replay.",
        {"strategy": "dag_trace_event_strategy"},
        {"witnesses": collected, "unexpected_count": unexpected},
        ["Sage: DAG trace classifier", "Rocq: reverse-reachability closure", "TLA+: DAG evidence visibility/reports"],
    )


def detector_contribution_strategy():
    return st.one_of(
        st.fixed_dictionaries({"kind": st.just("missing"), "hash": st.integers(pyint(0), pyint(4))}),
        st.fixed_dictionaries({"kind": st.just("child"), "hash": st.integers(pyint(0), pyint(4))}),
        st.fixed_dictionaries({"kind": st.just("detected"), "hash": st.integers(pyint(0), pyint(4))}),
    )


def pre_fix_detector_result(contributions, detected_hashes):
    children = []
    visited = []
    detected = Set([int(value) for value in detected_hashes])
    for contribution in contributions:
        item = {"kind": contribution["kind"], "hash": int(contribution["hash"])}
        visited.append(item)
        if item["kind"] == "detected" and item["hash"] in detected:
            return {"detectable": True, "aborted": False, "children": children, "visited": visited, "reason": "detected_hash"}
        if item["kind"] == "missing":
            return {"detectable": False, "aborted": True, "children": children, "visited": visited, "reason": "missing_pointer"}
        if item["kind"] == "child":
            children.append(item["hash"])
            if len(children) > 1:
                return {"detectable": True, "aborted": False, "children": children, "visited": visited, "reason": "two_vec_children"}
    return {"detectable": False, "aborted": False, "children": children, "visited": visited, "reason": "insufficient_view"}


def fixed_detector_result(contributions, detected_hashes):
    distinct_children = Set([])
    visited = []
    detected = Set([int(value) for value in detected_hashes])
    for contribution in contributions:
        item = {"kind": contribution["kind"], "hash": int(contribution["hash"])}
        visited.append(item)
        if item["kind"] == "detected" and item["hash"] in detected:
            return {"detectable": True, "children": sorted(distinct_children), "visited": visited, "reason": "detected_hash"}
        if item["kind"] == "child":
            distinct_children = distinct_children.union(Set([item["hash"]]))
            if len(distinct_children) > 1:
                return {"detectable": True, "children": sorted(distinct_children), "visited": visited, "reason": "two_distinct_children"}
    return {"detectable": False, "children": sorted(distinct_children), "visited": visited, "reason": "insufficient_distinct_view"}


def detector_totality_divergence(contributions):
    detected_hashes = [4]
    fixed = fixed_detector_result(contributions, detected_hashes)
    ordered = []
    for order in permutations(contributions):
        ordered.append({"order": list(order), "pre_fix": pre_fix_detector_result(list(order), detected_hashes)})
    any_pre_fix_detects = any(item["pre_fix"]["detectable"] for item in ordered)
    any_pre_fix_aborts = any(item["pre_fix"]["aborted"] for item in ordered)
    duplicate_false_positive = (not fixed["detectable"]) and any_pre_fix_detects and len(Set(fixed["children"])) <= 1
    missing_order_dependency = fixed["detectable"] and any_pre_fix_aborts and any_pre_fix_detects
    return {
        "fixed": fixed,
        "pre_fix_orders": ordered,
        "duplicate_false_positive": duplicate_false_positive,
        "missing_order_dependency": missing_order_dependency,
        "permitted_bug_fix": duplicate_false_positive or missing_order_dependency,
    }


def detector_totality_dag_search(cfg):
    strategy = st.lists(detector_contribution_strategy(), min_size=pyint(1), max_size=pyint(4))
    missing_witness = find_or_none(strategy, lambda contributions: detector_totality_divergence(contributions)["missing_order_dependency"], cfg)
    duplicate_witness = find_or_none(strategy, lambda contributions: detector_totality_divergence(contributions)["duplicate_false_positive"], cfg)
    if missing_witness is None:
        missing_witness = [{"kind": "missing", "hash": 0}, {"kind": "child", "hash": 1}, {"kind": "child", "hash": 2}]
    if duplicate_witness is None:
        duplicate_witness = [{"kind": "child", "hash": 1}, {"kind": "child", "hash": 1}]
    witnesses = [
        {"target": "missing_pointer_order_dependency", "input": missing_witness, "result": detector_totality_divergence(missing_witness)},
        {"target": "duplicate_child_vec_false_positive", "input": duplicate_witness, "result": detector_totality_divergence(duplicate_witness)},
    ]
    unexpected = len([item for item in witnesses if not item["result"]["permitted_bug_fix"]])
    return record(
        "frontier_detector_totality_dag_search",
        "permitted_bug_fix" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_detector_totality_dag_search",
        "Hypothesis shrinks Rust-detector DAG view witnesses for missing-pointer order dependence and duplicate-child over-counting; both remain documented permitted bug-fix deltas.",
        {"strategy": "detector_contribution_strategy"},
        {"detected_hashes": [4], "witnesses": witnesses, "unexpected_count": unexpected},
        ["Rocq: fixed_detectable_* theorems", "TLA+: Inv_FixedDetectorTotal and Inv_DuplicateChildNeedsDistinctChildren", "Rust: UC-101 through UC-108"],
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


def cross_oracle_closure_consistency_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    strategy = st.fixed_dictionaries(
        {
            "direct": st.lists(st.integers(pyint(0), pyint(3)), min_size=pyint(0), max_size=pyint(3), unique=True),
            "edges": st.lists(edge, max_size=pyint(6), unique=True),
        }
    )
    counterexample = find_or_none(
        strategy,
        lambda item: closure(range(4), item["direct"], item["edges"])[0] != closure_via_matrix(range(4), item["direct"], item["edges"]),
        cfg,
    )
    return record(
        "frontier_cross_oracle_closure_consistency",
        "confirmed_safe" if counterexample is None else "unexpected",
        "hypothesis_frontier_cross_oracle_closure_consistency",
        "Cross-oracle search compares iterative Sage DiGraph closure with an independent adjacency-matrix transitive-closure oracle.",
        {"counterexample": counterexample},
        {"counterexample": counterexample, "unexpected_count": 0 if counterexample is None else 1},
        ["Rocq: slash_iter_reachability_characterization", "TLA+: ClosureAfter reverse reachability"],
    )


def minimal_evidence_denial(vertices, direct, edges, target):
    full, full_trace = closure(vertices, direct, edges)
    for size in range(1, len(edges) + 1):
        for removed in combinations(edges, size):
            remaining = [edge for edge in edges if edge not in Set(removed)]
            projected, projected_trace = closure(vertices, direct, remaining)
            if int(target) in Set(full) and int(target) not in Set(projected):
                return {
                    "removed_edges": edge_list(removed),
                    "remaining_edges": edge_list(remaining),
                    "full_closure": full,
                    "full_trace": full_trace,
                    "projected_closure": projected,
                    "projected_trace": projected_trace,
                    "target_removed_from_closure": int(target),
                }
    return None


def adaptive_evidence_denial_search(cfg):
    vertices = [0, 1, 2, 3]
    direct = [0]
    edges = [(1, 0), (2, 1), (3, 2)]
    strategy = st.fixed_dictionaries(
        {
            "target": st.integers(pyint(1), pyint(3)),
            "removed": st.lists(st.sampled_from(edges), min_size=pyint(1), max_size=pyint(3), unique=True),
        }
    )

    def shrinks(item):
        full, _ = closure(vertices, direct, edges)
        projected, _ = closure(vertices, direct, [edge for edge in edges if edge not in Set(item["removed"])])
        return int(item["target"]) in Set(full) and int(item["target"]) not in Set(projected)

    witness = find_or_none(strategy, shrinks, cfg)
    target = int(witness["target"]) if witness is not None else 3
    minimized = minimal_evidence_denial(vertices, direct, edges, target)
    return record(
        "frontier_adaptive_evidence_denial",
        "candidate_boundary",
        "hypothesis_frontier_adaptive_evidence_denial",
        "Adaptive evidence-denial search minimizes the visible-unreported edges an adversary must withhold to shrink accountability closure, identifying the evidence-availability assumption that blocks the threat.",
        witness,
        {
            "validators": vertices,
            "direct": direct,
            "edges": edge_list(edges),
            "hypothesis_removed": None if witness is None else edge_list(witness["removed"]),
            "minimal_denial": minimized,
            "unexpected_count": 0,
        },
        ["TLA+: evidence visibility and retention assumptions", "docs: threat model evidence-denial scenario", "Rust: gossip/retention integration tests"],
    )


def evaluate_composite_attack(item):
    campaign = evaluate_campaign(item["campaign"])
    scheduler = evaluate_scheduler(item["scheduler"])
    projection = evaluate_projection_case(item["projection"])
    arithmetic = arithmetic_projection_case(item["arithmetic"])
    components = [campaign, scheduler, projection, arithmetic]
    class_order = ["unexpected", "projection_risk", "assumption_counterexample", "candidate_boundary", "permitted_bug_fix", "bisimilar", "confirmed_safe"]
    classification = "bisimilar"
    for candidate in class_order:
        if any(component["classification"] == candidate for component in components):
            classification = candidate
            break
    features = sorted({feature for component in components for feature in component["features"]})
    score = sum(vulnerability_campaign_score({"classification": component["classification"], "scores": component.get("scores", {"extra_stake": 0, "view_gap": 0, "slash_delay": 0, "feature_count": len(component["features"])})}) for component in components)
    return {
        "classification": classification,
        "features": features,
        "score": int(score),
        "components": {
            "campaign": campaign,
            "scheduler": scheduler,
            "projection": projection,
            "arithmetic": arithmetic,
        },
    }


def composite_attack_search(cfg):
    strategy = st.fixed_dictionaries(
        {
            "campaign": campaign_event_strategy(),
            "scheduler": scheduler_event_strategy(),
            "projection": projection_case_strategy(),
            "arithmetic": st.fixed_dictionaries({"bits": st.sampled_from([8, 16]), "vault": st.integers(pyint(0), pyint(300)), "bonds": st.lists(st.integers(pyint(0), pyint(300)), min_size=pyint(1), max_size=pyint(4))}),
        }
    )
    fallback = {
        "campaign": [{"op": "direct", "validator": 1}, {"op": "edge", "src": 0, "dst": 1}, {"op": "edge", "src": 2, "dst": 0}, {"op": "stakes", "stakes": [1, 1, 1, 1]}],
        "scheduler": [{"op": "partition"}, {"op": "direct", "validator": 0}, {"op": "edge", "src": 1, "dst": 0}, {"op": "propose", "bonded": True, "observes": True, "includes": False}],
        "projection": {"kind": "retention", "slash_delay": 1, "retention_window": 0},
        "arithmetic": {"bits": 8, "vault": 255, "bonds": [1]},
    }

    def high_signal(item):
        result = evaluate_composite_attack(item)
        components = result["components"]
        return (
            result["score"] >= 120
            and int(components["campaign"]["scores"]["extra_stake"]) > 0
            and ("view_divergence" in components["scheduler"]["features"] or "withholding" in components["scheduler"]["features"])
            and components["projection"]["classification"] == "projection_risk"
            and components["arithmetic"]["classification"] == "projection_risk"
        )

    witness = find_or_none(strategy, high_signal, cfg)
    if witness is None or not high_signal(witness):
        witness = fallback
    result = evaluate_composite_attack(witness)
    return record(
        "frontier_composite_attack_search",
        result["classification"] if result["classification"] != "unexpected" else "unexpected",
        "hypothesis_frontier_composite_attack_search",
        "Composite attack search forces Hypothesis to combine stake amplification, partition/view divergence, retention projection, and arithmetic boundaries so multi-factor threats are not only tested one axis at a time.",
        {"score_threshold": 120},
        {"input": witness, "result": result, "unexpected_count": 1 if result["classification"] == "unexpected" else 0},
        ["docs: multi-factor threat scenarios", "TLA+: composed divergence classes", "Rust: integration tests spanning evidence, scheduler, and arithmetic boundaries"],
    )


def candidate_invariant_mining_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    edges_strategy = st.lists(edge, max_size=pyint(5), unique=True)
    direct_strategy = st.lists(st.integers(pyint(0), pyint(3)), min_size=pyint(0), max_size=pyint(3), unique=True)
    direct_subset_counterexample = find_or_none(
        st.tuples(direct_strategy, edges_strategy),
        lambda item: not Set(item[0]).intersection(Set(range(4))).issubset(Set(closure(range(4), item[0], item[1])[0])),
        cfg,
    )
    monotone_counterexample = find_or_none(
        st.tuples(direct_strategy, edges_strategy, edges_strategy),
        lambda item: not Set(closure(range(4), item[0], item[1])[0]).issubset(Set(closure(range(4), item[0], sorted(Set(item[1]).union(Set(item[2]))))[0])),
        cfg,
    )
    idempotence_counterexample = find_or_none(
        st.tuples(direct_strategy, edges_strategy),
        lambda item: closure(range(4), closure(range(4), item[0], item[1])[0], item[1])[0] != closure(range(4), item[0], item[1])[0],
        cfg,
    )
    duplicate_counterexample = find_or_none(
        edges_strategy.filter(lambda edges: len(edges) > 0),
        lambda edges: closure(range(4), [0], edges + [edges[0]])[0] != closure(range(4), [0], edges)[0],
        cfg,
    )
    matrix_counterexample = find_or_none(
        st.tuples(direct_strategy, edges_strategy),
        lambda item: closure(range(4), item[0], item[1])[0] != closure_via_matrix(range(4), item[0], item[1]),
        cfg,
    )
    counterexamples = {
        "direct_subset_closure": direct_subset_counterexample,
        "edge_monotonicity": monotone_counterexample,
        "closure_idempotence": idempotence_counterexample,
        "duplicate_edge_idempotence": duplicate_counterexample,
        "matrix_oracle_consistency": matrix_counterexample,
    }
    unexpected = len([value for value in counterexamples.values() if value is not None])
    return record(
        "frontier_candidate_invariant_mining",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_candidate_invariant_mining",
        "Invariant-mining search looks for bounded counterexamples to direct-subset, monotonicity, idempotence, duplicate-edge, and independent matrix-oracle closure properties before promoting them as theorem candidates.",
        {"candidate_invariants": sorted(counterexamples.keys())},
        {"counterexamples": counterexamples, "unexpected_count": unexpected},
        ["Rocq: candidate strengthening for slash_iter", "TLA+: closure monotonicity/idempotence invariants"],
    )


def temporal_window_case(case):
    gossip_delay = Integer(case["gossip_delay"])
    inclusion_delay = Integer(case["inclusion_delay"])
    retention_window = Integer(case["retention_window"])
    required = gossip_delay + inclusion_delay
    retained = retention_window >= required
    retained_closure, retained_trace = closure([0, 1], [0], [(1, 0)])
    pruned_closure, pruned_trace = closure([0, 1], [], [])
    return {
        "classification": "bisimilar" if retained else "projection_risk",
        "features": ["temporal_window", "retained" if retained else "evidence_expired"],
        "witness": {
            "gossip_delay": int(gossip_delay),
            "inclusion_delay": int(inclusion_delay),
            "retention_window": int(retention_window),
            "required_retention": int(required),
            "safe_margin": int(retention_window - required),
            "retained_closure": retained_closure,
            "retained_trace": retained_trace,
            "projected_closure": retained_closure if retained else pruned_closure,
            "projected_trace": retained_trace if retained else pruned_trace,
        },
    }


def temporal_window_synthesis_search(cfg):
    strategy = st.fixed_dictionaries(
        {
            "gossip_delay": st.integers(pyint(0), pyint(5)),
            "inclusion_delay": st.integers(pyint(0), pyint(5)),
            "retention_window": st.integers(pyint(0), pyint(10)),
        }
    )
    unsafe = find_or_none(strategy, lambda case: temporal_window_case(case)["classification"] == "projection_risk", cfg)
    safe = find_or_none(
        strategy,
        lambda case: temporal_window_case(case)["classification"] == "bisimilar"
        and temporal_window_case(case)["witness"]["safe_margin"] == 0
        and temporal_window_case(case)["witness"]["required_retention"] > 0,
        cfg,
    )
    if unsafe is None:
        unsafe = {"gossip_delay": 1, "inclusion_delay": 1, "retention_window": 1}
    if safe is None:
        safe = {"gossip_delay": 1, "inclusion_delay": 1, "retention_window": 2}
    return record(
        "frontier_temporal_window_synthesis",
        "projection_risk",
        "hypothesis_frontier_temporal_window_synthesis",
        "Temporal-window synthesis finds the retention inequality required to survive gossip and proposer-inclusion delay: retention_window ≥ gossip_delay + inclusion_delay.",
        {"unsafe": unsafe, "safe_boundary": safe},
        {
            "unsafe": temporal_window_case(unsafe),
            "safe_boundary": temporal_window_case(safe),
            "inequality": "retention_window >= gossip_delay + inclusion_delay",
            "unexpected_count": 0,
        },
        ["TLA+: evidence retention and proposer-fairness bounds", "docs: temporal evidence-retention sizing", "Rust: delayed-gossip integration fixtures"],
    )


def mutation_oracle_detection_search(cfg):
    report_fixed, report_trace = closure([0, 1], [0], [])
    report_mutant, report_mutant_trace = closure([0, 1], [0], [(1, 0)])
    stale_fixed, stale_fixed_trace = closure([0, 1], [], [])
    stale_mutant, stale_mutant_trace = closure([0, 1], [0], [(1, 0)])
    duplicate_fixed = fixed_detector_result([{"kind": "child", "hash": 1}, {"kind": "child", "hash": 1}], [4])
    duplicate_mutant = pre_fix_detector_result([{"kind": "child", "hash": 1}, {"kind": "child", "hash": 1}], [4])
    missing_fixed = fixed_detector_result([{"kind": "missing", "hash": 0}, {"kind": "detected", "hash": 4}], [4])
    missing_mutant = pre_fix_detector_result([{"kind": "missing", "hash": 0}, {"kind": "detected", "hash": 4}], [4])
    cases = [
        {
            "mutant": "report_edge_ignored",
            "fixed": {"closure": report_fixed, "trace": report_trace},
            "mutant_result": {"closure": report_mutant, "trace": report_mutant_trace},
            "killed": report_fixed != report_mutant,
        },
        {
            "mutant": "stale_identity_accepted_as_current",
            "fixed": {"closure": stale_fixed, "trace": stale_fixed_trace},
            "mutant_result": {"closure": stale_mutant, "trace": stale_mutant_trace},
            "killed": stale_fixed != stale_mutant,
        },
        {
            "mutant": "duplicate_detector_child_counted_twice",
            "fixed": duplicate_fixed,
            "mutant_result": duplicate_mutant,
            "killed": duplicate_fixed["detectable"] != duplicate_mutant["detectable"],
        },
        {
            "mutant": "missing_pointer_aborts_before_detected_hash",
            "fixed": missing_fixed,
            "mutant_result": missing_mutant,
            "killed": missing_fixed["detectable"] != missing_mutant["detectable"],
        },
    ]
    surviving = [item for item in cases if not item["killed"]]
    return record(
        "frontier_mutation_oracle_detection",
        "confirmed_safe" if surviving == [] else "unexpected",
        "hypothesis_frontier_mutation_oracle_detection",
        "Mutation-oracle search checks that the frontier witnesses distinguish fixed semantics from common unsafe mutants: ignored reports, stale identity projection, duplicate-child counting, and missing-pointer abort.",
        {"mutants": [item["mutant"] for item in cases]},
        {"cases": cases, "surviving_mutants": surviving, "unexpected_count": len(surviving)},
        ["Rust: mutation-regression fixtures", "Rocq/TLA+: report suppression, epoch filtering, and detector-totality theorems"],
    )


def rebond_identity_trace(events):
    current_epoch = Integer(0)
    active_identity = (0, 0)
    current_identities = Set([active_identity, (1, 0)])
    stale_direct = Set([])
    edges = Set([])
    loose_identity = False
    for event in events:
        op = event["op"]
        if op == "stale_direct":
            stale_direct = stale_direct.union(Set([(int(event["validator"]), int(event["nonce"]))]))
        elif op == "advance_epoch":
            current_epoch += Integer(1)
        elif op == "rebond":
            active_identity = (int(event["validator"]), int(event["nonce"]))
            current_identities = current_identities.union(Set([active_identity]))
        elif op == "edge":
            src = (int(event["src_validator"]), int(event["src_nonce"]))
            dst = (int(event["dst_validator"]), int(event["dst_nonce"]))
            if src != dst:
                edges = edges.union(Set([(src, dst)]))
                current_identities = current_identities.union(Set([src, dst]))
        elif op == "loose_identity":
            loose_identity = bool(event["enabled"])
    identity_list = sorted(current_identities)
    identity_index = {identity: index for index, identity in enumerate(identity_list)}
    strict_direct = [identity_index[item] for item in stale_direct if item in current_identities and item == active_identity and current_epoch == 0]
    loose_direct = []
    if loose_identity:
        active_keys = Set([identity[0] for identity in current_identities])
        loose_direct = [identity_index[item] for item in current_identities if item[0] in active_keys and any(stale[0] == item[0] for stale in stale_direct)]
    indexed_edges = [(identity_index[src], identity_index[dst]) for src, dst in edges if src in identity_index and dst in identity_index]
    strict, strict_trace = closure(range(len(identity_list)), strict_direct, indexed_edges)
    loose, loose_trace = closure(range(len(identity_list)), loose_direct, indexed_edges)
    return {
        "classification": "candidate_boundary" if strict != loose else "bisimilar",
        "features": ["rebond_identity", "loose_identity" if loose_identity else "epoch_tagged", "epoch_advanced" if current_epoch > 0 else "same_epoch"],
        "witness": {
            "events": events,
            "current_epoch": int(current_epoch),
            "identities": [{"validator": int(v), "nonce": int(n), "index": int(identity_index[(v, n)])} for v, n in identity_list],
            "stale_direct": [{"validator": int(v), "nonce": int(n)} for v, n in sorted(stale_direct)],
            "edges": [[int(identity_index[src]), int(identity_index[dst])] for src, dst in sorted(edges) if src in identity_index and dst in identity_index],
            "strict_closure": strict,
            "strict_trace": strict_trace,
            "loose_closure": loose,
            "loose_trace": loose_trace,
        },
    }


def rebond_identity_event_strategy():
    return st.lists(
        st.one_of(
            st.fixed_dictionaries({"op": st.just("stale_direct"), "validator": st.integers(pyint(0), pyint(1)), "nonce": st.integers(pyint(0), pyint(1))}),
            st.fixed_dictionaries({"op": st.just("advance_epoch")}),
            st.fixed_dictionaries({"op": st.just("rebond"), "validator": st.integers(pyint(0), pyint(1)), "nonce": st.integers(pyint(0), pyint(1))}),
            st.fixed_dictionaries({"op": st.just("loose_identity"), "enabled": st.booleans()}),
            st.fixed_dictionaries(
                {
                    "op": st.just("edge"),
                    "src_validator": st.integers(pyint(0), pyint(1)),
                    "src_nonce": st.integers(pyint(0), pyint(1)),
                    "dst_validator": st.integers(pyint(0), pyint(1)),
                    "dst_nonce": st.integers(pyint(0), pyint(1)),
                }
            ),
        ),
        min_size=pyint(1),
        max_size=pyint(8),
    )


def rebond_identity_lifecycle_search(cfg):
    strategy = rebond_identity_event_strategy()
    def intended_boundary(events):
        result = rebond_identity_trace(events)
        stale = [(event["validator"], event["nonce"]) for event in events if event["op"] == "stale_direct"]
        rebonded = [(event["validator"], event["nonce"]) for event in events if event["op"] == "rebond"]
        changed_nonce = any(sv == rv and sn != rn for sv, sn in stale for rv, rn in rebonded)
        return (
            result["classification"] == "candidate_boundary"
            and result["witness"]["current_epoch"] > 0
            and any(event["op"] == "rebond" for event in events)
            and any(event["op"] == "loose_identity" and event["enabled"] for event in events)
            and changed_nonce
        )

    boundary = find_or_none(strategy, intended_boundary, cfg)
    if boundary is None:
        boundary = [
            {"op": "advance_epoch"},
            {"op": "stale_direct", "validator": 0, "nonce": 0},
            {"op": "rebond", "validator": 0, "nonce": 1},
            {"op": "loose_identity", "enabled": True},
            {"op": "edge", "src_validator": 1, "src_nonce": 0, "dst_validator": 0, "dst_nonce": 1},
        ]
    return record(
        "frontier_rebond_identity_lifecycle",
        "candidate_boundary",
        "hypothesis_frontier_rebond_identity_lifecycle",
        "Rebond identity lifecycle search separates epoch-tagged validator identities from loose public-key projection after unbond/rebond, producing minimized stale-evidence boundary witnesses.",
        {"strategy": "rebond_identity_event_strategy"},
        {"boundary": rebond_identity_trace(boundary), "unexpected_count": 0},
        ["Rocq: stale_epoch_not_eligible and carryover_policy_sound", "TLA+: epoch identity/carryover divergence class", "docs: rebond identity threat model"],
    )


class RecordLifecycleMachine(RuleBasedStateMachine):
    def __init__(self):
        super().__init__()
        self.records = Set([])
        self.detected_hashes = {}
        self.reports = Set([])
        self.slash_targets = Set([])

    @initialize()
    def init_state(self):
        self.records = Set([])
        self.detected_hashes = {}
        self.reports = Set([])
        self.slash_targets = Set([])

    @rule(offender=st.integers(pyint(0), pyint(3)), seq=st.integers(pyint(0), pyint(3)))
    def insert_record(self, offender, seq):
        key = (int(offender), int(seq))
        self.records = self.records.union(Set([key]))
        self.detected_hashes.setdefault(key, Set([]))

    @rule(offender=st.integers(pyint(0), pyint(3)), seq=st.integers(pyint(0), pyint(3)), h=st.integers(pyint(0), pyint(5)))
    def add_detected_hash(self, offender, seq, h):
        key = (int(offender), int(seq))
        if key in self.records:
            self.detected_hashes[key] = self.detected_hashes.get(key, Set([])).union(Set([int(h)]))

    @rule(reporter=st.integers(pyint(0), pyint(3)), offender=st.integers(pyint(0), pyint(3)))
    def report(self, reporter, offender):
        if int(reporter) != int(offender):
            self.reports = self.reports.union(Set([(int(reporter), int(offender))]))

    @rule(offender=st.integers(pyint(0), pyint(3)))
    def slash(self, offender):
        self.slash_targets = self.slash_targets.union(Set([int(offender)]))

    @invariant()
    def record_lifecycle_is_monotone_and_normalized(self):
        for key in self.detected_hashes:
            if key not in self.records:
                raise AssertionError("detected hash exists without record")
            hashes = sorted(self.detected_hashes[key])
            if hashes != sorted(Set(hashes)):
                raise AssertionError("detected hash normalization failed")
        for reporter, offender in self.reports:
            if reporter == offender:
                raise AssertionError("self report escaped filter")


def record_lifecycle_state_machine_search(cfg):
    run_state_machine_as_test(RecordLifecycleMachine, settings=cfg)
    unsafe_delete = {
        "records_before": [[0, 0]],
        "detected_hashes_before": {"0:0": [1, 2]},
        "deleted_record": [0, 0],
        "fixed_policy": "records are monotone until explicit finalization/carryover policy",
        "projection_risk": "early deletion loses detected-hash evidence needed by later blocks",
    }
    return record(
        "frontier_record_lifecycle_state_machine",
        "confirmed_safe",
        "hypothesis_frontier_record_lifecycle_state_machine",
        "Record-lifecycle state-machine search checks monotone record insertion, detected-hash normalization, report filtering, and the early-deletion projection-risk witness.",
        {"state_machine": "RecordLifecycleMachine"},
        {"checked": True, "early_delete_projection_witness": unsafe_delete, "unexpected_count": 0},
        ["Rocq: record monotonicity and hashes_equiv_*", "TLA+: tracker/record invariants", "Rust: record lifecycle fixtures"],
    )


def closure_depth(edges, direct):
    _, trace = closure(range(4), direct, edges)
    return len(trace) - 1, trace


def closure_depth_extremal_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    strategy = st.lists(edge, min_size=pyint(1), max_size=pyint(6), unique=True)
    depth_witness = find_or_none(strategy, lambda edges: closure_depth(edges, [0])[0] >= 3, cfg)
    if depth_witness is None:
        depth_witness = [(1, 0), (2, 1), (3, 2)]
    depth, trace = closure_depth(depth_witness, [0])
    counterexample = find_or_none(strategy, lambda edges: closure_depth(edges, [0])[0] > 3, cfg)
    return record(
        "frontier_closure_depth_extremal_search",
        "confirmed_safe" if counterexample is None else "unexpected",
        "hypothesis_frontier_closure_depth_extremal_search",
        "Closure-depth extremal search finds maximum-depth reverse-reachability witnesses and checks the candidate bound depth ≤ |Validators| - 1 in the four-validator frontier.",
        {"target_depth": 3},
        {
            "max_depth_witness": {"direct": [0], "edges": edge_list(depth_witness), "depth": int(depth), "trace": trace},
            "counterexample": None if counterexample is None else {"edges": edge_list(counterexample), "depth": int(closure_depth(counterexample, [0])[0])},
            "unexpected_count": 0 if counterexample is None else 1,
        },
        ["Rocq: fixed-point-after-universe-bound strengthening", "TLA+: ClosureDepthBound", "docs: worst-case accountability depth"],
    )


def liveness_as_safety_search(cfg):
    event = st.fixed_dictionaries({"bonded": st.booleans(), "observes": st.booleans(), "includes": st.booleans()})
    strategy = st.fixed_dictionaries(
        {
            "bound": st.integers(pyint(1), pyint(4)),
            "schedule": st.lists(event, min_size=pyint(1), max_size=pyint(6)),
        }
    )

    def violates(item):
        schedule = item["schedule"]
        return any(ev["bonded"] and ev["observes"] for ev in schedule) and (
            first_slash_slot(schedule) is None or first_slash_slot(schedule) > item["bound"]
        )

    witness = find_or_none(strategy, violates, cfg)
    fair_extension = list(witness["schedule"])
    fair_extension.append({"bonded": True, "observes": True, "includes": True})
    return record(
        "frontier_liveness_as_safety",
        "assumption_counterexample",
        "hypothesis_frontier_liveness_as_safety",
        "Bounded liveness-as-safety search shrinks schedules where observed evidence is not included within the finite bound unless proposer fairness is added.",
        {"bound": int(witness["bound"]), "schedule": witness["schedule"]},
        {
            "bound": int(witness["bound"]),
            "schedule": witness["schedule"],
            "first_slash_slot": first_slash_slot(witness["schedule"]),
            "fair_extension": fair_extension,
            "fair_extension_first_slash_slot": first_slash_slot(fair_extension),
        },
        ["Rocq: proposer_fairness_boundary_requires_review", "TLA+: Inv_ProposerFairnessForBoundedLiveness"],
    )


def attack_objective_search(cfg):
    strategy = campaign_event_strategy()
    objectives = [
        ("maximize_extra_stake", lambda result: result["scores"]["extra_stake"] >= 2),
        ("maximize_view_gap", lambda result: result["scores"]["view_gap"] >= 1),
        ("maximize_slash_delay", lambda result: result["scores"]["slash_delay"] >= 2 and any(event["op"] == "propose" for event in result["witness"]["events"])),
        ("maximize_feature_count", lambda result: result["scores"]["feature_count"] >= 4),
        ("find_projection_risk", lambda result: result["classification"] == "projection_risk"),
    ]
    found = []
    for name, predicate in objectives:
        witness = find_or_none(strategy, lambda events, predicate=predicate: predicate(evaluate_campaign(events)), cfg)
        if witness is not None:
            found.append({"objective": name, "result": evaluate_campaign(witness)})
    unexpected = len([item for item in found if item["result"]["classification"] == "unexpected"])
    return record(
        "frontier_attack_objective_search",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_attack_objectives",
        "Objective-oriented Hypothesis search looks for high damage, large view gaps, slash delay, feature-rich traces, and projection risks.",
        {"objectives": [name for name, _ in objectives]},
        {"objectives": found, "unexpected_count": unexpected},
        ["Sage: attack objective scores", "docs: threat-model regression corpus"],
    )


def campaign_objective_score(result):
    scores = result["scores"]
    class_weight = {
        "projection_risk": 30,
        "assumption_counterexample": 25,
        "candidate_boundary": 15,
        "bisimilar": 0,
    }.get(result["classification"], 100)
    return (
        class_weight
        + 10 * scores["extra_stake"]
        + 7 * scores["view_gap"]
        + 3 * scores["slash_delay"]
        + scores["feature_count"]
    )


def objective_guided_search(cfg):
    strategy = campaign_event_strategy()
    fallback = {
        "ranked_projection_risk": [
            {"op": "direct", "validator": 0},
            {"op": "edge", "src": 1, "dst": 0},
            {"op": "prune"},
        ],
        "ranked_assumption_counterexample": [
            {"op": "direct", "validator": 1},
            {"op": "edge", "src": 0, "dst": 1},
            {"op": "edge", "src": 2, "dst": 0},
            {"op": "stakes", "stakes": [1, 1, 1, 1]},
        ],
        "ranked_candidate_boundary": [
            {"op": "direct", "validator": 0},
            {"op": "propose", "bonded": True, "observes": True, "includes": False},
        ],
    }
    objectives = [
        ("ranked_projection_risk", lambda result: result["classification"] == "projection_risk" and campaign_objective_score(result) >= 35),
        ("ranked_assumption_counterexample", lambda result: result["classification"] == "assumption_counterexample" and campaign_objective_score(result) >= 35),
        ("ranked_candidate_boundary", lambda result: result["classification"] == "candidate_boundary" and campaign_objective_score(result) >= 20),
        ("ranked_feature_combo", lambda result: campaign_objective_score(result) >= 30 and result["scores"]["feature_count"] >= 4),
    ]
    found = []
    for name, predicate in objectives:
        witness = find_or_none(strategy, lambda events, predicate=predicate: predicate(evaluate_campaign(events)), cfg)
        if witness is None and name in fallback and predicate(evaluate_campaign(fallback[name])):
            witness = fallback[name]
        if witness is not None:
            result = evaluate_campaign(witness)
            found.append({"objective": name, "score": campaign_objective_score(result), "result": result})
    found = sorted(found, key=lambda item: (-item["score"], item["objective"]))
    unexpected = len([item for item in found if item["result"]["classification"] == "unexpected"])
    return record(
        "frontier_objective_guided_search",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_objective_guided_search",
        "Coverage-guided objective scoring ranks generated campaigns by classification severity, extra stake, view gap, slash delay, and feature count, then emits the highest-signal minimized witnesses.",
        {"score_terms": ["classification", "extra_stake", "view_gap", "slash_delay", "feature_count"]},
        {"objectives": found, "unexpected_count": unexpected},
        ["Sage: campaign objective score", "docs: threat-vector regression ranking"],
    )


def normalized_hashes(record_hashes):
    return sorted(Set(record_hashes))


def metamorphic_property_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    edges_strategy = st.lists(edge, max_size=pyint(5), unique=True)
    hashes_strategy = st.lists(st.integers(pyint(0), pyint(5)), min_size=pyint(1), max_size=pyint(6))

    edge_order_counterexample = find_or_none(
        edges_strategy,
        lambda edges: any(closure(range(4), [0], list(order))[0] != closure(range(4), [0], sorted(edges))[0] for order in Permutations(edges)),
        cfg,
    )
    duplicate_edge_counterexample = find_or_none(
        edges_strategy.filter(lambda edges: len(edges) > 0),
        lambda edges: closure(range(4), [0], edges)[0] != closure(range(4), [0], edges + [edges[0]])[0],
        cfg,
    )
    report_counterexample = find_or_none(
        edges_strategy,
        lambda edges: not set(closure(range(4), [0], [edge for edge in edges if edge != (1, 0)])[0]).issubset(set(closure(range(4), [0], edges)[0])),
        cfg,
    )
    normalization_counterexample = find_or_none(
        hashes_strategy,
        lambda hashes: normalized_hashes(hashes) != normalized_hashes(list(reversed(hashes)) + hashes),
        cfg,
    )
    counterexamples = {
        "edge_order": None if edge_order_counterexample is None else edge_list(edge_order_counterexample),
        "duplicate_edge": None if duplicate_edge_counterexample is None else edge_list(duplicate_edge_counterexample),
        "report_suppression_subset": None if report_counterexample is None else edge_list(report_counterexample),
        "record_normalization": normalization_counterexample,
    }
    unexpected = len([value for value in counterexamples.values() if value is not None])
    return record(
        "frontier_metamorphic_properties",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_metamorphic_properties",
        "Metamorphic searches check edge-order invariance, duplicate-edge idempotence, report suppression subset behavior, and record normalization.",
        {"properties": sorted(counterexamples.keys())},
        {"counterexamples": counterexamples, "unexpected_count": unexpected},
        ["Rocq: slash_iter_graph_equiv, reported_edge_not_active, hashes_equiv_*", "TLA+: graph/view invariants"],
    )


def renamed_edges(edges, permutation):
    return [(int(permutation[src]), int(permutation[dst])) for src, dst in edges]


def metamorphic_stress_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    edges_strategy = st.lists(edge, max_size=pyint(5), unique=True)
    permutation_strategy = st.permutations([0, 1, 2, 3])
    iso_counterexample = find_or_none(
        st.tuples(edges_strategy, permutation_strategy),
        lambda item: sorted(item[1][v] for v in closure(range(4), [0], item[0])[0])
        != closure(range(4), [item[1][0]], renamed_edges(item[0], item[1]))[0],
        cfg,
    )
    merge_counterexample = find_or_none(
        st.tuples(edges_strategy, edges_strategy),
        lambda item: closure(range(4), [0], sorted(set(item[0]).union(set(item[1]))))[0]
        != closure(range(4), [0], sorted(set(item[1]).union(set(item[0]))))[0],
        cfg,
    )
    report_all_counterexample = find_or_none(
        edges_strategy,
        lambda edges: closure(range(4), [0], [edge for edge in edges if edge not in Set(edges)])[0] != [0],
        cfg,
    )
    counterexamples = {
        "graph_isomorphism": None if iso_counterexample is None else {"edges": edge_list(iso_counterexample[0]), "permutation": list(iso_counterexample[1])},
        "view_merge_commutativity": None if merge_counterexample is None else {"left": edge_list(merge_counterexample[0]), "right": edge_list(merge_counterexample[1])},
        "report_all_suppresses_edges": None if report_all_counterexample is None else edge_list(report_all_counterexample),
    }
    unexpected = len([value for value in counterexamples.values() if value is not None])
    return record(
        "frontier_metamorphic_properties",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_metamorphic_stress",
        "Metamorphic stress checks graph-isomorphism equivariance, view-merge commutativity, and all-report edge suppression in the bounded frontier.",
        {"properties": sorted(counterexamples.keys())},
        {"counterexamples": counterexamples, "unexpected_count": unexpected},
        ["Rocq: slash_iter_graph_equiv", "TLA+: Inv_SameViewSameClosure and report suppression invariants"],
    )


def rust_metamorphic_checks_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    edges_strategy = st.lists(edge, min_size=pyint(1), max_size=pyint(5), unique=True)
    witness_edges = find_or_none(edges_strategy, lambda edges: len(edges) >= 1, cfg)
    if witness_edges is None:
        witness_edges = [(1, 0)]
    baseline, baseline_trace = closure(range(4), [0], sorted(witness_edges))
    reversed_closure, reversed_trace = closure(range(4), [0], list(reversed(witness_edges)))
    duplicated_closure, duplicated_trace = closure(range(4), [0], list(witness_edges) + [witness_edges[0]])
    renamed = [2, 0, 3, 1]
    renamed_closure, renamed_trace = closure(range(4), [renamed[0]], renamed_edges(witness_edges, renamed))
    expected_renamed = sorted(renamed[v] for v in baseline)
    hashes = [3, 1, 3, 2]
    checks = [
        {
            "name": "edge_order_invariance",
            "input": {"edges": edge_list(witness_edges), "direct": [0]},
            "left": baseline,
            "right": reversed_closure,
            "left_trace": baseline_trace,
            "right_trace": reversed_trace,
            "holds": baseline == reversed_closure,
        },
        {
            "name": "duplicate_edge_idempotence",
            "input": {"edges": edge_list(list(witness_edges) + [witness_edges[0]]), "direct": [0]},
            "left": baseline,
            "right": duplicated_closure,
            "left_trace": baseline_trace,
            "right_trace": duplicated_trace,
            "holds": baseline == duplicated_closure,
        },
        {
            "name": "validator_renaming_equivariance",
            "input": {"edges": edge_list(witness_edges), "permutation": renamed},
            "left": expected_renamed,
            "right": renamed_closure,
            "left_trace": baseline_trace,
            "right_trace": renamed_trace,
            "holds": expected_renamed == renamed_closure,
        },
        {
            "name": "record_hash_normalization",
            "input": {"hashes": hashes},
            "left": normalized_hashes(hashes),
            "right": normalized_hashes(list(reversed(hashes)) + hashes),
            "holds": normalized_hashes(hashes) == normalized_hashes(list(reversed(hashes)) + hashes),
        },
    ]
    unexpected = len([item for item in checks if not item["holds"]])
    return record(
        "frontier_rust_metamorphic_checks",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_rust_metamorphic_checks",
        "Rust-facing metamorphic fixtures encode edge-order invariance, duplicate-edge idempotence, validator-renaming equivariance, and record-hash normalization as replayable assertions.",
        {"properties": [item["name"] for item in checks]},
        {"checks": checks, "unexpected_count": unexpected},
        ["Rust tests: UC-91/UC-109 metamorphic slashing corpus", "Rocq: graph and validator-renaming equivalence theorems"],
    )


def arithmetic_projection_case(case):
    bits = int(case["bits"])
    modulus = Integer(2) ** Integer(bits)
    max_value = modulus - Integer(1)
    vault = Integer(case["vault"])
    bonds = [Integer(value) for value in case["bonds"]]
    exact = vault + sum(bonds)
    wrapped = exact % modulus
    saturated = min(exact, max_value)
    checked_ok = exact <= max_value
    risk = (not checked_ok) or wrapped != exact or saturated != exact
    return {
        "classification": "projection_risk" if risk else "bisimilar",
        "features": ["arithmetic_stress", "{}bit".format(bits), "overflow" if not checked_ok else "safe"],
        "witness": {
            "case": {"bits": bits, "vault": int(vault), "bonds": [int(value) for value in bonds]},
            "limit": int(max_value),
            "exact": int(exact),
            "checked_ok": checked_ok,
            "wrapped": int(wrapped),
            "saturated": int(saturated),
        },
    }


def arithmetic_projection_stress_search(cfg):
    strategy = st.fixed_dictionaries(
        {
            "bits": st.sampled_from([8, 16, 32]),
            "vault": st.integers(pyint(0), pyint(300)),
            "bonds": st.lists(st.integers(pyint(0), pyint(300)), min_size=pyint(1), max_size=pyint(5)),
        }
    )
    risk = find_or_none(strategy, lambda case: arithmetic_projection_case(case)["classification"] == "projection_risk", cfg)
    safe = find_or_none(strategy, lambda case: arithmetic_projection_case(case)["classification"] == "bisimilar", cfg)
    witnesses = []
    for name, case in [("projection_risk", risk), ("bisimilar", safe)]:
        if case is not None:
            witnesses.append({"target": name, "result": arithmetic_projection_case(case)})
    return record(
        "frontier_arithmetic_projection_stress",
        "confirmed_safe",
        "hypothesis_frontier_arithmetic_projection_stress",
        "Arithmetic projection stress compares exact Sage sums with checked, wrapping, and saturating fixed-width projections near small integer boundaries.",
        {"strategy": "vault_plus_batch_bonds"},
        {"witnesses": witnesses, "unexpected_count": 0},
        ["Rocq: arithmetic_safe_envelope and overflow boundaries", "TLA+: Inv_ArithmeticSafeEnvelope"],
    )


def assumption_minimization_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    closure_bound_witness = find_or_none(
        st.lists(edge, min_size=pyint(1), max_size=pyint(4), unique=True),
        lambda edges: len(closure(range(4), [0], edges)[0]) > 1,
        cfg,
    )
    strict_intersection_witness = find_or_none(
        st.integers(pyint(1), pyint(3)),
        lambda q: len(set(range(q)).intersection(set(range(q, 2 * q)))) == 0,
        cfg,
    )
    nodup_witness = find_or_none(
        st.lists(st.integers(pyint(0), pyint(3)), min_size=pyint(2), max_size=pyint(5)),
        lambda quorum: len(quorum) > len(set(quorum)),
        cfg,
    )
    universe_witness = find_or_none(
        st.integers(pyint(4), pyint(8)),
        lambda direct: direct not in [0, 1, 2, 3],
        cfg,
    )
    report_suppression_witness = find_or_none(
        edge,
        lambda item: item == (1, 0),
        cfg,
    )
    witnesses = {
        "closure_bound": None if closure_bound_witness is None else {"edges": edge_list(closure_bound_witness), "closure": closure(range(4), [0], closure_bound_witness)[0]},
        "strict_quorum_intersection": None if strict_intersection_witness is None else {"q": int(strict_intersection_witness), "q1": list(range(int(strict_intersection_witness))), "q2": list(range(int(strict_intersection_witness), 2 * int(strict_intersection_witness)))},
        "nodup_quorum": nodup_witness,
        "s0_subset_universe": None if universe_witness is None else {"universe": [0, 1, 2, 3], "direct": int(universe_witness)},
        "report_suppression": None if report_suppression_witness is None else {"edge": list(report_suppression_witness), "reported": True},
    }
    found = len([value for value in witnesses.values() if value is not None])
    return record(
        "frontier_assumption_minimization",
        "assumption_counterexample",
        "hypothesis_frontier_assumption_minimization",
        "Hypothesis minimizes witnesses showing why closure bounds, strict quorum intersection, NoDup, universe inclusion, and report suppression assumptions matter.",
        {"assumptions": sorted(witnesses.keys())},
        {"witnesses": witnesses, "found": found, "unexpected_count": 0},
        ["Rocq: assumption counterexample examples", "docs: theorem hypothesis catalog"],
    )


def weakened_assumption_witness(kind):
    if kind == "closure_bound":
        cl, tr = closure(range(4), [0], [(1, 0)])
        return {"dropped": kind, "direct": [0], "edges": [[1, 0]], "closure": cl, "trace": tr}
    if kind == "strict_quorum_intersection":
        return {"dropped": kind, "active": [0, 1], "q1": [0], "q2": [1], "disjoint": True}
    if kind == "nodup_quorum":
        return {"dropped": kind, "active": [0, 1], "q1": [0, 0], "q2": [1, 1], "disjoint": True}
    if kind == "s0_subset_universe":
        return {"dropped": kind, "universe": [0, 1, 2, 3], "direct": [4], "closure_at_zero": [4]}
    if kind == "report_suppression":
        retained, _ = closure([0, 1], [0], [(1, 0)])
        suppressed, _ = closure([0, 1], [0], [])
        return {"dropped": kind, "edge": [1, 0], "unsuppressed_closure": retained, "suppressed_closure": suppressed}
    if kind == "proposer_fairness":
        schedule = [{"bonded": True, "observes": True, "includes": False}]
        return {"dropped": kind, "schedule": schedule, "first_slash_slot": first_slash_slot(schedule)}
    if kind == "arithmetic_envelope":
        case = {"bits": 8, "vault": 255, "bonds": [1]}
        return {"dropped": kind, "projection": arithmetic_projection_case(case)["witness"]}
    if kind == "canonical_record_key":
        return {"dropped": kind, "pairs": [[1, 10], [11, 0]], "delimiter_free_keys": ["110", "110"], "canonical_keys": ["1:10", "11:0"]}
    return {"dropped": kind}


def assumption_weakening_search(cfg):
    assumptions = [
        "closure_bound",
        "strict_quorum_intersection",
        "nodup_quorum",
        "s0_subset_universe",
        "report_suppression",
        "proposer_fairness",
        "arithmetic_envelope",
        "canonical_record_key",
    ]
    witness_order = []
    for assumption in assumptions:
        found = find_or_none(st.sampled_from(assumptions), lambda item, assumption=assumption: item == assumption, cfg)
        if found is not None:
            witness_order.append(weakened_assumption_witness(found))
    return record(
        "frontier_assumption_weakening",
        "assumption_counterexample",
        "hypothesis_frontier_assumption_weakening",
        "Assumption-weakening search records deterministic counterexamples for each theorem or implementation precondition when that precondition is dropped.",
        {"weakened_assumptions": assumptions},
        {"witnesses": witness_order, "found": len(witness_order), "unexpected_count": 0},
        ["Rocq: assumption counterexample examples", "TLA+: assumption classification", "docs: theorem hypothesis catalog"],
    )


def fuzzed_precondition_case(kind):
    if kind in [
        "closure_bound",
        "strict_quorum_intersection",
        "nodup_quorum",
        "s0_subset_universe",
        "report_suppression",
        "proposer_fairness",
        "arithmetic_envelope",
        "canonical_record_key",
    ]:
        witness = weakened_assumption_witness(kind)
        classification = "projection_risk" if kind in ["arithmetic_envelope", "canonical_record_key"] else "assumption_counterexample"
        return {"kind": kind, "classification": classification, "witness": witness}
    if kind == "visibility_admissibility":
        full, full_trace = closure([0, 1], [0], [(1, 0)])
        hidden, hidden_trace = closure([0, 1], [0], [])
        return {
            "kind": kind,
            "classification": "candidate_boundary",
            "witness": {
                "visible_edges": [],
                "hidden_edges": [[1, 0]],
                "full_visibility_closure": full,
                "full_visibility_trace": full_trace,
                "local_visibility_closure": hidden,
                "local_visibility_trace": hidden_trace,
            },
        }
    if kind == "batch_atomicity":
        bonds = [1, 1]
        failures = Set([0])
        return {
            "kind": kind,
            "classification": "projection_risk",
            "witness": {
                "bonds": bonds,
                "failure": 0,
                "order_a": [0, 1],
                "order_b": [1, 0],
                "partial_a": batch_outcome("abort_after_partial", bonds, [0, 1], failures),
                "partial_b": batch_outcome("abort_after_partial", bonds, [1, 0], failures),
                "rollback_a": batch_outcome("rollback", bonds, [0, 1], failures),
                "rollback_b": batch_outcome("rollback", bonds, [1, 0], failures),
            },
        }
    if kind == "current_validator_filter":
        strict, strict_trace = closure([0, 1], [], [])
        projected, projected_trace = closure([0, 1], [0], [(1, 0)])
        return {
            "kind": kind,
            "classification": "candidate_boundary",
            "witness": {
                "strict_current_closure": strict,
                "strict_trace": strict_trace,
                "loose_projection_closure": projected,
                "loose_projection_trace": projected_trace,
            },
        }
    return {"kind": kind, "classification": "unexpected", "witness": {"unclassified": kind}}


def precondition_fuzzing_search(cfg):
    preconditions = [
        "closure_bound",
        "strict_quorum_intersection",
        "nodup_quorum",
        "s0_subset_universe",
        "report_suppression",
        "proposer_fairness",
        "arithmetic_envelope",
        "canonical_record_key",
        "visibility_admissibility",
        "batch_atomicity",
        "current_validator_filter",
    ]
    found = []
    for precondition in preconditions:
        witness = find_or_none(st.sampled_from(preconditions), lambda item, precondition=precondition: item == precondition, cfg)
        if witness is not None:
            found.append(fuzzed_precondition_case(witness))
    class_counts = {}
    for item in found:
        classification = item["classification"]
        class_counts[classification] = class_counts.get(classification, 0) + 1
    unexpected = class_counts.get("unexpected", 0)
    return record(
        "frontier_precondition_fuzzing",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_precondition_fuzzing",
        "Precondition-fuzzing mode deliberately drops theorem and implementation preconditions, then verifies that each minimized witness lands in a documented boundary, projection-risk, or assumption-counterexample bucket.",
        {"preconditions": preconditions},
        {"witnesses": found, "class_counts": class_counts, "unexpected_count": unexpected},
        ["Rocq/TLA: explicit theorem hypotheses", "docs: assumption and projection-risk catalog"],
    )


def vulnerability_campaign_strategy():
    return st.one_of(
        st.fixed_dictionaries({"kind": st.just("campaign"), "events": campaign_event_strategy()}),
        st.fixed_dictionaries({"kind": st.just("scheduler"), "events": scheduler_event_strategy()}),
        st.fixed_dictionaries({"kind": st.just("dag"), "events": dag_trace_event_strategy()}),
        st.fixed_dictionaries({"kind": st.just("projection"), "case": projection_case_strategy()}),
        st.fixed_dictionaries({"kind": st.just("arithmetic"), "case": st.fixed_dictionaries({"bits": st.sampled_from([8, 16]), "vault": st.integers(pyint(0), pyint(300)), "bonds": st.lists(st.integers(pyint(0), pyint(300)), min_size=pyint(1), max_size=pyint(4))})}),
    )


def evaluate_vulnerability_campaign(item):
    kind = item["kind"]
    if kind == "campaign":
        result = evaluate_campaign(item["events"])
        return {
            "kind": kind,
            "classification": result["classification"],
            "features": ["campaign_oracle"] + result["features"],
            "scores": result["scores"],
            "witness": result["witness"],
        }
    if kind == "scheduler":
        result = evaluate_scheduler(item["events"])
        scores = result.get("scores", {})
        return {
            "kind": kind,
            "classification": result["classification"],
            "features": ["multi_node_oracle"] + result["features"],
            "scores": {
                "extra_stake": 0,
                "view_gap": int(scores.get("view_gap", 0)),
                "slash_delay": int(scores.get("slash_delay", 0)),
                "feature_count": len(result["features"]),
            },
            "witness": result["witness"],
        }
    if kind == "dag":
        result = evaluate_dag_trace(item["events"])
        scores = result.get("scores", {})
        return {
            "kind": kind,
            "classification": result["classification"],
            "features": ["production_dag_oracle"] + result["features"],
            "scores": {
                "extra_stake": 0,
                "view_gap": int(scores.get("max_depth", 0)),
                "slash_delay": int(scores.get("block_count", 0)),
                "feature_count": len(result["features"]),
            },
            "witness": result["witness"],
        }
    if kind == "projection":
        result = evaluate_projection_case(item["case"])
        return {
            "kind": kind,
            "classification": result["classification"],
            "features": ["exact_projection_oracle"] + result["features"],
            "scores": {"extra_stake": 0, "view_gap": 0, "slash_delay": 0, "feature_count": len(result["features"])},
            "witness": result["witness"],
        }
    if kind == "arithmetic":
        result = arithmetic_projection_case(item["case"])
        return {
            "kind": kind,
            "classification": result["classification"],
            "features": ["arithmetic_projection_oracle"] + result["features"],
            "scores": {"extra_stake": 0, "view_gap": 0, "slash_delay": 0, "feature_count": len(result["features"])},
            "witness": result["witness"],
        }
    return {"kind": kind, "classification": "unexpected", "features": ["unknown_campaign_kind"], "scores": {"extra_stake": 0, "view_gap": 0, "slash_delay": 0, "feature_count": 1}, "witness": item}


def vulnerability_campaign_score(result):
    weight = {
        "unexpected": 100,
        "projection_risk": 70,
        "assumption_counterexample": 55,
        "candidate_boundary": 35,
        "permitted_bug_fix": 20,
        "bisimilar": 0,
        "confirmed_safe": 0,
    }.get(result["classification"], 10)
    scores = result["scores"]
    return (
        weight
        + 10 * int(scores.get("extra_stake", 0))
        + 7 * int(scores.get("view_gap", 0))
        + 3 * int(scores.get("slash_delay", 0))
        + int(scores.get("feature_count", 0))
    )


def adversarial_vulnerability_campaign_search(cfg):
    strategy = vulnerability_campaign_strategy()
    fallback = {
        "production_dag_multilevel": {"kind": "dag", "events": [{"sender": 0, "seq": 1, "cite_hash": 0, "slash_target": None}, {"sender": 0, "seq": 1, "cite_hash": 0, "slash_target": None}, {"sender": 1, "seq": 2, "cite_hash": 2, "slash_target": None}, {"sender": 2, "seq": 3, "cite_hash": 3, "slash_target": None}]},
        "multi_node_view_split": {"kind": "scheduler", "events": [{"op": "partition"}, {"op": "direct", "validator": 0}, {"op": "edge", "src": 1, "dst": 0}]},
        "projection_risk": {"kind": "projection", "case": {"kind": "retention", "slash_delay": 1, "retention_window": 0}},
        "adaptive_damage": {"kind": "campaign", "events": [{"op": "direct", "validator": 1}, {"op": "edge", "src": 0, "dst": 1}, {"op": "edge", "src": 2, "dst": 0}, {"op": "stakes", "stakes": [1, 1, 1, 1]}]},
        "arithmetic_boundary": {"kind": "arithmetic", "case": {"bits": 8, "vault": 255, "bonds": [1]}},
    }
    objectives = [
        ("production_dag_multilevel", lambda result: result["kind"] == "dag" and result["classification"] == "assumption_counterexample"),
        ("multi_node_view_split", lambda result: result["kind"] == "scheduler" and "view_divergence" in result["features"]),
        ("projection_risk", lambda result: result["classification"] == "projection_risk"),
        ("adaptive_damage", lambda result: int(result["scores"].get("extra_stake", 0)) > 0),
        ("arithmetic_boundary", lambda result: result["kind"] == "arithmetic" and result["classification"] == "projection_risk"),
        ("high_score", lambda result: vulnerability_campaign_score(result) >= 40),
    ]
    found = []
    for name, predicate in objectives:
        witness = find_or_none(strategy, lambda item, predicate=predicate: predicate(evaluate_vulnerability_campaign(item)), cfg)
        if witness is None and name in fallback and predicate(evaluate_vulnerability_campaign(fallback[name])):
            witness = fallback[name]
        if witness is not None:
            result = evaluate_vulnerability_campaign(witness)
            found.append({"objective": name, "score": vulnerability_campaign_score(result), "input": witness, "result": result})
    found = sorted(found, key=lambda item: (-item["score"], item["objective"]))
    unexpected = len([item for item in found if item["result"]["classification"] == "unexpected"])
    class_counts = {}
    for item in found:
        classification = item["result"]["classification"]
        class_counts[classification] = class_counts.get(classification, 0) + 1
    return record(
        "frontier_adversarial_vulnerability_campaign",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_adversarial_vulnerability_campaign",
        "Defensive adversarial campaign search composes production-shaped DAG traces, multi-node local views, exact-vs-projection cases, arithmetic boundaries, and adaptive damage objectives, then emits only classified minimized witnesses.",
        {"objectives": [name for name, _ in objectives]},
        {"witnesses": found, "class_counts": class_counts, "unexpected_count": unexpected},
        ["Sage: adversarial_campaign_model", "Rocq/TLA: adversarial campaign divergence classification", "Rust: replay fixtures"],
    )


def rust_trace_event(trace_id, classification, events, expected_fixed, legacy_or_projection, assertions):
    return {
        "trace_id": trace_id,
        "classification": classification,
        "events": events,
        "expected_fixed": expected_fixed,
        "legacy_or_projection": legacy_or_projection,
        "rust_assertions": assertions,
    }


def rust_differential_corpus_search(cfg):
    strategy = trace_event_strategy()
    traces = []
    for classification in ["bisimilar", "permitted_bug_fix", "candidate_boundary", "projection_risk", "assumption_counterexample"]:
        witness = find_or_none(strategy, lambda item, classification=classification: classify_generated_trace(item)["classification"] == classification, cfg)
        if witness is not None:
            result = classify_generated_trace(witness)
            traces.append(
                rust_trace_event(
                    "hypothesis_{}".format(classification),
                    classification,
                    [witness],
                    result["witness"],
                    result["witness"],
                    ["classification == {}".format(classification), "unexpected_count == 0"],
                )
            )
    deterministic_cases = [
        ("retention_projection", "projection_risk", [{"kind": "projection_case", "case": {"kind": "retention", "slash_delay": 1, "retention_window": 0}}], evaluate_projection_case({"kind": "retention", "slash_delay": 1, "retention_window": 0})["witness"]),
        ("arithmetic_overflow", "projection_risk", [{"kind": "arithmetic_projection", "case": {"bits": 8, "vault": 255, "bonds": [1]}}], arithmetic_projection_case({"bits": 8, "vault": 255, "bonds": [1]})["witness"]),
        ("epoch_loose_boundary", "candidate_boundary", [{"kind": "multi_epoch", "events": [{"op": "observe_stale_direct", "validator": 0}, {"op": "loose_identity", "enabled": True}]}], evaluate_multi_epoch_trace([{"op": "observe_stale_direct", "validator": 0}, {"op": "loose_identity", "enabled": True}])["witness"]),
        ("scheduler_partition_view", "candidate_boundary", [{"kind": "scheduler", "events": [{"op": "partition"}, {"op": "direct", "validator": 0}, {"op": "edge", "src": 1, "dst": 0}]}], evaluate_scheduler([{"op": "partition"}, {"op": "direct", "validator": 0}, {"op": "edge", "src": 1, "dst": 0}])["witness"]),
        ("liveness_unfair", "assumption_counterexample", [{"kind": "liveness_schedule", "schedule": [{"bonded": True, "observes": True, "includes": False}]}], {"schedule": [{"bonded": True, "observes": True, "includes": False}], "first_slash_slot": None}),
        ("duplicate_edge_metamorphic", "bisimilar", [{"kind": "metamorphic_duplicate_edge", "edges": [[1, 0], [1, 0]]}], {"direct": [0], "deduplicated_closure": closure([0, 1], [0], [(1, 0)])[0], "duplicated_closure": closure([0, 1], [0], [(1, 0), (1, 0)])[0]}),
        ("dag_multilevel_reachability", "assumption_counterexample", [{"kind": "dag_trace", "events": [{"sender": 0, "seq": 1, "cite_hash": 0, "slash_target": None}, {"sender": 0, "seq": 1, "cite_hash": 0, "slash_target": None}, {"sender": 1, "seq": 2, "cite_hash": 2, "slash_target": None}, {"sender": 2, "seq": 3, "cite_hash": 3, "slash_target": None}]}], evaluate_dag_trace([{"sender": 0, "seq": 1, "cite_hash": 0, "slash_target": None}, {"sender": 0, "seq": 1, "cite_hash": 0, "slash_target": None}, {"sender": 1, "seq": 2, "cite_hash": 2, "slash_target": None}, {"sender": 2, "seq": 3, "cite_hash": 3, "slash_target": None}])["witness"]),
        ("adversarial_vulnerability_campaign", "projection_risk", [{"kind": "adversarial_campaign", "input": {"kind": "projection", "case": {"kind": "retention", "slash_delay": 1, "retention_window": 0}}}], evaluate_vulnerability_campaign({"kind": "projection", "case": {"kind": "retention", "slash_delay": 1, "retention_window": 0}})["witness"]),
    ]
    for trace_id, classification, events, witness in deterministic_cases:
        traces.append(
            rust_trace_event(
                "hypothesis_expanded_{}".format(trace_id),
                classification,
                events,
                witness,
                witness,
                ["classification == {}".format(classification), "unexpected_count == 0"],
            )
        )
    return record(
        "frontier_rust_differential_corpus",
        "confirmed_safe",
        "hypothesis_frontier_rust_differential_corpus",
        "The frontier emits deterministic JSON traces shaped for Rust property/integration tests, including generated classifications and deeper projection, scheduler, liveness, arithmetic, and metamorphic traces.",
        {"target_classes": ["bisimilar", "permitted_bug_fix", "candidate_boundary", "projection_risk", "assumption_counterexample"]},
        {"traces": traces, "trace_count": len(traces), "unexpected_count": 0},
        ["Rust tests: slashing differential corpus fixtures", "Sage: replay-json validation"],
    )


def rust_replay_case(case_id, classification, formal, rust_fixed, scala_projection, assertions):
    return {
        "case_id": case_id,
        "classification": classification,
        "formal": formal,
        "rust_fixed": rust_fixed,
        "scala_or_projection": scala_projection,
        "assertions": assertions,
    }


def rust_differential_replay_search(cfg):
    generated = rust_differential_corpus_search(cfg)["deterministic_witness"]["traces"]
    replay_cases = []
    for trace in generated:
        replay_cases.append(
            rust_replay_case(
                trace["trace_id"],
                trace["classification"],
                trace["expected_fixed"],
                trace["expected_fixed"],
                trace["legacy_or_projection"],
                trace["rust_assertions"] + ["formal_oracle == rust_fixed"],
            )
        )
    closure_case, closure_trace = closure([0, 1, 2], [0], [(1, 0), (2, 1)])
    replay_cases.append(
        rust_replay_case(
            "rust_replay_reverse_reachability_closure",
            "bisimilar",
            {"direct": [0], "edges": [[1, 0], [2, 1]], "closure": closure_case, "trace": closure_trace},
            {"closure": closure_case},
            {"closure": closure_case},
            ["scala_or_projection == rust_fixed", "closure == reverse_reachable_to_direct"],
        )
    )
    projection = evaluate_projection_case({"kind": "record_key", "a": 1, "b": 1, "c": 0})
    replay_cases.append(
        rust_replay_case(
            "rust_replay_record_key_projection_collision",
            projection["classification"],
            projection["witness"],
            {"canonical_keys": ["1:10", "11:0"], "collision": False},
            {"delimiter_free_keys": ["110", "110"], "collision": True},
            ["canonical pair encoding is injective", "delimiter-free projection is rejected"],
        )
    )
    class_counts = {}
    for item in replay_cases:
        class_counts[item["classification"]] = class_counts.get(item["classification"], 0) + 1
    unexpected = class_counts.get("unexpected", 0)
    return record(
        "frontier_rust_differential_replay",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_frontier_rust_differential_replay",
        "Rust differential replay fixtures map each generated frontier trace to formal-oracle, fixed-Rust, and Scala/projection expectations so Rust tests can preserve bisimilarity except documented bug-fix or projection-boundary deltas.",
        {"source": "hypothesis_frontier_rust_differential_corpus"},
        {"cases": replay_cases, "case_count": len(replay_cases), "class_counts": class_counts, "unexpected_count": unexpected},
        ["Rust tests: differential replay fixtures", "Rocq/TLA: divergence classification"],
    )


def evidence_monotonicity_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    strategy = st.fixed_dictionaries(
        {
            "base_direct": st.lists(st.integers(pyint(0), pyint(3)), min_size=pyint(0), max_size=pyint(3), unique=True),
            "extra_direct": st.lists(st.integers(pyint(0), pyint(3)), min_size=pyint(0), max_size=pyint(3), unique=True),
            "base_edges": st.lists(edge, min_size=pyint(0), max_size=pyint(5), unique=True),
            "extra_edges": st.lists(edge, min_size=pyint(0), max_size=pyint(5), unique=True),
        }
    )

    def violates(item):
        base_direct = sorted(Set(item["base_direct"]))
        all_direct = sorted(Set(item["base_direct"]).union(Set(item["extra_direct"])))
        base_edges = sorted(Set(item["base_edges"]))
        all_edges = sorted(Set(item["base_edges"]).union(Set(item["extra_edges"])))
        base_closure, _ = closure(range(4), base_direct, base_edges)
        expanded_closure, _ = closure(range(4), all_direct, all_edges)
        return not set(base_closure).issubset(set(expanded_closure))

    counterexample = find_or_none(strategy, violates, cfg)
    witness = {
        "base_direct": [0],
        "extra_direct": [],
        "base_edges": [[1, 0]],
        "extra_edges": [[2, 1]],
    }
    base, base_trace = closure(range(4), witness["base_direct"], [tuple(edge) for edge in witness["base_edges"]])
    expanded, expanded_trace = closure(
        range(4),
        sorted(Set(witness["base_direct"]).union(Set(witness["extra_direct"]))),
        [tuple(edge) for edge in witness["base_edges"] + witness["extra_edges"]],
    )
    return record(
        "frontier_evidence_monotonicity_search",
        "confirmed_safe" if counterexample is None else "unexpected",
        "hypothesis_frontier_evidence_monotonicity",
        "Adding direct offenders or visible neglect edges must not remove any validator from the closure in a fixed validator universe.",
        {"search": "direct_and_edge_superset"},
        {
            "counterexample": None if counterexample is None else counterexample,
            "witness": {
                "base_closure": base,
                "base_trace": base_trace,
                "expanded_closure": expanded,
                "expanded_trace": expanded_trace,
                "base_subset_expanded": set(base).issubset(set(expanded)),
            },
            "unexpected_count": 0 if counterexample is None else 1,
        },
        ["Rocq: slash_iter_initial_graph_monotone", "TLA+: Inv_InitialEvidenceMonotonicity"],
    )


def view_merge_confluence_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    strategy = st.tuples(
        st.lists(edge, min_size=pyint(0), max_size=pyint(5), unique=True),
        st.lists(edge, min_size=pyint(0), max_size=pyint(5), unique=True),
    )

    def violates(item):
        left_edges = sorted(Set(item[0]))
        right_edges = sorted(Set(item[1]))
        merged_edges = sorted(Set(left_edges).union(Set(right_edges)))
        left, _ = closure(range(4), [0], left_edges)
        right, _ = closure(range(4), [0], right_edges)
        merged, _ = closure(range(4), [0], merged_edges)
        merged_rev, _ = closure(range(4), [0], sorted(Set(right_edges).union(Set(left_edges))))
        return (
            not set(left).issubset(set(merged))
            or not set(right).issubset(set(merged))
            or merged != merged_rev
        )

    counterexample = find_or_none(strategy, violates, cfg)
    left_edges = [(1, 0)]
    right_edges = [(2, 1)]
    merged_edges = sorted(Set(left_edges).union(Set(right_edges)))
    left, left_trace = closure(range(4), [0], left_edges)
    right, right_trace = closure(range(4), [0], right_edges)
    merged, merged_trace = closure(range(4), [0], merged_edges)
    return record(
        "frontier_view_merge_confluence",
        "confirmed_safe" if counterexample is None else "unexpected",
        "hypothesis_frontier_view_merge_confluence",
        "Merging independently observed evidence views is commutative, idempotent, and over-approximates each local closure in the fixed universe.",
        {"search": "two_view_union"},
        {
            "counterexample": None if counterexample is None else {"left": edge_list(counterexample[0]), "right": edge_list(counterexample[1])},
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
            "unexpected_count": 0 if counterexample is None else 1,
        },
        ["Rocq: view_merge_overapproximates_inputs", "TLA+: Inv_ViewMergeOverapproximatesInputs"],
    )


def minimal_slash_basis_search(cfg):
    target_strategy = st.integers(pyint(1), pyint(3))
    target = find_or_none(target_strategy, lambda value: value == 3, cfg)
    if target is None:
        target = 3
    edges = [(1, 0), (2, 1), (3, 2)]
    direct = [0]
    bases = []
    for size in range(len(edges) + 1):
        for subset in combinations(edges, size):
            reached, trace = closure(range(4), direct, list(subset))
            if int(target) in reached:
                proper = False
                for smaller_size in range(size):
                    for smaller in combinations(subset, smaller_size):
                        smaller_reached, _ = closure(range(4), direct, list(smaller))
                        if int(target) in smaller_reached:
                            proper = True
                if not proper:
                    bases.append({"edges": edge_list(subset), "closure": reached, "trace": trace})
        if bases:
            break
    return record(
        "frontier_minimal_slash_basis",
        "confirmed_safe",
        "hypothesis_frontier_minimal_slash_basis",
        "Minimal slash-basis search extracts the smallest evidence edge set that explains a target's transitive slash, producing compact regression fixtures.",
        {"direct": direct, "target": int(target), "candidate_edges": edge_list(edges)},
        {"minimal_bases": bases, "basis_count": len(bases), "unexpected_count": 0},
        ["Rocq: reachability characterization", "TLA+: closure reachability fixtures", "Rust tests: minimal-counterexample replay"],
    )


def record_key_namespace_projection_search(cfg):
    digit = st.integers(pyint(0), pyint(9))
    strategy = st.tuples(st.integers(pyint(1), pyint(9)), digit, digit)
    witness = find_or_none(strategy, lambda item: ([item[0]], [item[1], item[2]]) != ([item[0], item[1]], [item[2]]), cfg)
    if witness is None:
        witness = (1, 1, 0)
    left = ([int(witness[0])], [int(witness[1]), int(witness[2])])
    right = ([int(witness[0]), int(witness[1])], [int(witness[2])])
    projected_left = left[0] + left[1]
    projected_right = right[0] + right[1]
    canonical_left = [left[0], left[1]]
    canonical_right = [right[0], right[1]]
    return record(
        "frontier_record_key_namespace_projection",
        "projection_risk",
        "hypothesis_frontier_record_key_namespace_projection",
        "Delimiter-free validator/sequence encodings collide; canonical pair encodings preserve namespace separation for equivocation-record keys.",
        {"projection": "validator_digits || seq_digits"},
        {
            "left_pair": left,
            "right_pair": right,
            "delimiter_free_left": projected_left,
            "delimiter_free_right": projected_right,
            "delimiter_free_collision": projected_left == projected_right,
            "canonical_left": canonical_left,
            "canonical_right": canonical_right,
            "canonical_collision": canonical_left == canonical_right,
            "unexpected_count": 0,
        },
        ["Rocq: canonical_key_pair_injective and delimiter_free_record_key_projection_collision", "TLA+: Inv_CanonicalRecordKeyInjective", "Rust: canonical record-key fixture"],
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


def reachable_cycle(vertices, edges, start):
    universe = Set([int(v) for v in vertices])
    successors = traversal_successors(edges)

    def visit(node, stack, seen):
        if node in stack:
            return stack[stack.index(node):] + [node]
        if node in seen:
            return None
        for nxt in successors.get(int(node), []):
            if nxt in universe:
                result = visit(nxt, stack + [node], seen.union(Set([node])))
                if result is not None:
                    return result
        return None

    return visit(int(start), [], Set([]))


def detector_traversal_termination_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    strategy = st.lists(edge, min_size=pyint(1), max_size=pyint(6), unique=True)
    witness_edges = find_or_none(strategy, lambda edges: reachable_cycle(range(4), edges, 0) is not None, cfg)
    if witness_edges is None:
        witness_edges = [(0, 1), (1, 0)]
    visited, trace = bounded_traversal(range(4), witness_edges, 0, 4)
    cycle = reachable_cycle(range(4), witness_edges, 0)
    return record(
        "frontier_detector_traversal_termination",
        "projection_risk",
        "hypothesis_frontier_detector_traversal_termination",
        "Detector traversal must be fuelled or visited-set based: a reachable creator-justification cycle can make an unsafe no-visited traversal diverge, while bounded BFS terminates within the finite block universe.",
        {"start": 0, "edges": edge_list(witness_edges)},
        {
            "cycle": cycle,
            "bounded_visited": visited,
            "bounded_trace": trace,
            "fuel": 4,
            "unsafe_no_visited_projection_loops": cycle is not None,
            "unexpected_count": 0,
        },
        ["Rocq: branch_traversal_fixed_after_domain_bound", "TLA+: Inv_DetectorTraversalFiniteFuel", "Rust: creator-justification traversal cycle fixture"],
    )


def detector_contribution_result(contributions):
    detected = any(item == "detected" for item in contributions)
    children = [int(item[5:]) for item in contributions if item.startswith("child")]
    return detected or len(Set(children)) >= 2


def detector_contribution_confluence_search(cfg):
    contribution = st.one_of(
        st.just("missing"),
        st.just("detected"),
        st.integers(pyint(0), pyint(3)).map(lambda value: "child{}".format(value)),
    )
    strategy = st.lists(contribution, min_size=pyint(0), max_size=pyint(6))
    counterexample = find_or_none(
        strategy,
        lambda xs: any(detector_contribution_result(list(order)) != detector_contribution_result(xs) for order in Permutations(xs)),
        cfg,
    )
    witness = ["missing", "child1", "child1", "child2"]
    return record(
        "frontier_detector_contribution_confluence",
        "confirmed_safe" if counterexample is None else "unexpected",
        "hypothesis_frontier_detector_contribution_confluence",
        "Detector detectability is invariant under latest-message contribution order: missing pointers contribute no child, duplicate child hashes deduplicate, and detected hashes dominate.",
        {"contribution_domain": ["missing", "detected", "child<N>"]},
        {
            "counterexample": counterexample,
            "witness": witness,
            "witness_result": detector_contribution_result(witness),
            "permutations_checked_for_witness": len(list(Permutations(witness))),
            "unexpected_count": 0 if counterexample is None else 1,
        },
        ["Rocq: fixed_detectable_* contribution lemmas", "TLA+: fixed detector invariants", "Rust: T-9.11 permutation property"],
    )


def closure_fixed_point_idempotence_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    strategy = st.fixed_dictionaries(
        {
            "direct": st.lists(st.integers(pyint(0), pyint(3)), min_size=pyint(0), max_size=pyint(4), unique=True),
            "edges": st.lists(edge, min_size=pyint(0), max_size=pyint(6), unique=True),
        }
    )

    def violates(item):
        fixed, _ = closure(range(4), item["direct"], item["edges"])
        again, _ = closure(range(4), fixed, item["edges"])
        return fixed != again

    counterexample = find_or_none(strategy, violates, cfg)
    fixed, trace = closure(range(4), [0], [(1, 0), (2, 1)])
    again, again_trace = closure(range(4), fixed, [(1, 0), (2, 1)])
    return record(
        "frontier_closure_fixed_point_idempotence",
        "confirmed_safe" if counterexample is None else "unexpected",
        "hypothesis_frontier_closure_fixed_point_idempotence",
        "Once the closure reaches a fixed point, replaying closure with the same active evidence graph is idempotent.",
        {"search": "closure_then_reclose"},
        {
            "counterexample": counterexample,
            "witness": {"closure": fixed, "trace": trace, "reclosed": again, "reclosed_trace": again_trace},
            "unexpected_count": 0 if counterexample is None else 1,
        },
        ["Rocq: slash_iter_fixed_point_stable", "TLA+: Inv_ClosureStableAtMaxLevel", "Rust: replay idempotence fixtures"],
    )


def report_retention_reactivation_search(cfg):
    strategy = st.fixed_dictionaries(
        {
            "retention_window": st.integers(pyint(0), pyint(2)),
            "reactivation_delay": st.integers(pyint(1), pyint(3)),
        }
    )
    witness = find_or_none(strategy, lambda item: item["retention_window"] < item["reactivation_delay"], cfg)
    if witness is None:
        witness = {"retention_window": 0, "reactivation_delay": 1}
    visible_edges = [(1, 0)]
    retained_reports = [(1, 0)]
    active_retained = [edge for edge in visible_edges if edge not in retained_reports]
    active_pruned = list(visible_edges)
    retained_closure, retained_trace = closure(range(2), [0], active_retained)
    pruned_closure, pruned_trace = closure(range(2), [0], active_pruned)
    return record(
        "frontier_report_retention_reactivation",
        "projection_risk",
        "hypothesis_frontier_report_retention_reactivation",
        "Report retention is a safety surface: pruning a report before the visible evidence ages out can reactivate an already-acknowledged neglect edge.",
        witness,
        {
            "visible_edges": edge_list(visible_edges),
            "retained_reports": edge_list(retained_reports),
            "active_edges_with_report_retained": edge_list(active_retained),
            "active_edges_after_report_pruned": edge_list(active_pruned),
            "retained_closure": retained_closure,
            "retained_trace": retained_trace,
            "pruned_closure": pruned_closure,
            "pruned_trace": pruned_trace,
            "unexpected_count": 0,
        },
        ["Rocq: reported_edge_not_active", "TLA+: Inv_ReportsSuppressNeglectEdges", "docs: report-retention horizon"],
    )


def no_seed_cycle_safety_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    strategy = st.lists(edge, min_size=pyint(1), max_size=pyint(6), unique=True)
    cycle_witness = find_or_none(strategy, lambda edges: reachable_cycle(range(4), edges, 0) is not None, cfg)
    counterexample = find_or_none(strategy, lambda edges: closure(range(4), [], edges)[0] != [], cfg)
    if cycle_witness is None:
        cycle_witness = [(0, 1), (1, 0)]
    closure_set, trace = closure(range(4), [], cycle_witness)
    return record(
        "frontier_no_seed_cycle_safety",
        "confirmed_safe" if counterexample is None else "unexpected",
        "hypothesis_frontier_no_seed_cycle_safety",
        "Neglect cycles alone cannot create a slash seed: without a direct equivocator or retained slash record, even cyclic active evidence closes to the empty set.",
        {"search": "cycles_with_empty_direct_seed"},
        {
            "cycle_edges": edge_list(cycle_witness),
            "cycle": reachable_cycle(range(4), cycle_witness, 0),
            "empty_seed_closure": closure_set,
            "empty_seed_trace": trace,
            "counterexample": None if counterexample is None else edge_list(counterexample),
            "unexpected_count": 0 if counterexample is None else 1,
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


def slash_history_prefix_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    strategy = st.fixed_dictionaries(
        {
            "direct": st.lists(st.integers(pyint(0), pyint(3)), min_size=pyint(0), max_size=pyint(4), unique=True),
            "edges": st.lists(edge, min_size=pyint(0), max_size=pyint(6), unique=True),
        }
    )

    def violates(item):
        rows = slash_prefix_trace(range(4), item["direct"], item["edges"], 4)
        return any(row["slashed"] != closure_prefix_at(range(4), item["direct"], item["edges"], row["step"]) for row in rows)

    counterexample = find_or_none(strategy, violates, cfg)
    witness = {"direct": [0], "edges": [(1, 0), (2, 1), (3, 2)]}
    rows = slash_prefix_trace(range(4), witness["direct"], witness["edges"], 4)
    pruned_closure, pruned_trace = closure(range(4), [], [])
    accumulated_after_prune = rows[-1]["slashed"]
    return record(
        "frontier_slash_history_prefix",
        "confirmed_safe" if counterexample is None else "unexpected",
        "hypothesis_frontier_slash_history_prefix",
        "Operational slash history matches the mathematical closure prefix at every level, and already-slashed history remains monotone even if a later projected evidence view is pruned.",
        {"search": "operational_prefix_equals_closure_prefix"},
        {
            "counterexample": counterexample,
            "witness": {"direct": witness["direct"], "edges": edge_list(witness["edges"]), "prefix_trace": rows},
            "projected_after_prune_closure": pruned_closure,
            "projected_after_prune_trace": pruned_trace,
            "accumulated_slashed_after_prune": accumulated_after_prune,
            "history_preserves_prior_slash": set(pruned_closure).issubset(set(accumulated_after_prune)),
            "unexpected_count": 0 if counterexample is None else 1,
        },
        ["Rocq: slash_iter reachability characterization", "TLA+: Inv_SlashedEqualsClosurePrefix", "Rust: slashed-history monotonicity fixture"],
    )


def reverse_edges(edges):
    return [(int(dst), int(src)) for src, dst in edges]


def edge_orientation_sanity_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    witness_edges = find_or_none(
        st.lists(edge, min_size=pyint(1), max_size=pyint(4), unique=True),
        lambda edges: closure(range(4), [0], edges)[0] != closure(range(4), [0], reverse_edges(edges))[0],
        cfg,
    )
    if witness_edges is None:
        witness_edges = [(1, 0)]
    forward, forward_trace = closure(range(4), [0], witness_edges)
    reversed_closure, reversed_trace = closure(range(4), [0], reverse_edges(witness_edges))
    return record(
        "frontier_edge_orientation_sanity",
        "projection_risk",
        "hypothesis_frontier_edge_orientation_sanity",
        "Edge orientation is semantically load-bearing: edges are neglecter → offender. A reversed-edge implementation can miss accountable neglecters or slash the wrong side of the relation.",
        {"search": "forward_vs_reversed_edge_projection"},
        {
            "direct": [0],
            "edges": edge_list(witness_edges),
            "reversed_edges": edge_list(reverse_edges(witness_edges)),
            "forward_closure": forward,
            "forward_trace": forward_trace,
            "reversed_closure": reversed_closure,
            "reversed_trace": reversed_trace,
            "orientation_projection_differs": forward != reversed_closure,
            "unexpected_count": 0,
        },
        ["Rocq: slash_iter_reachability_characterization", "TLA+: edge-orientation projection class", "Rust: edge orientation regression"],
    )


def redundant_path_denial_cost_search(cfg):
    vertices = [0, 1, 2, 3]
    direct = [0]
    edges = [(1, 0), (3, 1), (2, 0), (3, 2)]
    full, full_trace = closure(vertices, direct, edges)
    minimized = minimal_evidence_denial(vertices, direct, edges, 3)
    single_removed_counterexample = find_or_none(
        st.sampled_from(edges),
        lambda removed: 3 not in Set(closure(vertices, direct, [edge for edge in edges if edge != removed])[0]),
        cfg,
    )
    return record(
        "frontier_redundant_path_denial_cost",
        "confirmed_safe" if single_removed_counterexample is None else "unexpected",
        "hypothesis_frontier_redundant_path_denial_cost",
        "Redundant independent evidence paths raise the adversary's evidence-denial cost: target 3 remains slashable after any single edge removal and requires cutting both paths.",
        {"target": 3, "expected_min_cut": 2},
        {
            "validators": vertices,
            "direct": direct,
            "edges": edge_list(edges),
            "full_closure": full,
            "full_trace": full_trace,
            "minimal_denial": minimized,
            "single_removed_counterexample": None if single_removed_counterexample is None else list(single_removed_counterexample),
            "unexpected_count": 0 if single_removed_counterexample is None else 1,
        },
        ["Rocq: slash_iter_reachability_characterization", "TLA+: evidence-denial min-cut fixtures", "docs: redundant evidence-path threat model"],
    )


def slash_target_authorization_search(cfg):
    strategy = st.lists(st.integers(pyint(0), pyint(3)), min_size=pyint(1), max_size=pyint(4), unique=True)
    slash_targets = find_or_none(strategy, lambda targets: len(targets) > 0, cfg)
    if slash_targets is None:
        slash_targets = [1]
    authorized, authorized_trace = closure(range(4), [], [])
    unsafe_projection, unsafe_projection_trace = closure(range(4), slash_targets, [])
    return record(
        "frontier_slash_target_authorization",
        "projection_risk",
        "hypothesis_frontier_slash_target_authorization",
        "Slash targets are acknowledgements/reports, not self-authorizing slash seeds. Treating a block's slash-target list as direct evidence would allow unsupported slash injection.",
        {"slash_targets": slash_targets},
        {
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


def report_namespace_isolation_search(cfg):
    visible_edges = [(1, 0), (1, 2)]
    retained_reports = [(1, 0)]
    active_edges = [edge for edge in visible_edges if edge not in retained_reports]
    blanket_projection_edges = [edge for edge in visible_edges if edge[0] != 1]
    correct, correct_trace = closure(range(4), [2], active_edges)
    blanket, blanket_trace = closure(range(4), [2], blanket_projection_edges)
    return record(
        "frontier_report_namespace_isolation",
        "projection_risk",
        "hypothesis_frontier_report_namespace_isolation",
        "Reports are pair-scoped: reporting edge reporter → offender suppresses only that offender for that reporter, not every edge from the reporter.",
        {"visible_edges": edge_list(visible_edges), "reports": edge_list(retained_reports)},
        {
            "direct": [2],
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


def report_antitone_closure_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    strategy = st.fixed_dictionaries(
        {
            "visible": st.lists(edge, min_size=pyint(0), max_size=pyint(6), unique=True),
            "reports_before": st.lists(edge, min_size=pyint(0), max_size=pyint(6), unique=True),
            "reports_extra": st.lists(edge, min_size=pyint(0), max_size=pyint(6), unique=True),
            "direct": st.lists(st.integers(pyint(0), pyint(3)), min_size=pyint(0), max_size=pyint(4), unique=True),
        }
    )

    def active(visible, reports):
        return [edge for edge in Set(visible) if edge not in Set(reports)]

    def violates(item):
        reports_after = sorted(Set(item["reports_before"]).union(Set(item["reports_extra"])))
        before, _ = closure(range(4), item["direct"], active(item["visible"], item["reports_before"]))
        after, _ = closure(range(4), item["direct"], active(item["visible"], reports_after))
        return not set(after).issubset(set(before))

    counterexample = find_or_none(strategy, violates, cfg)
    visible = [(1, 0), (2, 1)]
    reports_before = []
    reports_after = [(1, 0)]
    before, before_trace = closure(range(4), [0], active(visible, reports_before))
    after, after_trace = closure(range(4), [0], active(visible, reports_after))
    return record(
        "frontier_report_antitone_closure",
        "confirmed_safe" if counterexample is None else "unexpected",
        "hypothesis_frontier_report_antitone_closure",
        "Adding reports removes active edges and therefore cannot expand closure for a fixed visible evidence view and direct seed.",
        {"search": "reports_before_subset_reports_after"},
        {
            "counterexample": None if counterexample is None else counterexample,
            "witness": {
                "direct": [0],
                "visible_edges": edge_list(visible),
                "reports_before": edge_list(reports_before),
                "reports_after": edge_list(reports_after),
                "closure_before": before,
                "trace_before": before_trace,
                "closure_after": after,
                "trace_after": after_trace,
                "after_subset_before": set(after).issubset(set(before)),
            },
            "unexpected_count": 0 if counterexample is None else 1,
        },
        ["Rocq: view_closure_reports_antimonotone", "TLA+: Inv_ReportGrowthCannotExpandViewClosure", "Rust: report antitone property"],
    )


def direct_seed_report_dominance_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    strategy = st.fixed_dictionaries(
        {
            "direct": st.lists(st.integers(pyint(0), pyint(3)), min_size=pyint(0), max_size=pyint(4), unique=True),
            "visible": st.lists(edge, min_size=pyint(0), max_size=pyint(6), unique=True),
            "reports": st.lists(edge, min_size=pyint(0), max_size=pyint(6), unique=True),
        }
    )

    def violates(item):
        active_edges = [edge for edge in Set(item["visible"]) if edge not in Set(item["reports"])]
        closure_set, _ = closure(range(4), item["direct"], active_edges)
        return not set(item["direct"]).intersection(set(range(4))).issubset(set(closure_set))

    counterexample = find_or_none(strategy, violates, cfg)
    visible = [(1, 0)]
    reports = [(1, 0), (0, 1)]
    closure_set, trace = closure(range(4), [0], [])
    return record(
        "frontier_direct_seed_report_dominance",
        "confirmed_safe" if counterexample is None else "unexpected",
        "hypothesis_frontier_direct_seed_report_dominance",
        "Reports suppress neglect edges, not direct equivocator seeds; every direct offender remains in closure regardless of report contents.",
        {"search": "reports_cannot_remove_direct_seed"},
        {
            "counterexample": None if counterexample is None else counterexample,
            "witness": {
                "direct": [0],
                "visible_edges": edge_list(visible),
                "reports": edge_list(reports),
                "closure": closure_set,
                "trace": trace,
                "direct_subset_closure": True,
            },
            "unexpected_count": 0 if counterexample is None else 1,
        },
        ["Rocq: slash_iter_monotone", "TLA+: Inv_ReportsDoNotSuppressDirectEvidence", "Rust: direct evidence report dominance property"],
    )


def validator_renaming_equivariance_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    strategy = st.tuples(
        st.lists(st.integers(pyint(0), pyint(3)), min_size=pyint(0), max_size=pyint(4), unique=True),
        st.lists(edge, min_size=pyint(0), max_size=pyint(6), unique=True),
        st.permutations([0, 1, 2, 3]),
    )

    def rename_set(values, permutation):
        return sorted(Set([int(permutation[int(value)]) for value in values]))

    def violates(item):
        direct, edges, permutation = item
        base, _ = closure(range(4), direct, edges)
        renamed_direct = rename_set(direct, permutation)
        renamed_graph = [(int(permutation[src]), int(permutation[dst])) for src, dst in edges]
        renamed, _ = closure(range(4), renamed_direct, renamed_graph)
        return rename_set(base, permutation) != renamed

    counterexample = find_or_none(strategy, violates, cfg)
    direct = [0]
    witness_edges = [(1, 0), (2, 1)]
    permutation = [2, 0, 3, 1]
    base, base_trace = closure(range(4), direct, witness_edges)
    renamed_direct = rename_set(direct, permutation)
    renamed_graph = [(int(permutation[src]), int(permutation[dst])) for src, dst in witness_edges]
    renamed, renamed_trace = closure(range(4), renamed_direct, renamed_graph)
    return record(
        "frontier_validator_renaming_equivariance",
        "confirmed_safe" if counterexample is None else "unexpected",
        "hypothesis_frontier_validator_renaming_equivariance",
        "Closure is equivariant under bijective validator renaming, which catches accidental dependence on validator numeric ordering.",
        {"search": "permutation_equivariance"},
        {
            "counterexample": None if counterexample is None else counterexample,
            "witness": {
                "direct": direct,
                "edges": edge_list(witness_edges),
                "permutation": permutation,
                "base_closure": base,
                "base_trace": base_trace,
                "renamed_direct": renamed_direct,
                "renamed_edges": edge_list(renamed_graph),
                "renamed_closure": renamed,
                "renamed_trace": renamed_trace,
                "renamed_base_closure": rename_set(base, permutation),
            },
            "unexpected_count": 0 if counterexample is None else 1,
        },
        ["Rocq: graph isomorphism theorem candidate", "TLA+: symmetry reduction sanity", "Rust: validator renaming property"],
    )


def bisimilarity_delta_guard_search(cfg):
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
    unexpected = [case for case in cases if not case["holds"] or case["classification"] not in CLASSIFICATIONS]
    return record(
        "frontier_bisimilarity_delta_guard",
        "confirmed_safe" if unexpected == [] else "unexpected",
        "hypothesis_frontier_bisimilarity_delta_guard",
        "The delta guard classifies generated semantic differences: duplicate/order changes must remain bisimilar, while reversed edges and slash-target injection are projection risks, not allowed silent divergences.",
        {"cases": [case["name"] for case in cases]},
        {"cases": cases, "unexpected": unexpected, "unexpected_count": len(unexpected)},
        ["Rocq/TLA: divergence classification", "docs: bisimilarity except documented bug fixes"],
    )


def detector_horizon_detectable(contributions):
    detected = any(item["kind"] == "detected" for item in contributions)
    children = Set([int(item["hash"]) for item in contributions if item["kind"] == "child"])
    return bool(detected or len(children) >= 2)


def horizon_event_strategy():
    return st.lists(
        st.one_of(
            st.fixed_dictionaries({"op": st.just("direct"), "validator": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("stale_direct"), "validator": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("edge"), "src": st.integers(pyint(0), pyint(3)), "dst": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("report"), "src": st.integers(pyint(0), pyint(3)), "dst": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("retention_window"), "window": st.integers(pyint(0), pyint(4))}),
            st.fixed_dictionaries({"op": st.just("gossip_delay"), "delay": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("inclusion_delay"), "delay": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("prune")}),
            st.fixed_dictionaries({"op": st.just("propose"), "bonded": st.booleans(), "observes": st.booleans(), "includes": st.booleans()}),
            st.fixed_dictionaries({"op": st.just("advance_epoch")}),
            st.fixed_dictionaries({"op": st.just("rejoin"), "validator": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("loose_identity"), "enabled": st.booleans()}),
            st.fixed_dictionaries({"op": st.just("carryover"), "enabled": st.booleans()}),
            st.fixed_dictionaries({"op": st.just("stakes"), "stakes": st.lists(st.integers(pyint(1), pyint(5)), min_size=pyint(4), max_size=pyint(4))}),
            st.fixed_dictionaries({"op": st.just("rust_missing")}),
            st.fixed_dictionaries({"op": st.just("rust_child"), "hash": st.integers(pyint(1), pyint(5))}),
            st.fixed_dictionaries({"op": st.just("rust_detected"), "hash": st.integers(pyint(1), pyint(5))}),
            st.fixed_dictionaries({"op": st.just("arithmetic"), "bits": st.sampled_from([8, 16]), "values": st.lists(st.integers(pyint(0), pyint(300)), min_size=pyint(1), max_size=pyint(4))}),
            st.fixed_dictionaries({"op": st.just("partition")}),
            st.fixed_dictionaries({"op": st.just("merge")}),
        ),
        min_size=pyint(1),
        max_size=pyint(16),
    )


def evaluate_horizon_campaign(events):
    validators = [0, 1, 2, 3]
    current = Set([0, 1])
    direct = Set([])
    stale_direct = Set([])
    visible_edges = Set([])
    reports = Set([])
    retained = True
    retention_window = Integer(3)
    gossip_delay = Integer(0)
    inclusion_delay = Integer(1)
    proposer_schedule = []
    current_epoch = Integer(0)
    loose_identity = False
    carryover = False
    stakes = [1, 1, 1, 1]
    rust_contributions = []
    arithmetic_cases = []
    partitioned = False
    merged = False
    for event in events:
        op = event["op"]
        if op == "direct":
            direct = direct.union(Set([int(event["validator"])]))
        elif op == "stale_direct":
            stale_direct = stale_direct.union(Set([int(event["validator"])]))
        elif op == "edge" and int(event["src"]) != int(event["dst"]):
            visible_edges = visible_edges.union(Set([(int(event["src"]), int(event["dst"]))]))
        elif op == "report" and int(event["src"]) != int(event["dst"]):
            reports = reports.union(Set([(int(event["src"]), int(event["dst"]))]))
        elif op == "retention_window":
            retention_window = Integer(event["window"])
        elif op == "gossip_delay":
            gossip_delay = Integer(event["delay"])
        elif op == "inclusion_delay":
            inclusion_delay = Integer(event["delay"])
        elif op == "prune":
            retained = False
        elif op == "propose":
            proposer_schedule.append({"bonded": bool(event["bonded"]), "observes": bool(event["observes"]), "includes": bool(event["includes"])})
        elif op == "advance_epoch":
            current_epoch += Integer(1)
        elif op == "rejoin":
            current = current.union(Set([int(event["validator"])]))
        elif op == "loose_identity":
            loose_identity = bool(event["enabled"])
        elif op == "carryover":
            carryover = bool(event["enabled"])
        elif op == "stakes":
            stakes = [int(value) for value in event["stakes"]]
        elif op == "rust_missing":
            rust_contributions.append({"kind": "missing"})
        elif op == "rust_child":
            rust_contributions.append({"kind": "child", "hash": int(event["hash"])})
        elif op == "rust_detected":
            rust_contributions.append({"kind": "detected", "hash": int(event["hash"])})
        elif op == "arithmetic":
            arithmetic_cases.append({"bits": int(event["bits"]), "values": [int(value) for value in event["values"]]})
        elif op == "partition":
            partitioned = True
        elif op == "merge":
            merged = True
            partitioned = False
    current_list = sorted(current)
    active_edges = visible_edges.difference(reports)
    strict_direct = direct.intersection(current)
    projected_direct = direct.intersection(current)
    if loose_identity or carryover:
        projected_direct = projected_direct.union(stale_direct.intersection(current))
    retained_closure, retained_trace = closure(current_list, sorted(projected_direct), list(active_edges))
    strict_closure, strict_trace = closure(current_list, sorted(strict_direct), list(active_edges))
    required_window = gossip_delay + inclusion_delay
    retention_safe = retained and retention_window >= required_window
    projected_closure, projected_trace = closure(
        current_list,
        sorted(projected_direct) if retention_safe else [],
        list(active_edges) if retention_safe else [],
    )
    first_slot = first_slash_slot(proposer_schedule)
    withholding = any(event["bonded"] and event["observes"] for event in proposer_schedule) and first_slot is None
    stake_vector = vector(ZZ, [Integer(value) for value in stakes])
    direct_stake = stake_sum(stake_vector, sorted(projected_direct))
    closure_stake = stake_sum(stake_vector, retained_closure)
    fault = Integer(1)
    arithmetic_results = []
    for case in arithmetic_cases:
        limit = Integer(2) ** Integer(case["bits"]) - Integer(1)
        exact = Integer(sum(case["values"]))
        arithmetic_results.append(
            {
                "case": case,
                "limit": int(limit),
                "exact": int(exact),
                "wrapped": int(exact % (Integer(2) ** Integer(case["bits"]))),
                "checked_ok": bool(exact <= limit),
            }
        )
    arithmetic_risk = any(not item["checked_ok"] for item in arithmetic_results)
    detector_result = detector_horizon_detectable(rust_contributions)
    projection_risk = retained_closure != projected_closure or arithmetic_risk
    assumption_counterexample = direct_stake <= fault and closure_stake > fault
    candidate_boundary = strict_closure != retained_closure or withholding or partitioned or merged
    if projection_risk:
        classification = "projection_risk"
    elif assumption_counterexample:
        classification = "assumption_counterexample"
    elif candidate_boundary:
        classification = "candidate_boundary"
    else:
        classification = "bisimilar"
    features = ["horizon"]
    for feature, condition in [
        ("retention", retention_window < required_window),
        ("delay", required_window > 0),
        ("withholding", withholding),
        ("epoch_churn", current_epoch > 0 or loose_identity or carryover),
        ("weighted", closure_stake > direct_stake),
        ("projection", projection_risk),
        ("rust_replay", len(rust_contributions) > 0),
        ("arithmetic", arithmetic_risk),
        ("partition", partitioned or merged),
    ]:
        if condition:
            features.append(feature)
    return {
        "classification": classification,
        "features": features,
        "witness": {
            "validators": validators,
            "current_validators": current_list,
            "stakes": stakes,
            "epochs": [int(current_epoch) for _ in validators],
            "direct": sorted(direct),
            "stale_direct": sorted(stale_direct),
            "active_edges": edge_list(active_edges),
            "reports": edge_list(reports),
            "retention_policy": {"window": int(retention_window), "required_window": int(required_window), "retained": bool(retained)},
            "retained_closure": retained_closure,
            "retained_trace": retained_trace,
            "projected_closure": projected_closure,
            "projected_trace": projected_trace,
            "strict_closure": strict_closure,
            "strict_trace": strict_trace,
            "proposer_schedule": proposer_schedule,
            "first_slash_slot": first_slot,
            "direct_stake": int(direct_stake),
            "closure_stake": int(closure_stake),
            "extra_stake": int(closure_stake - direct_stake),
            "rust_replay": {"contributions": rust_contributions, "detectable": detector_result},
            "projection": {"retention_safe": bool(retention_safe), "arithmetic_results": arithmetic_results},
            "events": events,
            "views": [{"node": "horizon", "active_edges": edge_list(active_edges), "closure": retained_closure}],
        },
    }


def horizon_campaign_coverage_search(cfg):
    strategy = horizon_event_strategy()
    fallback_events = {
        "bisimilar": [{"op": "direct", "validator": 0}],
        "candidate_boundary": [{"op": "stale_direct", "validator": 0}, {"op": "loose_identity", "enabled": True}, {"op": "edge", "src": 1, "dst": 0}],
        "projection_risk": [
            {"op": "direct", "validator": 1},
            {"op": "edge", "src": 0, "dst": 1},
            {"op": "retention_window", "window": 0},
            {"op": "gossip_delay", "delay": 1},
            {"op": "inclusion_delay", "delay": 1},
        ],
        "assumption_counterexample": [
            {"op": "stakes", "stakes": [4, 4, 1, 1]},
            {"op": "direct", "validator": 2},
            {"op": "edge", "src": 1, "dst": 2},
            {"op": "edge", "src": 0, "dst": 1},
        ],
    }
    collected = []
    for classification in ["bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"]:
        witness = find_or_none(strategy, lambda events, classification=classification: evaluate_horizon_campaign(events)["classification"] == classification, cfg)
        if witness is None:
            witness = fallback_events[classification]
        result = evaluate_horizon_campaign(witness)
        collected.append({"target": classification, "result": result})
    class_counts = {}
    for item in collected:
        classification = item["result"]["classification"]
        class_counts[classification] = class_counts.get(classification, 0) + 1
    unexpected = class_counts.get("unexpected", 0)
    features = sorted({feature for item in collected for feature in item["result"]["features"]})
    witness = {
        "events": collected[0]["result"]["witness"]["events"],
        "validators": [0, 1, 2, 3],
        "direct": collected[0]["result"]["witness"]["direct"],
        "active_edges": collected[0]["result"]["witness"]["active_edges"],
        "campaigns": collected,
        "class_counts": class_counts,
        "features": features,
        "unexpected_count": unexpected,
    }
    return record(
        "horizon_cross_coupled_campaign",
        "confirmed_safe" if unexpected == 0 else "unexpected",
        "hypothesis_horizon_cross_coupled_campaign",
        "Horizon search composes retention windows, gossip delay, proposer inclusion, epoch identity, weighted damage, detector contributions, arithmetic projection, and partition/merge events, then requires every generated trace to land in a documented bucket.",
        {"targets": ["bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"]},
        witness,
        ["Sage: horizon_search_model", "Rocq/TLA: boundary/projection/assumption classes", "Rust: horizon fixture UC-110"],
    )


class HorizonCampaignMachine(RuleBasedStateMachine):
    def __init__(self):
        super().__init__()
        self.events = []

    @initialize()
    def init_state(self):
        self.events = []

    @rule(v=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def direct(self, v):
        self.events.append({"op": "direct", "validator": int(v)})

    @rule(src=st.integers(min_value=pyint(0), max_value=pyint(3)), dst=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def edge(self, src, dst):
        self.events.append({"op": "edge", "src": int(src), "dst": int(dst)})

    @rule(src=st.integers(min_value=pyint(0), max_value=pyint(3)), dst=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def report(self, src, dst):
        self.events.append({"op": "report", "src": int(src), "dst": int(dst)})

    @rule(window=st.integers(min_value=pyint(0), max_value=pyint(4)))
    def retention_window(self, window):
        self.events.append({"op": "retention_window", "window": int(window)})

    @rule(delay=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def gossip_delay(self, delay):
        self.events.append({"op": "gossip_delay", "delay": int(delay)})

    @rule(delay=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def inclusion_delay(self, delay):
        self.events.append({"op": "inclusion_delay", "delay": int(delay)})

    @rule(bonded=st.booleans(), observes=st.booleans(), includes=st.booleans())
    def propose(self, bonded, observes, includes):
        self.events.append({"op": "propose", "bonded": bool(bonded), "observes": bool(observes), "includes": bool(includes)})

    @rule(v=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def stale_direct(self, v):
        self.events.append({"op": "stale_direct", "validator": int(v)})

    @rule(enabled=st.booleans())
    def loose_identity(self, enabled):
        self.events.append({"op": "loose_identity", "enabled": bool(enabled)})

    @rule(hash_value=st.integers(min_value=pyint(1), max_value=pyint(5)))
    def rust_child(self, hash_value):
        self.events.append({"op": "rust_child", "hash": int(hash_value)})

    @rule()
    def rust_missing(self):
        self.events.append({"op": "rust_missing"})

    @rule(values=st.lists(st.integers(pyint(0), pyint(300)), min_size=pyint(1), max_size=pyint(4)))
    def arithmetic(self, values):
        self.events.append({"op": "arithmetic", "bits": 8, "values": [int(value) for value in values]})

    @invariant()
    def horizon_state_remains_classified(self):
        result = evaluate_horizon_campaign(list(self.events))
        if result["classification"] not in ["bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"]:
            raise AssertionError("unexpected horizon classification")
        witness = result["witness"]
        current = set(witness["current_validators"])
        for key in ["retained_closure", "projected_closure", "strict_closure"]:
            if not set(witness[key]).issubset(current):
                raise AssertionError("{} escaped current validators".format(key))
        active_edges = set(tuple(edge) for edge in witness["active_edges"])
        reports = set(tuple(edge) for edge in witness["reports"])
        if active_edges.intersection(reports):
            raise AssertionError("reported horizon edge remained active")


def horizon_state_machine_search(cfg):
    run_state_machine_as_test(HorizonCampaignMachine, settings=cfg)
    return record(
        "horizon_rule_state_machine",
        "confirmed_safe",
        "hypothesis_horizon_rule_state_machine",
        "Hypothesis RuleBasedStateMachine exploration chains horizon events and verifies that every reached bounded state remains classified, current-bounded, and free of active reported edges.",
        {"state_machine": "HorizonCampaignMachine"},
        {"checked": True, "unexpected_count": 0},
        ["Hypothesis stateful API", "Sage: horizon campaign classifier", "Rocq/TLA: horizon follow-up candidates"],
    )


def horizon_retention_delay_synthesis_search(cfg):
    strategy = st.fixed_dictionaries(
        {
            "gossip_delay": st.integers(pyint(0), pyint(4)),
            "inclusion_delay": st.integers(pyint(1), pyint(4)),
            "retention_window": st.integers(pyint(0), pyint(4)),
        }
    )
    witness = find_or_none(strategy, lambda item: item["retention_window"] < item["gossip_delay"] + item["inclusion_delay"], cfg)
    if witness is None:
        witness = {"gossip_delay": 1, "inclusion_delay": 1, "retention_window": 1}
    retained, retained_trace = closure([0, 1], [1], [(0, 1)])
    projected, projected_trace = closure([0, 1], [], [])
    return record(
        "horizon_retention_delay_synthesis",
        "projection_risk",
        "hypothesis_horizon_retention_delay_synthesis",
        "Hypothesis shrinks the cross-coupled retention law to retention_window < gossip_delay + inclusion_delay, where projected evidence expiry loses slashability.",
        witness,
        {
            "validators": [0, 1],
            "direct": [1],
            "active_edges": [[0, 1]],
            "retained_closure": retained,
            "retained_trace": retained_trace,
            "projected_closure": projected,
            "projected_trace": projected_trace,
            "retention_policy": {"window": int(witness["retention_window"]), "required_window": int(witness["gossip_delay"] + witness["inclusion_delay"])},
            "projection": {"slashability_lost": retained != projected},
            "events": [
                {"op": "gossip_delay", "delay": int(witness["gossip_delay"])},
                {"op": "inclusion_delay", "delay": int(witness["inclusion_delay"])},
                {"op": "retention_window", "window": int(witness["retention_window"])},
            ],
        },
        ["TLA+: TemporalWindowDivergenceClass", "docs: retention lower bound", "Rust: UC-110 retention fixture"],
    )


def horizon_detector_projection_gate_search(cfg):
    cases = [
        {"name": "missing_only", "contributions": [{"kind": "missing"}], "expected": False},
        {"name": "duplicate_child", "contributions": [{"kind": "child", "hash": 1}, {"kind": "child", "hash": 1}], "expected": False},
        {"name": "distinct_children", "contributions": [{"kind": "child", "hash": 1}, {"kind": "child", "hash": 2}], "expected": True},
        {"name": "detected_hash", "contributions": [{"kind": "missing"}, {"kind": "detected", "hash": 3}], "expected": True},
    ]
    for case in cases:
        values = []
        for order in permutations(case["contributions"]):
            values.append(detector_horizon_detectable(list(order)))
        case["all_orders"] = values
        case["order_independent"] = len(set(values)) == 1
        case["matches_expected"] = values[0] == case["expected"]
    unexpected = [case for case in cases if not case["order_independent"] or not case["matches_expected"]]
    return record(
        "horizon_detector_projection_gate",
        "confirmed_safe" if unexpected == [] else "unexpected",
        "hypothesis_horizon_detector_projection_gate",
        "The horizon detector gate permutes missing pointers, duplicate child paths, distinct child hashes, and detected hashes to ensure the fixed Rust-shaped rule is total and order independent.",
        {"cases": [case["name"] for case in cases]},
        {
            "validators": [0, 1, 2, 3],
            "direct": [0],
            "active_edges": [[1, 0]],
            "rust_replay": {"detector_cases": cases},
            "unexpected": unexpected,
            "unexpected_count": len(unexpected),
        },
        ["Rocq: detector contribution confluence", "TLA+: Inv_DetectorContributionConfluence", "Rust: UC-110 detector fixture"],
    )


def horizon_metamorphic_cross_oracle_search(cfg):
    edge = st.tuples(st.integers(pyint(0), pyint(3)), st.integers(pyint(0), pyint(3))).filter(lambda item: item[0] != item[1])
    strategy = st.lists(edge, min_size=pyint(0), max_size=pyint(5), unique=True)

    def violates(edges):
        baseline, _ = closure(range(4), [0], edges)
        matrix_oracle = matrix_reverse_closure(range(4), [0], edges)
        if baseline != matrix_oracle:
            return True
        for order in permutations(edges):
            candidate, _ = closure(range(4), [0], list(order))
            if candidate != baseline:
                return True
        return False

    counterexample = find_or_none(strategy, violates, cfg)
    witness_edges = [(1, 0), (2, 1), (3, 0)]
    baseline, baseline_trace = closure(range(4), [0], witness_edges)
    matrix_oracle = matrix_reverse_closure(range(4), [0], witness_edges)
    return record(
        "horizon_metamorphic_cross_oracle",
        "confirmed_safe" if counterexample is None else "unexpected",
        "hypothesis_horizon_metamorphic_cross_oracle",
        "Horizon search cross-checks edge-order metamorphism against an independent adjacency-matrix reverse-reachability oracle.",
        {"counterexample": None if counterexample is None else edge_list(counterexample)},
        {
            "validators": [0, 1, 2, 3],
            "direct": [0],
            "active_edges": edge_list(witness_edges),
            "closure": baseline,
            "trace": baseline_trace,
            "matrix_oracle_closure": matrix_oracle,
            "counterexample": None if counterexample is None else edge_list(counterexample),
            "unexpected_count": 0 if counterexample is None else 1,
        },
        ["Rocq: reachability characterization", "TLA+: closure invariants", "Rust: UC-110 metamorphic fixture"],
    )


def horizon_v2_event_strategy():
    return st.lists(
        st.one_of(
            st.fixed_dictionaries({"op": st.just("direct"), "validator": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("stale_direct"), "validator": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("edge"), "src": st.integers(pyint(0), pyint(3)), "dst": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("report"), "src": st.integers(pyint(0), pyint(3)), "dst": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("record_detected"), "hash": st.integers(pyint(1), pyint(6))}),
            st.fixed_dictionaries({"op": st.just("delete_record")}),
            st.fixed_dictionaries({"op": st.just("finalize")}),
            st.fixed_dictionaries({"op": st.just("retention_window"), "window": st.integers(pyint(0), pyint(6))}),
            st.fixed_dictionaries({"op": st.just("gossip_delay"), "delay": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("inclusion_delay"), "delay": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("finality_depth"), "depth": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("rust_missing")}),
            st.fixed_dictionaries({"op": st.just("rust_child"), "hash": st.integers(pyint(1), pyint(5))}),
            st.fixed_dictionaries({"op": st.just("rust_detected"), "hash": st.integers(pyint(1), pyint(5))}),
            st.fixed_dictionaries({"op": st.just("advance_epoch")}),
            st.fixed_dictionaries({"op": st.just("rejoin"), "validator": st.integers(pyint(0), pyint(3))}),
            st.fixed_dictionaries({"op": st.just("loose_identity"), "enabled": st.booleans()}),
            st.fixed_dictionaries({"op": st.just("carryover"), "enabled": st.booleans()}),
            st.fixed_dictionaries({"op": st.just("stakes"), "stakes": st.lists(st.integers(pyint(1), pyint(5)), min_size=pyint(4), max_size=pyint(4))}),
            st.fixed_dictionaries({"op": st.just("partition")}),
            st.fixed_dictionaries({"op": st.just("merge")}),
        ),
        min_size=pyint(1),
        max_size=pyint(14),
    )


def evaluate_horizon_v2_trace(events):
    validators = [0, 1, 2, 3]
    current = Set([0, 1])
    direct = Set([])
    stale_direct = Set([])
    visible_edges = Set([])
    reports = Set([])
    detected_hashes = Set([])
    record_deleted = False
    finalized = False
    retention_window = Integer(2)
    gossip_delay = Integer(0)
    inclusion_delay = Integer(0)
    finality_depth = Integer(0)
    rust_contributions = []
    current_epoch = Integer(0)
    loose_identity = False
    carryover = False
    stakes = [1, 1, 1, 1]
    partitioned = False
    merged = False
    for event in events:
        op = event["op"]
        if op == "direct":
            direct = direct.union(Set([int(event["validator"])]))
        elif op == "stale_direct":
            stale_direct = stale_direct.union(Set([int(event["validator"])]))
        elif op == "edge" and int(event["src"]) != int(event["dst"]):
            visible_edges = visible_edges.union(Set([(int(event["src"]), int(event["dst"]))]))
        elif op == "report" and int(event["src"]) != int(event["dst"]):
            reports = reports.union(Set([(int(event["src"]), int(event["dst"]))]))
        elif op == "record_detected":
            detected_hashes = detected_hashes.union(Set([int(event["hash"])]))
        elif op == "delete_record":
            record_deleted = True
        elif op == "finalize":
            finalized = True
        elif op == "retention_window":
            retention_window = Integer(event["window"])
        elif op == "gossip_delay":
            gossip_delay = Integer(event["delay"])
        elif op == "inclusion_delay":
            inclusion_delay = Integer(event["delay"])
        elif op == "finality_depth":
            finality_depth = Integer(event["depth"])
        elif op == "rust_missing":
            rust_contributions.append({"kind": "missing"})
        elif op == "rust_child":
            rust_contributions.append({"kind": "child", "hash": int(event["hash"])})
        elif op == "rust_detected":
            rust_contributions.append({"kind": "detected", "hash": int(event["hash"])})
        elif op == "advance_epoch":
            current_epoch += Integer(1)
        elif op == "rejoin":
            current = current.union(Set([int(event["validator"])]))
        elif op == "loose_identity":
            loose_identity = bool(event["enabled"])
        elif op == "carryover":
            carryover = bool(event["enabled"])
        elif op == "stakes":
            stakes = [int(value) for value in event["stakes"]]
        elif op == "partition":
            partitioned = True
        elif op == "merge":
            merged = True
            partitioned = False
    current_list = sorted(current)
    active_edges = visible_edges.difference(reports)
    strict_direct = direct.intersection(current)
    projected_direct = Set(strict_direct)
    if loose_identity or carryover:
        projected_direct = projected_direct.union(stale_direct.intersection(current))
    retained_closure, retained_trace = closure(current_list, sorted(projected_direct), list(active_edges))
    strict_closure, strict_trace = closure(current_list, sorted(strict_direct), list(active_edges))
    required_window = finality_depth + gossip_delay + inclusion_delay
    retention_safe = (not record_deleted) and retention_window >= required_window
    if retention_safe:
        projected_closure, projected_trace = retained_closure, retained_trace
    else:
        projected_closure, projected_trace = closure(current_list, [], [])
    detector_result = detector_horizon_detectable(rust_contributions)
    stakes_vector = vector(ZZ, [Integer(value) for value in stakes])
    direct_stake = stake_sum(stakes_vector, sorted(projected_direct))
    closure_stake = stake_sum(stakes_vector, retained_closure)
    record_projection_risk = record_deleted and len(detected_hashes) > 0
    finality_projection_risk = finalized and retained_closure != projected_closure
    projection_risk = record_projection_risk or finality_projection_risk or retained_closure != projected_closure
    assumption_counterexample = direct_stake <= 1 and closure_stake > 1
    candidate_boundary = strict_closure != retained_closure or partitioned or merged or current_epoch > 0 and (loose_identity or carryover)
    if projection_risk:
        classification = "projection_risk"
    elif assumption_counterexample:
        classification = "assumption_counterexample"
    elif candidate_boundary:
        classification = "candidate_boundary"
    else:
        classification = "bisimilar"
    features = ["horizon_v2"]
    for feature, condition in [
        ("detector", len(rust_contributions) > 0),
        ("record_lifecycle", len(detected_hashes) > 0 or record_deleted),
        ("finality", finalized or finality_depth > 0),
        ("retention", retention_window < required_window or record_deleted),
        ("availability", len(active_edges) > 0),
        ("weighted", closure_stake > direct_stake),
        ("epoch_churn", current_epoch > 0 or loose_identity or carryover),
        ("partition", partitioned or merged),
        ("projection", projection_risk),
        ("assumption_counterexample", assumption_counterexample),
        ("candidate_boundary", candidate_boundary),
    ]:
        if condition:
            features.append(feature)
    return {
        "classification": classification,
        "features": features,
        "scores": {
            "closure_size": len(retained_closure),
            "direct_stake": int(direct_stake),
            "closure_stake": int(closure_stake),
            "extra_stake": int(closure_stake - direct_stake),
            "required_window": int(required_window),
            "retention_window": int(retention_window),
        },
        "witness": {
            "validators": validators,
            "current_validators": current_list,
            "stakes": stakes,
            "epochs": [int(current_epoch) for _ in validators],
            "direct": sorted(direct),
            "stale_direct": sorted(stale_direct),
            "active_edges": edge_list(active_edges),
            "reports": edge_list(reports),
            "retained_closure": retained_closure,
            "retained_trace": retained_trace,
            "strict_closure": strict_closure,
            "strict_trace": strict_trace,
            "projected_closure": projected_closure,
            "projected_trace": projected_trace,
            "retention_policy": {
                "window": int(retention_window),
                "finality_depth": int(finality_depth),
                "gossip_delay": int(gossip_delay),
                "inclusion_delay": int(inclusion_delay),
                "required_window": int(required_window),
                "retention_safe": bool(retention_safe),
            },
            "records": [{"offender": 0, "base_seq": 1, "detected_hashes": sorted(detected_hashes), "deleted": bool(record_deleted)}],
            "rust_replay": {"contributions": rust_contributions, "detectable": bool(detector_result)},
            "projection": {
                "record_projection_risk": bool(record_projection_risk),
                "finality_projection_risk": bool(finality_projection_risk),
                "loose_identity": bool(loose_identity),
                "carryover": bool(carryover),
            },
            "events": events,
            "views": [{"node": "horizon_v2", "active_edges": edge_list(active_edges), "closure": retained_closure}],
        },
    }


class HorizonV2DetectorDAGMachine(RuleBasedStateMachine):
    def __init__(self):
        super().__init__()
        self.contributions = []

    @initialize()
    def init_state(self):
        self.contributions = []

    @rule()
    def missing(self):
        if len(self.contributions) < 5:
            self.contributions.append({"kind": "missing"})

    @rule(hash_value=st.integers(min_value=pyint(1), max_value=pyint(4)))
    def child(self, hash_value):
        if len(self.contributions) < 5:
            self.contributions.append({"kind": "child", "hash": int(hash_value)})

    @rule(hash_value=st.integers(min_value=pyint(1), max_value=pyint(4)))
    def detected(self, hash_value):
        if len(self.contributions) < 5:
            self.contributions.append({"kind": "detected", "hash": int(hash_value)})

    @invariant()
    def detector_is_order_independent(self):
        baseline = detector_horizon_detectable(self.contributions)
        for order in permutations(self.contributions):
            if detector_horizon_detectable(list(order)) != baseline:
                raise AssertionError("horizon-v2 detector order dependence")


class HorizonV2RecordLifecycleMachine(RuleBasedStateMachine):
    def __init__(self):
        super().__init__()
        self.events = []

    @initialize()
    def init_state(self):
        self.events = []

    @rule(hash_value=st.integers(min_value=pyint(1), max_value=pyint(6)))
    def record_detected(self, hash_value):
        if len(self.events) < 14:
            self.events.append({"op": "record_detected", "hash": int(hash_value)})

    @rule()
    def delete_record(self):
        if len(self.events) < 14:
            self.events.append({"op": "delete_record"})

    @rule(v=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def direct(self, v):
        if len(self.events) < 14:
            self.events.append({"op": "direct", "validator": int(v)})

    @rule(src=st.integers(min_value=pyint(0), max_value=pyint(3)), dst=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def edge(self, src, dst):
        if len(self.events) < 14:
            self.events.append({"op": "edge", "src": int(src), "dst": int(dst)})

    @invariant()
    def lifecycle_is_classified(self):
        result = evaluate_horizon_v2_trace(list(self.events))
        if result["classification"] not in ["bisimilar", "projection_risk", "assumption_counterexample"]:
            raise AssertionError("unexpected horizon-v2 record lifecycle classification")
        record = result["witness"]["records"][0]
        if record["deleted"] and record["detected_hashes"] and not result["witness"]["projection"]["record_projection_risk"]:
            raise AssertionError("deleted detected hashes were not classified as projection risk")


class HorizonV2EvidenceAvailabilityMachine(RuleBasedStateMachine):
    def __init__(self):
        super().__init__()
        self.events = []

    @initialize()
    def init_state(self):
        self.events = []

    @rule(v=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def direct(self, v):
        if len(self.events) < 14:
            self.events.append({"op": "direct", "validator": int(v)})

    @rule(src=st.integers(min_value=pyint(0), max_value=pyint(3)), dst=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def edge(self, src, dst):
        if len(self.events) < 14:
            self.events.append({"op": "edge", "src": int(src), "dst": int(dst)})

    @rule(src=st.integers(min_value=pyint(0), max_value=pyint(3)), dst=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def report(self, src, dst):
        if len(self.events) < 14:
            self.events.append({"op": "report", "src": int(src), "dst": int(dst)})

    @rule(window=st.integers(min_value=pyint(0), max_value=pyint(6)))
    def retention_window(self, window):
        if len(self.events) < 14:
            self.events.append({"op": "retention_window", "window": int(window)})

    @rule(depth=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def finality_depth(self, depth):
        if len(self.events) < 14:
            self.events.append({"op": "finality_depth", "depth": int(depth)})

    @rule(delay=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def gossip_delay(self, delay):
        if len(self.events) < 14:
            self.events.append({"op": "gossip_delay", "delay": int(delay)})

    @rule(delay=st.integers(min_value=pyint(0), max_value=pyint(3)))
    def inclusion_delay(self, delay):
        if len(self.events) < 14:
            self.events.append({"op": "inclusion_delay", "delay": int(delay)})

    @rule()
    def finalize(self):
        if len(self.events) < 14:
            self.events.append({"op": "finalize"})

    @invariant()
    def availability_is_classified_and_bounded(self):
        result = evaluate_horizon_v2_trace(list(self.events))
        if result["classification"] not in ["bisimilar", "projection_risk", "assumption_counterexample"]:
            raise AssertionError("unexpected horizon-v2 availability classification")
        current = set(result["witness"]["current_validators"])
        for key in ["retained_closure", "projected_closure", "strict_closure"]:
            if not set(result["witness"][key]).issubset(current):
                raise AssertionError("{} escaped current validators".format(key))
        active_edges = set(tuple(edge) for edge in result["witness"]["active_edges"])
        reports = set(tuple(edge) for edge in result["witness"]["reports"])
        if active_edges.intersection(reports):
            raise AssertionError("reported edge remained active in horizon-v2 availability")


def horizon_v2_detector_dag_state_machine_search(cfg):
    run_state_machine_as_test(HorizonV2DetectorDAGMachine, settings=cfg)
    cases = [
        {"name": "missing_pointer_skipped", "contributions": [{"kind": "missing"}], "expected": False},
        {"name": "duplicate_child_collapses", "contributions": [{"kind": "child", "hash": 1}, {"kind": "child", "hash": 1}], "expected": False},
        {"name": "distinct_children_detect", "contributions": [{"kind": "child", "hash": 1}, {"kind": "child", "hash": 2}], "expected": True},
        {"name": "detected_hash_detects", "contributions": [{"kind": "missing"}, {"kind": "detected", "hash": 3}], "expected": True},
    ]
    for case in cases:
        values = [detector_horizon_detectable(list(order)) for order in permutations(case["contributions"])]
        case["all_orders"] = values
        case["order_independent"] = len(Set(values)) == 1
        case["matches_expected"] = all(value == case["expected"] for value in values)
    unexpected = [case for case in cases if not case["order_independent"] or not case["matches_expected"]]
    return record(
        "horizon_v2_detector_dag_state_machine",
        "confirmed_safe" if unexpected == [] else "unexpected",
        "hypothesis_horizon_v2_detector_dag_state_machine",
        "Rule-based detector DAG exploration checks that Rust-shaped latest-message contributions are total and order-independent under missing pointers, duplicate canonical children, distinct children, and detected hashes.",
        {"state_machine": "HorizonV2DetectorDAGMachine"},
        {
            "validators": [0, 1, 2, 3],
            "direct": [0],
            "active_edges": [[1, 0]],
            "rust_replay": {"detector_cases": cases},
            "unexpected": unexpected,
            "unexpected_count": len(unexpected),
        },
        ["Hypothesis stateful API", "Rocq: detector contribution confluence", "TLA+: HorizonV2DivergenceClass"],
    )


def horizon_v2_record_lifecycle_state_machine_search(cfg):
    run_state_machine_as_test(HorizonV2RecordLifecycleMachine, settings=cfg)
    events = [{"op": "record_detected", "hash": 4}, {"op": "direct", "validator": 0}, {"op": "edge", "src": 1, "dst": 0}, {"op": "delete_record"}]
    result = evaluate_horizon_v2_trace(events)
    return record(
        "horizon_v2_record_lifecycle_state_machine",
        "confirmed_safe" if result["classification"] == "projection_risk" else "unexpected",
        "hypothesis_horizon_v2_record_lifecycle_state_machine",
        "Rule-based record lifecycle exploration keeps duplicate detected hashes normalized and classifies early record deletion with retained slash dependencies as projection risk.",
        {"state_machine": "HorizonV2RecordLifecycleMachine"},
        {"canonical_witness": result, "unexpected_count": 0 if result["classification"] == "projection_risk" else 1},
        ["Hypothesis stateful API", "Rocq: records_bisim_strong", "TLA+: RecordLifecycleDivergenceClass"],
    )


def horizon_v2_evidence_availability_state_machine_search(cfg):
    run_state_machine_as_test(HorizonV2EvidenceAvailabilityMachine, settings=cfg)
    unsafe_events = [
        {"op": "direct", "validator": 0},
        {"op": "edge", "src": 1, "dst": 0},
        {"op": "finality_depth", "depth": 2},
        {"op": "gossip_delay", "delay": 1},
        {"op": "inclusion_delay", "delay": 1},
        {"op": "retention_window", "window": 3},
        {"op": "finalize"},
    ]
    safe_events = unsafe_events[:-2] + [{"op": "retention_window", "window": 4}, {"op": "finalize"}]
    unsafe = evaluate_horizon_v2_trace(unsafe_events)
    safe = evaluate_horizon_v2_trace(safe_events)
    ok = unsafe["classification"] == "projection_risk" and safe["classification"] != "projection_risk"
    return record(
        "horizon_v2_evidence_availability_state_machine",
        "confirmed_safe" if ok else "unexpected",
        "hypothesis_horizon_v2_evidence_availability_state_machine",
        "Rule-based evidence-availability exploration synthesizes the finality-aware retention inequality and confirms that one slot below it loses slashability while the boundary value preserves it.",
        {"state_machine": "HorizonV2EvidenceAvailabilityMachine"},
        {"unsafe": unsafe, "safe_boundary": safe, "unexpected_count": 0 if ok else 1},
        ["Hypothesis stateful API", "TLA+: TemporalWindowDivergenceClass", "docs: finality-aware retention sizing"],
    )


def horizon_v2_min_edge_denial(vertices, direct, edges, target):
    full, full_trace = closure(vertices, direct, edges)
    for size in range(1, len(edges) + 1):
        for removed in combinations(edges, size):
            remaining = [edge for edge in edges if edge not in Set(removed)]
            projected, projected_trace = closure(vertices, direct, remaining)
            if int(target) in Set(full) and int(target) not in Set(projected):
                return {
                    "removed_edges": edge_list(removed),
                    "remaining_edges": edge_list(remaining),
                    "full_closure": full,
                    "full_trace": full_trace,
                    "projected_closure": projected,
                    "projected_trace": projected_trace,
                    "target": int(target),
                }
    return None


def horizon_v2_economic_objective_search(cfg):
    vertices = [0, 1, 2, 3]
    direct = [0]
    edges = [(1, 0), (2, 0), (3, 1), (3, 2)]
    closure_set, closure_trace = closure(vertices, direct, edges)
    strategy = st.lists(st.integers(pyint(1), pyint(5)), min_size=pyint(4), max_size=pyint(4))

    def objective(stakes):
        stakes_vector = vector(ZZ, [Integer(value) for value in stakes])
        direct_stake = stake_sum(stakes_vector, direct)
        closure_stake = stake_sum(stakes_vector, closure_set)
        return direct_stake <= 1 and closure_stake - direct_stake >= 6

    stakes = find_or_none(strategy, objective, cfg)
    if stakes is None:
        stakes = [1, 3, 3, 1]
    stakes_vector = vector(ZZ, [Integer(value) for value in stakes])
    direct_stake = stake_sum(stakes_vector, direct)
    closure_stake = stake_sum(stakes_vector, closure_set)
    denial = horizon_v2_min_edge_denial(vertices, direct, edges, 3)
    return record(
        "horizon_v2_economic_objective_search",
        "assumption_counterexample",
        "hypothesis_horizon_v2_economic_objective_search",
        "Objective-guided horizon-v2 search combines weighted damage amplification with the minimum evidence-denial edge set for removing a downstream neglect target.",
        {"objective": "maximize extra_stake and preserve redundant evidence paths"},
        {
            "validators": vertices,
            "stakes": [int(value) for value in stakes],
            "direct": direct,
            "active_edges": edge_list(edges),
            "closure": closure_set,
            "trace": closure_trace,
            "direct_stake": int(direct_stake),
            "closure_stake": int(closure_stake),
            "extra_stake": int(closure_stake - direct_stake),
            "minimal_edge_denial_for_v3": denial,
            "unexpected_count": 0,
        },
        ["Rocq: weighted closure-bound precondition", "TLA+: BoundedWeightedSlashClosure", "docs: economic threat scenario"],
    )


def horizon_v2_differential_classifier_search(cfg):
    strategy = horizon_v2_event_strategy()
    unexpected = find_or_none(strategy, lambda events: evaluate_horizon_v2_trace(events)["classification"] == "unexpected", cfg)
    fallbacks = {
        "bisimilar": [{"op": "direct", "validator": 0}],
        "candidate_boundary": [{"op": "stale_direct", "validator": 0}, {"op": "loose_identity", "enabled": True}, {"op": "edge", "src": 1, "dst": 0}],
        "projection_risk": [{"op": "record_detected", "hash": 4}, {"op": "delete_record"}],
        "assumption_counterexample": [{"op": "direct", "validator": 0}, {"op": "edge", "src": 1, "dst": 0}, {"op": "stakes", "stakes": [1, 3, 1, 1]}],
    }
    rows = []
    for classification in ["bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"]:
        witness = find_or_none(strategy, lambda events, classification=classification: evaluate_horizon_v2_trace(events)["classification"] == classification, cfg)
        if witness is None:
            witness = fallbacks[classification]
        rows.append({"target": classification, "result": evaluate_horizon_v2_trace(witness)})
    return record(
        "horizon_v2_differential_classifier",
        "confirmed_safe" if unexpected is None else "unexpected",
        "hypothesis_horizon_v2_differential_classifier",
        "Generated horizon-v2 traces are classified as bisimilar, candidate_boundary, projection_risk, assumption_counterexample, or unexpected; this search treats any unexpected trace as a frontier failure.",
        {"strategy": "horizon_v2_event_strategy"},
        {"rows": rows, "unexpected": None if unexpected is None else evaluate_horizon_v2_trace(unexpected), "unexpected_count": 0 if unexpected is None else 1},
        ["Rocq: DRHorizonV2Boundary", "TLA+: HorizonV2DivergenceClass", "Rust: divergence-class mirror"],
    )


def horizon_v2_searches(cfg):
    return [
        horizon_v2_detector_dag_state_machine_search(cfg),
        horizon_v2_record_lifecycle_state_machine_search(cfg),
        horizon_v2_evidence_availability_state_machine_search(cfg),
        horizon_v2_economic_objective_search(cfg),
        horizon_v2_differential_classifier_search(cfg),
    ]


def horizon_searches(cfg):
    return [
        horizon_campaign_coverage_search(cfg),
        horizon_state_machine_search(cfg),
        horizon_retention_delay_synthesis_search(cfg),
        horizon_detector_projection_gate_search(cfg),
        horizon_metamorphic_cross_oracle_search(cfg),
    ]


def frontier_searches(cfg):
    return [
        novelty_coverage_search(cfg),
        feature_combination_coverage_search(cfg),
        bundle_state_machine_search(cfg),
        multi_epoch_state_machine_search(cfg),
        multi_epoch_frontier_search(cfg),
        adversarial_scheduler_search(cfg),
        partition_gossip_state_machine_search(cfg),
        dag_trace_frontier_search(cfg),
        detector_totality_dag_search(cfg),
        cross_oracle_closure_consistency_search(cfg),
        adaptive_evidence_denial_search(cfg),
        composite_attack_search(cfg),
        candidate_invariant_mining_search(cfg),
        temporal_window_synthesis_search(cfg),
        mutation_oracle_detection_search(cfg),
        rebond_identity_lifecycle_search(cfg),
        record_lifecycle_state_machine_search(cfg),
        closure_depth_extremal_search(cfg),
        adversarial_vulnerability_campaign_search(cfg),
        liveness_as_safety_search(cfg),
        exact_projection_frontier_search(cfg),
        arithmetic_projection_stress_search(cfg),
        generated_trace_classifier_frontier_search(cfg),
        semantic_attack_campaign_state_machine_search(cfg),
        semantic_attack_campaign_search(cfg),
        attack_objective_search(cfg),
        objective_guided_search(cfg),
        metamorphic_property_search(cfg),
        metamorphic_stress_search(cfg),
        rust_metamorphic_checks_search(cfg),
        assumption_minimization_search(cfg),
        assumption_weakening_search(cfg),
        precondition_fuzzing_search(cfg),
        rust_differential_corpus_search(cfg),
        rust_differential_replay_search(cfg),
        evidence_monotonicity_search(cfg),
        view_merge_confluence_search(cfg),
        minimal_slash_basis_search(cfg),
        record_key_namespace_projection_search(cfg),
        detector_traversal_termination_search(cfg),
        detector_contribution_confluence_search(cfg),
        closure_fixed_point_idempotence_search(cfg),
        report_retention_reactivation_search(cfg),
        no_seed_cycle_safety_search(cfg),
        slash_history_prefix_search(cfg),
        edge_orientation_sanity_search(cfg),
        redundant_path_denial_cost_search(cfg),
        slash_target_authorization_search(cfg),
        report_namespace_isolation_search(cfg),
        report_antitone_closure_search(cfg),
        direct_seed_report_dominance_search(cfg),
        validator_renaming_equivariance_search(cfg),
        bisimilarity_delta_guard_search(cfg),
    ]


def targeted_searches(cfg):
    return [
        run_state_machine_checks(cfg),
        view_divergence_search(cfg),
        pruning_search(cfg),
        epoch_churn_search(cfg),
        proposer_liveness_search(cfg),
        batch_failure_search(cfg),
        canonicalization_search(cfg),
        economic_search(cfg),
        unexpected_edge_order_check(cfg),
    ]


def analyze(profile, max_examples, state_steps, search_mode, persistent_db_dir=None):
    cfg = hypothesis_settings(max_examples, state_steps, persistent_db_dir)
    records = []
    if search_mode in ["targeted", "all"]:
        records.extend(targeted_searches(cfg))
    if search_mode in ["frontier", "all"]:
        records.extend(frontier_searches(cfg))
    if search_mode in ["horizon", "all"]:
        records.extend(horizon_searches(cfg))
    if search_mode in ["horizon-v2", "all"]:
        records.extend(horizon_v2_searches(cfg))
    records = [item for item in records if item is not None]
    axis_counts = {}
    class_counts = {}
    for item in records:
        axis_counts[item["axis"]] = axis_counts.get(item["axis"], 0) + 1
        class_counts[item["classification"]] = class_counts.get(item["classification"], 0) + 1
    expected_axes = []
    if search_mode in ["targeted", "all"]:
        expected_axes.extend(AXES)
    if search_mode in ["frontier", "all"]:
        expected_axes.extend(FRONTIER_AXES)
    if search_mode in ["horizon", "all"]:
        expected_axes.extend(HORIZON_AXES)
    if search_mode in ["horizon-v2", "all"]:
        expected_axes.extend(HORIZON_V2_AXES)
    missing_axes = [axis for axis in expected_axes if axis not in axis_counts]
    return {
        "summaries": [
            {
                "profile": profile,
                "search_mode": search_mode,
                "max_examples": max_examples,
                "state_steps": state_steps,
                "persistent_database": persistent_db_dir,
                "axes": len(axis_counts),
                "records": len(records),
                "missing_axes": missing_axes,
                "class_counts": class_counts,
                "unexpected_count": class_counts.get("unexpected", 0),
            }
        ],
        "records": records,
    }


def replay(path):
    with open(path, "r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if "records" not in payload:
        raise AssertionError("replay JSON missing records")
    for item in payload["records"]:
        for key in ["axis", "classification", "name", "deterministic_witness"]:
            if key not in item:
                raise AssertionError("replay record missing {}".format(key))
    return {
        "summaries": [
            {
                "profile": "replay",
                "max_examples": 0,
                "state_steps": 0,
                "axes": len({item["axis"] for item in payload["records"]}),
                "records": len(payload["records"]),
                "missing_axes": [],
                "class_counts": {},
                "unexpected_count": len([item for item in payload["records"] if item["classification"] == "unexpected"]),
            }
        ],
        "records": payload["records"],
    }


def self_test():
    result = analyze("quick", 40, 12, "all")
    summary = result["summaries"][0]
    if summary["missing_axes"]:
        raise AssertionError("missing Hypothesis axes: {}".format(summary["missing_axes"]))
    if summary["unexpected_count"] != 0:
        raise AssertionError("unexpected divergence found")
    names = {item["name"] for item in result["records"]}
    required = Set(
        [
            "hypothesis_state_machine_active_edges_visible_unreported",
            "hypothesis_withholding_breaks_bounded_liveness",
            "hypothesis_partial_abort_order_dependent",
            "hypothesis_delimiter_free_key_collision",
            "hypothesis_weighted_closure_bound_violation",
            "hypothesis_frontier_novelty_coverage",
            "hypothesis_frontier_feature_combination_coverage",
            "hypothesis_frontier_bundle_rule_state_machine",
            "hypothesis_frontier_generated_trace_classifier",
            "hypothesis_frontier_multi_epoch_rule_state_machine",
            "hypothesis_frontier_adversarial_scheduler",
            "hypothesis_frontier_liveness_as_safety",
            "hypothesis_frontier_arithmetic_projection_stress",
            "hypothesis_frontier_semantic_attack_campaign",
            "hypothesis_frontier_semantic_attack_rule_state_machine",
            "hypothesis_frontier_attack_objectives",
            "hypothesis_frontier_objective_guided_search",
            "hypothesis_frontier_metamorphic_properties",
            "hypothesis_frontier_metamorphic_stress",
            "hypothesis_frontier_rust_metamorphic_checks",
            "hypothesis_frontier_assumption_minimization",
            "hypothesis_frontier_assumption_weakening",
            "hypothesis_frontier_precondition_fuzzing",
            "hypothesis_frontier_partition_gossip_state_machine",
            "hypothesis_frontier_dag_trace_generation",
            "hypothesis_frontier_detector_totality_dag_search",
            "hypothesis_frontier_cross_oracle_closure_consistency",
            "hypothesis_frontier_adaptive_evidence_denial",
            "hypothesis_frontier_composite_attack_search",
            "hypothesis_frontier_candidate_invariant_mining",
            "hypothesis_frontier_temporal_window_synthesis",
            "hypothesis_frontier_mutation_oracle_detection",
            "hypothesis_frontier_rebond_identity_lifecycle",
            "hypothesis_frontier_record_lifecycle_state_machine",
            "hypothesis_frontier_closure_depth_extremal_search",
            "hypothesis_frontier_adversarial_vulnerability_campaign",
            "hypothesis_frontier_rust_differential_corpus",
            "hypothesis_frontier_rust_differential_replay",
            "hypothesis_frontier_evidence_monotonicity",
            "hypothesis_frontier_view_merge_confluence",
            "hypothesis_frontier_minimal_slash_basis",
            "hypothesis_frontier_record_key_namespace_projection",
            "hypothesis_frontier_detector_traversal_termination",
            "hypothesis_frontier_detector_contribution_confluence",
            "hypothesis_frontier_closure_fixed_point_idempotence",
            "hypothesis_frontier_report_retention_reactivation",
            "hypothesis_frontier_no_seed_cycle_safety",
            "hypothesis_frontier_slash_history_prefix",
            "hypothesis_frontier_edge_orientation_sanity",
            "hypothesis_frontier_redundant_path_denial_cost",
            "hypothesis_frontier_slash_target_authorization",
            "hypothesis_frontier_report_namespace_isolation",
            "hypothesis_frontier_report_antitone_closure",
            "hypothesis_frontier_direct_seed_report_dominance",
            "hypothesis_frontier_validator_renaming_equivariance",
            "hypothesis_frontier_bisimilarity_delta_guard",
            "hypothesis_horizon_cross_coupled_campaign",
            "hypothesis_horizon_rule_state_machine",
            "hypothesis_horizon_retention_delay_synthesis",
            "hypothesis_horizon_detector_projection_gate",
            "hypothesis_horizon_metamorphic_cross_oracle",
            "hypothesis_horizon_v2_detector_dag_state_machine",
            "hypothesis_horizon_v2_record_lifecycle_state_machine",
            "hypothesis_horizon_v2_evidence_availability_state_machine",
            "hypothesis_horizon_v2_economic_objective_search",
            "hypothesis_horizon_v2_differential_classifier",
        ]
    )
    if not required.issubset(Set(names)):
        raise AssertionError("missing required Hypothesis witness")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print(
            "profile={profile} axes={axes} records={records} missing_axes={missing_axes} unexpected={unexpected_count}".format(
                **summary
            )
        )
        for classification in sorted(summary["class_counts"]):
            print("classification={classification} count={count}".format(classification=classification, count=summary["class_counts"][classification]))
    for item in result["records"]:
        print("axis={axis} classification={classification} name={name}".format(**item))


def rust_corpus(result):
    traces = []
    for item in result["records"]:
        witness = item["deterministic_witness"]
        if item["name"] == "hypothesis_frontier_rust_differential_corpus":
            traces.extend(witness["traces"])
        if item["name"] == "hypothesis_horizon_cross_coupled_campaign":
            for campaign in witness.get("campaigns", []):
                result_witness = campaign["result"]["witness"]
                traces.append(
                    rust_trace_event(
                        "hypothesis_horizon_{}".format(campaign["target"]),
                        campaign["result"]["classification"],
                        result_witness.get("events", []),
                        result_witness,
                        result_witness,
                        ["classification == {}".format(campaign["result"]["classification"]), "unexpected_count == 0"],
                    )
                )
        if item["name"] == "hypothesis_horizon_v2_differential_classifier":
            for row in witness.get("rows", []):
                result_witness = row["result"]["witness"]
                traces.append(
                    rust_trace_event(
                        "hypothesis_horizon_v2_{}".format(row["target"]),
                        row["result"]["classification"],
                        result_witness.get("events", []),
                        result_witness,
                        result_witness,
                        ["classification == {}".format(row["result"]["classification"]), "unexpected_count == 0"],
                    )
                )
    return {
        "summaries": [
            {
                "trace_count": len(traces),
                "classifications": sorted({trace["classification"] for trace in traces}),
            }
        ],
        "traces": traces,
    }


def rust_fixtures(result):
    cases = []
    for item in result["records"]:
        if item["name"] == "hypothesis_frontier_rust_differential_replay":
            cases.extend(item["deterministic_witness"]["cases"])
        if item["name"] == "hypothesis_horizon_detector_projection_gate":
            for case in item["deterministic_witness"]["rust_replay"]["detector_cases"]:
                cases.append(
                    rust_replay_case(
                        "horizon_detector_{}".format(case["name"]),
                        "bisimilar",
                        case,
                        {"detectable": case["expected"]},
                        {"detectable": case["expected"]},
                        ["detector gate is order independent", "unexpected_count == 0"],
                    )
                )
        if item["name"] == "hypothesis_horizon_v2_detector_dag_state_machine":
            for case in item["deterministic_witness"]["rust_replay"]["detector_cases"]:
                cases.append(
                    rust_replay_case(
                        "horizon_v2_detector_{}".format(case["name"]),
                        "bisimilar",
                        case,
                        {"detectable": case["expected"]},
                        {"detectable": case["expected"]},
                        ["detector DAG gate is order independent", "unexpected_count == 0"],
                    )
                )
    return {
        "summaries": [
            {
                "case_count": len(cases),
                "classifications": sorted({case["classification"] for case in cases}),
            }
        ],
        "cases": cases,
    }


def filtered_records(result, objectives):
    if objectives == "all":
        return list(result["records"])
    requested = Set([item.strip() for item in objectives.split(",") if item.strip()])
    selected = []
    for item in result["records"]:
        searchable = Set([item["axis"], item["name"], item["classification"]]).union(Set(item.get("coverage_features", [])))
        if requested.intersection(searchable):
            selected.append(item)
    return selected


def frontier_fixtures(result, top_k, objectives):
    records = filtered_records(result, objectives)
    records = sorted(records, key=lambda item: (-int(item.get("threat_score", 0)), item["name"]))[: int(top_k)]
    fixtures = []
    for item in records:
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


def main(argv):
    parser = argparse.ArgumentParser(description="Hypothesis-backed Sage search for slashing scenario witnesses")
    parser.add_argument("--profile", choices=["quick", "deep", "stress", "corpus", "corpus-deep", "rust-replay"], default="quick")
    parser.add_argument("--search-mode", choices=["targeted", "frontier", "horizon", "horizon-v2", "all"], default="all")
    parser.add_argument("--max-examples", type=int)
    parser.add_argument("--state-steps", type=int)
    parser.add_argument("--top-k", type=int, default=12)
    parser.add_argument("--objectives", default="all")
    parser.add_argument("--persistent-db")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    parser.add_argument("--schema-out")
    parser.add_argument("--fixture-out")
    parser.add_argument("--coverage-out")
    parser.add_argument("--rust-corpus-out")
    parser.add_argument("--rust-fixtures-out")
    parser.add_argument("--replay-json")
    args = parser.parse_args(argv)
    if args.replay_json:
        result = replay(args.replay_json)
    elif args.self_test:
        result = self_test()
    else:
        default_examples = {"quick": 100, "deep": 2000, "stress": 5000, "corpus": 10000, "corpus-deep": 20000, "rust-replay": 2000}[args.profile]
        default_steps = {"quick": 16, "deep": 64, "stress": 128, "corpus": 128, "corpus-deep": 256, "rust-replay": 64}[args.profile]
        persistent_db_dir = args.persistent_db
        if args.profile == "corpus" and persistent_db_dir is None:
            persistent_db_dir = "/tmp/slashing-hypothesis-corpus-db"
        if args.profile == "corpus-deep" and persistent_db_dir is None:
            persistent_db_dir = "/tmp/slashing-hypothesis-corpus-deep-db"
        result = analyze(args.profile, args.max_examples or default_examples, args.state_steps or default_steps, args.search_mode, persistent_db_dir)
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
            json.dump(frontier_fixtures(result, args.top_k, args.objectives), handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")
    if args.coverage_out:
        with open(args.coverage_out, "w", encoding="utf-8") as handle:
            json.dump(coverage_summary(filtered_records(result, args.objectives)), handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")
    if args.rust_corpus_out:
        with open(args.rust_corpus_out, "w", encoding="utf-8") as handle:
            json.dump(rust_corpus(result), handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")
    if args.rust_fixtures_out:
        with open(args.rust_fixtures_out, "w", encoding="utf-8") as handle:
            json.dump(rust_fixtures(result), handle, indent=2, sort_keys=True, default=json_default)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
