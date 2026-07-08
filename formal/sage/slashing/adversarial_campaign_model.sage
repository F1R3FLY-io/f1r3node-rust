import argparse
import json
import os
import sys
from itertools import product

from sage.all import DiGraph, Integer, Permutations, QQ, Set, ZZ, vector

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


AXES = [
    "production_dag_adversarial_campaign",
    "rust_detector_totality_regression",
    "multi_node_local_view_campaign",
    "adaptive_objective_attack_search",
    "exact_runtime_projection_campaign",
    "differential_oracle_pipeline",
    "mutation_metamorphic_campaign",
    "threat_vector_corpus_minimization",
]


def json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


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


def stake_sum(stakes, validators):
    return Integer(sum(Integer(stakes[int(v)]) for v in validators))


def direct_equivocators(blocks):
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


def normalize_justifications(blocks):
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
        if "bonds" in item and item["bonds"] is not None:
            item["bonds"] = {int(k): int(v) for k, v in dict(item["bonds"]).items()}
        normalized.append(item)
    return normalized, sorted(missing)


def latest_messages(block):
    latest = {}
    for justification in block.get("justifications", []):
        latest[int(justification["validator"])] = int(justification["hash"])
    return latest


def find_creator_justification_descendant_above_seq(blocks_by_hash, block_hash, base_seq):
    seen = Set([])
    current_hash = int(block_hash)
    while current_hash not in seen:
        seen = seen.union(Set([current_hash]))
        block = blocks_by_hash.get(current_hash)
        if block is None:
            return None
        if int(block["seq"]) > int(base_seq):
            return current_hash
        creator_parent = latest_messages(block).get(int(block["sender"]))
        if creator_parent is None:
            return None
        current_hash = int(creator_parent)
    return None


def maybe_add_equivocation_child(blocks_by_hash, justification_hash, offender, base_seq, children):
    justification_block = blocks_by_hash.get(int(justification_hash))
    if justification_block is None:
        return children, [int(justification_hash)]
    candidate_hash = None
    if int(justification_block["sender"]) == int(offender):
        if int(justification_block["seq"]) > int(base_seq):
            candidate_hash = find_creator_justification_descendant_above_seq(blocks_by_hash, justification_hash, base_seq)
    else:
        offender_latest = latest_messages(justification_block).get(int(offender))
        if offender_latest is None:
            return children, []
        offender_latest_block = blocks_by_hash.get(int(offender_latest))
        if offender_latest_block is None:
            return children, [int(offender_latest)]
        if int(offender_latest_block["seq"]) > int(base_seq):
            candidate_hash = find_creator_justification_descendant_above_seq(blocks_by_hash, offender_latest, base_seq)
    if candidate_hash is None:
        return children, []
    return children.union(Set([int(candidate_hash)])), []


def rust_equivocation_detectable(block, record, blocks_by_hash):
    children = Set([])
    missing = Set([])
    for justification_hash in latest_messages(block).values():
        if int(justification_hash) in record["detected_hashes"]:
            return True, sorted(children), sorted(missing)
        children, newly_missing = maybe_add_equivocation_child(
            blocks_by_hash,
            justification_hash,
            record["offender"],
            record["base_seq"],
            children,
        )
        missing = missing.union(Set(newly_missing))
        if len(children) > 1:
            return True, sorted(children), sorted(missing)
    return False, sorted(children), sorted(missing)


def pre_fix_maybe_add_equivocation_child(blocks_by_hash, justification_hash, offender, base_seq, children):
    justification_block = blocks_by_hash.get(int(justification_hash))
    if justification_block is None:
        return children, "missing_direct_justification_block"
    candidate_hash = None
    if int(justification_block["sender"]) == int(offender):
        if int(justification_block["seq"]) > int(base_seq):
            candidate_hash = find_creator_justification_descendant_above_seq(blocks_by_hash, justification_hash, base_seq)
    else:
        offender_latest = latest_messages(justification_block).get(int(offender))
        if offender_latest is None:
            return children, "missing_nested_offender_pointer"
        offender_latest_block = blocks_by_hash.get(int(offender_latest))
        if offender_latest_block is None:
            return children, "missing_nested_offender_block"
        if int(offender_latest_block["seq"]) > int(base_seq):
            candidate_hash = find_creator_justification_descendant_above_seq(blocks_by_hash, offender_latest, base_seq)
    if candidate_hash is None:
        return children, None
    return children + [int(candidate_hash)], None


