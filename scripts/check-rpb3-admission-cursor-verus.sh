#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export TLC_REPO_ROOT="$repo_root"
# The common CanonicalPath proof reaches roughly 1 GiB RSS; a 1 GiB soft
# throttle spent its ten-minute limit reclaiming while consuming 10s CPU.
export TLC_MEMORY_HIGH="${TLC_MEMORY_HIGH:-2G}"
export TLC_RSS="${TLC_RSS:-3G}"
export TLC_CPU_QUOTA="${TLC_CPU_QUOTA:-100%}"
export TLC_TASKS_MAX="${TLC_TASKS_MAX:-64}"
export TLC_RUNTIME_MAX_SEC="${TLC_RUNTIME_MAX_SEC:-600}"
source "$repo_root/scripts/lib/tlc-run.sh"

verus_bin="${VERUS:-$(command -v verus)}"
model_dir="$repo_root/formal/verus"
evidence_dir="$repo_root/target/verification/rpb3-admission-cursor"
mkdir -p "$evidence_dir"
exec 9>"$evidence_dir/.gate.lock"
if ! flock -n 9; then
  echo "rpb3-admission-cursor: another gate run is writing this evidence directory" >&2
  exit 1
fi

source_files=(
  "$model_dir/rpb3_admission_cursor.rs"
  "$model_dir/rpb3_staged_admission_refinement.rs"
  "$model_dir/rpb3_generated_descriptor_projection.rs"
  "$model_dir/rpb3_score_admission_bridge.rs"
  "$model_dir/rpb3_score_arena_source_refinement.rs"
  "$model_dir/rpb3_wire_cursor_source_refinement.rs"
  "$model_dir/rpb3_runtime_frame_source_refinement.rs"
  "$model_dir/rpb3_canonical_path_driver_source_refinement.rs"
  "$model_dir/rpb3_canonical_path_driver_product_integration.rs"
  "$model_dir/rpb3_canonical_path_root_source_refinement.rs"
  "$model_dir/rpb3_canonical_path_epm_return_producer.rs"
  "$model_dir/rpb3_epm_source_accept_refinement.rs"
  "$model_dir/rpb3_epm_nested_source_refinement.rs"
  "$model_dir/rpb3_epm_score_replay_refinement.rs"
  "$model_dir/rpb3_ezipper_source_refinement.rs"
  "$model_dir/rpb3_finish_macro_refinement.rs"
  "$model_dir/rpb3_score_cursor_refinement.rs"
  "$model_dir/rpb3_map_frame_refinement.rs"
  "$model_dir/rpb3_epathmap_event_refinement.rs"
  "$model_dir/rpb3_path_parser_refinement.rs"
  "$model_dir/rpb3_canonical_path_common_product.rs"
  "$model_dir/rpb3_epathmap_push_macro_refinement.rs"
  "$model_dir/rpb3_epathmap_program31_complete_macro_refinement.rs"
  "$model_dir/rpb3_epathmap_nested_complete_macro_refinement.rs"
  "$model_dir/rpb3_epathmap_canonical_path_complete_macro_refinement.rs"
  "$model_dir/rpb3_canonical_path_raw_par_complete_macro_refinement.rs"
  "$model_dir/rpb3_canonical_path_rejected_nested_return_refinement.rs"
  "$model_dir/rpb3_live_canonical_path_source_refinement.rs"
  "$model_dir/rpb3_live_canonical_path_trace_refinement.rs"
  "$model_dir/rpb3_live_canonical_path_descendant_action_lift.rs"
  "$model_dir/rpb3_live_canonical_path_score_action_lift.rs"
  "$model_dir/rpb3_live_canonical_path_pop_action_lift.rs"
  "$model_dir/rpb3_live_canonical_path_map_finish_action_lift.rs"
  "$model_dir/rpb3_live_canonical_path_push_action_lift.rs"
  "$model_dir/rpb3_live_canonical_path_terminal_refinement.rs"
  "$model_dir/rpb3_live_canonical_path_root_terminal_trace.rs"
  "$model_dir/rpb3_canonical_path_terminal_driver_observation.rs"
  "$model_dir/rpb3_canonical_path_persistent_state_source.rs"
  "$model_dir/rpb3_canonical_path_historical_checkpoint_source.rs"
  "$model_dir/rpb3_canonical_path_forward_capacity_source.rs"
  "$model_dir/rpb3_canonical_path_owner_health_source.rs"
  "$model_dir/rpb3_canonical_path_root_complete_source_correspondence.rs"
  "$model_dir/rpb3_epm_nested_accept_checked_stats_refinement.rs"
  "$model_dir/rpb3_map_key_source_correspondence.rs"
  "$model_dir/rpb3_map_key_full_source_composition.rs"
  "$model_dir/rpb3_epathmap_terminal_source_observation.rs"
  "$model_dir/rpb3_epathmap_end_source_selector.rs"
  "$model_dir/rpb3_epm_push_source_action_refinement.rs"
  "$model_dir/rpb3_epm_push_transport_source_refinement.rs"
  "$model_dir/rpb3_epm_restore_run_expansion.rs"
  "$model_dir/rpb3_epm_batch_cursor_composition.rs"
  "$model_dir/rpb3_epm_batch_cursor_source_extraction.rs"
  "$model_dir/rpb3_epm_active_line_work_source.rs"
  "$model_dir/rpb3_act_source_product_integration.rs"
  "$model_dir/rpb3_epm_unwind_event_lift.rs"
  "$model_dir/rpb3_epm_unwind_product_lift.rs"
  "$model_dir/rpb3_epm_push_live_action_refinement.rs"
  "$model_dir/rpb3_epm_loop_continuation_source.rs"
  "$model_dir/rpb3_epm_loop_observation_bridge.rs"
  "$model_dir/rpb3_integrated_source_trace_composition.rs"
  "$model_dir/rpb3_epathmap_terminal_nested_return_composition.rs"
  "$model_dir/rpb3_canonical_path_eof_owner_obligation_refinement.rs"
  "$model_dir/rpb3_integrated_admission.rs"
  "$model_dir/rpb3_scalar_continue_source_refinement.rs"
  "$model_dir/rpb3_indexed_region_source_projection.rs"
  "$model_dir/rpb3_source_observation.rs"
  "$model_dir/rpb3_map_eof_indexed_view_identity.rs"
  "$model_dir/rpb3_map_push_receipt_source_extraction.rs"
  "$model_dir/rpb3_map_push_backing_identity.rs"
  "$model_dir/rpb3_map_push_scoped_identity_classes.rs"
  "$model_dir/rpb3_map_push_finite_observation.rs"
  "$model_dir/rpb3_map_push_selected_subview_extraction.rs"
  "$model_dir/rpb3_map_push_live_parent_bounds.rs"
  "$model_dir/rpb3_map_push_fresh_child_extraction.rs"
  "$model_dir/rpb3_map_push_live_child_projection.rs"
  "$model_dir/rpb3_map_push_checked_receipt_projection.rs"
  "$model_dir/rpb3_generic_message_eof_source_refinement.rs"
  "$model_dir/rpb3_message_driver_source_integration.rs"
  "$model_dir/rpb3_score_transcript.rs"
  "$repo_root/models/src/rust/rholang/canonical_score_bytes.rs"
  "$repo_root/models/src/rust/rholang/canonical_bytes_arena.rs"
  "$repo_root/models/src/rust/rholang/canonical_source_tree.rs"
  "$repo_root/models/src/rust/rholang/canonical_semantics.rs"
  "$repo_root/models/src/rust/rholang/canonical_path_admission.rs"
  "$repo_root/models/src/rust/canonical_path.rs"
  "$repo_root/models/src/rust/epathmap_trie_codec.rs"
  "$repo_root/rspace++/src/rspace/pathmap_codec.rs"
  "$repo_root/rspace++/tests/support/pathmap_codec_validated_plan.rs"
  "$repo_root/models/src/rust/rholang/indexed_protobuf.rs"
  "$repo_root/models/src/rust/rholang/indexed_source_projection_properties.rs"
  "$repo_root/models/src/rust/rholang/protobuf_admission.rs"
  "$repo_root/models/src/rust/rholang/admission_live_core.rs"
  "$repo_root/models/src/rust/rholang/protobuf_cursor.rs"
  "$repo_root/models/src/rust/rholang/protobuf_schema.rs"
  "$repo_root/models/src/rust/rholang/match_schema.rs"
  "$repo_root/models/src/rust/rholang/mod.rs"
  "$repo_root/models/build.rs"
  "$repo_root/models/codegen/schema.rs"
  "$repo_root/models/src/main/protobuf/RhoTypes.proto"
  "$repo_root/Cargo.toml"
  "$repo_root/Cargo.lock"
  "$repo_root/models/Cargo.toml"
  "$repo_root/rspace++/Cargo.toml"
  "$repo_root/models/src/rust/rholang/rpb3_root_projection_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_staged_admission_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_generated_oneof_seen_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_score_sequence_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_epathmap_receipt_sequence_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_active_frame_sequence_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_nested_epathmap_active_frame_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_epathmap_obligation_return_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_canonical_path_root_complete_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_canonical_path_root_escape_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_canonical_path_root_nested_epm_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_epathmap_batch_scalar_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_canonical_path_parser_source_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_message_eof_trace_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_generic_message_field_source_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_epathmap_multikey_batch_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_canonical_path_return_source_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_nested_epathmap_return_view_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_canonical_path_terminal_source_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_canonical_path_root_push_source_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_map_key_source_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_map_checked_event_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_map_push_receipt_source_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_raw_region_tree_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_par_source_summary_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_map_push_fresh_child_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_map_push_live_child_projection_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_message_scalar_iteration_image_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_epathmap_terminal_source_properties.rs"
  "$repo_root/models/src/rust/rholang/rpb3_message_complete_checked_event_properties.rs"
  "$repo_root/models/tests/support/rpb3_score_branch_properties.rs"
  "$repo_root/models/tests/support/rpb3_root_finish_trace.rs"
  "$repo_root/models/tests/support/standalone_path_admission.rs"
  "$repo_root/models/tests/support/canonical_path_opaque_interior.rs"
  "$repo_root/models/tests/cost_token_stack_admission.rs"
  "$repo_root/models/tests/rpb3_descriptor_projection.rs"
  "$repo_root/models/tests/rpb3_auxiliary_typed_source_bridge.rs"
  "$repo_root/models/tests/rpb3_epm1_independent_oracle.rs"
  "$repo_root/models/tests/formal_equivalence_manifest.rs"
  "$repo_root/models/tests/support/cost_token_stack_rpb3_admission_differential.rs"
  "$repo_root/formal/verified_score_store/Cargo.toml"
  "$repo_root/formal/verified_score_store/src/lib.rs"
  "$repo_root/scripts/check-rpb3-score-branch-inventory.py"
  "$repo_root/scripts/generate-rpb3-admission-mutant.py"
  "$repo_root/scripts/check-rpb3-admission-cursor-verus.sh"
  "$repo_root/scripts/check-stack-safe-pda-proofs.sh"
)
sha256sum "${source_files[@]}" | tee "$evidence_dir/source-manifest.sha256"
# Content hashes alone miss a source edit reverted before the final check.
# Record inode and nanosecond modification/change times for the same inputs.
source_identity_manifest="$evidence_dir/source-manifest.stat"
stat -c '%n|%d|%i|%s|%y|%z' -- "${source_files[@]}" \
  >"$source_identity_manifest"

python3 "$repo_root/scripts/check-rpb3-score-branch-inventory.py" \
  "$model_dir/rpb3_score_admission_bridge.rs"

score_store_log="$evidence_dir/verified-score-store.log"
if ! python3 -c 'import sys, tomllib; data = tomllib.load(open(sys.argv[1], "rb")); sys.exit(data.get("package", {}).get("metadata", {}).get("verus", {}).get("verify") is not True)' \
  "$repo_root/formal/verified_score_store/Cargo.toml"; then
  echo "rpb3-admission-cursor: verified score store is not opted into Verus" >&2
  exit 1
fi
tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" cargo verus verify \
  --manifest-path "$repo_root/formal/verified_score_store/Cargo.toml" \
  -p verified-score-store --offline --fwd-verus-args-to roots \
  -- --no-cheating --num-threads 1 --rlimit 30 >"$score_store_log" 2>&1
if rg -q '^verification results:: ' "$score_store_log"; then
  if ! rg -Fxq 'verification results:: 90 verified, 0 errors' "$score_store_log"; then
    cat "$score_store_log"
    echo "rpb3-admission-cursor: verified score store proof count changed" >&2
    exit 1
  fi
elif ! rg -q '^[[:space:]]*Finished `dev` profile ' "$score_store_log"; then
  cat "$score_store_log"
  echo "rpb3-admission-cursor: verified score store did not finish" >&2
  exit 1
fi

cp "$model_dir/rpb3_generated_descriptor_projection.rs" \
  "$evidence_dir/rpb3_generated_descriptor_projection.rs"

baseline_log="$evidence_dir/baseline.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  "$model_dir/rpb3_admission_cursor.rs" >"$baseline_log" 2>&1; then
  cat "$baseline_log"
  echo "rpb3-admission-cursor: baseline Verus proof failed" >&2
  exit 1
fi
if ! rg -q 'verification results:: [1-9][0-9]* verified, 0 errors' "$baseline_log"; then
  cat "$baseline_log"
  echo "rpb3-admission-cursor: baseline proof did not complete" >&2
  exit 1
