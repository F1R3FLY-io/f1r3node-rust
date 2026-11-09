#!/usr/bin/env python3

import re
import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit(
        "usage: check-rpb3-score-branch-inventory.py <score-bridge-source>"
    )

source = Path(sys.argv[1]).read_text()


def section(start: str, end: str) -> str:
    first = source.find(start)
    if first < 0:
        raise SystemExit(f"missing score-bridge declaration: {start}")
    last = source.find(end, first + len(start))
    if last < 0:
        raise SystemExit(f"missing score-bridge declaration: {end}")
    return source[first:last]


step = re.compile(r"\bmatch (bridge_[A-Za-z0-9_]+_step)\(before\)")
candidate = re.compile(
    r"\blet candidate_[0-9]+ = match (bridge_[A-Za-z0-9_]+_step)\(before\)"
)
expected = [
    "bridge_raw_par_step",
    "bridge_cost_atom_absent_step",
    "bridge_cost_atom_bytes_step",
    "bridge_cost_bound_step",
    "bridge_cost_atom_bound_forward_step",
    "bridge_cost_signature_step",
    "bridge_cost_region_step",
    "bridge_cost_token_stack_step",
    "bridge_bundle_step",
    "bridge_if_step",
    "bridge_send_step",
    "bridge_match_step",
    "bridge_absent_specialized_constructor_step",
    "bridge_absent_connective_step",
    "bridge_receive_bind_step",
    "bridge_match_case_step",
    "bridge_bool_oneof_step",
    "bridge_expr_atom_step",
    "bridge_expr_numeric_step",
    "bridge_expr_big_rational_step",
    "bridge_expr_fixed_point_step",
    "bridge_sys_auth_token_step",
    "bridge_new_empty_collections_step",
    "bridge_new_one_injection_step",
    "bridge_new_one_absent_injection_step",
    "bridge_new_one_uri_step",
    "bridge_new_collections_step",
    "bridge_expr_payload_forward_step",
    "bridge_expr_structural_step",
    "bridge_evar_step",
    "bridge_ordered_sequence_step",
    "bridge_emethod_step",
    "bridge_emap_finish_step",
    "bridge_connective_not_step",
    "bridge_connective_varref_step",
    "bridge_connective_collection_step",
    "bridge_unforgeable_bytes_step",
    "bridge_receive_present_condition_step",
    "bridge_receive_absent_condition_step",
    "bridge_nonsemantic_step",
]
relation = step.findall(
    section(
        "pub open spec fn bridge_score_source_step(",
        "pub enum BridgeScoreFinishSelection",
    )
)
if relation != expected:
    raise SystemExit(
        "score-source relation differs from the required ordered branch inventory: "
        f"expected {len(expected)} entries, found {len(relation)} entries and "
        f"{len(set(relation))} distinct branches"
    )

selected = []
for index in range(1, 6):
    group = f"pub open spec fn bridge_score_finish_selection_group_{index}("
    proof = f"pub proof fn bridge_score_finish_selection_group_{index}_sound("
    next_group = (
        f"pub open spec fn bridge_score_finish_selection_group_{index + 1}("
        if index < 5
        else "pub open spec fn bridge_score_finish_selected("
    )
    choices = candidate.findall(section(group, proof))
    sound_choices = candidate.findall(section(proof, next_group))
    if not choices or choices != sound_choices:
        raise SystemExit(f"score finish group {index} and its soundness proof disagree")
    selected.extend(choices)

if selected != relation:
    raise SystemExit(
        "score finish selector differs from the source relation in branch names or order"
    )

print(
    f"RPB3 score branch inventory: {len(relation)} ordered source and selector branches agree"
)