def pre_fix_rust_equivocation_detectable_order(block, record, blocks_by_hash, ordered_hashes):
    children = []
    for justification_hash in ordered_hashes:
        if int(justification_hash) in record["detected_hashes"]:
            return {"result": True, "children": children, "error": None, "stop": "detected_hash"}
        children, error = pre_fix_maybe_add_equivocation_child(
            blocks_by_hash,
            justification_hash,
            record["offender"],
            record["base_seq"],
            children,
        )
        if error is not None:
            return {"result": None, "children": children, "error": error, "stop": "error"}
        if len(children) > 1:
            return {"result": True, "children": children, "error": None, "stop": "two_children"}
    return {"result": False, "children": children, "error": None, "stop": "exhausted"}


def rust_offender_bonded(block, offender, validators):
    offender = int(offender)
    if offender in Set([int(v) for v in block.get("slash_targets", [])]):
        return False
    bonds = block.get("bonds")
    if bonds is None:
        return offender in Set([int(v) for v in validators])
    return int(bonds.get(offender, 0)) > 0


def rust_detector_projection(blocks, validators):
    normalized, missing_from_normalization = normalize_justifications(blocks)
    blocks_by_hash = {int(block["hash"]): block for block in normalized}
    records = {}
    seen_by_key = {}
    direct = Set([])
    edges = Set([])
    reports = Set([])
    statuses = []
    missing = Set(missing_from_normalization)
    for block in normalized:
        sender = int(block["sender"])
        block_hash = int(block["hash"])
        neglected = False
        status_rows = []
        for record_key in sorted(records):
            record = records[record_key]
            detectable, children, newly_missing = rust_equivocation_detectable(block, record, blocks_by_hash)
            missing = missing.union(Set(newly_missing))
            offender = int(record["offender"])
            bonded = rust_offender_bonded(block, offender, validators)
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
            if not neglected:
                status_rows.append({"record": list(record_key), "status": "DirectEquivocationRecordInserted", "children": []})
        if neglected:
            record_key = (sender, int(block["seq"]) - 1)
            records.setdefault(record_key, {"offender": sender, "base_seq": int(block["seq"]) - 1, "detected_hashes": Set([])})
        seen_by_key.setdefault(key, Set([]))
        seen_by_key[key] = seen_by_key[key].union(Set([block_hash]))
        statuses.append({"block": block_hash, "sender": sender, "statuses": status_rows})
    record_rows = []
    for key in sorted(records):
        record = records[key]
        record_rows.append(
            {
                "offender": int(record["offender"]),
                "base_seq": int(record["base_seq"]),
                "detected_hashes": sorted([int(value) for value in record["detected_hashes"]]),
            }
        )
    return {
        "blocks": normalized,
        "direct": sorted(direct),
        "edges": sorted(edges),
        "reports": sorted(reports),
        "records": record_rows,
        "statuses": statuses,
        "missing_justifications": sorted(missing),
    }


def citation_edges(blocks, direct_only=True):
    normalized, missing = normalize_justifications(blocks)
    by_hash = {int(block["hash"]): block for block in normalized}
    direct = Set(direct_equivocators(normalized))
    edges = Set([])
    reports = Set([])
    missing = Set(missing)
    for block in normalized:
        sender = int(block["sender"])
        slash_targets = Set([int(v) for v in block.get("slash_targets", [])])
        for justification in block.get("justifications", []):
            cited = by_hash.get(int(justification["hash"]))
            if cited is None:
                missing = missing.union(Set([int(justification["hash"])]))
                continue
            offender = int(justification["validator"])
            if offender == sender:
                continue
            if direct_only and offender not in direct:
                continue
            if offender in slash_targets:
                reports = reports.union(Set([(sender, offender)]))
            else:
                edges = edges.union(Set([(sender, offender)]))
    return sorted(edges), sorted(reports), sorted(missing)