fi
staged_source="$model_dir/rpb3_staged_admission_refinement.rs"
staged_proofs=(
  start_is_private
  staged_step_refines_one_driver_transition
  staged_step_preserves_event_order
  staged_push_retains_parent_and_appends_child
  staged_eof_pop_accept_is_exact
  staged_abort_does_not_publish
  staged_trace_refines_driver
)
if [[ "$(rg -c '^pub proof fn ' "$staged_source")" -ne "${#staged_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: staged driver proof inventory changed" >&2
  exit 1
fi
for proof in "${staged_proofs[@]}"; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$staged_source"; then
    echo "rpb3-admission-cursor: missing staged driver proof $proof" >&2
    exit 1
  fi
done
staged_log="$evidence_dir/staged-admission-refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --verify-root --crate-type lib \
  "$staged_source" >"$staged_log" 2>&1; then
  cat "$staged_log"
  echo "rpb3-admission-cursor: staged driver proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 7 verified, 0 errors' "$staged_log"; then
  cat "$staged_log"
  echo "rpb3-admission-cursor: staged driver proof did not verify" >&2
  exit 1
fi
sha256sum "$staged_log"
scalar_source="$model_dir/rpb3_scalar_continue_source_refinement.rs"
scalar_source_proofs=(
  root_init_source_derives_initial_state
  root_init_empty_generated_witness_is_initial
  scalar_continue_source_image_exact
  scalar_continue_source_value_and_identity_exact
  scalar_continue_source_slot_delta_exact
  scalar_continue_source_anchor_exact
  scalar_continue_source_continue_enabled
  scalar_continue_source_indexed_enabled
  scalar_continue_source_post_refines_enabled_indexed_step
  root_init_empty_send_witness_is_initial
)
scalar_source_proof_count="$(rg -c '^pub proof fn ' "$scalar_source")"
if [[ "$scalar_source_proof_count" -ne "${#scalar_source_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: scalar Continue public proof inventory changed" >&2
  exit 1
fi
for proof in "${scalar_source_proofs[@]}"; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$scalar_source"; then
    echo "rpb3-admission-cursor: missing scalar Continue proof $proof" >&2
    exit 1
  fi
done
scalar_source_log="$evidence_dir/scalar-continue-source.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --verify-root \
  "$scalar_source" \
  >"$scalar_source_log" 2>&1; then
  cat "$scalar_source_log"
  echo "rpb3-admission-cursor: scalar Continue source proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 10 verified, 0 errors' "$scalar_source_log"; then
  cat "$scalar_source_log"
  echo "rpb3-admission-cursor: scalar Continue source proof did not verify" >&2
  exit 1
fi
sha256sum "$scalar_source_log"
indexed_region_source="$model_dir/rpb3_indexed_region_source_projection.rs"
indexed_region_source_proofs=(
  bound_rust_api_projects_exactly
  indexed_view_projects_exactly
  borrowed_root_projection_exact
  frozen_root_projection_exact
  composed_root_projection_exact
  empty_root_projection_exact
  subview_projection_exact
  empty_subview_projection_exact
  subview_with_end_projection_exact
  same_view_preserves_image
)
indexed_region_source_proof_count="$(rg -c '^pub proof fn ' "$indexed_region_source")"
if [[ "$indexed_region_source_proof_count" -ne "${#indexed_region_source_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: indexed-region public proof inventory changed" >&2
  exit 1
fi
for proof in "${indexed_region_source_proofs[@]}"; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$indexed_region_source"; then
    echo "rpb3-admission-cursor: missing indexed-region proof $proof" >&2
    exit 1
  fi
done
indexed_region_source_log="$evidence_dir/indexed-region-source.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --verify-root \
  "$indexed_region_source" \
  >"$indexed_region_source_log" 2>&1; then
  cat "$indexed_region_source_log"
  echo "rpb3-admission-cursor: indexed-region source proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 10 verified, 0 errors' "$indexed_region_source_log"; then
  cat "$indexed_region_source_log"
  echo "rpb3-admission-cursor: indexed-region source proof did not verify" >&2
  exit 1
fi
sha256sum "$indexed_region_source_log"
for proof in admission_indexed_field_child_source_exact \
  admission_discontinuous_empty_child_anchor_witness \
  concrete_message_push_indexed_source_anchor_exact \
  concrete_message_push_indexed_rejects_wrong_source \
  concrete_empty_set_map_parent_pop_is_reachable \
  concrete_empty_eset_expr_tag22_is_source_derived \
  concrete_empty_emap_expr_tag23_is_source_derived \
  admission_canonical_path_push_indexed_preserves_wf \
  admission_canonical_path_return_indexed_preserves_wf \
  admission_canonical_path_root_source_derives_initial_wf \
  admission_canonical_path_empty_root_witness_is_initial; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$model_dir/rpb3_admission_cursor.rs"; then
    echo "rpb3-admission-cursor: missing admission proof $proof" >&2
    exit 1
  fi
done
for proof in map_admission_value_push_indexed_source_anchor_exact \
  map_admission_value_push_indexed_rejects_wrong_source \
  map_admission_value_accept_pop_source_anchor_exact \
  map_admission_entry_complete_source_anchor_exact; do
  if ! rg -q "^pub proof fn ${proof}\\(" \
    "$model_dir/rpb3_map_frame_refinement.rs"; then
    echo "rpb3-admission-cursor: missing map-frame proof $proof" >&2
    exit 1
  fi
done

if ! rg -q '^pub proof fn initial_plain_tags_refute_unchanged_gate\(' \
  "$model_dir/rpb3_epathmap_event_refinement.rs"; then
  echo "rpb3-admission-cursor: missing ordinary Edge gate counterexample" >&2
  exit 1
fi
for model in rpb3_score_admission_bridge rpb3_score_cursor_refinement \
  rpb3_map_frame_refinement rpb3_epathmap_event_refinement; do
  model_log="$evidence_dir/$model.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
    "$model_dir/$model.rs" >"$model_log" 2>&1; then
    cat "$model_log"
    echo "rpb3-admission-cursor: $model Verus proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: [1-9][0-9]* verified, 0 errors' "$model_log"; then
    cat "$model_log"
    echo "rpb3-admission-cursor: $model proof did not complete" >&2
    exit 1
  fi
done

parser_source="$model_dir/rpb3_path_parser_refinement.rs"
for proof in initial_parser_source_snapshot_has_witness \
  two_byte_varint_decodes_130 \
  utf8_ed_rejects_surrogate_prefix \
  initial_plain_parser_tags_change_gate \
  one_child_list_parser_bubbles_to_root \
  one_key_region_parser_focuses_and_settles \
  nested_invalid_tag_fault_freezes_parser_on_later_plain_edge; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$parser_source"; then
    echo "rpb3-admission-cursor: missing canonical-path parser proof $proof" >&2
    exit 1
  fi
done
parser_log="$evidence_dir/path-parser-refinement-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 "$parser_source" \
  >"$parser_log" 2>&1; then
  cat "$parser_log"
  echo "rpb3-admission-cursor: canonical-path parser refinement failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: [0-9]+ verified, 0 errors' "$parser_log"; then
  cat "$parser_log"
  echo "rpb3-admission-cursor: canonical-path parser refinement did not verify" >&2
  exit 1
fi

common_path_source="$model_dir/rpb3_canonical_path_common_product.rs"
for proof in live_canonical_path_common_root_is_wf \
  canonical_path_internal_edge_preserves_wf \
  canonical_path_internal_value_preserves_wf \
  canonical_path_internal_trace_prefix_wf \
  boolean_root_common_witness \
  boolean_root_internal_edge_value_witness \
  boolean_root_scan_fold_source_witness \
  boolean_root_complete_macro_witness \
  boolean_root_loop_source_witness \
  opaque_escape_bulk_parser_state_witness \
  opaque_escape_final_parser_byte_transition \
  opaque_escape_final_loop_edge_witness \
  opaque_escape_final_payload_witness \
  opaque_escape_bulk_loop_source_witness \
  boolean_root_loop_selected_value_witness \
  boolean_root_checked_eof_completion_witness \
  boolean_root_common_dispatch_complete_witness; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$common_path_source"; then
    echo "rpb3-admission-cursor: missing common canonical-path proof $proof" >&2
    exit 1
  fi
done
common_path_log="$evidence_dir/common-canonical-path-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 \
  --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_canonical_path_common_product \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$common_path_log" 2>&1; then
  cat "$common_path_log"
  echo "rpb3-admission-cursor: common canonical-path refinement failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 22 verified, 0 errors' "$common_path_log"; then
  cat "$common_path_log"
  echo "rpb3-admission-cursor: common canonical-path refinement did not verify" >&2
  exit 1
fi

for proof in empty_append_certificate \
  certified_interval_entries_valid \
  kind_encoded_len_matches_transcript \
  prepare_score_entry_matches_checked_append \
  checked_append_preserves_certificate \
  checked_append_attempt_preserves_certificate \
  checked_append_attempt_preserves_certified_interval \
  certified_empty_arena_wf \
  certified_append_attempt_preserves_wf; do
  if ! rg -q "^pub proof fn ${proof}\\(" \
    "$model_dir/rpb3_score_transcript.rs"; then
    echo "rpb3-admission-cursor: missing score append certificate proof $proof" >&2
    exit 1
  fi
done

for proof in bridge_certified_interval_entries_exact \
  bridge_initial_append_certificate \
  bridge_append_preserves_certificate \
  bridge_append_unattached_preserves_certificate \
  bridge_append_unattached_attempt_preserves_certificate \
  bridge_append_unattached_attempt_preserves_certified_interval; do
  if ! rg -q "^pub proof fn ${proof}\\(" \
    "$model_dir/rpb3_score_admission_bridge.rs"; then
    echo "rpb3-admission-cursor: missing score bridge certificate proof $proof" >&2
    exit 1
  fi
done

for proof in bridge_emap_complete_key_handles_exact \
  bridge_canonical_varint_end_unique \
  bridge_canonical_field_tag_and_end_unique \
  bridge_par_locally_free_range_unique_at \
  bridge_par_identity_from_source_is_unique \
  bridge_empty_par_body_has_empty_identity \
  bridge_locally_free_only_par_has_empty_identity \
  bridge_par_locally_free_range_is_source_bounded \
  bridge_par_identity_is_source_slice \
  bridge_emap_finish_source_complete_and_incomplete \
  bridge_emap_complete_finish_source_exact \
  bridge_emap_incomplete_finish_selected_none \
  bridge_key_value_pair_source_eof_selected_none \
  bridge_key_value_pair_parent_pop_preserves_source; do
  if ! rg -q "^pub proof fn ${proof}\\(" \
    "$model_dir/rpb3_score_admission_bridge.rs"; then
    echo "rpb3-admission-cursor: missing EMap score bridge proof $proof" >&2
    exit 1
  fi
done

for proof in bridge_primary_component_source_step_has_unique_finish \
  bridge_cost_token_stack_source_step_has_unique_finish \
  bridge_gunforgeable_source_step_matches_selected \
  bridge_gunforgeable_source_step_has_unique_finish \
  bridge_connective_source_step_matches_selected \
  bridge_connective_source_step_has_unique_finish \
  bridge_expr_source_step_matches_selected \
  bridge_expr_source_step_has_unique_finish \
  bridge_score_source_step_preserves_driver_counts \
  bridge_receive_complete_source_step_has_unique_finish \
  bridge_new_complete_source_step_matches_selected \
  bridge_new_complete_source_step_has_unique_finish; do
  if ! rg -q "^pub proof fn ${proof}\\(" \
    "$model_dir/rpb3_score_admission_bridge.rs"; then
    echo "rpb3-admission-cursor: missing score selector proof $proof" >&2
    exit 1
  fi
done

for model in rpb3_score_arena_source_refinement rpb3_wire_cursor_source_refinement \
  rpb3_epm_source_accept_refinement rpb3_epm_nested_source_refinement; do
  model_log="$evidence_dir/$model.log"
  model_rlimit=()
  if [[ "$model" == rpb3_wire_cursor_source_refinement ]]; then
    model_rlimit=(--rlimit 30)
  fi
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
    "${model_rlimit[@]}" --verify-root "$model_dir/$model.rs" >"$model_log" 2>&1; then
    cat "$model_log"
    echo "rpb3-admission-cursor: $model source proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: [1-9][0-9]* verified, 0 errors' "$model_log"; then
    cat "$model_log"
    echo "rpb3-admission-cursor: $model source proof did not complete" >&2
    exit 1
  fi
done

for proof in runtime_append_attempt_refines_score_append \
  runtime_reservation_failure_preserves_next_handle \
  runtime_success_preserves_append_trace \
  runtime_failed_attempt_preserves_append_trace \
  runtime_operational_attempt_preserves_replay_or_appends; do
  if ! rg -q "^pub proof fn ${proof}\\(" \
    "$model_dir/rpb3_score_arena_source_refinement.rs"; then
    echo "rpb3-admission-cursor: missing score-arena proof $proof" >&2
    exit 1
  fi
done

finish_log="$evidence_dir/rpb3_finish_macro_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --cfg rpb3_finish_standalone --verify-root \
  "$model_dir/rpb3_finish_macro_refinement.rs" \
  >"$finish_log" 2>&1; then
  cat "$finish_log"
  echo "rpb3-admission-cursor: finish macro Verus proof failed" >&2
  exit 1
fi
if ! rg -q 'verification results:: 3 verified, 0 errors' "$finish_log"; then
  cat "$finish_log"
  echo "rpb3-admission-cursor: finish macro witness did not verify" >&2
  exit 1
fi

runtime_log="$evidence_dir/rpb3_runtime_frame_source_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --cfg rpb3_runtime_standalone --verify-root \
  "$model_dir/rpb3_runtime_frame_source_refinement.rs" \
  >"$runtime_log" 2>&1; then
  cat "$runtime_log"
  echo "rpb3-admission-cursor: runtime frame source proof failed" >&2
  exit 1
fi
if ! rg -q 'verification results:: [1-9][0-9]* verified, 0 errors' "$runtime_log"; then
  cat "$runtime_log"
  echo "rpb3-admission-cursor: runtime frame source proof did not complete" >&2
  exit 1
fi
for proof in runtime_noscore_key_value_pair_modeled_witness \
  runtime_noscore_key_value_pair_present_modeled_witness \
  runtime_noscore_opaque_modeled_witness \
  runtime_noscore_expr_structural_modeled_witness \
  runtime_noscore_emethod_modeled_witness \
  runtime_push_indexed_checked_storage \
  source_checked_frame_request_push_indexed_refines \
  runtime_epm_source_score_finish_is_checked \
  runtime_epm_root_modeled_witness \
  runtime_epm_child_modeled_witness \
  source_checked_frame_request_push_fresh_refines \
  source_checked_frame_request_push_indexed_fresh_refines \
  source_checked_fresh_root_refines \
  source_initial_indexed_cursor_rejects_consumed; do
  if ! rg -q "^pub proof fn ${proof}\\(" \
    "$model_dir/rpb3_runtime_frame_source_refinement.rs"; then
    echo "rpb3-admission-cursor: missing runtime proof $proof" >&2
    exit 1
  fi
done

epm_push_macro_log="$evidence_dir/rpb3_epathmap_push_macro_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --verify-root \
  "$model_dir/rpb3_epathmap_push_macro_refinement.rs" \
  >"$epm_push_macro_log" 2>&1; then
  cat "$epm_push_macro_log"
  echo "rpb3-admission-cursor: EPathMap Push macro proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$epm_push_macro_log"; then
  cat "$epm_push_macro_log"
  echo "rpb3-admission-cursor: EPathMap Push macro proof did not complete" >&2
  exit 1
fi
if ! rg -q '^pub proof fn linked_epm_push_macro_preserves_source_and_progress\(' \
  "$model_dir/rpb3_epathmap_push_macro_refinement.rs"; then
  echo "rpb3-admission-cursor: missing EPathMap Push macro proof" >&2
  exit 1
fi

epm_program31_complete_log="$evidence_dir/rpb3_epathmap_program31_complete_macro_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --verify-root \
  "$model_dir/rpb3_epathmap_program31_complete_macro_refinement.rs" \
  >"$epm_program31_complete_log" 2>&1; then
  cat "$epm_program31_complete_log"
  echo "rpb3-admission-cursor: program-31 EPathMap Complete macro proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$epm_program31_complete_log"; then
  cat "$epm_program31_complete_log"
  echo "rpb3-admission-cursor: program-31 EPathMap Complete macro proof did not complete" >&2
  exit 1
fi
if ! rg -q '^pub proof fn program31_epm_complete_macro_preserves_source_and_score\(' \
  "$model_dir/rpb3_epathmap_program31_complete_macro_refinement.rs"; then
  echo "rpb3-admission-cursor: missing program-31 EPathMap Complete macro proof" >&2
  exit 1
fi

epm_nested_complete_log="$evidence_dir/rpb3_epathmap_nested_complete_macro_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --verify-root \
  "$model_dir/rpb3_epathmap_nested_complete_macro_refinement.rs" \
  >"$epm_nested_complete_log" 2>&1; then
  cat "$epm_nested_complete_log"
  echo "rpb3-admission-cursor: nested EPathMap Complete macro proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$epm_nested_complete_log"; then
  cat "$epm_nested_complete_log"
  echo "rpb3-admission-cursor: nested EPathMap Complete macro proof did not complete" >&2
  exit 1
fi
if ! rg -q '^pub proof fn nested_epm_complete_macro_preserves_source_and_score\(' \
  "$model_dir/rpb3_epathmap_nested_complete_macro_refinement.rs"; then
  echo "rpb3-admission-cursor: missing nested EPathMap Complete macro proof" >&2
  exit 1
fi

epm_canonical_path_complete_log="$evidence_dir/rpb3_epathmap_canonical_path_complete_macro_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --verify-root \
  "$model_dir/rpb3_epathmap_canonical_path_complete_macro_refinement.rs" \
  >"$epm_canonical_path_complete_log" 2>&1; then
  cat "$epm_canonical_path_complete_log"
  echo "rpb3-admission-cursor: CanonicalPath-parent EPathMap Complete macro proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$epm_canonical_path_complete_log"; then
  cat "$epm_canonical_path_complete_log"
  echo "rpb3-admission-cursor: CanonicalPath-parent EPathMap Complete macro proof did not complete" >&2
  exit 1
fi
if ! rg -q '^pub proof fn canonical_path_epm_complete_macro_preserves_source_and_score\(' \
  "$model_dir/rpb3_epathmap_canonical_path_complete_macro_refinement.rs"; then
  echo "rpb3-admission-cursor: missing CanonicalPath-parent EPathMap Complete macro proof" >&2
  exit 1
fi

canonical_path_raw_par_complete_log="$evidence_dir/rpb3_canonical_path_raw_par_complete_macro_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --verify-root \
  "$model_dir/rpb3_canonical_path_raw_par_complete_macro_refinement.rs" \
  >"$canonical_path_raw_par_complete_log" 2>&1; then
  cat "$canonical_path_raw_par_complete_log"
  echo "rpb3-admission-cursor: CanonicalPath RawPar Complete macro proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 1 verified, 0 errors' "$canonical_path_raw_par_complete_log"; then
  cat "$canonical_path_raw_par_complete_log"
  echo "rpb3-admission-cursor: CanonicalPath RawPar Complete macro proof did not complete" >&2
  exit 1
fi
if ! rg -q '^pub proof fn canonical_path_raw_par_complete_macro_preserves_source_and_score\(' \
  "$model_dir/rpb3_canonical_path_raw_par_complete_macro_refinement.rs"; then
  echo "rpb3-admission-cursor: missing CanonicalPath RawPar Complete macro proof" >&2
  exit 1
fi

canonical_path_rejected_nested_log="$evidence_dir/rpb3_canonical_path_rejected_nested_return_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --verify-root \
  "$model_dir/rpb3_canonical_path_rejected_nested_return_refinement.rs" \
  >"$canonical_path_rejected_nested_log" 2>&1; then
  cat "$canonical_path_rejected_nested_log"
  echo "rpb3-admission-cursor: CanonicalPath rejected nested return proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$canonical_path_rejected_nested_log"; then
  cat "$canonical_path_rejected_nested_log"
  echo "rpb3-admission-cursor: CanonicalPath rejected nested return proof did not complete" >&2
  exit 1
fi
for proof in rejected_nested_epm_return_applies_and_preserves \
  rejected_nested_epm_root_eof_is_error; do
  if ! rg -q "^pub proof fn ${proof}\\(" \
    "$model_dir/rpb3_canonical_path_rejected_nested_return_refinement.rs"; then
    echo "rpb3-admission-cursor: missing CanonicalPath rejected nested return proof $proof" >&2
    exit 1
  fi
done

canonical_path_log="$evidence_dir/rpb3_canonical_path_driver_source_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --cfg rpb3_canonical_path_standalone --verify-root \
  "$model_dir/rpb3_canonical_path_driver_source_refinement.rs" \
  >"$canonical_path_log" 2>&1; then
  cat "$canonical_path_log"
  echo "rpb3-admission-cursor: CanonicalPath source handoff proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$canonical_path_log"; then
  cat "$canonical_path_log"
  echo "rpb3-admission-cursor: CanonicalPath source handoff proof did not complete" >&2
  exit 1
fi
for proof in canonical_path_selected_child_is_exact canonical_path_observed_push_is_exact; do
  if ! rg -q "^pub proof fn ${proof}\\(" \
    "$model_dir/rpb3_canonical_path_driver_source_refinement.rs"; then
    echo "rpb3-admission-cursor: missing CanonicalPath source proof $proof" >&2
    exit 1
  fi
done

canonical_path_root_log="$evidence_dir/rpb3_canonical_path_root_source_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --cfg rpb3_canonical_path_root_standalone --verify-root \
  "$model_dir/rpb3_canonical_path_root_source_refinement.rs" \
  >"$canonical_path_root_log" 2>&1; then
  cat "$canonical_path_root_log"
  echo "rpb3-admission-cursor: CanonicalPath root source proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$canonical_path_root_log"; then
  cat "$canonical_path_root_log"
  echo "rpb3-admission-cursor: CanonicalPath root source proof did not complete" >&2
  exit 1
fi
for proof in canonical_path_root_obligations_balanced canonical_path_root_complete_is_exact; do
  if ! rg -q "^pub proof fn ${proof}\\(" \
    "$model_dir/rpb3_canonical_path_root_source_refinement.rs"; then
    echo "rpb3-admission-cursor: missing CanonicalPath root proof $proof" >&2
    exit 1
  fi
done

live_canonical_path_source="$model_dir/rpb3_live_canonical_path_source_refinement.rs"
live_canonical_path_proofs=(
  indexed_from_admission_is_exact
  live_canonical_path_root_is_wf
  live_canonical_path_push_preserves_ancestors
  live_canonical_path_push_preserves_wf
  live_canonical_path_return_preserves_ancestors
  live_canonical_path_return_preserves_wf
)
live_canonical_path_proof_count="$(rg -c '^pub proof fn ' "$live_canonical_path_source")"
if [[ "$live_canonical_path_proof_count" -ne "${#live_canonical_path_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: live CanonicalPath public proof inventory changed" >&2
  exit 1
fi
for proof in "${live_canonical_path_proofs[@]}"; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$live_canonical_path_source"; then
    echo "rpb3-admission-cursor: missing live CanonicalPath proof $proof" >&2
    exit 1
  fi
done
for proof in "${live_canonical_path_proofs[@]}"; do
  proof_log="$evidence_dir/live-canonical-path-wrapper-$proof.log"
  expected_count=1
  if [[ "$proof" == live_canonical_path_return_preserves_wf ]]; then
    expected_count=2
  fi
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_live_canonical_path_source_refinement \
    --verify-function "$proof" "$model_dir/rpb3_integrated_admission.rs" \
    >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: live CanonicalPath wrapper proof $proof failed" >&2
    exit 1
  fi
  if ! rg -q "^verification results:: ${expected_count} verified, 0 errors" "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: live CanonicalPath wrapper proof $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done

live_canonical_path_trace_source="$model_dir/rpb3_live_canonical_path_trace_refinement.rs"
for proof in canonical_descendant_preserves_retained_path \
  live_canonical_path_trace_step_preserves_wf \
  live_canonical_path_root_trace_exists \
  live_canonical_path_trace_prefix_preserves_wf; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$live_canonical_path_trace_source"; then
    echo "rpb3-admission-cursor: missing live CanonicalPath trace proof $proof" >&2
    exit 1
  fi
done
live_canonical_path_trace_log="$evidence_dir/live-canonical-path-trace-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_live_canonical_path_trace_refinement \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$live_canonical_path_trace_log" 2>&1; then
  cat "$live_canonical_path_trace_log"
  echo "rpb3-admission-cursor: live CanonicalPath trace module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 4 verified, 0 errors' "$live_canonical_path_trace_log"; then
  cat "$live_canonical_path_trace_log"
  echo "rpb3-admission-cursor: live CanonicalPath trace module did not verify" >&2
  exit 1
fi

live_canonical_path_descendant_source="$model_dir/rpb3_live_canonical_path_descendant_action_lift.rs"
for proof in canonical_steady_descendant_step_refines_retained_source \
  canonical_steady_descendant_step_preserves_trace_wf; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$live_canonical_path_descendant_source"; then
    echo "rpb3-admission-cursor: missing live CanonicalPath descendant action proof $proof" >&2
    exit 1
  fi
done
live_canonical_path_descendant_log="$evidence_dir/live-canonical-path-descendant-action-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated \
  --cfg rpb3_live_push \
  --verify-only-module rpb3_live_canonical_path_descendant_action_lift \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$live_canonical_path_descendant_log" 2>&1; then
  cat "$live_canonical_path_descendant_log"
  echo "rpb3-admission-cursor: live CanonicalPath descendant action module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$live_canonical_path_descendant_log"; then
  cat "$live_canonical_path_descendant_log"
  echo "rpb3-admission-cursor: live CanonicalPath descendant action module did not verify" >&2
  exit 1
fi

live_canonical_path_score_source="$model_dir/rpb3_live_canonical_path_score_action_lift.rs"
for proof in canonical_message_child_score_refines_retained_source \
  canonical_message_child_score_preserves_trace_wf \
  canonical_root_score_excluded_by_path_root; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$live_canonical_path_score_source"; then
    echo "rpb3-admission-cursor: missing live CanonicalPath score action proof $proof" >&2
    exit 1
  fi
done
live_canonical_path_score_log="$evidence_dir/live-canonical-path-score-action-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated \
  --cfg rpb3_live_push \
  --verify-only-module rpb3_live_canonical_path_score_action_lift \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$live_canonical_path_score_log" 2>&1; then
  cat "$live_canonical_path_score_log"
  echo "rpb3-admission-cursor: live CanonicalPath score action module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 3 verified, 0 errors' "$live_canonical_path_score_log"; then
  cat "$live_canonical_path_score_log"
  echo "rpb3-admission-cursor: live CanonicalPath score action module did not verify" >&2
  exit 1
fi

live_canonical_path_pop_source="$model_dir/rpb3_live_canonical_path_pop_action_lift.rs"
for proof in canonical_nested_pop_refines_retained_source \
  canonical_nested_pop_preserves_trace_wf; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$live_canonical_path_pop_source"; then
    echo "rpb3-admission-cursor: missing live CanonicalPath nested-pop action proof $proof" >&2
    exit 1
  fi
done
live_canonical_path_pop_log="$evidence_dir/live-canonical-path-pop-action-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated \
  --cfg rpb3_live_push \
  --verify-only-module rpb3_live_canonical_path_pop_action_lift \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$live_canonical_path_pop_log" 2>&1; then
  cat "$live_canonical_path_pop_log"
  echo "rpb3-admission-cursor: live CanonicalPath nested-pop action module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$live_canonical_path_pop_log"; then
  cat "$live_canonical_path_pop_log"
  echo "rpb3-admission-cursor: live CanonicalPath nested-pop action module did not verify" >&2
  exit 1
fi

live_canonical_path_map_finish_source="$model_dir/rpb3_live_canonical_path_map_finish_action_lift.rs"
for proof in canonical_map_finish_refines_retained_source \
  canonical_map_finish_preserves_trace_wf; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$live_canonical_path_map_finish_source"; then
    echo "rpb3-admission-cursor: missing live CanonicalPath Map finish proof $proof" >&2
    exit 1
  fi
done
live_canonical_path_map_finish_log="$evidence_dir/live-canonical-path-map-finish-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated \
  --cfg rpb3_live_push \
  --verify-only-module rpb3_live_canonical_path_map_finish_action_lift \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$live_canonical_path_map_finish_log" 2>&1; then
  cat "$live_canonical_path_map_finish_log"
  echo "rpb3-admission-cursor: live CanonicalPath Map finish module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$live_canonical_path_map_finish_log"; then
  cat "$live_canonical_path_map_finish_log"
  echo "rpb3-admission-cursor: live CanonicalPath Map finish module did not verify" >&2
  exit 1
fi

live_canonical_path_push_source="$model_dir/rpb3_live_canonical_path_push_action_lift.rs"
live_canonical_path_push_proofs=(
  canonical_message_push_plain_frame_shape
  canonical_message_push_indexed_frame_shape
  canonical_message_push_observed_frame_shape
  canonical_message_push_observed_child_exact
  canonical_map_value_push_observed_frame_shape
  canonical_map_value_push_observed_child_exact
  canonical_nested_push_observed_shape
  canonical_nested_push_retained_frames
  canonical_nested_push_retained_return_headers
  canonical_nested_push_refines_retained_source
  canonical_nested_push_preserves_trace_wf
)
for proof in "${live_canonical_path_push_proofs[@]}"; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$live_canonical_path_push_source"; then
    echo "rpb3-admission-cursor: missing live CanonicalPath descendant Push proof $proof" >&2
    exit 1
  fi
done
live_canonical_path_push_log="$evidence_dir/live-canonical-path-push-action-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 60 --cfg rpb3_integrated \
  --cfg rpb3_live_push \
  --verify-only-module rpb3_live_canonical_path_push_action_lift \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$live_canonical_path_push_log" 2>&1; then
  cat "$live_canonical_path_push_log"
  echo "rpb3-admission-cursor: live CanonicalPath descendant Push module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 11 verified, 0 errors' "$live_canonical_path_push_log"; then
  cat "$live_canonical_path_push_log"
  echo "rpb3-admission-cursor: live CanonicalPath descendant Push module did not verify" >&2
  exit 1
fi

live_canonical_path_terminal_source="$model_dir/rpb3_live_canonical_path_terminal_refinement.rs"
for proof in live_canonical_path_terminal_enables_root_source \
  live_canonical_path_terminal_complete_is_exact; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$live_canonical_path_terminal_source"; then
    echo "rpb3-admission-cursor: missing live CanonicalPath terminal proof $proof" >&2
    exit 1
  fi
done
live_canonical_path_terminal_log="$evidence_dir/live-canonical-path-terminal-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_live_canonical_path_terminal_refinement \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$live_canonical_path_terminal_log" 2>&1; then
  cat "$live_canonical_path_terminal_log"
  echo "rpb3-admission-cursor: live CanonicalPath terminal module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$live_canonical_path_terminal_log"; then
  cat "$live_canonical_path_terminal_log"
  echo "rpb3-admission-cursor: live CanonicalPath terminal module did not verify" >&2
  exit 1
fi

live_canonical_path_root_trace_source="$model_dir/rpb3_live_canonical_path_root_terminal_trace.rs"
for proof in canonical_path_trace_terminal_source_is_ready \
  canonical_path_root_trace_to_successful_terminal_is_exact; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$live_canonical_path_root_trace_source"; then
    echo "rpb3-admission-cursor: missing live CanonicalPath root-to-terminal trace proof $proof" >&2
    exit 1
  fi
done
live_canonical_path_root_trace_log="$evidence_dir/live-canonical-path-root-terminal-trace-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_live_canonical_path_root_terminal_trace \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$live_canonical_path_root_trace_log" 2>&1; then
  cat "$live_canonical_path_root_trace_log"
  echo "rpb3-admission-cursor: live CanonicalPath root-to-terminal trace module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$live_canonical_path_root_trace_log"; then
  cat "$live_canonical_path_root_trace_log"
  echo "rpb3-admission-cursor: live CanonicalPath root-to-terminal trace module did not verify" >&2
  exit 1
fi

canonical_path_terminal_driver_source="$model_dir/rpb3_canonical_path_terminal_driver_observation.rs"
for proof in canonical_path_terminal_driver_success_is_exact \
  canonical_path_terminal_driver_failure_has_no_complete \
  canonical_path_terminal_driver_stats_overflow_has_no_complete; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$canonical_path_terminal_driver_source"; then
    echo "rpb3-admission-cursor: missing CanonicalPath terminal driver observation proof $proof" >&2
    exit 1
  fi
done
canonical_path_terminal_driver_log="$evidence_dir/canonical-path-terminal-driver-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_canonical_path_terminal_driver_observation \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$canonical_path_terminal_driver_log" 2>&1; then
  cat "$canonical_path_terminal_driver_log"
  echo "rpb3-admission-cursor: CanonicalPath terminal driver observation module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 3 verified, 0 errors' "$canonical_path_terminal_driver_log"; then
  cat "$canonical_path_terminal_driver_log"
  echo "rpb3-admission-cursor: CanonicalPath terminal driver observation module did not verify" >&2
  exit 1
fi

canonical_path_root_complete_source="$model_dir/rpb3_canonical_path_root_complete_source_correspondence.rs"
for proof in canonical_path_root_complete_source_enables_driver \
  canonical_path_root_complete_source_corresponds \
  canonical_path_root_complete_source_rejects_mismatch; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$canonical_path_root_complete_source"; then
    echo "rpb3-admission-cursor: missing CanonicalPath root Complete source proof $proof" >&2
    exit 1
  fi
done
canonical_path_root_complete_log="$evidence_dir/canonical-path-root-complete-source-correspondence-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated \
  --cfg rpb3_live_push \
  --verify-only-module rpb3_canonical_path_root_complete_source_correspondence \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$canonical_path_root_complete_log" 2>&1; then
  cat "$canonical_path_root_complete_log"
  echo "rpb3-admission-cursor: CanonicalPath root Complete source correspondence failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 3 verified, 0 errors' "$canonical_path_root_complete_log"; then
  cat "$canonical_path_root_complete_log"
  echo "rpb3-admission-cursor: CanonicalPath root Complete source correspondence did not verify" >&2
  exit 1
fi

nested_epm_checked_source="$model_dir/rpb3_epm_nested_accept_checked_stats_refinement.rs"
for proof in nested_epm_accept_resolution_comes_from_child \
  nested_epm_accept_counter_overflow_has_no_effect \
  nested_epm_accept_checked_success_is_exact; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$nested_epm_checked_source"; then
    echo "rpb3-admission-cursor: missing nested EPathMap checked accept proof $proof" >&2
    exit 1
  fi
done
nested_epm_checked_log="$evidence_dir/epm-nested-accept-checked-stats-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_nested_accept_checked_stats_refinement \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$nested_epm_checked_log" 2>&1; then
  cat "$nested_epm_checked_log"
  echo "rpb3-admission-cursor: nested EPathMap checked accept source proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 3 verified, 0 errors' "$nested_epm_checked_log"; then
  cat "$nested_epm_checked_log"
  echo "rpb3-admission-cursor: nested EPathMap checked accept source proof did not verify" >&2
  exit 1
fi

map_key_source="$model_dir/rpb3_map_key_source_correspondence.rs"
for proof in map_key_utf8_scan_matches_admission \
  map_key_source_derives_continue; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$map_key_source"; then
    echo "rpb3-admission-cursor: missing MapKey source proof $proof" >&2
    exit 1
  fi
done
map_key_source_log="$evidence_dir/map-key-source-correspondence-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_map_key_source_correspondence \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$map_key_source_log" 2>&1; then
  cat "$map_key_source_log"
  echo "rpb3-admission-cursor: MapKey source correspondence failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 3 verified, 0 errors' "$map_key_source_log"; then
  cat "$map_key_source_log"
  echo "rpb3-admission-cursor: MapKey source correspondence did not verify" >&2
  exit 1
fi

map_key_full_source="$model_dir/rpb3_map_key_full_source_composition.rs"
for proof in map_key_full_source_selects_admission \
  map_key_full_source_refines_live_continue \
  map_key_source_enables_admission \
  map_key_full_source_enabled; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$map_key_full_source"; then
    echo "rpb3-admission-cursor: missing MapKey full-source proof $proof" >&2
    exit 1
  fi
done
map_key_full_source_log="$evidence_dir/map-key-full-source-composition-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_map_key_full_source_composition \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$map_key_full_source_log" 2>&1; then
  cat "$map_key_full_source_log"
  echo "rpb3-admission-cursor: MapKey full-source composition failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 4 verified, 0 errors' "$map_key_full_source_log"; then
  cat "$map_key_full_source_log"
  echo "rpb3-admission-cursor: MapKey full-source composition did not verify" >&2
  exit 1
fi

epm_terminal_source="$model_dir/rpb3_epathmap_terminal_source_observation.rs"
for proof in epm_terminal_source_success_enabled \
  epm_terminal_source_success_is_exact \
  epm_terminal_source_overflow_has_no_complete; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$epm_terminal_source"; then
    echo "rpb3-admission-cursor: missing EPathMap terminal source proof $proof" >&2
    exit 1
  fi
done
epm_terminal_source_log="$evidence_dir/epm-terminal-source-observation-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epathmap_terminal_source_observation \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_terminal_source_log" 2>&1; then
  cat "$epm_terminal_source_log"
  echo "rpb3-admission-cursor: EPathMap terminal source observation failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 4 verified, 0 errors' "$epm_terminal_source_log"; then
  cat "$epm_terminal_source_log"
  echo "rpb3-admission-cursor: EPathMap terminal source observation did not verify" >&2
  exit 1
fi

epm_push_source="$model_dir/rpb3_epm_push_source_action_refinement.rs"
if ! rg -q '^pub proof fn epm_push_source_enables_integrated_action\(' "$epm_push_source"; then
  echo "rpb3-admission-cursor: missing EPathMap Push source action proof" >&2
  exit 1
fi
epm_push_source_log="$evidence_dir/epm-push-source-action-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_push_source_action_refinement \
  --verify-function epm_push_source_enables_integrated_action \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_push_source_log" 2>&1; then
  cat "$epm_push_source_log"
  echo "rpb3-admission-cursor: EPathMap Push source action failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 1 verified, 0 errors' "$epm_push_source_log"; then
  cat "$epm_push_source_log"
  echo "rpb3-admission-cursor: EPathMap Push source action did not verify" >&2
  exit 1
fi

epm_push_transport="$model_dir/rpb3_epm_push_transport_source_refinement.rs"
for proof in epm_push_transport_prefix_is_cursor_exact \
  epm_push_batch_commit_matches_last_scalar_source \
  epm_push_batch_commit_preserves_source_anchor \
  epm_push_rust_request_preserves_selected_child \
  epm_push_returned_request_is_selected \
  epm_push_transport_terminal_is_source_selected; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$epm_push_transport"; then
    echo "rpb3-admission-cursor: missing EPathMap Push transport proof $proof" >&2
    exit 1
  fi
done
epm_push_transport_log="$evidence_dir/epm-push-transport-source-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_push_transport_source_refinement \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_push_transport_log" 2>&1; then
  cat "$epm_push_transport_log"
  echo "rpb3-admission-cursor: EPathMap Push transport source failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 11 verified, 0 errors' "$epm_push_transport_log"; then
  cat "$epm_push_transport_log"
  echo "rpb3-admission-cursor: EPathMap Push transport source did not verify" >&2
  exit 1
fi

epm_restore_run="$model_dir/rpb3_epm_restore_run_expansion.rs"
for proof in accepted_forward_publishes_exact_virtual_checkpoints \
  accepted_coalesced_forward_extends_virtual_checkpoints \
  accepted_unwind_selects_prefix_and_pops_exact_checkpoints; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$epm_restore_run"; then
    echo "rpb3-admission-cursor: missing EPathMap restore-run proof $proof" >&2
    exit 1
  fi
done
epm_restore_run_log="$evidence_dir/epm-restore-run-expansion-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_restore_run_expansion \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_restore_run_log" 2>&1; then
  cat "$epm_restore_run_log"
  echo "rpb3-admission-cursor: EPathMap restore-run expansion failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 3 verified, 0 errors' "$epm_restore_run_log"; then
  cat "$epm_restore_run_log"
  echo "rpb3-admission-cursor: EPathMap restore-run expansion did not verify" >&2
  exit 1
fi

canonical_path_persistent_source="$model_dir/rpb3_canonical_path_persistent_state_source.rs"
for proof in checkpoint_get_preserves_saved_state \
  saved_run_prefix_states_keep_identity \
  scalar_leave_selected_is_saved_predecessor \
  scalar_leave_predecessor_matches_batch_source_core \
  saved_payload_progress_changes_with_depth \
  saved_non_obligation_stays_absent \
  saved_awaiting_resolution_stays_zero \
  linked_fault_or_dead_retains_saved_stamp \
  linked_payload_prefix_witness \
  changed_current_state_non_obligation_witness; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$canonical_path_persistent_source"; then
    echo "rpb3-admission-cursor: missing canonical-path saved-state proof $proof" >&2
    exit 1
  fi
done
canonical_path_persistent_source_log="$evidence_dir/canonical-path-persistent-state-source-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_canonical_path_persistent_state_source \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$canonical_path_persistent_source_log" 2>&1; then
  cat "$canonical_path_persistent_source_log"
  echo "rpb3-admission-cursor: canonical-path saved-state source failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 11 verified, 0 errors' "$canonical_path_persistent_source_log"; then
  cat "$canonical_path_persistent_source_log"
  echo "rpb3-admission-cursor: canonical-path saved-state source did not verify" >&2
  exit 1
fi

canonical_path_historical_source="$model_dir/rpb3_canonical_path_historical_checkpoint_source.rs"
for proof in historical_predecessors_select_scalar_leaves \
  positive_first_payload_end_is_restored_prefix_anchor \
  changed_current_token_two_leave_witness; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$canonical_path_historical_source"; then
    echo "rpb3-admission-cursor: missing canonical-path historical checkpoint proof $proof" >&2
    exit 1
  fi
done
canonical_path_historical_source_log="$evidence_dir/canonical-path-historical-checkpoint-source-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_canonical_path_historical_checkpoint_source \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$canonical_path_historical_source_log" 2>&1; then
  cat "$canonical_path_historical_source_log"
  echo "rpb3-admission-cursor: canonical-path historical checkpoint source failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 3 verified, 0 errors' "$canonical_path_historical_source_log"; then
  cat "$canonical_path_historical_source_log"
  echo "rpb3-admission-cursor: canonical-path historical checkpoint source did not verify" >&2
  exit 1
fi

canonical_path_forward_capacity_source="$model_dir/rpb3_canonical_path_forward_capacity_source.rs"
for proof in positive_forward_capacity_bounds \
  accepted_width_derives_forward_capacity_facts \
  three_byte_capacity_witness; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$canonical_path_forward_capacity_source"; then
    echo "rpb3-admission-cursor: missing canonical-path forward capacity proof $proof" >&2
    exit 1
  fi
done
canonical_path_forward_capacity_source_log="$evidence_dir/canonical-path-forward-capacity-source-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_canonical_path_forward_capacity_source \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$canonical_path_forward_capacity_source_log" 2>&1; then
  cat "$canonical_path_forward_capacity_source_log"
  echo "rpb3-admission-cursor: canonical-path forward capacity source failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 3 verified, 0 errors' "$canonical_path_forward_capacity_source_log"; then
  cat "$canonical_path_forward_capacity_source_log"
  echo "rpb3-admission-cursor: canonical-path forward capacity source did not verify" >&2
  exit 1
fi

canonical_path_owner_health_source="$model_dir/rpb3_canonical_path_owner_health_source.rs"
for proof in captured_owner_entry_facts \
  old_captured_owner_entry_survives_push \
  new_captured_owner_entry_is_well_formed \
  append_captured_open_owner_preserves_store \
  focus_step_preserves_captured_owner_health \
  owner_source_step_preserves_health \
  reachable_captured_owners_remain_open \
  reachable_positive_capacity_has_open_owner_root \
  accepted_forward_facts_from_reachable_owner_trace \
  nested_owner_dead_child_witness; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$canonical_path_owner_health_source"; then
    echo "rpb3-admission-cursor: missing canonical-path owner health proof $proof" >&2
    exit 1
  fi
done
canonical_path_owner_health_source_log="$evidence_dir/canonical-path-owner-health-source-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_canonical_path_owner_health_source \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$canonical_path_owner_health_source_log" 2>&1; then
  cat "$canonical_path_owner_health_source_log"
  echo "rpb3-admission-cursor: canonical-path owner health source failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 13 verified, 0 errors' "$canonical_path_owner_health_source_log"; then
  cat "$canonical_path_owner_health_source_log"
  echo "rpb3-admission-cursor: canonical-path owner health source did not verify" >&2
  exit 1
fi

epm_batch_composition="$model_dir/rpb3_epm_batch_cursor_composition.rs"
for proof in accepted_forward_batch_composes_cursor_and_restore_run \
  accepted_coalesced_forward_batch_composes_cursor_and_restore_run \
  payload_run_has_historical_checkpoint_projection \
  accepted_unwind_batch_one_scalar_restore_step \
  accepted_unwind_batch_has_scalar_restore_steps \
  accepted_unwind_batch_composes_cursor_and_restore_run \
  unwind_witness_historical_run_wf \
  unwind_witness_validated_plan_matches \
  unwind_witness_encoded_prefix \
  unwind_witness_encoded_act_plan \
  unwind_witness_expanded_act_events \
  unwind_witness_concrete_act_layout \
  unwind_witness_act_layout \
  accepted_unwind_has_no_current_pending_witness \
  accepted_unwind_has_changed_checkpoint_witness; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$epm_batch_composition"; then
    echo "rpb3-admission-cursor: missing EPathMap batch composition proof $proof" >&2
    exit 1
  fi
done
epm_batch_composition_log="$evidence_dir/epm-batch-cursor-composition-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_batch_cursor_composition \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_batch_composition_log" 2>&1; then
  cat "$epm_batch_composition_log"
  echo "rpb3-admission-cursor: EPathMap batch composition failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 16 verified, 0 errors' "$epm_batch_composition_log"; then
  cat "$epm_batch_composition_log"
  echo "rpb3-admission-cursor: EPathMap batch composition did not verify" >&2
  exit 1
fi

epm_batch_cursor_source="$model_dir/rpb3_epm_batch_cursor_source_extraction.rs"
for proof in forward_line_work_has_scalar_event \
  unwind_line_work_has_scalar_event \
  expanded_emit_prefix_at \
  attachment_preserves_event_count \
  attachment_preserves_batch_wire_at \
  attached_map_value_at_prefix \
  two_leaf_source_validation_has_exact_structure \
  selected_map_value_attachment_binds_body \
  selected_map_value_projects_validated_record \
  selected_map_value_source_is_unique \
  accepted_indexed_line_step_is_plan_event \
  accepted_indexed_line_batch_has_exact_cursor_range \
  accepted_act_source_count_equals_expansion; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$epm_batch_cursor_source"; then
    echo "rpb3-admission-cursor: missing EPathMap batch cursor source proof $proof" >&2
    exit 1
  fi
done
epm_batch_cursor_source_log="$evidence_dir/epm-batch-cursor-source-extraction-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_batch_cursor_source_extraction \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_batch_cursor_source_log" 2>&1; then
  cat "$epm_batch_cursor_source_log"
  echo "rpb3-admission-cursor: EPathMap batch cursor source failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 43 verified, 0 errors' "$epm_batch_cursor_source_log"; then
  cat "$epm_batch_cursor_source_log"
  echo "rpb3-admission-cursor: EPathMap batch cursor source did not verify" >&2
  exit 1
fi

epm_active_line_work_source="$model_dir/rpb3_epm_active_line_work_source.rs"
for proof in accepted_width_fits_event_headroom \
  prepared_line_derives_scalar_schedule_guard \
  prepared_line_step_has_bounded_source_and_event_index \
  overflow_limited_prefix_witness; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$epm_active_line_work_source"; then
    echo "rpb3-admission-cursor: missing EPathMap active-line source proof $proof" >&2
    exit 1
  fi
done
epm_active_line_work_source_log="$evidence_dir/epm-active-line-work-source-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_active_line_work_source \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_active_line_work_source_log" 2>&1; then
  cat "$epm_active_line_work_source_log"
  echo "rpb3-admission-cursor: EPathMap active-line source failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 4 verified, 0 errors' "$epm_active_line_work_source_log"; then
  cat "$epm_active_line_work_source_log"
  echo "rpb3-admission-cursor: EPathMap active-line source did not verify" >&2
  exit 1
fi

epm_unwind_event_lift="$model_dir/rpb3_epm_unwind_event_lift.rs"
for proof in unwind_virtual_frame_has_layout \
  accepted_unwind_one_full_epm_event \
  accepted_unwind_full_epm_event_sequence \
  linked_unwind_virtual_state_is_source_wf \
  linked_unwind_admission_update_is_idempotent \
  accepted_unwind_one_linked_continue \
  accepted_unwind_linked_continue_sequence; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$epm_unwind_event_lift"; then
    echo "rpb3-admission-cursor: missing EPathMap Unwind event lift proof $proof" >&2
    exit 1
  fi
done
epm_unwind_event_lift_log="$evidence_dir/epm-unwind-event-lift-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_unwind_event_lift \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_unwind_event_lift_log" 2>&1; then
  cat "$epm_unwind_event_lift_log"
  echo "rpb3-admission-cursor: EPathMap Unwind event lift failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 7 verified, 0 errors' "$epm_unwind_event_lift_log"; then
  cat "$epm_unwind_event_lift_log"
  echo "rpb3-admission-cursor: EPathMap Unwind event lift did not verify" >&2
  exit 1
fi

epm_unwind_product_lift="$model_dir/rpb3_epm_unwind_product_lift.rs"
for proof in unwind_product_virtual_matches_linked_source \
  accepted_unwind_one_product_internal_continue \
  accepted_unwind_product_one_step_preserves_wf \
  accepted_unwind_product_virtual_wf \
  accepted_unwind_product_internal_sequence \
  live_unwind_virtual_state_wf \
  accepted_unwind_one_live_internal_step \
  accepted_unwind_live_internal_sequence \
  live_runtime_internal_epm_continue_preserves_wf \
  live_runtime_unwind_virtual_state_wf \
  accepted_unwind_live_runtime_internal_sequence; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$epm_unwind_product_lift"; then
    echo "rpb3-admission-cursor: missing EPathMap Unwind product lift proof $proof" >&2
    exit 1
  fi
done
epm_unwind_product_lift_log="$evidence_dir/epm-unwind-product-lift-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_unwind_product_lift \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_unwind_product_lift_log" 2>&1; then
  cat "$epm_unwind_product_lift_log"
  echo "rpb3-admission-cursor: EPathMap Unwind product lift failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 12 verified, 0 errors' "$epm_unwind_product_lift_log"; then
  cat "$epm_unwind_product_lift_log"
  echo "rpb3-admission-cursor: EPathMap Unwind product lift did not verify" >&2
  exit 1
fi

epm_push_live="$model_dir/rpb3_epm_push_live_action_refinement.rs"
for proof in epm_push_live_action_is_checked epm_push_live_action_dispatches; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$epm_push_live"; then
    echo "rpb3-admission-cursor: missing live EPathMap Push action proof $proof" >&2
    exit 1
  fi
done
epm_push_live_log="$evidence_dir/epm-push-live-action-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 \
  --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_push_live_action_refinement \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_push_live_log" 2>&1; then
  cat "$epm_push_live_log"
  echo "rpb3-admission-cursor: live EPathMap Push action failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 2 verified, 0 errors' "$epm_push_live_log"; then
  cat "$epm_push_live_log"
  echo "rpb3-admission-cursor: live EPathMap Push action did not verify" >&2
  exit 1
fi

epm_loop_continuation="$model_dir/rpb3_epm_loop_continuation_source.rs"
for proof in forward_scalar_prefix_preserves_image \
  loop_internal_tick_preserves_image \
  finite_loop_prefix_preserves_image \
  finite_loop_selects_push_or_complete \
  finite_loop_push_dispatches_exact_image \
  finite_loop_complete_dispatches_exact_image; do
  if ! rg -q "^(pub )?proof fn ${proof}\\(" "$epm_loop_continuation"; then
    echo "rpb3-admission-cursor: missing EPathMap loop continuation proof $proof" >&2
    exit 1
  fi
done
epm_loop_continuation_log="$evidence_dir/epm-loop-continuation-source-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 \
  --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_loop_continuation_source \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_loop_continuation_log" 2>&1; then
  cat "$epm_loop_continuation_log"
  echo "rpb3-admission-cursor: EPathMap loop continuation source proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 6 verified, 0 errors' "$epm_loop_continuation_log"; then
  cat "$epm_loop_continuation_log"
  echo "rpb3-admission-cursor: EPathMap loop continuation source did not verify" >&2
  exit 1
fi

epm_loop_observation_bridge="$model_dir/rpb3_epm_loop_observation_bridge.rs"
for proof in checked_scalar_value_source_has_exact_stability \
  checked_push_value_source_binds_selected_body \
  observed_top_run_selects_virtual_restore_suffix \
  nonempty_restore_history_source_has_witness \
  scalar_edge_without_bulk_window_has_source_witness \
  scalar_edge_source_shape_refines_reducer_path_length \
  scalar_leave_source_shape_refines_reducer_path_length \
  scalar_value_source_shape_preserves_reducer_path_summary \
  checked_loop_successor_has_exact_cursor_delta \
  checked_scalar_leave_source_has_witness \
  scalar_reducer_projection_yields_internal_live_step \
  scalar_reducer_projection_is_live_action \
  checked_scalar_source_yields_loop_tick \
  checked_forward_source_yields_loop_tick \
  selected_unwind_run_excludes_another_restoration \
  checked_unwind_source_yields_loop_tick \
  checked_push_source_yields_stop_readiness \
  checked_complete_source_yields_stop_readiness; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$epm_loop_observation_bridge"; then
    echo "rpb3-admission-cursor: missing EPathMap loop observation bridge proof $proof" >&2
    exit 1
  fi
done
epm_loop_observation_bridge_log="$evidence_dir/epm-loop-observation-bridge-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 \
  --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_loop_observation_bridge \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_loop_observation_bridge_log" 2>&1; then
  cat "$epm_loop_observation_bridge_log"
  echo "rpb3-admission-cursor: EPathMap loop observation bridge failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 18 verified, 0 errors' "$epm_loop_observation_bridge_log"; then
  cat "$epm_loop_observation_bridge_log"
  echo "rpb3-admission-cursor: EPathMap loop observation bridge did not verify" >&2
  exit 1
fi

epm_terminal_nested_source="$model_dir/rpb3_epathmap_terminal_nested_return_composition.rs"
for proof in terminal_nested_source_enabled terminal_nested_source_corresponds \
  terminal_nested_end_is_live_action; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$epm_terminal_nested_source"; then
    echo "rpb3-admission-cursor: missing EPathMap terminal nested return proof $proof" >&2
    exit 1
  fi
done
epm_terminal_nested_log="$evidence_dir/epm-terminal-nested-return-composition-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated \
  --cfg rpb3_live_push \
  --verify-only-module rpb3_epathmap_terminal_nested_return_composition \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_terminal_nested_log" 2>&1; then
  cat "$epm_terminal_nested_log"
  echo "rpb3-admission-cursor: EPathMap terminal nested return composition failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 3 verified, 0 errors' "$epm_terminal_nested_log"; then
  cat "$epm_terminal_nested_log"
  echo "rpb3-admission-cursor: EPathMap terminal nested return composition did not verify" >&2
  exit 1
fi

epm_end_source_selector="$model_dir/rpb3_epathmap_end_source_selector.rs"
if ! rg -q '^pub proof fn epm_end_source_selects_live_action\(' \
  "$epm_end_source_selector"; then
  echo "rpb3-admission-cursor: missing EPathMap End source selector proof" >&2
  exit 1
fi
epm_end_source_selector_log="$evidence_dir/epm-end-source-selector-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated \
  --cfg rpb3_live_push \
  --verify-only-module rpb3_epathmap_end_source_selector \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$epm_end_source_selector_log" 2>&1; then
  cat "$epm_end_source_selector_log"
  echo "rpb3-admission-cursor: EPathMap End source selector failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 1 verified, 0 errors' \
  "$epm_end_source_selector_log"; then
  cat "$epm_end_source_selector_log"
  echo "rpb3-admission-cursor: EPathMap End source selector did not verify" >&2
  exit 1
fi

canonical_path_owner_source="$model_dir/rpb3_canonical_path_eof_owner_obligation_refinement.rs"
for proof in focus_captures_from_paths_retain_stack \
  settle_focus_owner_vector_unchanged \
  select_focus_retained_prefix_preserves_stack \
  select_focus_then_capture_preserves_owner_stack \
  select_focus_then_source_captures_preserves_owner_stack \
  focused_owner_capture_retains_stack \
  focused_owner_root_eof_cannot_succeed \
  successful_root_eof_value_has_no_active_obligation; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$canonical_path_owner_source"; then
    echo "rpb3-admission-cursor: missing CanonicalPath EOF owner/obligation proof $proof" >&2
    exit 1
  fi
done
canonical_path_owner_log="$evidence_dir/canonical-path-eof-owner-obligation-root.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --verify-root \
  "$canonical_path_owner_source" \
  >"$canonical_path_owner_log" 2>&1; then
  cat "$canonical_path_owner_log"
  echo "rpb3-admission-cursor: CanonicalPath EOF owner/obligation model failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 8 verified, 0 errors' "$canonical_path_owner_log"; then
  cat "$canonical_path_owner_log"
  echo "rpb3-admission-cursor: CanonicalPath EOF owner/obligation model did not verify" >&2
  exit 1
fi

canonical_path_epm_return_log="$evidence_dir/rpb3_canonical_path_epm_return_producer.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --verify-root --verify-function canonical_path_epm_completion_derives_return_summary \
  "$model_dir/rpb3_canonical_path_epm_return_producer.rs" \
  >"$canonical_path_epm_return_log" 2>&1; then
  cat "$canonical_path_epm_return_log"
  echo "rpb3-admission-cursor: CanonicalPath EPathMap return producer proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 1 verified, 0 errors' "$canonical_path_epm_return_log"; then
  cat "$canonical_path_epm_return_log"
  echo "rpb3-admission-cursor: CanonicalPath EPathMap return producer proof did not complete" >&2
  exit 1
fi
if ! rg -q '^pub proof fn canonical_path_epm_completion_derives_return_summary\(' \
  "$model_dir/rpb3_canonical_path_epm_return_producer.rs"; then
  echo "rpb3-admission-cursor: missing CanonicalPath EPathMap return producer proof" >&2
  exit 1
fi

score_replay_log="$evidence_dir/rpb3_epm_score_replay_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --cfg rpb3_integrated --verify-root \
  "$model_dir/rpb3_epm_score_replay_refinement.rs" \
  >"$score_replay_log" 2>&1; then
  cat "$score_replay_log"
  echo "rpb3-admission-cursor: EPM score replay proof failed" >&2
  exit 1
fi
if ! rg -q 'verification results:: [1-9][0-9]* verified, 0 errors' "$score_replay_log"; then
  cat "$score_replay_log"
  echo "rpb3-admission-cursor: EPM score replay proof did not complete" >&2
  exit 1
fi

ezipper_source_log="$evidence_dir/rpb3_ezipper_source_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --verify-root \
  "$model_dir/rpb3_ezipper_source_refinement.rs" \
  >"$ezipper_source_log" 2>&1; then
  cat "$ezipper_source_log"
  echo "rpb3-admission-cursor: EZipper source refinement proof failed" >&2
  exit 1
fi
if ! rg -q 'verification results:: [1-9][0-9]* verified, 0 errors' "$ezipper_source_log"; then
  cat "$ezipper_source_log"
  echo "rpb3-admission-cursor: EZipper source refinement proof did not complete" >&2
  exit 1
fi
for proof in ezipper_absent_modeled_pop_is_source_bound \
  ezipper_modeled_complete_route_is_reachable \
  ezipper_modeled_route_rejects_fabricated_summaries \
  ezipper_receipt_uses_indexed_snapshot \
  ezipper_missing_pathmap_has_no_complete_score \
  ezipper_scored_constructor_transcript_exact \
  ezipper_complete_route_preserves_source_and_score; do
  if ! rg -q "^pub proof fn ${proof}\\(" \
    "$model_dir/rpb3_ezipper_source_refinement.rs"; then
    echo "rpb3-admission-cursor: missing EZipper source proof $proof" >&2
    exit 1
  fi
done

integrated_core_proofs=(
  map_root_initial_exact unchanged_admission_arena_preserves_exact
  product_indexed_child_source_anchor_exact
  product_wire_indexed_field_anchor_exact
  product_frame_source_anchors_root product_frame_source_anchors_push
  product_frame_source_anchors_pop
  product_map_indexed_push_anchor_exact
  product_map_indexed_continue_anchor_exact
  product_map_value_indexed_push_anchor_exact
  integrated_source_step_preserves_exact map_root_trace_prefix_exact
  integrated_reachable_exact product_score_wf_intro
  product_epm_receipts_carried_preserves
  product_epm_finish_receipt_preserves product_epm_source_receipt_binds_root_eof
  product_epm_source_receipt_binds_top_eof
  product_epm_source_witness_lifts_accept
  product_epm_modeled_empty_source_accepts
  product_epm_modeled_empty_nested_source_accepts
  product_epm_modeled_empty_nested_source_lifts_accept
  product_epm_modeled_empty_nested_scores
  product_epm_modeled_empty_nested_complete_ready
  product_epm_modeled_empty_nested_complete_poststate
  product_epm_modeled_empty_snapshot_score_length
  product_epm_empty_snapshot_selected_nonempty
  product_epm_modeled_empty_source_scores
  live_runtime_epm_source_eof_dispatch_exact
  live_runtime_product_epm_end_source_preserves_wf
  live_source_product_epm_finish_accept_preserves_wf
  live_runtime_product_epm_finish_accept_preserves_wf
  live_epm_modeled_empty_nested_cursor_ready
  live_epm_modeled_empty_nested_source_before_wf
  live_epm_modeled_empty_nested_before_wf
  live_epm_modeled_empty_nested_accept_step_exact
  live_epm_modeled_empty_nested_after_wf
  live_epm_modeled_empty_nested_after_cursor_end
  live_epm_modeled_empty_nested_accept_witness
  live_epm_modeled_empty_nested_eof_accepts
  live_epm_modeled_empty_nested_poststate_selected
  live_epm_modeled_empty_nested_poststate_wf
  runtime_epm_source_child_selected_complete
  live_runtime_epm_source_child_eof_accepts
  product_epm_source_bridge_finish_selected_exact
  product_epm_source_root_score_selected_exact
  product_epm_source_child_score_selected_exact
  product_epm_child_return_source_exact product_root_initial_exact
  product_map_push_preserves_exact product_map_pop_preserves_exact
  product_map_steady_preserves_exact product_map_step_preserves_exact
  product_epm_handoff_preserves_exact product_epm_continue_preserves_exact
  product_root_score_shape_preserves_exact product_root_score_step_preserves_exact
  product_epm_push_preserves_map_wf product_epm_push_preserves_indexed
  product_epm_push_preserves_source_anchors
  product_epm_push_preserves_exact product_epm_score_preserves_map_wf
  product_epm_score_preserves_exact product_message_child_score_preserves_exact
  product_epm_child_return_preserves_map_wf
  product_epm_child_return_preserves_indexed product_epm_child_return_preserves_exact
  product_epm_complete_preserves_exact product_epm_finish_accept_preserves_exact
  live_runtime_product_step_preserves_ancestor_prefix
  product_source_step_preserves_exact product_root_trace_prefix_exact
  live_source_arena_entries_agree live_source_product_continue_field_is_read
  live_runtime_epm_source_score_selected_matches_product
  product_runtime_epm_source_score_selected_matches_product
  product_runtime_arena_from_score_projects
  product_epm_source_child_complete_selected_exact
  live_runtime_epm_source_child_complete_selected_exact
  live_source_product_epm_child_complete_preserves_wf
  live_runtime_epm_child_complete_runtime_wf
  live_runtime_epm_child_poststate_score_agrees
  live_runtime_epm_source_child_complete_preserves_wf
  live_source_product_continue_preserves_wf
  live_source_product_continue_indexed_preserves_wf
  live_source_product_map_key_preserves_wf
  live_runtime_product_continue_preserves_wf
  live_runtime_product_continue_indexed_preserves_wf
  live_runtime_product_map_key_preserves_wf
  live_runtime_product_map_key_uses_read_field
  live_runtime_product_continue_uses_read_field
  live_runtime_product_continue_indexed_uses_read_field
  live_runtime_product_continue_indexed_anchor_exact
  live_runtime_product_root_end_is_ready
)
integrated_push_proofs=(
  live_runtime_child_cursor_is_payload
  live_runtime_parent_cursor_after_field_projects
  runtime_map_value_push_image_is_bounded
  live_runtime_product_map_value_push_preserves_wf
  live_runtime_product_map_value_push_indexed_preserves_wf
  live_runtime_product_map_value_push_indexed_source_ids_match
  live_runtime_product_map_value_push_observed_is_exact
  live_runtime_product_map_value_accept_preserves_wf
  live_runtime_product_map_entry_finish_source_shape
  live_source_product_map_entry_finish_preserves_wf
  live_runtime_product_map_entry_finish_preserves_wf
  live_source_product_message_child_complete_preserves_wf
  live_runtime_product_message_child_complete_preserves_wf
  live_runtime_product_message_child_score_preserves_wf
  live_runtime_product_message_child_score_enables_runtime_gate
  live_runtime_product_message_child_complete_runtime_exact
  live_runtime_product_message_child_eof_driver_exact
  live_source_product_epm_map_value_accept_preserves_wf
  live_runtime_product_epm_map_value_accept_preserves_wf
  live_runtime_product_message_push_is_checked
  live_runtime_product_message_push_components_wf
  live_runtime_product_message_push_score_agrees
  live_runtime_product_message_push_arena_agrees
  live_runtime_product_message_push_preserves_wf
  live_runtime_product_message_push_observed_is_exact
  live_runtime_product_message_push_indexed_is_checked
  live_runtime_product_message_push_indexed_components_wf
  live_runtime_product_message_push_indexed_preserves_wf
  live_runtime_product_message_push_indexed_observed_is_exact
  checked_message_replay_step_preserves_full_core
  checked_message_push_snapshot_and_core_establish_live_fold
  live_runtime_product_message_epm_push_indexed_preserves_wf
  live_runtime_product_message_epm_push_indexed_observed_is_exact
  product_wire_indexed_child_source_ids_exact
  live_runtime_product_message_push_indexed_source_ids_match
  live_runtime_product_root_score_preserves_wf
  live_runtime_product_step_preserves_wf
  live_runtime_product_singleton_trace
  live_runtime_product_trace_prefix_wf
)
integrated_witness_proofs=(
  live_runtime_empty_send_initial_is_wf
  live_runtime_empty_send_root_end_is_ready
  live_runtime_empty_send_source_finish_is_reachable
  live_runtime_empty_send_bridge_source_is_exact
  live_runtime_empty_send_product_root_step_is_exact
  live_runtime_empty_send_root_score_is_reachable
  live_runtime_empty_send_root_score_trace_is_exact
  live_runtime_empty_send_root_score_live_step_is_exact
  live_runtime_empty_send_root_score_live_after_is_wf
  live_runtime_empty_send_root_score_live_trace_is_exact
  live_runtime_send_channel_initial_is_wf
  live_runtime_send_channel_child_is_valid
  live_runtime_send_channel_push_is_possible
  live_runtime_send_channel_push_reaches_live_state
  live_runtime_send_channel_child_end_is_exact
  live_runtime_discontinuous_send_initial_is_wf
  live_runtime_discontinuous_send_admission_push_is_possible
  live_runtime_discontinuous_send_wire_field_is_read
  live_runtime_discontinuous_send_indexed_push_reaches_live_state
  live_runtime_discontinuous_send_indexed_observed_push_is_reachable
)
integrated_private_proofs=(
  product_epm_modeled_empty_nested_scored_child_shape
  product_epm_modeled_empty_nested_derived_summary
  product_epm_modeled_empty_nested_complete_enabled
  product_epm_modeled_empty_nested_admission_complete_dispatch
  product_epm_modeled_empty_nested_map_complete_dispatch
  product_epm_modeled_empty_nested_product_complete_dispatch
)
integrated_epm_score_private_proofs=(
  product_epm_score_indexed_frames_preserved
  product_epm_score_source_anchors_preserved
)
integrated_message_child_score_private_proofs=(
  product_message_child_score_frame_image_preserved
  product_message_child_score_product_wf
  product_message_child_score_preserves_ancestor_prefix
)
integrated_message_child_complete_private_proofs=(
  live_runtime_message_child_complete_map_dispatch
  live_runtime_message_child_complete_pre_pop_exact
)
integrated_path_private_proofs=(
  live_runtime_retained_path_step_preserves_sync
)
integrated_source="$model_dir/rpb3_integrated_admission.rs"
source_observation="$model_dir/rpb3_source_observation.rs"
source_observation_proofs=(
  map_eof_live_snapshot_source_bound
  map_eof_live_finish_has_checked_receipt
  map_eof_live_snapshot_rejects_cached_error
  message_eof_send_finish_from_source_summary_exact
  message_eof_unattached_append_count
  message_eof_constructor_append_count
  message_eof_send_finish_from_source_append_count_exact
  message_eof_send_finish_from_source_is_exact
  message_eof_send_observation_from_fold_matches_before
  message_eof_send_finish_from_fold_selected_exact
  message_eof_primary_component_finish_from_source_summary_exact
  message_eof_primary_component_finish_from_source_append_count_exact
  message_eof_primary_component_finish_from_source_is_exact
  message_eof_primary_component_observation_from_fold_matches_before
  message_eof_primary_component_finish_from_fold_selected_exact
  message_child_complete_source_match_components
  message_child_complete_source_cursor_facts
  message_child_complete_source_frame_facts
  message_child_complete_source_wire_end_facts
  message_child_complete_source_facts
  live_runtime_message_child_complete_source_dispatch
  live_runtime_message_child_complete_source_receipts
  live_runtime_message_child_complete_source_map_product
  live_runtime_message_child_complete_source_wire_indexed
  live_runtime_message_child_complete_source_live_step
  live_runtime_message_child_complete_source_post_shape
  live_runtime_message_child_complete_from_source_is_exact
)
map_eof_indexed_view_source="$model_dir/rpb3_map_eof_indexed_view_identity.rs"
map_eof_indexed_view_proofs=(
  symbolic_same_view_binds_eof_receipt_images
  live_wire_indexed_view_binds_eof_receipt_image
  distinct_borrowed_handles_forge_equal_indexed_image
  live_map_indexed_origin_binds_eof_receipt
  indexed_map_push_establishes_eof_source_origin
  valid_plain_map_eof_has_no_indexed_source_image
  product_map_step_preserves_indexed_origin_at
)
map_push_receipt_source="$model_dir/rpb3_map_push_receipt_source_extraction.rs"
map_push_receipt_proofs=(
  checked_map_push_receipt_projects_body_and_anchors
  checked_map_push_receipt_reconciles_live_parent
  checked_map_push_view_and_fresh_child_select_indexed_action
  equal_map_push_images_do_not_prove_same_view
)
map_push_backing_source="$model_dir/rpb3_map_push_backing_identity.rs"
map_push_backing_proofs=(
  exact_indexed_same_view_is_key_equality
  checked_map_push_exact_views_project_symbolic_identity
  equal_image_distinct_borrowed_handles_fail_same_view
  matching_root_with_changed_end_fails_same_view
  canonical_empty_composed_roots_have_same_view
)
map_push_scoped_classes_source="$model_dir/rpb3_map_push_scoped_identity_classes.rs"
map_push_scoped_classes_proofs=(
  scoped_map_push_identity_classes_are_injective
  checked_map_push_scoped_identity_selects_symbolic_views
  checked_map_push_scoped_identity_selects_indexed_action
  equal_image_distinct_backing_has_distinct_scoped_class
)
map_push_finite_observation_source="$model_dir/rpb3_map_push_finite_observation.rs"
map_push_finite_observation_proofs=(
  checked_map_push_finite_observation_selects_symbolic_views
  checked_map_push_finite_observation_selects_indexed_action
  equal_map_push_images_with_different_classes_are_rejected
)
map_push_selected_subview_source="$model_dir/rpb3_map_push_selected_subview_extraction.rs"
map_push_selected_subview_proofs=(
  checked_map_push_selected_source_reconstructs_subview
  checked_map_push_selected_source_selects_indexed_action
  changed_terminal_anchor_does_not_reconstruct_selected_subview
)
map_push_live_parent_bounds_source="$model_dir/rpb3_map_push_live_parent_bounds.rs"
map_push_live_parent_bounds_proofs=(
  checked_map_push_live_parent_has_full_body_bounds
  checked_map_push_source_bounds_select_indexed_action
  narrowed_message_window_disagrees_with_concrete_cursor
)
map_push_fresh_child_source="$model_dir/rpb3_map_push_fresh_child_extraction.rs"
map_push_fresh_child_proofs=(
  checked_map_push_source_core_and_defaults_make_fresh_child
  checked_map_push_source_core_selects_indexed_action
  forged_body_id_preserves_source_core_but_not_freshness
)
map_push_live_child_source="$model_dir/rpb3_map_push_live_child_projection.rs"
map_push_live_child_proofs=(
  checked_map_push_observed_child_projects_full_fresh_state
  checked_map_push_observed_child_selects_indexed_action
  pending_value_forgery_preserves_bytes_but_is_not_initial
)
map_push_checked_receipt_source="$model_dir/rpb3_map_push_checked_receipt_projection.rs"
map_push_checked_receipt_proofs=(
  checked_map_push_live_cursor_rejects_forged_field
  checked_map_push_live_cursor_discards_forged_template_field
  checked_map_push_live_cursor_selects_indexed_field
  checked_map_push_receipt_child_is_observed_projection
  checked_map_push_constructed_receipt_selects_indexed_action
  source_bound_receipt_discards_forged_template_body_id
  concrete_new_map_push_bytes_facts
  concrete_new_map_push_varints
  concrete_new_map_push_field_step
  concrete_new_map_root_initial_exact
  concrete_new_map_wire_cursor_projects
  concrete_new_map_runtime_field_exact
  concrete_new_map_live_before_wf
  concrete_new_map_root_and_wire_exact
  concrete_new_map_push_field_at_cursor
  concrete_new_map_fresh_child_exact
  concrete_new_map_receipt_exact
  concrete_new_map_live_before_exact
  concrete_new_map_fold_matches
  concrete_new_map_push_receipt_matches_fold
  concrete_new_map_push_source_step_exact
  concrete_new_map_receipt_and_fold_exact
  concrete_new_map_admission_push_step_exact
  concrete_new_map_source_image_facts
  concrete_new_map_source_action_exact
  concrete_new_map_message_driver_exact
  concrete_new_map_child_source_exact
  concrete_new_map_admission_next_frames
  concrete_new_map_integrated_next_wf
  concrete_new_map_integrated_push_exact
  concrete_new_map_storage_observation_exact
  concrete_new_map_runtime_push_exact
  concrete_new_map_parent_transition_exact
  concrete_new_map_push_guards
  concrete_new_map_product_map_outcome_exact
  concrete_new_map_checked_push_exists
  concrete_new_map_push_live_selected_exact
  concrete_new_map_forged_body_rejected
  concrete_new_map_push_selector_is_nonvacuous
)
map_eof_indexed_view_proof_count="$(rg -c '^pub proof fn ' "$map_eof_indexed_view_source")"
if [[ "$map_eof_indexed_view_proof_count" -ne "${#map_eof_indexed_view_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map EOF indexed-view proof inventory changed" >&2
  exit 1
fi
map_push_receipt_proof_count="$(rg -c '^pub proof fn ' "$map_push_receipt_source")"
if [[ "$map_push_receipt_proof_count" -ne "${#map_push_receipt_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push receipt proof inventory changed" >&2
  exit 1
fi
map_push_backing_proof_count="$(rg -c '^pub proof fn ' "$map_push_backing_source")"
if [[ "$map_push_backing_proof_count" -ne "${#map_push_backing_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push backing proof inventory changed" >&2
  exit 1
fi
map_push_scoped_classes_proof_count="$(rg -c '^pub proof fn ' "$map_push_scoped_classes_source")"
if [[ "$map_push_scoped_classes_proof_count" -ne "${#map_push_scoped_classes_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push scoped identity proof inventory changed" >&2
  exit 1
fi
map_push_finite_observation_proof_count="$(rg -c '^pub proof fn ' "$map_push_finite_observation_source")"
if [[ "$map_push_finite_observation_proof_count" -ne "${#map_push_finite_observation_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push finite observation proof inventory changed" >&2
  exit 1
fi
map_push_selected_subview_proof_count="$(rg -c '^pub proof fn ' "$map_push_selected_subview_source")"
if [[ "$map_push_selected_subview_proof_count" -ne "${#map_push_selected_subview_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push selected subview proof inventory changed" >&2
  exit 1
fi
map_push_live_parent_bounds_proof_count="$(rg -c '^pub proof fn ' "$map_push_live_parent_bounds_source")"
if [[ "$map_push_live_parent_bounds_proof_count" -ne "${#map_push_live_parent_bounds_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push live parent bounds proof inventory changed" >&2
  exit 1
fi
map_push_fresh_child_proof_count="$(rg -c '^pub proof fn ' "$map_push_fresh_child_source")"
if [[ "$map_push_fresh_child_proof_count" -ne "${#map_push_fresh_child_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push fresh child proof inventory changed" >&2
  exit 1
fi
map_push_live_child_proof_count="$(rg -c '^pub proof fn ' "$map_push_live_child_source")"
if [[ "$map_push_live_child_proof_count" -ne "${#map_push_live_child_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push live child proof inventory changed" >&2
  exit 1
fi
map_push_checked_receipt_proof_count="$(rg -c '^pub proof fn ' "$map_push_checked_receipt_source")"
if [[ "$map_push_checked_receipt_proof_count" -ne "${#map_push_checked_receipt_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push checked receipt proof inventory changed" >&2
  exit 1
fi
source_observation_proof_count="$(rg -c '^pub proof fn ' "$source_observation")"
if [[ "$source_observation_proof_count" -ne "${#source_observation_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: source-observation proof inventory changed" >&2
  exit 1
fi
declare -A source_observation_expected_proofs=()
for proof in "${source_observation_proofs[@]}"; do
  if [[ -n "${source_observation_expected_proofs[$proof]:-}" ]]; then
    echo "rpb3-admission-cursor: duplicate source-observation proof $proof" >&2
    exit 1
  fi
  source_observation_expected_proofs[$proof]=1
done
while IFS= read -r declaration; do
  proof="${declaration#pub proof fn }"
  if [[ -z "${source_observation_expected_proofs[$proof]:-}" ]]; then
    echo "rpb3-admission-cursor: unlisted source-observation proof $proof" >&2
    exit 1
  fi
done < <(rg -o '^pub proof fn [A-Za-z0-9_]+' "$source_observation")
integrated_proof_count="$(rg -c '^pub proof fn ' "$integrated_source")"
integrated_private_proof_count="$(rg -c '^proof fn product_epm_modeled_empty_nested_' "$integrated_source")"
if [[ "$integrated_private_proof_count" -ne "${#integrated_private_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: nested completion private proof inventory changed" >&2
  exit 1
fi
for proof in "${integrated_private_proofs[@]}"; do
  if ! rg -q "^proof fn ${proof}\\(" "$integrated_source"; then
    echo "rpb3-admission-cursor: missing nested completion helper $proof" >&2
    exit 1
  fi
done
integrated_epm_score_private_proof_count="$(rg -c '^proof fn product_epm_score_' "$integrated_source")"
if [[ "$integrated_epm_score_private_proof_count" -ne "${#integrated_epm_score_private_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: EPM score private proof inventory changed" >&2
  exit 1
fi
for proof in "${integrated_epm_score_private_proofs[@]}"; do
  if ! rg -q "^proof fn ${proof}\\(" "$integrated_source"; then
    echo "rpb3-admission-cursor: missing EPM score helper $proof" >&2
    exit 1
  fi
done
integrated_message_child_score_private_proof_count="$(rg -c '^proof fn product_message_child_score_' "$integrated_source")"
if [[ "$integrated_message_child_score_private_proof_count" -ne "${#integrated_message_child_score_private_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: message child score private proof inventory changed" >&2
  exit 1
fi
for proof in "${integrated_message_child_score_private_proofs[@]}"; do
  if ! rg -q "^proof fn ${proof}\\(" "$integrated_source"; then
    echo "rpb3-admission-cursor: missing message child score helper $proof" >&2
    exit 1
  fi
done
integrated_message_child_complete_private_proof_count="$(rg -c '^proof fn live_runtime_message_child_complete_' "$integrated_source")"
if [[ "$integrated_message_child_complete_private_proof_count" -ne "${#integrated_message_child_complete_private_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: message child complete private proof inventory changed" >&2
  exit 1
fi
for proof in "${integrated_message_child_complete_private_proofs[@]}"; do
  if ! rg -q "^proof fn ${proof}\\(" "$integrated_source"; then
    echo "rpb3-admission-cursor: missing message child complete helper $proof" >&2
    exit 1
  fi
done
integrated_path_private_proof_count="$(rg -c '^proof fn live_runtime_retained_path_' "$integrated_source")"
if [[ "$integrated_path_private_proof_count" -ne "${#integrated_path_private_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: retained path private proof inventory changed" >&2
  exit 1
fi
for proof in "${integrated_path_private_proofs[@]}"; do
  if ! rg -q "^proof fn ${proof}\\(" "$integrated_source"; then
    echo "rpb3-admission-cursor: missing retained path helper $proof" >&2
    exit 1
  fi
done
integrated_expected_count="$((${#integrated_core_proofs[@]} + ${#integrated_push_proofs[@]} + ${#integrated_witness_proofs[@]} + 5))"
if [[ "$integrated_proof_count" -ne "$integrated_expected_count" ]]; then
  echo "rpb3-admission-cursor: integrated proof inventory changed" >&2
  exit 1
fi
declare -A integrated_expected_proofs=()
for proof in "${integrated_core_proofs[@]}" "${integrated_push_proofs[@]}" \
  "${integrated_witness_proofs[@]}" product_root_finish_claim_exact \
  live_runtime_root_complete_matches_source \
  live_runtime_product_root_complete_is_exact \
  live_runtime_empty_send_complete_is_exact \
  live_runtime_empty_send_root_complete_live_trace_is_exact; do
  if [[ -n "${integrated_expected_proofs[$proof]:-}" ]]; then
    echo "rpb3-admission-cursor: duplicate integrated proof $proof" >&2
    exit 1
  fi
  integrated_expected_proofs[$proof]=1
done
while IFS= read -r declaration; do
  proof="${declaration#pub proof fn }"
  if [[ -z "${integrated_expected_proofs[$proof]:-}" ]]; then
    echo "rpb3-admission-cursor: unlisted integrated proof $proof" >&2
    exit 1
  fi
done < <(rg -o '^pub proof fn [A-Za-z0-9_]+' "$integrated_source")
for proof in "${integrated_private_proofs[@]}"; do
  proof_log="$evidence_dir/integrated-private-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 --rlimit 30 \
    --cfg rpb3_integrated --cfg rpb3_live_push --verify-root --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: integrated helper $proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: integrated helper $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${integrated_epm_score_private_proofs[@]}"; do
  proof_log="$evidence_dir/integrated-private-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 --rlimit 30 \
    --cfg rpb3_integrated --cfg rpb3_live_push --verify-root --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPM score helper $proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPM score helper $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${integrated_message_child_score_private_proofs[@]}"; do
  proof_log="$evidence_dir/integrated-private-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 --rlimit 30 \
    --cfg rpb3_integrated --cfg rpb3_live_push --verify-root --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: message child score helper $proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: message child score helper $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${integrated_message_child_complete_private_proofs[@]}"; do
  proof_log="$evidence_dir/integrated-private-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 --rlimit 30 \
    --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-root --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: message child complete helper $proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: message child complete helper $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${integrated_path_private_proofs[@]}"; do
  proof_log="$evidence_dir/integrated-private-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 --rlimit 30 \
    --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-root --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: retained path helper $proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: retained path helper $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${integrated_core_proofs[@]}"; do
  proof_log="$evidence_dir/integrated-$proof.log"
  expected_verified=1
  if [[ "$proof" == live_source_product_epm_finish_accept_preserves_wf
      || "$proof" == live_runtime_epm_child_complete_runtime_wf ]]; then
    expected_verified=2
  fi
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push --verify-root --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: integrated $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q "verification results:: $expected_verified verified, 0 errors" "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: integrated $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${integrated_push_proofs[@]}"; do
  proof_log="$evidence_dir/integrated-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 100 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-root --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: integrated $proof proof failed" >&2
    exit 1
  fi
  expected_verified=1
  if [[ "$proof" == live_source_product_map_entry_finish_preserves_wf
      || "$proof" == live_source_product_message_child_complete_preserves_wf
      || "$proof" == live_source_product_epm_map_value_accept_preserves_wf ]]; then
    expected_verified=2
  fi
  if ! rg -q "verification results:: $expected_verified verified, 0 errors" "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: integrated $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${source_observation_proofs[@]}"; do
  proof_log="$evidence_dir/source-observation-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_source_observation --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: source-observation $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: source-observation $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${map_eof_indexed_view_proofs[@]}"; do
  proof_log="$evidence_dir/map-eof-indexed-view-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_map_eof_indexed_view_identity --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map EOF indexed-view $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map EOF indexed-view $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${map_push_receipt_proofs[@]}"; do
  proof_log="$evidence_dir/map-push-receipt-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_map_push_receipt_source_extraction --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push receipt $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push receipt $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${map_push_backing_proofs[@]}"; do
  proof_log="$evidence_dir/map-push-backing-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_map_push_backing_identity --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push backing $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push backing $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${map_push_scoped_classes_proofs[@]}"; do
  proof_log="$evidence_dir/map-push-scoped-identity-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_map_push_scoped_identity_classes --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push scoped identity $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push scoped identity $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${map_push_finite_observation_proofs[@]}"; do
  proof_log="$evidence_dir/map-push-finite-observation-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_map_push_finite_observation --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push finite observation $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push finite observation $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${map_push_selected_subview_proofs[@]}"; do
  proof_log="$evidence_dir/map-push-selected-subview-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_map_push_selected_subview_extraction --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push selected subview $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push selected subview $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${map_push_live_parent_bounds_proofs[@]}"; do
  proof_log="$evidence_dir/map-push-live-parent-bounds-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_map_push_live_parent_bounds --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push live parent bounds $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push live parent bounds $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${map_push_fresh_child_proofs[@]}"; do
  proof_log="$evidence_dir/map-push-fresh-child-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_map_push_fresh_child_extraction --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push fresh child $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push fresh child $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${map_push_live_child_proofs[@]}"; do
  proof_log="$evidence_dir/map-push-live-child-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_map_push_live_child_projection --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push live child $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push live child $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${map_push_checked_receipt_proofs[@]}"; do
  proof_log="$evidence_dir/map-push-checked-receipt-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_map_push_checked_receipt_projection --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push checked receipt $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: map push checked receipt $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${integrated_witness_proofs[@]}"; do
  proof_log="$evidence_dir/integrated-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --cfg rpb3_live_witness --verify-root --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: integrated $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: integrated $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done

generic_message_eof_source="$model_dir/rpb3_generic_message_eof_source_refinement.rs"
for proof in generic_message_eof_selected_result_exact \
  generic_component_eof_selected_unique; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$generic_message_eof_source"; then
    echo "rpb3-admission-cursor: missing generic message EOF source proof $proof" >&2
    exit 1
  fi
done
generic_message_eof_log="$evidence_dir/generic-message-eof-source-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_generic_message_eof_source_refinement \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$generic_message_eof_log" 2>&1; then
  cat "$generic_message_eof_log"
  echo "rpb3-admission-cursor: generic message EOF source proof failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 3 verified, 0 errors' "$generic_message_eof_log"; then
  cat "$generic_message_eof_log"
  echo "rpb3-admission-cursor: generic message EOF source did not verify" >&2
  exit 1
fi

for integration_module in rpb3_canonical_path_driver_product_integration \
  rpb3_message_driver_source_integration; do
  case "$integration_module" in
    rpb3_canonical_path_driver_product_integration) expected_proofs=9 ;;
    rpb3_message_driver_source_integration) expected_proofs=34 ;;
  esac
  integration_log="$evidence_dir/$integration_module.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 --rlimit 30 \
    --cfg rpb3_integrated --cfg rpb3_live_push --cfg rpb3_terminal \
    --verify-only-module "$integration_module" \
    "$model_dir/rpb3_integrated_admission.rs" \
    >"$integration_log" 2>&1; then
    cat "$integration_log"
    echo "rpb3-admission-cursor: $integration_module failed" >&2
    exit 1
  fi
  if ! rg -q "^verification results:: $expected_proofs verified, 0 errors" "$integration_log"; then
    cat "$integration_log"
    echo "rpb3-admission-cursor: $integration_module did not verify" >&2
    exit 1
  fi
done

act_source_product_log="$evidence_dir/rpb3_act_source_product_integration.log"
for proof in prepared_forward_batch_aligns_act_source \
  accepted_product_forward_batch_projects_source \
  accepted_product_map_value_projects_push; do
  if ! rg -q "^pub proof fn ${proof}\\(" \
    "$model_dir/rpb3_act_source_product_integration.rs"; then
    echo "rpb3-admission-cursor: missing ACT source/product proof $proof" >&2
    exit 1
  fi
done
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 \
  --cfg rpb3_integrated --cfg rpb3_live_push --cfg rpb3_terminal \
  --verify-only-module rpb3_act_source_product_integration \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$act_source_product_log" 2>&1; then
  cat "$act_source_product_log"
  echo "rpb3-admission-cursor: ACT source/product integration failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 12 verified, 0 errors' "$act_source_product_log"; then
  cat "$act_source_product_log"
  echo "rpb3-admission-cursor: ACT source/product integration did not verify" >&2
  exit 1
fi

integrated_source_trace="$model_dir/rpb3_integrated_source_trace_composition.rs"
for proof in integrated_source_trace_prefix_preserves_wf \
  integrated_source_trace_root_return_exact \
  checked_other_epm_complete_selects_live_action \
  checked_other_epm_terminal_end_matches_accept_end \
  checked_other_epm_complete_composes_terminal_selector \
  epm_internal_edges_to_integrated_trace \
  epm_scalar_tick_flattens \
  epm_forward_tick_flattens \
  epm_unwind_tick_flattens \
  epm_loop_tick_flattens_to_integrated_trace \
  integrated_source_trace_concat \
  integrated_source_trace_append_step \
  epm_only_continue_concat \
  epm_finite_loop_prefix_flattens \
  epm_finite_loop_complete_accepted_trace \
  epm_modeled_empty_complete_joint_guard_satisfiable \
  epm_modeled_empty_two_scalar_complete_joint_guard_satisfiable \
  checked_other_epm_complete_rejects_mutated_source; do
  if ! rg -q "^pub proof fn ${proof}\\(" "$integrated_source_trace"; then
    echo "rpb3-admission-cursor: missing integrated source trace proof $proof" >&2
    exit 1
  fi
done
integrated_source_trace_log="$evidence_dir/integrated-source-trace-composition-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 \
  --cfg rpb3_integrated --cfg rpb3_live_push --cfg rpb3_terminal \
  --verify-only-module rpb3_integrated_source_trace_composition \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$integrated_source_trace_log" 2>&1; then
  cat "$integrated_source_trace_log"
  echo "rpb3-admission-cursor: integrated source trace composition failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 21 verified, 0 errors' "$integrated_source_trace_log"; then
  cat "$integrated_source_trace_log"
  echo "rpb3-admission-cursor: integrated source trace composition did not verify" >&2
  exit 1
fi

terminal_log="$evidence_dir/rpb3_terminal_root_claim.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push --cfg rpb3_terminal \
  --verify-root --verify-function product_root_finish_claim_exact \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$terminal_log" 2>&1; then
  cat "$terminal_log"
  echo "rpb3-admission-cursor: terminal root claim proof failed" >&2
  exit 1
fi
if ! rg -q 'verification results:: [1-9][0-9]* verified, 0 errors' "$terminal_log"; then
  cat "$terminal_log"
  echo "rpb3-admission-cursor: terminal root claim proof did not complete" >&2
  exit 1
fi

terminal_source_log="$evidence_dir/rpb3_terminal_source_root_claim.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push --cfg rpb3_terminal \
  --verify-root --verify-function live_runtime_root_complete_matches_source \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$terminal_source_log" 2>&1; then
  cat "$terminal_source_log"
  echo "rpb3-admission-cursor: terminal source root proof failed" >&2
  exit 1
fi
if ! rg -q 'verification results:: 1 verified, 0 errors' "$terminal_source_log"; then
  cat "$terminal_source_log"
  echo "rpb3-admission-cursor: terminal source root proof did not verify" >&2
  exit 1
fi

terminal_live_log="$evidence_dir/rpb3_terminal_live_root_claim.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push --cfg rpb3_terminal \
  --verify-root --verify-function live_runtime_product_root_complete_is_exact \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$terminal_live_log" 2>&1; then
  cat "$terminal_live_log"
  echo "rpb3-admission-cursor: terminal live root proof failed" >&2
  exit 1
fi
if ! rg -q 'verification results:: 1 verified, 0 errors' "$terminal_live_log"; then
  cat "$terminal_live_log"
  echo "rpb3-admission-cursor: terminal live root proof did not verify" >&2
  exit 1
fi

terminal_witness_log="$evidence_dir/rpb3_terminal_empty_send_witness.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --cfg rpb3_live_witness --cfg rpb3_terminal \
  --verify-root --verify-function live_runtime_empty_send_complete_is_exact \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$terminal_witness_log" 2>&1; then
  cat "$terminal_witness_log"
  echo "rpb3-admission-cursor: terminal empty Send proof failed" >&2
  exit 1
fi
if ! rg -q 'verification results:: 1 verified, 0 errors' "$terminal_witness_log"; then
  cat "$terminal_witness_log"
  echo "rpb3-admission-cursor: terminal empty Send proof did not verify" >&2
  exit 1
fi

terminal_trace_log="$evidence_dir/rpb3_terminal_empty_send_live_trace.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --cfg rpb3_live_witness --cfg rpb3_terminal \
  --verify-root --verify-function live_runtime_empty_send_root_complete_live_trace_is_exact \
  "$model_dir/rpb3_integrated_admission.rs" \
  >"$terminal_trace_log" 2>&1; then
  cat "$terminal_trace_log"
  echo "rpb3-admission-cursor: terminal live trace proof failed" >&2
  exit 1
fi
if ! rg -q 'verification results:: 1 verified, 0 errors' "$terminal_trace_log"; then
  cat "$terminal_trace_log"
  echo "rpb3-admission-cursor: terminal live trace proof did not verify" >&2
  exit 1
fi

for mutant in unchecked_length dropped_child skipped_root non_progress; do
  mutant_source="$evidence_dir/$mutant.rs"
  mutant_log="$evidence_dir/$mutant.log"
  case "$mutant" in
    unchecked_length|non_progress) expected_proof=admission_continue_refines ;;
    dropped_child) expected_proof=admission_push_refines ;;
    skipped_root) expected_proof=admission_root_refines ;;
  esac
  python3 "$repo_root/scripts/generate-rpb3-admission-mutant.py" \
    "$mutant" "$model_dir/rpb3_admission_cursor.rs" "$mutant_source"
  if tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
    "$mutant_source" >"$mutant_log" 2>&1; then
    cat "$mutant_log"
    echo "rpb3-admission-cursor: $mutant unexpectedly verified" >&2
    exit 1
  fi
  if ! rg -Fq 'error: postcondition not satisfied' "$mutant_log" \
    || ! rg -Fq "$mutant_source:" "$mutant_log" \
    || ! rg -Fq "pub proof fn $expected_proof(" "$mutant_log" \
    || ! rg -q 'verification results:: [0-9]+ verified, [1-9][0-9]* errors' "$mutant_log"; then
    cat "$mutant_log"
    echo "rpb3-admission-cursor: $mutant failed without its expected proof error" >&2
    exit 1
  fi
done

for dependency in rpb3_admission_cursor rpb3_score_transcript \
  rpb3_score_admission_bridge rpb3_finish_macro_refinement; do
  cp "$model_dir/$dependency.rs" "$evidence_dir/$dependency.rs"
done
mutant=unbound_epm_snapshot
mutant_source="$evidence_dir/$mutant.rs"
mutant_log="$evidence_dir/$mutant.log"
python3 "$repo_root/scripts/generate-rpb3-admission-mutant.py" \
  "$mutant" "$model_dir/rpb3_runtime_frame_source_refinement.rs" "$mutant_source"
if tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" --no-cheating --num-threads 1 \
  --rlimit 30 --cfg rpb3_integrated --cfg rpb3_runtime_standalone --verify-root \
  --verify-function runtime_epm_source_score_finish_is_checked \
  "$mutant_source" >"$mutant_log" 2>&1; then
  cat "$mutant_log"
  echo "rpb3-admission-cursor: $mutant unexpectedly verified" >&2
  exit 1
fi
if ! rg -Fq 'error: postcondition not satisfied' "$mutant_log" \
  || ! rg -Fq "$mutant_source:" "$mutant_log" \
  || ! rg -Fq 'ensures match runtime_epm_source_score_finish_selected(before, snapshot_bytes)' "$mutant_log" \
  || ! rg -Fq 'failed this postcondition' "$mutant_log" \
  || ! rg -q 'verification results:: [0-9]+ verified, [1-9][0-9]* errors' "$mutant_log"; then
  cat "$mutant_log"
  echo "rpb3-admission-cursor: $mutant failed without its expected proof error" >&2
  exit 1
fi

# Keep the integrated root and its modules together in a shadow tree so the
# selected source-bridge proof reads only the deliberately mutated module.
epm_mutant_dir="$evidence_dir/unbound-epm-unwind-shadow"
mkdir -p "$epm_mutant_dir"
cp "$model_dir"/*.rs "$epm_mutant_dir/"
mutant=unbound_epm_unwind_checkpoints
mutant_source="$epm_mutant_dir/rpb3_epm_loop_observation_bridge.rs"
mutant_log="$evidence_dir/$mutant.log"
python3 "$repo_root/scripts/generate-rpb3-admission-mutant.py" \
  "$mutant" "$model_dir/rpb3_epm_loop_observation_bridge.rs" "$mutant_source"
if tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 \
  --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_loop_observation_bridge \
  --verify-function selected_unwind_run_excludes_another_restoration \
  "$epm_mutant_dir/rpb3_integrated_admission.rs" \
  >"$mutant_log" 2>&1; then
  cat "$mutant_log"
  echo "rpb3-admission-cursor: $mutant unexpectedly verified" >&2
  exit 1
fi
if ! rg -Fq 'error: postcondition not satisfied' "$mutant_log" \
  || ! rg -Fq "$mutant_source:" "$mutant_log" \
  || ! rg -Fq 'selected_unwind_run_excludes_another_restoration' "$mutant_log" \
  || ! rg -q 'verification results:: [0-9]+ verified, [1-9][0-9]* errors' "$mutant_log"; then
  cat "$mutant_log"
  echo "rpb3-admission-cursor: $mutant failed without its expected proof error" >&2
  exit 1
fi

epm_stability_mutant_dir="$evidence_dir/unbound-epm-value-stability-shadow"
mkdir -p "$epm_stability_mutant_dir"
cp "$model_dir"/*.rs "$epm_stability_mutant_dir/"
mutant=unbound_epm_value_stability
mutant_source="$epm_stability_mutant_dir/rpb3_epm_loop_observation_bridge.rs"
mutant_log="$evidence_dir/$mutant.log"
python3 "$repo_root/scripts/generate-rpb3-admission-mutant.py" \
  "$mutant" "$model_dir/rpb3_epm_loop_observation_bridge.rs" "$mutant_source"
if tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 \
  --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_loop_observation_bridge \
  --verify-function checked_scalar_value_source_has_exact_stability \
  "$epm_stability_mutant_dir/rpb3_integrated_admission.rs" \
  >"$mutant_log" 2>&1; then
  cat "$mutant_log"
  echo "rpb3-admission-cursor: $mutant unexpectedly verified" >&2
  exit 1
fi
if ! rg -Fq 'error: postcondition not satisfied' "$mutant_log" \
  || ! rg -Fq "$mutant_source:" "$mutant_log" \
  || ! rg -Fq 'checked_scalar_value_source_has_exact_stability' "$mutant_log" \
  || ! rg -q 'verification results:: [0-9]+ verified, [1-9][0-9]* errors' "$mutant_log"; then
  cat "$mutant_log"
  echo "rpb3-admission-cursor: $mutant failed without its expected proof error" >&2
  exit 1
fi

epm_body_mutant_dir="$evidence_dir/unbound-epm-push-body-shadow"
mkdir -p "$epm_body_mutant_dir"
cp "$model_dir"/*.rs "$epm_body_mutant_dir/"
mutant=unbound_epm_push_body_end
mutant_source="$epm_body_mutant_dir/rpb3_epm_loop_observation_bridge.rs"
mutant_log="$evidence_dir/$mutant.log"
python3 "$repo_root/scripts/generate-rpb3-admission-mutant.py" \
  "$mutant" "$model_dir/rpb3_epm_loop_observation_bridge.rs" "$mutant_source"
if tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 \
  --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_loop_observation_bridge \
  --verify-function checked_push_value_source_binds_selected_body \
  "$epm_body_mutant_dir/rpb3_integrated_admission.rs" \
  >"$mutant_log" 2>&1; then
  cat "$mutant_log"
  echo "rpb3-admission-cursor: $mutant unexpectedly verified" >&2
  exit 1
fi
if ! rg -Fq 'error: postcondition not satisfied' "$mutant_log" \
  || ! rg -Fq "$mutant_source:" "$mutant_log" \
  || ! rg -Fq 'checked_push_value_source_binds_selected_body' "$mutant_log" \
  || ! rg -q 'verification results:: [0-9]+ verified, [1-9][0-9]* errors' "$mutant_log"; then
  cat "$mutant_log"
  echo "rpb3-admission-cursor: $mutant failed without its expected proof error" >&2
  exit 1
fi

if ! sha256sum --quiet --check "$evidence_dir/source-manifest.sha256"; then
  echo "rpb3-admission-cursor: source changed during verification" >&2
  exit 1
fi
if ! cmp -s "$source_identity_manifest" <(
  stat -c '%n|%d|%i|%s|%y|%z' -- "${source_files[@]}"
); then
  echo "rpb3-admission-cursor: source metadata changed during verification" >&2
  exit 1
fi
echo "rpb3-admission-cursor: baseline and eight causal mutants passed"