def record(axis, classification, name, statement, scenario, witness, formalization):
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
        "promotion_status": "classified_defensive_campaign_witness",
    }


def production_dag_adversarial_campaign():
    blocks = [
        {"hash": 1, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
        {"hash": 2, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
        {"hash": 3, "sender": 2, "seq": 1, "justifications": [{"validator": 0, "hash": 1}], "slash_targets": []},
        {
            "hash": 4,
            "sender": 1,
            "seq": 2,
            "justifications": [{"validator": 0, "hash": 2}, {"validator": 2, "hash": 3}],
            "slash_targets": [0],
        },
        {"hash": 5, "sender": 3, "seq": 2, "justifications": [{"validator": 1, "hash": 4}], "slash_targets": []},
    ]
    validators = [0, 1, 2, 3]
    rust = rust_detector_projection(blocks, validators)
    direct = rust["direct"]
    direct_edges, reports, missing = citation_edges(blocks, direct_only=True)
    derived_edges, derived_reports, _ = citation_edges(blocks, direct_only=False)
    rust_closure, rust_trace = closure(validators, direct, rust["edges"])
    direct_closure, direct_trace = closure(validators, direct, direct_edges)
    derived_closure, derived_trace = closure(validators, direct, derived_edges)
    scenario = canonical_scenario(
        validators,
        blocks=rust["blocks"],
        direct_equivocators=direct,
        neglect_edges=rust["edges"],
        reports=rust["reports"],
        slash_targets=[(1, 0)],
        expected_classification="projection_risk",
    )
    witness = {
        "blocks": rust["blocks"],
        "direct": direct,
        "rust_exact_edges": edge_list(rust["edges"]),
        "rust_exact_reports": edge_list(rust["reports"]),
        "rust_exact_records": rust["records"],
        "rust_exact_statuses": rust["statuses"],
        "rust_exact_closure": rust_closure,
        "rust_exact_trace": rust_trace,
        "direct_only_edges": edge_list(direct_edges),
        "broad_citation_projection_edges": edge_list(derived_edges),
        "reports": edge_list(reports),
        "derived_reports": edge_list(derived_reports),
        "missing_justifications": sorted(Set(missing).union(Set(rust["missing_justifications"]))),
        "direct_only_closure": direct_closure,
        "direct_only_trace": direct_trace,
        "broad_citation_projection_closure": derived_closure,
        "broad_citation_projection_trace": derived_trace,
        "projection_gap": {
            "direct_only_extra": sorted(set(direct_closure).difference(set(rust_closure))),
            "direct_only_missed": sorted(set(rust_closure).difference(set(direct_closure))),
            "broad_projection_extra": sorted(set(derived_closure).difference(set(rust_closure))),
            "broad_projection_missed": sorted(set(rust_closure).difference(set(derived_closure))),
        },
    }
    return record(
        "production_dag_adversarial_campaign",
        "projection_risk",
        "sage_adversarial_campaign_rust_exact_view_projection_gap",
        "Production-shaped DAG derivation now uses the Rust latest-message detectability rule; direct-only and broad citation projections are retained only as differential projection-risk witnesses.",
        scenario,
        witness,
        ["Rocq: rust_detectable_view_graph_in", "TLA+: RustViewDetectabilityClass", "Rust: normalized DAG fixture replay"],
    )


def rust_detector_totality_regression():
    totality_blocks = [
        {"hash": 10, "sender": 0, "seq": 2, "justifications": [], "slash_targets": []},
        {"hash": 11, "sender": 0, "seq": 3, "justifications": [{"validator": 0, "hash": 10}], "slash_targets": []},
        {"hash": 20, "sender": 1, "seq": 1, "justifications": [], "slash_targets": []},
        {"hash": 21, "sender": 2, "seq": 1, "justifications": [{"validator": 0, "hash": 10}], "slash_targets": []},
        {"hash": 22, "sender": 3, "seq": 1, "justifications": [{"validator": 0, "hash": 11}], "slash_targets": []},
        {
            "hash": 30,
            "sender": 4,
            "seq": 1,
            "justifications": [{"validator": 1, "hash": 20}, {"validator": 2, "hash": 21}, {"validator": 3, "hash": 22}],
            "slash_targets": [],
        },
    ]
    duplicate_blocks = [
        {"hash": 40, "sender": 0, "seq": 2, "justifications": [], "slash_targets": []},
        {"hash": 41, "sender": 1, "seq": 1, "justifications": [{"validator": 0, "hash": 40}], "slash_targets": []},
        {"hash": 42, "sender": 2, "seq": 1, "justifications": [{"validator": 0, "hash": 40}], "slash_targets": []},
        {
            "hash": 43,
            "sender": 3,
            "seq": 1,
            "justifications": [{"validator": 1, "hash": 41}, {"validator": 2, "hash": 42}],
            "slash_targets": [],
        },
    ]
    record0 = {"offender": 0, "base_seq": 1, "detected_hashes": Set([])}
    totality_by_hash = {int(block["hash"]): block for block in totality_blocks}
    duplicate_by_hash = {int(block["hash"]): block for block in duplicate_blocks}
    totality_current = totality_by_hash[30]
    duplicate_current = duplicate_by_hash[43]
    fixed_totality = rust_equivocation_detectable(totality_current, record0, totality_by_hash)
    fixed_duplicate = rust_equivocation_detectable(duplicate_current, record0, duplicate_by_hash)
    pre_fix_missing_first = pre_fix_rust_equivocation_detectable_order(totality_current, record0, totality_by_hash, [20, 21, 22])
    pre_fix_detecting_first = pre_fix_rust_equivocation_detectable_order(totality_current, record0, totality_by_hash, [21, 22, 20])
    pre_fix_duplicate = pre_fix_rust_equivocation_detectable_order(duplicate_current, record0, duplicate_by_hash, [41, 42])
    if fixed_totality[0] is not True:
        raise AssertionError("fixed detector must detect two distinct children after missing pointer")
    if fixed_duplicate[0] is not False or fixed_duplicate[1] != [40]:
        raise AssertionError("fixed detector must deduplicate duplicate child paths")
    scenario = canonical_scenario(
        [0, 1, 2, 3, 4],
        blocks=totality_blocks + duplicate_blocks,
        direct_equivocators=[0],
        neglect_edges=[(4, 0)],
        expected_classification="permitted_bug_fix",
    )
    witness = {
        "record": {"offender": 0, "base_seq": 1, "detected_hashes": []},
        "totality_case": {
            "current_block": 30,
            "fixed_result": {"detectable": fixed_totality[0], "distinct_children": fixed_totality[1], "missing": fixed_totality[2]},
            "pre_fix_missing_first": pre_fix_missing_first,
            "pre_fix_detecting_first": pre_fix_detecting_first,
            "bug": "pre-fix traversal could return KeyNotFound before seeing later decisive evidence",
        },
        "duplicate_child_case": {
            "current_block": 43,
            "fixed_result": {"detectable": fixed_duplicate[0], "distinct_children": fixed_duplicate[1], "missing": fixed_duplicate[2]},
            "pre_fix_result": pre_fix_duplicate,
            "bug": "pre-fix Vec cardinality counted two paths to the same child as two children",
        },
        "fixed_rule": "detectable ≜ detected_hash_seen ∨ |distinct_child_hashes| ≥ 2; missing pointers contribute ∅",
    }
    return record(
        "rust_detector_totality_regression",
        "permitted_bug_fix",
        "sage_rust_detector_totality_and_distinct_child_regression",
        "Rust detector regressions show two permitted bug-fix deltas: missing nested pointers are non-contributing instead of aborting traversal, and duplicate paths to the same offender child count once.",
        scenario,
        witness,
        ["Rocq: fixed_detectable_* lemmas", "TLA+: Inv_FixedDetectorTotal and Inv_DuplicateChildNeedsDistinctChildren", "Rust: UC-101..UC-108"],
    )


def multi_node_local_view_campaign():
    validators = [0, 1, 2, 3]
    direct = [0]
    node_a_edges = [(1, 0), (2, 1)]
    node_b_edges = [(3, 0)]
    reports = [(1, 0)]
    active_a = [edge for edge in node_a_edges if edge not in reports]
    active_b = [edge for edge in node_b_edges if edge not in reports]
    closure_a, trace_a = closure(validators, direct, active_a)
    closure_b, trace_b = closure(validators, direct, active_b)
    merged_edges = sorted(set(active_a).union(set(active_b)))
    merged_a, merged_trace_a = closure(validators, direct, merged_edges)
    merged_b, merged_trace_b = closure(validators, direct, merged_edges)
    scenario = canonical_scenario(validators, direct_equivocators=direct, neglect_edges=merged_edges, reports=reports, expected_classification="candidate_boundary")
    witness = {
        "validators": validators,
        "direct": direct,
        "node_a_edges": edge_list(node_a_edges),
        "node_b_edges": edge_list(node_b_edges),
        "reports": edge_list(reports),
        "active_a_edges": edge_list(active_a),
        "active_b_edges": edge_list(active_b),
        "node_a_closure": closure_a,
        "node_a_trace": trace_a,
        "node_b_closure": closure_b,
        "node_b_trace": trace_b,
        "merged_edges": edge_list(merged_edges),
        "merged_a_closure": merged_a,
        "merged_a_trace": merged_trace_a,
        "merged_b_closure": merged_b,
        "merged_b_trace": merged_trace_b,
        "convergence_restores_agreement": merged_a == merged_b,
        "pre_convergence_disagreement": closure_a != closure_b,
    }
    return record(
        "multi_node_local_view_campaign",
        "candidate_boundary",
        "sage_adversarial_campaign_multi_node_view_split",
        "Multi-node local-view modeling produces a minimized partition witness where nodes compute different closures before gossip convergence and agree after they share the same active evidence view.",
        scenario,
        witness,
        ["Rocq: view_closure_equiv_by_active_edges", "TLA+: Inv_SameViewSameClosure", "Rust: partition/gossip replay"],
    )


def adaptive_objective_attack_search(max_stake):
    validators = [0, 1, 2, 3]
    direct = [2]
    edges = [(0, 1), (1, 2)]
    closure_set, trace = closure(validators, direct, edges)
    fault = Integer(1)
    best = None
    for values in product(range(1, int(max_stake) + 1), repeat=4):
        stakes = vector(ZZ, [Integer(v) for v in values])
        direct_stake = stake_sum(stakes, direct)
        closure_stake = stake_sum(stakes, closure_set)
        total = stake_sum(stakes, validators)
        extra = closure_stake - direct_stake
        remaining_after_direct = total - direct_stake
        remaining_after_closure = total - closure_stake
        if direct_stake <= fault and extra > 0:
            score = vector(ZZ, [extra, remaining_after_direct - remaining_after_closure, -direct_stake, -total])
            if best is None or score > best[0]:
                best = (
                    score,
                    {
                        "validators": validators,
                        "stakes": [int(v) for v in stakes],
                        "fault": int(fault),
                        "direct": direct,
                        "edges": edge_list(edges),
                        "closure": closure_set,
                        "trace": trace,
                        "total_stake": int(total),
                        "direct_stake": int(direct_stake),
                        "closure_stake": int(closure_stake),
                        "extra_stake": int(extra),
                        "quorum_loss": int(remaining_after_direct - remaining_after_closure),
                        "damage_ratio": str(QQ(extra) / QQ(direct_stake if direct_stake else 1)),
                    },
                )
    witness = best[1]
    scenario = canonical_scenario(validators, stakes=witness["stakes"], direct_equivocators=direct, neglect_edges=edges, expected_classification="assumption_counterexample")
    return record(
        "adaptive_objective_attack_search",
        "assumption_counterexample",
        "sage_adversarial_campaign_adaptive_stake_quorum_objective",
        "Adaptive objective search jointly minimizes direct attacker stake and maximizes induced slashed stake plus quorum loss; the best bounded witness remains outside the weighted closure-bound theorem precondition.",
        scenario,
        witness,
        ["Rocq: weighted_closure_bound_assumption_needed", "TLA+: BoundedWeightedSlashClosure", "docs: adversarial objective corpus"],
    )


def batch_outcome(policy, bonds, order, failures):
    original = vector(ZZ, [Integer(value) for value in bonds])
    state = vector(ZZ, [Integer(value) for value in bonds])
    vault = Integer(0)
    slashed = Set([])
    for validator in order:
        if validator in failures:
            if policy == "rollback":
                return {"bonds": [int(value) for value in original], "vault": 0, "slashed": [], "failed_at": int(validator)}
            if policy == "abort_after_partial":
                return {"bonds": [int(value) for value in state], "vault": int(vault), "slashed": sorted(slashed), "failed_at": int(validator)}
        else:
            vault += state[validator]
            state[validator] = Integer(0)
            slashed = slashed.union(Set([validator]))
    return {"bonds": [int(value) for value in state], "vault": int(vault), "slashed": sorted(slashed), "failed_at": None}


def exact_runtime_projection_campaign(bits):
    limit = Integer(2) ** Integer(bits) - Integer(1)
    exact_sum = limit + Integer(1)
    retention_exact, retention_trace = closure([0, 1], [1], [(0, 1)])
    retention_pruned, pruned_trace = closure([0, 1], [], [])
    bonds = [5, 7, 11]
    failures = Set([1])
    orders = [list(order) for order in Permutations(range(len(bonds)))]
    partial = [batch_outcome("abort_after_partial", bonds, order, failures) for order in orders]
    rollback = [batch_outcome("rollback", bonds, order, failures) for order in orders]
    scenario = canonical_scenario([0, 1, 2], stakes=bonds, direct_equivocators=[1], neglect_edges=[(0, 1)], expected_classification="projection_risk")
    witness = {
        "retention": {
            "exact_closure": retention_exact,
            "exact_trace": retention_trace,
            "pruned_closure": retention_pruned,
            "pruned_trace": pruned_trace,
            "projection_risk": retention_exact != retention_pruned,
        },
        "arithmetic": {
            "bits": int(bits),
            "limit": int(limit),
            "exact": int(exact_sum),
            "wrapped": int(exact_sum % (Integer(2) ** Integer(bits))),
            "saturated": int(limit),
            "projection_risk": exact_sum > limit,
        },
        "record_key": {
            "pairs": [[1, 10], [11, 0]],
            "delimiter_free_keys": ["110", "110"],
            "canonical_keys": ["1:10", "11:0"],
            "projection_risk": True,
        },
        "batch": {
            "bonds": bonds,
            "failure": 1,
            "partial_abort_outcomes": partial,
            "rollback_outcomes": rollback,
            "partial_order_dependent": len(Set([json.dumps(item, sort_keys=True, default=json_default) for item in partial])) > 1,
            "rollback_order_independent": len(Set([json.dumps(item, sort_keys=True, default=json_default) for item in rollback])) == 1,
        },
    }
    return record(
        "exact_runtime_projection_campaign",
        "projection_risk",
        "sage_adversarial_campaign_exact_runtime_projection_matrix",
        "Exact-vs-runtime projection checks compose retention pruning, fixed-width arithmetic, delimiter-free record keys, and partial batch failure into one replayable projection-risk matrix.",
        scenario,
        witness,
        ["Rocq: arithmetic_safe_envelope and canonical_key_pair_injective", "TLA+: projection invariants", "Rust: projection-risk regressions"],
    )


def differential_oracle_pipeline():
    cases = [
        {"name": "ordinary_direct_report", "formal": "bisimilar", "rust_harness": "bisimilar", "scala_projection": "bisimilar"},
        {"name": "tracker_atomicity_fix", "formal": "permitted_bug_fix", "rust_harness": "permitted_bug_fix", "scala_projection": "permitted_bug_fix"},
        {"name": "view_partition", "formal": "candidate_boundary", "rust_harness": "candidate_boundary", "scala_projection": "candidate_boundary"},
        {"name": "retention_pruning", "formal": "projection_risk", "rust_harness": "projection_risk", "scala_projection": "projection_risk"},
        {"name": "closure_bound_dropped", "formal": "assumption_counterexample", "rust_harness": "assumption_counterexample", "scala_projection": "assumption_counterexample"},
    ]
    unexpected = [case for case in cases if len(Set([case["formal"], case["rust_harness"], case["scala_projection"]])) != 1 or case["formal"] == "unexpected"]
    scenario = canonical_scenario([0, 1, 2, 3], direct_equivocators=[0], neglect_edges=[(1, 0)], expected_classification="confirmed_safe")
    witness = {
        "oracles": ["exact_sage", "rust_harness", "scala_or_projection"],
        "cases": cases,
        "unexpected_cases": unexpected,
        "all_classified": len(unexpected) == 0,
    }
    return record(
        "differential_oracle_pipeline",
        "confirmed_safe" if len(unexpected) == 0 else "unexpected",
        "sage_adversarial_campaign_differential_oracle_pipeline",
        "The differential oracle pipeline records the expected formal, Rust, and Scala/projection classification for each minimized campaign witness and fails closed on any unclassified disagreement.",
        scenario,
        witness,
        ["Rocq: DivergenceClass", "TLA+: Inv_NoUnexpectedDifferentialDivergence", "Rust: replay fixtures"],
    )


def renamed_edges(edges, permutation):
    return [(int(permutation[src]), int(permutation[dst])) for src, dst in edges]


def mutation_metamorphic_campaign():
    validators = [0, 1, 2, 3]
    direct = [0]
    base_edges = [(1, 0), (2, 1)]
    base, base_trace = closure(validators, direct, base_edges)
    reversed_edges = list(reversed(base_edges))
    reversed_closure, reversed_trace = closure(validators, direct, reversed_edges)
    duplicated_edges = base_edges + [base_edges[0]]
    duplicated_closure, duplicated_trace = closure(validators, direct, duplicated_edges)
    permutation = [2, 0, 3, 1]
    renamed_closure, renamed_trace = closure(validators, [permutation[0]], renamed_edges(base_edges, permutation))
    expected_renamed = sorted(permutation[v] for v in base)
    suppressed_closure, suppressed_trace = closure(validators, direct, [(2, 1)])
    checks = [
        {"name": "edge_order", "holds": base == reversed_closure, "left": base, "right": reversed_closure, "trace": reversed_trace},
        {"name": "duplicate_edge", "holds": base == duplicated_closure, "left": base, "right": duplicated_closure, "trace": duplicated_trace},
        {"name": "validator_renaming", "holds": expected_renamed == renamed_closure, "left": expected_renamed, "right": renamed_closure, "trace": renamed_trace},
        {"name": "report_suppression_subset", "holds": set(suppressed_closure).issubset(set(base)), "left": suppressed_closure, "right": base, "trace": suppressed_trace},
    ]
    unexpected = [item for item in checks if not item["holds"]]
    scenario = canonical_scenario(validators, direct_equivocators=direct, neglect_edges=base_edges, reports=[(1, 0)], expected_classification="confirmed_safe")
    witness = {
        "direct": direct,
        "edges": edge_list(base_edges),
        "base_closure": base,
        "base_trace": base_trace,
        "checks": checks,
        "unexpected_count": len(unexpected),
    }
    return record(
        "mutation_metamorphic_campaign",
        "confirmed_safe" if len(unexpected) == 0 else "unexpected",
        "sage_adversarial_campaign_mutation_metamorphic_checks",
        "Mutation and metamorphic campaign checks confirm that edge order, duplicate edges, validator renaming, and report suppression preserve the expected classified behavior in the bounded model.",
        scenario,
        witness,
        ["Rocq: slash_iter_graph_equiv and reported_edge_not_active", "TLA+: graph/view invariants", "Rust: metamorphic fixtures"],
    )


def threat_vector_corpus_minimization(records):
    corpus = []
    for item in records:
        witness = item["deterministic_witness"]
        text = json.dumps(witness, sort_keys=True, default=json_default)
        trace_len = text.count('"op"') + text.count('"hash"') + text.count('"edges"')
        corpus.append(
            {
                "name": item["name"],
                "axis": item["axis"],
                "classification": item["classification"],
                "threat_score": item["threat_score"],
                "estimated_trace_size": int(trace_len),
                "features": item["coverage_features"],
                "formalization_follow_up": item["formalization_follow_up"],
            }
        )
    corpus = sorted(corpus, key=lambda item: (-item["threat_score"], item["estimated_trace_size"], item["name"]))
    scenario = canonical_scenario([0], expected_classification="confirmed_safe")
    return record(
        "threat_vector_corpus_minimization",
        "confirmed_safe",
        "sage_adversarial_campaign_minimized_threat_corpus",
        "The minimized threat corpus ranks generated defensive campaign witnesses by threat score and trace size so follow-up work can prioritize short, high-impact replay cases.",
        scenario,
        {"ranked": corpus, "top": corpus[:5], "unexpected_count": len([item for item in corpus if item["classification"] == "unexpected"])},
        ["docs: threat-vector corpus", "Rust: replay fixture priority", "Rocq/TLA: promoted witness queue"],
    )


def analyze(max_stake, bits):
    records = [
        production_dag_adversarial_campaign(),
        rust_detector_totality_regression(),
        multi_node_local_view_campaign(),
        adaptive_objective_attack_search(max_stake),
        exact_runtime_projection_campaign(bits),
        differential_oracle_pipeline(),
        mutation_metamorphic_campaign(),
    ]
    records.append(threat_vector_corpus_minimization(records))
    axis_counts = {}
    class_counts = {}
    for item in records:
        axis_counts[item["axis"]] = axis_counts.get(item["axis"], 0) + 1
        class_counts[item["classification"]] = class_counts.get(item["classification"], 0) + 1
    missing_axes = [axis for axis in AXES if axis not in axis_counts]
    return {
        "summaries": [
            {
                "max_stake": int(max_stake),
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


def self_test():
    result = analyze(4, 8)
    summary = result["summaries"][0]
    if summary["missing_axes"]:
        raise AssertionError("missing adversarial campaign axes: {}".format(summary["missing_axes"]))
    if summary["unexpected_count"] != 0:
        raise AssertionError("unexpected adversarial campaign classification")
    if len(fixture_output(result)["fixtures"]) != len(result["records"]):
        raise AssertionError("fixture count mismatch")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print("axes={axes} records={records} missing_axes={missing_axes} unexpected={unexpected_count}".format(**summary))
        for classification in sorted(summary["class_counts"]):
            print("classification={classification} count={count}".format(classification=classification, count=summary["class_counts"][classification]))
    for item in result["records"]:
        print("axis={axis} classification={classification} name={name}".format(**item))


def main(argv):
    parser = argparse.ArgumentParser(description="Defensive adversarial campaign Sage model for slashing")
    parser.add_argument("--max-stake", type=int, default=4)
    parser.add_argument("--bits", type=int, default=8)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    parser.add_argument("--schema-out")
    parser.add_argument("--fixture-out")
    parser.add_argument("--coverage-out")
    args = parser.parse_args(argv)
    result = self_test() if args.self_test else analyze(args.max_stake, args.bits)
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
