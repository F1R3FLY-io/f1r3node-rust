#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export TLC_REPO_ROOT="$repo_root"
# The common CanonicalPath proof reaches roughly 1 GiB RSS; a 1 GiB soft
# throttle spent its ten-minute limit reclaiming while consuming 10s CPU.
# Per-function jobs on the integrated root now reach about 2.3 GiB with
# Verus and Z3 together, and a 2 GiB soft throttle stalled one the same way.
export TLC_MEMORY_HIGH="${TLC_MEMORY_HIGH:-4G}"
export TLC_RSS="${TLC_RSS:-6G}"
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
  "$model_dir/rpb3_integrated_dispatcher.rs"
  "$model_dir/rpb3_epm_path_invariant_integrated.rs"
  "$model_dir/rpb3_epm_obligation_return_witness.rs"
  "$model_dir/rpb3_score_finish_unique.rs"
  "$model_dir/rpb3_score_finish_groups.rs"
  "$model_dir/rpb3_no_score_child_eof_coverage.rs"
  "$model_dir/rpb3_plain_parser_source_refinement.rs"
  "$model_dir/rpb3_epm_obligation_return_live.rs"
  "$model_dir/rpb3_epm_parser_escape_invariant.rs"
  "$model_dir/rpb3_epm_obligation_return_source.rs"
  "$model_dir/rpb3_epm_frame_path_invariant.rs"
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
  "$repo_root/formal/verified_score_store/src/admission_observable_spec.rs"
  "$repo_root/formal/verified_score_store/src/admission_event_checker.rs"
  "$repo_root/scripts/check-rpb3-score-branch-inventory.py"
  "$repo_root/scripts/generate-rpb3-admission-mutant.py"
  "$repo_root/scripts/check-rpb3-admission-cursor-verus.sh"
  "$repo_root/scripts/check-stack-safe-pda-proofs.sh"
)
staged_proofs=(
  start_is_private
  staged_step_refines_one_driver_transition
  staged_step_preserves_event_order
  staged_push_retains_parent_and_appends_child
  staged_eof_pop_accept_is_exact
  staged_abort_does_not_publish
  staged_trace_refines_driver
)
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
live_canonical_path_proofs=(
  indexed_from_admission_is_exact
  live_canonical_path_root_is_wf
  live_canonical_path_push_preserves_ancestors
  live_canonical_path_push_preserves_wf
  live_canonical_path_return_preserves_ancestors
  live_canonical_path_return_preserves_wf
)
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
  live_epm_modeled_empty_nested_eof_product_identity
  live_epm_modeled_empty_nested_eof_admission_counts
  live_epm_modeled_empty_nested_eof_frame_shape
  live_epm_modeled_empty_nested_eof_accepts
  live_epm_modeled_empty_nested_poststate_selected
  live_epm_modeled_empty_nested_poststate_wf
  runtime_epm_source_child_selected_complete
  live_runtime_epm_source_child_eof_accepts
  product_epm_source_bridge_finish_selected_exact
  product_epm_source_root_score_selected_exact
  product_epm_source_child_score_selected_exact
  product_epm_child_return_source_exact product_root_initial_exact
  product_map_push_child_image
  product_map_push_admission_frames
  product_map_push_concrete_frames
  product_map_push_message_shape
  product_map_push_message_indexed_shape
  product_map_push_value_shape
  product_map_push_value_indexed_outcome
  product_map_push_value_indexed_admission
  product_map_push_value_indexed_shape
  product_map_push_shape
  product_map_push_index_wf
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
integrated_helper_proofs=(
  product_wire_indexed_source_agrees_same_backing
  live_runtime_wire_sources_steady
  live_runtime_wire_sources_pop
  live_runtime_wire_sources_push
  strict_ancestor_sequence_shape_preserves_prefix
  live_wire_cursor_shape_preserves_prefix
  product_carried_receipts_preserve_prefix
  product_epm_continue_preserves_ancestor_prefix
  product_epm_complete_preserves_ancestor_prefix
  product_epm_child_return_preserves_ancestor_prefix
  product_map_key_preserves_ancestor_prefix
  live_epm_push_preserves_ancestor_prefix
  product_epm_finish_accept_preserves_ancestor_prefix
  concrete_message_continue_preserves_ancestor_prefix
  product_message_continue_preserves_ancestor_prefix
  admission_push_preserves_ancestor_prefix
  concrete_message_push_preserves_ancestor_prefix
  product_message_push_preserves_ancestor_prefix
  product_map_value_push_preserves_ancestor_prefix
  admission_child_pop_preserves_ancestor_prefix
  product_map_pop_preserves_ancestor_prefix
)
integrated_terminal_proofs=(
  product_root_finish_claim_exact
  live_runtime_root_complete_matches_source
  live_runtime_product_root_complete_is_exact
  live_runtime_empty_send_complete_is_exact
  live_runtime_empty_send_root_complete_live_trace_is_exact
)
proofs_rpb3_integrated_admission=(
  "${integrated_core_proofs[@]}"
  "${integrated_push_proofs[@]}"
  "${integrated_witness_proofs[@]}"
  "${integrated_private_proofs[@]}"
  "${integrated_epm_score_private_proofs[@]}"
  "${integrated_message_child_score_private_proofs[@]}"
  "${integrated_message_child_complete_private_proofs[@]}"
  "${integrated_path_private_proofs[@]}"
  "${integrated_helper_proofs[@]}"
  "${integrated_terminal_proofs[@]}"
)
proofs_rpb3_staged_admission_refinement=("${staged_proofs[@]}")
proofs_rpb3_scalar_continue_source_refinement=("${scalar_source_proofs[@]}")
proofs_rpb3_indexed_region_source_projection=("${indexed_region_source_proofs[@]}")
proofs_rpb3_live_canonical_path_source_refinement=("${live_canonical_path_proofs[@]}")
proofs_rpb3_live_canonical_path_push_action_lift=("${live_canonical_path_push_proofs[@]}")
proofs_rpb3_source_observation=("${source_observation_proofs[@]}")
proofs_rpb3_map_eof_indexed_view_identity=("${map_eof_indexed_view_proofs[@]}")
proofs_rpb3_map_push_receipt_source_extraction=("${map_push_receipt_proofs[@]}")
proofs_rpb3_map_push_backing_identity=("${map_push_backing_proofs[@]}")
proofs_rpb3_map_push_scoped_identity_classes=("${map_push_scoped_classes_proofs[@]}")
proofs_rpb3_map_push_finite_observation=("${map_push_finite_observation_proofs[@]}")
proofs_rpb3_map_push_selected_subview_extraction=("${map_push_selected_subview_proofs[@]}")
proofs_rpb3_map_push_live_parent_bounds=("${map_push_live_parent_bounds_proofs[@]}")
proofs_rpb3_map_push_fresh_child_extraction=("${map_push_fresh_child_proofs[@]}")
proofs_rpb3_map_push_live_child_projection=("${map_push_live_child_proofs[@]}")
proofs_rpb3_map_push_checked_receipt_projection=("${map_push_checked_receipt_proofs[@]}")
proofs_rpb3_act_source_product_integration=(
  prepared_forward_batch_aligns_act_source
  accepted_product_forward_batch_projects_source
  accepted_map_value_wire_event
  accepted_source_count_bounds_formal_events
  accepted_map_value_event_index_bounded
  same_act_layout_source_preserves_events
  same_act_layout_source_preserves_wf
  accepted_map_value_post_layout
  accepted_map_value_cursor_projects_body
  active_product_epm_has_admission_bytes
  accepted_product_map_value_projects_body
  accepted_product_map_value_projects_push
)
proofs_rpb3_admission_cursor=(
  admission_emap_returned_pairs_have_source_key_identity
  admission_par_prefix_append_at
  admission_par_tag_slot_inverse
  admission_par_prefix_last_tag_has_slot
  admission_par_generated_field_rule
  admission_par_source_prefix_after_field
  admission_par_empty_frame_has_source_prefix
  admission_par_complete_prefix_has_parser
  admission_par_complete_prefix_has_identity
  admission_par_initial_trace_invariant
  admission_key_value_pair_return_key_has_source_identity
  admission_completed_child_summary_has_source_identity
  admission_par_prefix_complete_implies_sequence_at
  admission_empty_ordered_collection_path_stable
  admission_rejects_fabricated_receive_source
  admission_rejects_fabricated_match_case
  admission_last_frame_replacement_preserves_wf
  admission_initial_wf
  admission_continue_refines
  admission_continue_preserves_wf
  admission_continue_from_decoded_field
  admission_evar_message_rejects_scalar_continue
  admission_push_refines
  admission_push_preserves_wf
  admission_push_rejects_bad_slot
  admission_forward_bound_score_refines
  admission_forward_bound_score_preserves_wf
  admission_append_score_entry_preserves_wf
  admission_attach_score_preserves_wf
  admission_record_score_refines
  admission_record_score_preserves_wf
  admission_record_node_score_refines
  admission_record_integer_score_refines
  admission_record_bytes_score_refines
  admission_complete_child_preserves_par_source_trace
  admission_complete_child_refines
  admission_complete_child_preserves_wf
  admission_root_refines
  admission_incomplete_root_accepts
  admission_incomplete_root_example_refines
  admission_root_rejects_wrong_score_lane
  admission_wrong_lane_root_example_rejected
  admission_fabricated_child_physical_contained
  admission_fabricated_child_counterexamples
  admission_first_par_return
  admission_ten_family_completeness_witnesses
  admission_canonical_path_push_indexed_preserves_wf
  admission_canonical_path_return_indexed_preserves_wf
  admission_canonical_path_root_source_derives_initial_wf
  admission_canonical_path_empty_root_witness_is_initial
  admission_indexed_field_child_source_exact
  admission_discontinuous_empty_child_anchor_witness
  admission_record_scalar_occurrence_at_position_preserves_wf
  admission_record_scalar_occurrence_preserves_wf
  admission_record_scalar_occurrence_indexed_preserves_wf
  admission_record_scalar_occurrence_indexed_anchor_exact
  concrete_message_view_rel
  concrete_message_continue_indexed_simulates
  concrete_message_continue_indexed_anchor_exact
  concrete_message_continue_indexed_preserves_par_source_trace
  concrete_message_continue_simulates
  concrete_message_continue_preserves_par_source_trace
  admission_indexed_push_preserves_par_source_trace
  concrete_message_push_indexed_source_anchor_exact
  concrete_message_push_indexed_rejects_wrong_source
  concrete_message_push_simulates
  concrete_empty_set_map_parent_pop_is_reachable
  concrete_empty_eset_expr_tag22_is_source_derived
  concrete_empty_emap_expr_tag23_is_source_derived
  concrete_derived_emap_is_source_derived
  concrete_derived_path_stability_is_child_derived
  concrete_message_complete_child_simulates
  concrete_message_emap_expr_parent_pop_is_source_derived
  concrete_message_root_simulates
  concrete_message_driver_indexed_continue_anchor_exact
  concrete_message_driver_preserves_par_source_trace
  concrete_message_driver_step_simulates
  concrete_message_driver_preserves_wf_and_arena
  admission_aux_advance_is_separate
  admission_aux_advance_preserves_wf
)
proofs_rpb3_canonical_path_common_product=(
  live_canonical_path_common_root_is_wf
  canonical_path_internal_edge_preserves_wf
  canonical_path_internal_value_preserves_wf
  canonical_path_internal_trace_prefix_wf
  boolean_root_common_witness
  boolean_root_internal_edge_value_witness
  boolean_root_scan_fold_source_witness
  boolean_root_complete_macro_witness
  boolean_root_loop_source_witness
  opaque_escape_first_scalar_witness
  opaque_escape_length_started_witness
  opaque_escape_bulk_prefix_witness
  opaque_escape_bulk_parser_state_witness
  opaque_escape_final_parser_byte_transition
  opaque_escape_final_loop_edge_witness
  opaque_escape_final_payload_witness
  opaque_escape_bulk_loop_source_witness
  boolean_root_loop_selected_value_witness
  boolean_root_checked_eof_completion_witness
  boolean_root_common_dispatch_complete_witness
)
proofs_rpb3_canonical_path_driver_product_integration=(
  common_push_selected_preserves_wf
  common_loop_fold_retains_ancestors
  parser_settle_retains_stamp
  parser_resolution_retains_stamp
  common_return_selected_preserves_boundary
  common_push_macro_preserves_boundary
  canonical_path_driver_product_active_preserves_wf
  canonical_path_driver_product_active_sync
  boolean_root_driver_product_witness
)
proofs_rpb3_canonical_path_driver_source_refinement=(
  canonical_path_selected_child_is_exact
  canonical_path_observed_push_is_exact
)
proofs_rpb3_canonical_path_eof_owner_obligation_refinement=(
  focus_captures_from_paths_retain_stack
  settle_focus_owner_vector_unchanged
  select_focus_retained_prefix_preserves_stack
  select_focus_then_capture_preserves_owner_stack
  select_focus_then_source_captures_preserves_owner_stack
  focused_owner_capture_retains_stack
  focused_owner_root_eof_cannot_succeed
  successful_root_eof_value_has_no_active_obligation
)
proofs_rpb3_canonical_path_epm_return_producer=(
  canonical_path_epm_completion_derives_return_summary
)
proofs_rpb3_canonical_path_forward_capacity_source=(
  positive_forward_capacity_bounds
  accepted_width_derives_forward_capacity_facts
  three_byte_capacity_witness
)
proofs_rpb3_canonical_path_historical_checkpoint_source=(
  historical_predecessors_select_scalar_leaves
  positive_first_payload_end_is_restored_prefix_anchor
  changed_current_token_two_leave_witness
)
proofs_rpb3_canonical_path_owner_health_source=(
  captured_owner_entry_facts
  old_captured_owner_entry_survives_push
  new_captured_owner_entry_is_well_formed
  append_captured_open_owner_preserves_store
  focus_step_preserves_captured_owner_health
  owner_source_step_preserves_health
  reachable_captured_owners_remain_open
  reachable_positive_capacity_has_open_owner_root
  accepted_forward_facts_from_reachable_owner_trace
  nested_owner_dead_child_witness
)
proofs_rpb3_canonical_path_persistent_state_source=(
  scalar_leave_selected_is_saved_predecessor
  scalar_leave_predecessor_matches_batch_source_core
  checkpoint_get_preserves_saved_state
  saved_run_prefix_states_keep_identity
  saved_payload_progress_changes_with_depth
  saved_non_obligation_stays_absent
  saved_awaiting_resolution_stays_zero
  linked_fault_or_dead_retains_saved_stamp
  linked_payload_prefix_witness
  changed_current_state_non_obligation_witness
)
proofs_rpb3_canonical_path_raw_par_complete_macro_refinement=(
  canonical_path_raw_par_complete_macro_preserves_source_and_score
)
proofs_rpb3_canonical_path_rejected_nested_return_refinement=(
  rejected_nested_epm_return_applies_and_preserves
  rejected_nested_epm_root_eof_is_error
)
proofs_rpb3_canonical_path_root_complete_source_correspondence=(
  canonical_path_root_complete_source_enables_driver
  canonical_path_root_complete_source_corresponds
  canonical_path_root_complete_source_rejects_mismatch
)
proofs_rpb3_canonical_path_root_source_refinement=(
  canonical_path_root_obligations_balanced
  canonical_path_root_complete_is_exact
)
proofs_rpb3_canonical_path_terminal_driver_observation=(
  canonical_path_terminal_driver_success_is_exact
  canonical_path_terminal_driver_failure_has_no_complete
  canonical_path_terminal_driver_stats_overflow_has_no_complete
)
proofs_rpb3_epathmap_canonical_path_complete_macro_refinement=(
  canonical_path_epm_internal_prefix_preserves
  canonical_path_epm_complete_macro_preserves_source_and_score
)
proofs_rpb3_epathmap_end_source_selector=(
  epm_end_source_selects_live_action
)
proofs_rpb3_epathmap_event_refinement=(
  act_hashed_scan_matches_observed_buckets
  act_collision_hides_stale_equal_line
  act_hash_trace_reuse_is_byte_equal
  act_hash_trace_unequal_collision_cannot_reuse
  act_trace_scan_matches_observed_buckets
  act_hash_trace_record_step_preserves_alignment
  initial_plain_tags_refute_unchanged_gate
  simple_byte_array_plain_gate_trace
  end_requires_no_outstanding
  wrong_global_return_id_cannot_resolve
  wrong_local_obligation_id_cannot_resolve
  linked_epm_continue_preserves_source_and_progress
  linked_epm_push_preserves_parent_progress
)
proofs_rpb3_epathmap_nested_complete_macro_refinement=(
  nested_epm_internal_prefix_preserves
  nested_epm_complete_macro_preserves_source_and_score
)
proofs_rpb3_epathmap_program31_complete_macro_refinement=(
  program31_epm_internal_prefix_preserves
  program31_epm_complete_macro_preserves_source_and_score
)
proofs_rpb3_epathmap_push_macro_refinement=(
  linked_epm_internal_prefix_preserves
  linked_epm_push_macro_preserves_source_and_progress
)
proofs_rpb3_epathmap_terminal_nested_return_composition=(
  terminal_nested_source_enabled
  terminal_nested_source_corresponds
  terminal_nested_end_is_live_action
)
proofs_rpb3_epathmap_terminal_source_observation=(
  epm_terminal_end_preserves_layout
  epm_terminal_source_success_enabled
  epm_terminal_source_success_is_exact
  epm_terminal_source_overflow_has_no_complete
)
proofs_rpb3_epm_active_line_work_source=(
  accepted_width_fits_event_headroom
  prepared_line_derives_scalar_schedule_guard
  prepared_line_step_has_bounded_source_and_event_index
  overflow_limited_prefix_witness
)
proofs_rpb3_epm_batch_cursor_composition=(
  accepted_forward_batch_composes_cursor_and_restore_run
  accepted_coalesced_forward_batch_composes_cursor_and_restore_run
  payload_run_has_historical_checkpoint_projection
  saved_parser_gate_survives_global_counter_restore
  accepted_unwind_batch_one_scalar_restore_step
  accepted_unwind_batch_has_scalar_restore_steps
  accepted_unwind_batch_composes_cursor_and_restore_run
  unwind_witness_historical_run_wf
  unwind_witness_validated_plan_matches
  unwind_witness_encoded_prefix
  unwind_witness_encoded_act_plan
  unwind_witness_expanded_act_events
  unwind_witness_concrete_act_layout
  unwind_witness_act_layout
  accepted_unwind_has_no_current_pending_witness
  accepted_unwind_has_changed_checkpoint_witness
)
proofs_rpb3_epm_batch_cursor_source_extraction=(
  forward_line_work_has_scalar_event
  unwind_line_work_has_scalar_event
  expanded_emit_prefix_at
  attachment_preserves_event_count
  attachment_preserves_batch_wire_at
  attached_map_value_at_prefix
  two_leaf_source_validation_has_exact_structure
  selected_map_value_projects_validated_record
  selected_map_value_attachment_binds_body
  selected_map_value_source_is_unique
  accepted_indexed_line_step_is_plan_event
  accepted_indexed_line_batch_has_exact_cursor_range
  plan_event_units_range_additive
  plan_work_event_weight_additive
  plan_single_emit_weight
  plan_single_enter_weight
  plan_forward_line_work_weight
  plan_leave_line_work_weight
  plan_count_shape_implies_parent_partition
  act_matched_child_implies_count_shape
  act_matched_record_implies_count_shape
  act_records_imply_count_shape
  plan_child_preorder_step
  plan_child_head_matches_preorder_range
  plan_children_work_weight_matches_preorder_range
  plan_record_work_weight_from_parts
  plan_record_work_weight_matches_subtree
  act_expansion_preserves_event_weight
  source_suffix_units_match_plan_range
  accepted_source_plan_has_count_shape
  plan_raw_expansion_has_count
  accepted_plan_raw_expansion_has_count
  accepted_plan_attachment_preserves_raw_count
  accepted_act_source_count_equals_expansion
)
proofs_rpb3_epm_frame_path_invariant=(
  parser_top_only_obligation_free
  parser_pop_top_frames
  parser_replace_top_frames
  parser_bubble_frames
  parser_settle_frames
  parser_tag_step_frames
  parser_bytes_step_frames
  parser_varint_finish_frames
  parser_focus_step_frames
  parser_reusable_payload_top
  parser_feed_byte_frames
  parser_resolve_frames
  epm_layout_path_lengths
  epm_restore_push_wf
  epm_frame_path_wf_initial
  epm_frame_path_wf_plain_edge
  epm_frame_path_wf_start_edge
  epm_frame_path_wf_payload_edge
  epm_frame_path_wf_leave
  epm_frame_path_wf_value
  epm_frame_path_wf_end
  epm_frame_path_wf_event_step
  epm_frame_path_wf_map_value_accept
  epm_frame_path_wf_obligation_accept
  epm_frame_path_wf_accept_child
  epm_frame_path_wf_discharges_return_premises
)
proofs_rpb3_epm_loop_continuation_source=(
  forward_scalar_prefix_preserves_image
  loop_internal_tick_preserves_image
  finite_loop_prefix_preserves_image
  finite_loop_selects_push_or_complete
  finite_loop_push_dispatches_exact_image
  finite_loop_complete_dispatches_exact_image
)
proofs_rpb3_epm_loop_observation_bridge=(
  checked_scalar_value_source_has_exact_stability
  checked_push_value_source_binds_selected_body
  observed_top_run_selects_virtual_restore_suffix
  nonempty_restore_history_source_has_witness
  scalar_edge_without_bulk_window_has_source_witness
  scalar_edge_source_shape_refines_reducer_path_length
  scalar_leave_source_shape_refines_reducer_path_length
  scalar_value_source_shape_preserves_reducer_path_summary
  checked_loop_successor_has_exact_cursor_delta
  checked_scalar_leave_source_has_witness
  scalar_reducer_projection_yields_internal_live_step
  scalar_reducer_projection_is_live_action
  checked_forward_source_yields_loop_tick
  selected_unwind_run_excludes_another_restoration
  checked_unwind_source_yields_loop_tick
  checked_scalar_source_yields_loop_tick
  checked_push_source_yields_stop_readiness
  checked_complete_source_yields_stop_readiness
)
proofs_rpb3_epm_nested_accept_checked_stats_refinement=(
  nested_epm_accept_resolution_comes_from_child
  nested_epm_accept_counter_overflow_has_no_effect
  nested_epm_accept_checked_success_is_exact
)
proofs_rpb3_epm_nested_source_refinement=(
  source_epm_return_has_derived_summary
  source_epm_map_value_return_pop_shape
  program31_score_append_has_source_transcript
)
proofs_rpb3_epm_obligation_return_live=(
  epm_child_return_pop_shape
  product_epm_obligation_return_is_functional
  product_epm_obligation_return_preserves_ancestor_prefix
  live_source_epm_obligation_return_preserves_wf
  live_runtime_epm_obligation_return_preserves_wf
  live_epm_obligation_return_has_effects
  live_runtime_epm_raw_par_return_top_kind
  live_runtime_epm_raw_par_return_is_functional
  live_runtime_epm_raw_par_return_preserves_wf
  live_runtime_epm_raw_par_return_has_effects
  epm_raw_par_return_resolution_shape
  epm_raw_par_return_preflight_agrees
  epm_raw_par_return_excludes_child_steps
  live_runtime_epm_nested_return_top_kind
  live_runtime_epm_nested_return_is_functional
  live_runtime_epm_nested_return_preserves_wf
  live_runtime_epm_nested_return_has_effects
  epm_nested_return_resolution_shape
  epm_nested_return_parent_alive
  epm_end_then_nested_return_is_impossible
  epm_nested_return_excludes_finish_accept
)
proofs_rpb3_epm_obligation_return_source=(
  epm_gate_image_matches
  epm_raw_par_root_resolution
  epm_resolution_dead_settles
  epm_parent_resolved_pop_wf
  epm_raw_par_return_enabled
  epm_terminal_end_frame_is_nested_end_frame
  epm_end_admission_id_irrelevant
  epm_nested_return_enabled
  epm_nested_return_terminal_agrees
  epm_nested_return_checked_accept_agrees
  epm_raw_par_return_checked_accept_agrees
)
proofs_rpb3_epm_obligation_return_witness=(
  epm_raw_par_witness_nested_frames
  epm_raw_par_witness_parent_frame_wf
  epm_raw_par_witness_child_frame_wf
  epm_raw_par_witness_frames_wf
  epm_raw_par_witness_unscored_bridge_wf
  epm_raw_par_witness_append_entry
  epm_raw_par_witness_ordered_handles
  epm_raw_par_witness_score_entries
  epm_raw_par_witness_score_step
  epm_raw_par_witness_scored_bridge_wf
  epm_raw_par_witness_scored_shape
  epm_raw_par_witness_popped_frames_wf
  epm_raw_par_witness_admission_is_wf
  epm_raw_par_witness_parent_layout
  epm_raw_par_witness_returned_shape
  epm_raw_par_witness_resolves_root
  epm_raw_par_witness_parent_accepts
  epm_raw_par_witness_child_cursor_ends
  epm_raw_par_witness_owner_ready
  epm_raw_par_witness_owner_agrees
  epm_raw_par_witness_linked_sources
  epm_raw_par_witness_map_wf_from_parts
  epm_raw_par_witness_product_wf_from_parts
  epm_raw_par_witness_unscored_product_wf
  epm_raw_par_witness_scored_product_wf
  epm_raw_par_witness_product_is_wf
  epm_raw_par_witness_unscored_live_shape
  epm_raw_par_witness_scored_live_shape
  epm_raw_par_witness_returned_pop
  epm_raw_par_witness_live_wf_from_parts
  epm_raw_par_witness_unscored_state_wf
  epm_raw_par_witness_scored_state_wf
  epm_raw_par_witness_state_is_wf
  epm_raw_par_witness_stack_source
  epm_raw_par_witness_child_summary
  epm_raw_par_witness_derived_summary
  epm_raw_par_witness_parent_ready
  epm_raw_par_witness_linked_return
  epm_raw_par_witness_source_return
  epm_raw_par_witness_product_return
  epm_raw_par_witness_child_cursor_last
  epm_raw_par_witness_is_step
  epm_raw_par_witness_is_integrated_step
  epm_nested_dead_witness_frame_facts
  epm_nested_dead_witness_frames_wf
  epm_nested_dead_witness_linked_sources
  epm_nested_dead_witness_popped_wf
  epm_nested_dead_witness_admission_is_wf
  epm_nested_dead_witness_layout_carried
  epm_nested_dead_witness_layouts
  epm_nested_dead_witness_child_ends
  epm_nested_dead_witness_bridge_wf
  epm_nested_dead_witness_product_shape
  epm_nested_dead_witness_product_from_parts
  epm_nested_dead_witness_product_is_wf
  epm_nested_dead_witness_before_shape
  epm_nested_dead_witness_live_from_parts
  epm_nested_dead_witness_state_is_wf
  epm_nested_dead_witness_returned_shape
  epm_nested_dead_witness_resolves_dead
  epm_nested_dead_witness_parent_accepts
  epm_nested_dead_witness_stack_from_parts
  epm_nested_dead_witness_stack_source
  epm_nested_dead_witness_derived_summary
  epm_nested_dead_witness_parent_ready
  epm_nested_dead_witness_linked_return
  epm_nested_dead_witness_source_return
  epm_nested_dead_witness_product_return
  epm_nested_dead_witness_returned_pop
  epm_nested_dead_witness_is_step
  epm_nested_dead_witness_is_integrated_step
  epm_nested_alive_witness_act_layout_carried
  epm_nested_alive_witness_child_layout
  epm_nested_alive_witness_child_ends
  epm_nested_alive_witness_child_frame_facts
  epm_nested_alive_witness_frame_kinds
  epm_nested_alive_witness_regions_shape
  epm_nested_alive_witness_top_replaced_wf
  epm_nested_alive_witness_frames_wf
  epm_nested_alive_witness_linked_sources
  epm_nested_alive_witness_popped_wf
  epm_nested_alive_witness_admission_is_wf
  epm_nested_alive_witness_parent_layouts
  epm_nested_alive_witness_layouts
  epm_nested_alive_witness_bridge_wf
  epm_nested_alive_witness_product_shape
  epm_nested_alive_witness_product_is_wf
  epm_nested_alive_witness_before_shape
  epm_nested_alive_witness_live_parts
  epm_nested_alive_witness_state_is_wf
  epm_nested_alive_witness_returned_shape
  epm_nested_alive_witness_resolves_alive
  epm_nested_alive_witness_parent_accepts
  epm_nested_alive_witness_stack_source
  epm_nested_alive_witness_derived_parts
  epm_nested_alive_witness_derived_summary
  epm_nested_alive_witness_parent_ready
  epm_nested_alive_witness_linked_parts
  epm_nested_alive_witness_linked_return
  epm_nested_alive_witness_source_return
  epm_nested_alive_witness_return_parts
  epm_nested_alive_witness_product_return
  epm_nested_alive_witness_returned_pop
  epm_nested_alive_witness_is_step
  epm_nested_alive_witness_is_integrated_step
)
proofs_rpb3_epm_parser_escape_invariant=(
  parser_escape_wf_initial
  parser_escape_bubble_keeps_path
  parser_escape_settle_keeps_image
  parser_escape_wf_feed_byte
  parser_escape_wf_resolve
  parser_escape_wf_restore
  parser_escape_raw_par_top_is_root
)
proofs_rpb3_epm_path_invariant_integrated=(
  epm_frames_paths_wf_push_none
  epm_frames_paths_wf_prefix
  epm_frames_paths_wf_update
  parser_same_path_shape_keeps_facts
  parser_forward_payload_same_shape
  parser_restore_checkpoint_same_shape
  canonical_path_driver_epm_lengths
  canonical_path_push_epm_frames
  canonical_path_child_return_epm_frames
  integrated_canonical_path_push_keeps_epm_paths_wf
  integrated_canonical_path_child_return_keeps_epm_paths_wf
  integrated_canonical_path_root_complete_keeps_epm_paths_wf
  integrated_canonical_path_keeps_epm_paths_wf
  act_forward_after_epm_frames
  integrated_act_forward_keeps_epm_paths_wf
  unwind_product_virtual_epm_frames
  act_unwind_after_epm_frames
  act_unwind_selected_checkpoint
  integrated_act_unwind_keeps_epm_paths_wf
)
proofs_rpb3_epm_push_live_action_refinement=(
  epm_push_live_action_is_checked
  epm_push_live_action_dispatches
)
proofs_rpb3_epm_push_source_action_refinement=(
  epm_push_source_enables_integrated_action
)
proofs_rpb3_epm_push_transport_source_refinement=(
  epm_push_batch_commit_matches_last_scalar_source
  epm_push_linked_continue_cursor_step
  epm_push_batch_commit_preserves_source_anchor
  epm_push_transport_prefix_is_cursor_exact
  epm_push_rust_request_preserves_selected_child
  epm_push_returned_request_is_selected
  epm_push_returned_event_is_cursor_event
  epm_push_transport_terminal_is_source_selected
)
proofs_rpb3_epm_restore_run_expansion=(
  accepted_forward_publishes_exact_virtual_checkpoints
  accepted_coalesced_forward_extends_virtual_checkpoints
  accepted_unwind_selects_prefix_and_pops_exact_checkpoints
)
proofs_rpb3_epm_score_replay_refinement=(
  program31_score_replay_exact
)
proofs_rpb3_epm_source_accept_refinement=(
  epm_source_accept_carries_end_summary
  epm_source_finish_accept_preserves_wf
)
proofs_rpb3_epm_unwind_event_lift=(
  unwind_virtual_frame_has_layout
  accepted_unwind_one_full_epm_event
  accepted_unwind_full_epm_event_sequence
  linked_unwind_virtual_state_is_source_wf
  linked_unwind_admission_update_is_idempotent
  accepted_unwind_one_linked_continue
  accepted_unwind_linked_continue_sequence
)
proofs_rpb3_epm_unwind_product_lift=(
  unwind_product_virtual_matches_linked_source
  accepted_unwind_one_product_internal_continue
  accepted_unwind_product_one_step_preserves_wf
  accepted_unwind_product_virtual_wf
  accepted_unwind_product_internal_sequence
  live_unwind_virtual_state_wf
  accepted_unwind_one_live_internal_step
  accepted_unwind_live_internal_sequence
  live_runtime_internal_epm_continue_preserves_wf
  live_runtime_unwind_virtual_state_wf
  accepted_unwind_live_runtime_internal_sequence
)
proofs_rpb3_ezipper_source_refinement=(
  ezipper_absent_modeled_pop_is_source_bound
  ezipper_absent_modeled_rejects_fabricated_summary
  ezipper_modeled_empty_snapshot_score_length
  ezipper_modeled_receipt_is_source_bound
  ezipper_modeled_complete_route_is_reachable
  ezipper_pathmap_pop_rejects_false_score_and_fixed_point
  ezipper_expr_pop_rejects_false_score_and_fixed_point
  ezipper_modeled_route_rejects_fabricated_summaries
  ezipper_receipt_uses_indexed_snapshot
  ezipper_missing_pathmap_has_no_complete_score
  ezipper_unlinked_snapshot_has_no_complete_route
  ezipper_corrupt_receipt_has_no_complete_route
  ezipper_scored_constructor_transcript_exact
  ezipper_complete_route_preserves_source_and_score
)
proofs_rpb3_finish_macro_refinement=(
  modeled_finish_preserves_score_shape
  modeled_root_return_matches_pre_pop
  empty_send_finish_has_root_return
)
proofs_rpb3_generated_descriptor_projection=(
)
proofs_rpb3_generic_message_eof_source_refinement=(
  generic_message_eof_selected_result_exact
  generic_component_eof_selected_unique
)
proofs_rpb3_integrated_source_trace_composition=(
  integrated_source_trace_prefix_preserves_wf
  integrated_source_trace_root_return_exact
  checked_other_epm_complete_selects_live_action
  checked_other_epm_terminal_end_matches_accept_end
  checked_other_epm_complete_composes_terminal_selector
  epm_internal_edges_prefix_wf
  epm_internal_edges_to_integrated_trace
  epm_scalar_tick_flattens
  epm_forward_tick_flattens
  epm_unwind_tick_flattens
  epm_loop_tick_flattens_to_integrated_trace
  integrated_source_trace_concat
  integrated_source_trace_append_step
  epm_only_continue_concat
  epm_finite_loop_prefix_flattens
  epm_finite_loop_complete_accepted_trace
  epm_modeled_empty_complete_joint_guard_satisfiable
  epm_modeled_empty_two_scalar_complete_joint_guard_satisfiable
  checked_other_epm_complete_rejects_mutated_source
)
proofs_rpb3_live_canonical_path_descendant_action_lift=(
  canonical_steady_descendant_step_refines_retained_source
  canonical_steady_descendant_step_preserves_trace_wf
)
proofs_rpb3_live_canonical_path_map_finish_action_lift=(
  canonical_map_finish_refines_retained_source
  canonical_map_finish_preserves_trace_wf
)
proofs_rpb3_live_canonical_path_pop_action_lift=(
  canonical_message_child_complete_pop_shape
  canonical_map_value_accept_pop_shape
  canonical_nested_pop_shape_source_ready
  canonical_nested_pop_refines_retained_source
  canonical_nested_pop_preserves_trace_wf
)
proofs_rpb3_live_canonical_path_root_terminal_trace=(
  canonical_path_trace_terminal_source_is_ready
  canonical_path_root_trace_to_successful_terminal_is_exact
)
proofs_rpb3_live_canonical_path_score_action_lift=(
  canonical_message_child_score_refines_retained_source
  canonical_message_child_score_preserves_trace_wf
  canonical_root_score_excluded_by_path_root
)
proofs_rpb3_live_canonical_path_terminal_refinement=(
  live_canonical_path_terminal_enables_root_source
  live_canonical_path_terminal_complete_is_exact
)
proofs_rpb3_live_canonical_path_trace_refinement=(
  canonical_descendant_preserves_retained_path
  live_canonical_path_trace_step_preserves_wf
  live_canonical_path_root_trace_exists
  live_canonical_path_trace_prefix_preserves_wf
)
proofs_rpb3_map_frame_refinement=(
  map_initial_indexed_wf
  map_discontinuous_empty_value_push_possible
  map_initial_wf
  map_source_key_full_exists
  map_admission_value_push_indexed_source_anchor_exact
  map_admission_value_push_indexed_rejects_wrong_source
  map_admission_value_accept_pop_source_anchor_exact
  map_admission_entry_complete_source_anchor_exact
  map_fresh_child_is_source_initial
  integrated_new_map_push_uses_fresh_child
  integrated_initial_is_well_formed
  integrated_root_finish_returns_derived_summary
  integrated_driver_preserves_wf
  integrated_new_map_child_body_id_is_driver_assigned
  integrated_replay_preserves_wf
  integrated_map_completion_source_bound
  integrated_map_finish_pop_shape
  integrated_message_parent_accept_pop_shape
  map_key_step_exact
  map_key_field_is_shared_witness
  map_value_push_exact
  map_value_field_is_shared_witness
  map_value_accept_exact
  map_finish_exact
  map_finish_source_bound
  map_finish_implies_shared_summary
  map_varint_prefix_width_unique
  map_indexed_key_payload_unique
  map_source_key_unique
  map_source_choice_exact
  map_summary_matches_shared_source
  map_admission_entry_complete_exact
  map_complete_gate_exact
  map_pending_rejects_finish
  map_unaccepted_value_rejects_finish
  map_eof_snapshot_rejects_ineligible
  map_eof_checked_complete_source_bound
  map_eof_checked_complete_rejects_other
  map_eof_checked_complete_rejects_forged_owner
  map_eof_checked_complete_rejects_forged_region_or_cursor
  map_eof_checked_complete_rejects_forged_summary
  map_cursor_eof_has_checked_source_receipt
  map_decreasing_key_rejected
)
proofs_rpb3_map_key_full_source_composition=(
  map_key_full_source_selects_admission
  map_key_full_source_refines_live_continue
  map_key_source_enables_admission
  map_key_full_source_enabled
)
proofs_rpb3_map_key_source_correspondence=(
  map_key_utf8_scan_matches_admission
  map_key_source_derives_continue
)
proofs_rpb3_message_driver_source_integration=(
  send_live_continue_selected_dispatches
  send_source_live_continue_selected_exact
  send_live_push_selected_dispatches
  send_live_child_complete_selected_dispatches
  generated_map_child_complete_selected_dispatches
  generated_epm_child_complete_selected_dispatches
  send_source_live_push_selected_exact
  send_source_live_accept_selected_exact
  send_replay_selects_live_root_score_step
  send_live_root_return_selected_components
  send_replay_selects_live_root_return_step
  send_replay_selects_live_child_score_step
  send_driver_source_terminal_exact
  generated_message_scalar_live_selected_exact
  generated_message_published_child_identity_exact
  generated_message_push_checked_step_binds_fold
  generated_message_push_live_selected_exact
  generated_message_epm_push_selected_dispatches
  generated_message_epm_push_live_selected_exact
  generated_message_field_live_selected_exact
  generated_message_first_push_rejects_forged_published_body_id
  generated_message_accept_live_selected_exact
  generated_message_scored_eof_live_selected_exact
  generated_message_noscore_eof_live_selected_exact
  generated_message_replay_from_source_operations_exact
  generated_message_replay_field_selected_exact
  generated_message_replay_accept_selected_exact
  generated_message_replay_live_step_is_exact
  generated_message_replay_scored_eof_selected_exact
  generated_message_replay_noscore_eof_selected_exact
  generated_message_source_operations_scored_eof_selected_exact
  generated_message_source_operations_noscore_eof_selected_exact
)
proofs_rpb3_no_score_child_eof_coverage=(
  nsc_complete_step_facts
  nsc_classify_set_map_arm
  nsc_classify_scored_prefix
  nsc_classify_structural_arm
  nsc_classify_family_arm
  nsc_case_split_total
  nsc_route_selects_every_case
  nsc_quiet_group_one_steps
  nsc_quiet_group_two_steps
  nsc_quiet_group_three_steps
  nsc_quiet_group_four_steps
  nsc_quiet_group_five_steps
  nsc_quiet_group_six_steps
  nsc_quiet_groups_exclude_score_route
  nsc_emap_ezipper_score_selector_none
  nsc_selected_structural_frame
  nsc_noscore_route_excludes_score_route
  nsc_every_case_reaches_parent_accept
  nsc_selected_kind_is_pushed_summary
  nsc_ezipper_absent_routed_witness
  nsc_emap_incomplete_routed_witness
  nsc_inhabited_fixed_point_class
  nsc_inhabited_unit_class
  nsc_inhabited_var_ref_class
  nsc_inhabited_connective_body_class
  nsc_inhabited_bytes_class
  nsc_inhabited_receive_bind_class
  nsc_inhabited_match_case_class
  nsc_inhabited_big_rational_class
  nsc_inhabited_evar_class
  nsc_inhabited_structural_class
  nsc_inhabited_emethod_class
  nsc_inhabited_key_value_pair_class
  nsc_inhabited_opaque_class
  nsc_descriptor_low_entries
  nsc_descriptor_middle_entries
  nsc_descriptor_expr_entries
  nsc_descriptor_upper_entries
  nsc_descriptor_high_entries
  nsc_descriptor_children_classified
  nsc_class_arms_are_descriptor_edges
  nsc_every_class_has_descriptor_arm
  nsc_always_scored_has_no_case
  nsc_always_scored_child_has_no_prestate
  nsc_eof_coverage_universal
  nsc_source_noscore_route_on_emap_ezipper
)
proofs_rpb3_path_parser_refinement=(
  initial_parser_source_snapshot_has_witness
  initial_single_byte_varint
  two_byte_varint_decodes_130
  utf8_ed_rejects_surrogate_prefix
  initial_plain_parser_tags_change_gate
  one_child_list_parser_bubbles_to_root
  one_key_region_parser_focuses_and_settles
  nested_invalid_tag_fault_freezes_parser_on_later_plain_edge
)
proofs_rpb3_runtime_frame_source_refinement=(
  source_frame_push_observe_after_exact
  source_frame_push_observed_storage_is_checked
  source_message_finish_keeps_driver_control
  runtime_score_source_edge_exact
  runtime_child_eof_gate_intro
  runtime_child_eof_gate_depth
  runtime_score_finish_control_transitive
  runtime_unattached_keeps_driver_control
  runtime_constructor_keeps_driver_control
  runtime_send_finish_discharges_child_gate
  runtime_send_child_eof_accept_refines
  runtime_send_child_eof_accepts_when_parent_accepts
  runtime_bundle_finish_discharges_child_gate
  runtime_bundle_child_eof_accept_refines
  runtime_bundle_child_eof_accepts_when_parent_accepts
  runtime_if_finish_discharges_child_gate
  runtime_if_child_eof_accept_refines
  runtime_if_child_eof_accepts_when_parent_accepts
  runtime_match_finish_discharges_child_gate
  runtime_match_child_eof_accept_refines
  runtime_match_child_eof_accepts_when_parent_accepts
  runtime_nonsemantic_finish_discharges_child_gate
  runtime_nonsemantic_child_eof_accept_refines
  runtime_nonsemantic_child_eof_accepts_when_parent_accepts
  runtime_incomplete_send_finish_discharges_child_gate
  runtime_incomplete_send_child_eof_accept_refines
  runtime_incomplete_send_child_eof_accepts_when_parent_accepts
  runtime_incomplete_bundle_child_eof_uses_nonsemantic_finish
  runtime_incomplete_bundle_child_eof_accepts_when_parent_accepts
  source_message_finish_discharges_child_gate
  runtime_noscore_child_selector_excluded
  runtime_epm_source_score_finish_is_checked
  runtime_selected_score_finish_is_checked
  runtime_message_driver_control_split
  runtime_selected_eof_outcome_is_checked
  runtime_push_checked_storage
  runtime_push_indexed_checked_storage
  source_checked_frame_request_push_indexed_refines
  source_checked_frame_request_push_refines
  source_checked_frame_request_push_fresh_refines
  source_checked_frame_request_push_indexed_fresh_refines
  source_checked_fresh_root_refines
  source_initial_indexed_cursor_rejects_consumed
  runtime_continue_retains_stack_and_score
  runtime_continue_indexed_retains_stack_and_score
  runtime_child_accept_is_derived
  runtime_noscore_child_eof_is_checked
  runtime_child_eof_gate_transports
  runtime_child_eof_completes_after_source_score
  source_message_eof_child_accept_refines
  source_noscore_message_eof_child_accept_refines
  runtime_root_return_is_pre_pop_exact
  runtime_empty_send_return_is_reachable
  runtime_selected_empty_send_return_is_reachable
  runtime_noscore_receive_bind_modeled_witness
  runtime_noscore_match_case_modeled_witness
  runtime_noscore_big_rational_modeled_witness
  runtime_noscore_evar_modeled_witness
  runtime_noscore_expr_structural_modeled_case
  runtime_noscore_expr_structural_modeled_witness
  runtime_noscore_emethod_modeled_witness
  runtime_noscore_key_value_pair_modeled_witness
  runtime_noscore_key_value_pair_present_modeled_witness
  runtime_noscore_opaque_modeled_witness
  runtime_epm_root_modeled_witness
  runtime_epm_child_modeled_witness
  runtime_emap_ezipper_score_groups_quiet
  runtime_noscore_emap_ezipper_selected_quiet
  runtime_noscore_emap_ezipper_eof_is_complete_image
  runtime_noscore_witness_required_slots
  runtime_noscore_leaf_image_wf
  runtime_noscore_ezipper_absent_modeled_witness
  runtime_noscore_emap_incomplete_modeled_witness
  runtime_noscore_structural_summary_selected
  runtime_noscore_family_summary_selected
  runtime_noscore_child_kind_reads_pushed_summary
  source_noscore_route_agrees_with_runtime
  runtime_emap_ezipper_source_steps_quiet
  source_noscore_emap_incomplete_route_witness
  source_message_finish_route_reads_complete_summary
  runtime_root_return_extends_component_route
  runtime_root_return_observable_exact
  runtime_raw_par_finish_selected_exact
  runtime_par_root_return_witness
  runtime_epm_owner_step_is_selected
  runtime_epm_owner_generic_root_agrees
  runtime_epm_owner_generic_child_agrees
  runtime_epm_owner_generic_agrees
  runtime_epm_owner_generic_route_exact
  runtime_epm_owner_generic_root_witness
  runtime_epm_owner_generic_child_witness
)
proofs_rpb3_score_admission_bridge=(
  bridge_certified_interval_entries_exact
  bridge_existing_binding_has_exact_transcript
  bridge_binding_excludes_generic_zero_transcript
  bridge_initial_exact
  bridge_initial_append_certificate
  bridge_zero_count_matches
  bridge_node_children_lengths_match
  bridge_typed_record_matches
  bridge_append_exact
  bridge_integer_exact
  bridge_string_exact
  bridge_bytes_exact
  bridge_node_exact
  bridge_append_unattached_exact
  bridge_append_preserves_certificate
  bridge_append_unattached_preserves_certificate
  bridge_append_unattached_attempt_preserves_certificate
  bridge_append_unattached_attempt_preserves_certified_interval
  bridge_append_unattached_preserves_admission_wf
  bridge_score_prefix_transitive
  bridge_binding_prefix_transitive
  bridge_score_handle_preserved
  bridge_score_step_shape_reflexive
  bridge_score_step_shape_preserves_par_source_trace
  bridge_score_step_shape_transitive
  bridge_append_unattached_shape
  bridge_attach_score_shape
  bridge_cost_layers_valid
  bridge_expr_structural_exact
  bridge_evar_exact
  bridge_ordered_sequence_exact
  bridge_emethod_exact
  bridge_new_uri_fold_preserves
  bridge_new_uris_or_absent_preserves
  bridge_new_injection_fold_preserves
  bridge_new_injections_or_absent_preserves
  bridge_new_collections_exact
  bridge_append_coordinate_bytes_exact
  bridge_append_coordinate_bytes_shape
  bridge_absent_signer_unattached_exact
  bridge_absent_signer_unattached_shape
  bridge_cost_region_exact
  bridge_cost_region_shape
  bridge_old_children_transcript_preserved
  bridge_constructor_exact
  bridge_constructor_shape
  bridge_bundle_exact
  bridge_bundle_source_finish
  bridge_if_exact
  bridge_send_exact
  bridge_match_exact
  bridge_absent_specialized_constructor_exact
  bridge_absent_connective_exact
  bridge_receive_bind_exact
  bridge_match_case_exact
  bridge_bool_oneof_exact
  bridge_expr_atom_exact
  bridge_expr_numeric_exact
  bridge_expr_big_rational_exact
  bridge_expr_fixed_point_exact
  bridge_sys_auth_token_exact
  bridge_new_empty_collections_exact
  bridge_new_one_injection_exact
  bridge_new_one_absent_injection_exact
  bridge_new_one_uri_exact
  bridge_expr_payload_forward_exact
  bridge_connective_not_exact
  bridge_connective_varref_exact
  bridge_connective_collection_exact
  bridge_unforgeable_bytes_exact
  bridge_empty_par_unattached_exact
  bridge_receive_present_condition_exact
  bridge_receive_absent_condition_exact
  bridge_cost_token_stack_exact
  bridge_cost_signature_exact
  bridge_cost_atom_absent_exact
  bridge_cost_atom_bytes_exact
  bridge_cost_bound_exact
  bridge_cost_atom_bound_forward_exact
  bridge_nonsemantic_exact
  bridge_raw_par_exact
  bridge_add_score_finish_candidate_sound
  bridge_score_finish_selection_group_1_sound
  bridge_score_finish_selection_group_2_sound
  bridge_score_finish_selection_group_3_sound
  bridge_score_finish_selection_group_4_sound
  bridge_score_finish_selection_group_5_sound
  bridge_score_finish_selected_is_source_step
  bridge_score_finish_selected_preserves_checked
  bridge_primary_component_group_1_empty
  bridge_primary_component_group_2_exact
  bridge_primary_component_group_3_empty
  bridge_primary_component_group_4_empty
  bridge_primary_component_group_5_nonsemantic
  bridge_primary_component_finish_excludes_nonsemantic
  bridge_primary_component_finish_selected_exact
  bridge_primary_component_source_step_has_unique_finish
  bridge_cost_token_stack_source_step_has_unique_finish
  bridge_gunforgeable_source_step_matches_selected
  bridge_gunforgeable_source_step_has_unique_finish
  bridge_connective_source_step_matches_selected
  bridge_connective_source_step_has_unique_finish
  bridge_expr_source_step_matches_selected
  bridge_expr_source_step_has_unique_finish
  bridge_other_incomplete_group_1_empty
  bridge_other_incomplete_group_2_empty
  bridge_other_incomplete_group_3_empty
  bridge_other_incomplete_group_4_empty
  bridge_other_incomplete_group_5_nonsemantic
  bridge_other_incomplete_finish_selected_exact
  bridge_nonsemantic_incomplete_finish_selected_exact
  bridge_nonsemantic_incomplete_source_step_matches_selected
  bridge_nonsemantic_incomplete_source_step_has_unique_finish
  bridge_no_score_child_group_1_empty
  bridge_no_score_child_group_2_empty
  bridge_no_score_child_group_3_empty
  bridge_no_score_child_group_4_empty
  bridge_no_score_child_group_5_empty
  bridge_no_score_child_finish_selected_none
  bridge_receive_bind_source_absent_selected_none
  bridge_match_case_required_absent_selected_none
  bridge_receive_bind_absent_source_has_derived_no_score_return
  bridge_match_case_missing_required_has_derived_no_score_return
  bridge_unforgeable_unit_child_has_derived_no_score_return
  bridge_connective_varref_child_has_derived_no_score_return
  bridge_connective_body_child_has_derived_no_score_return
  bridge_unforgeable_bytes_child_has_derived_no_score_return
  bridge_big_rational_child_has_derived_no_score_return
  bridge_fixed_point_child_has_derived_no_score_return
  bridge_evar_absent_child_has_derived_no_score_return
  bridge_par_handles_for_slot_absent
  bridge_receive_complete_groups_1_to_4_empty
  bridge_receive_absent_condition_excludes_present
  bridge_receive_complete_finish_selected_exact
  bridge_receive_complete_source_step_has_unique_finish
  bridge_new_complete_groups_1_and_2_empty
  bridge_new_complete_group_5_empty
  bridge_string_bytes_for_slot_absent
  bridge_new_no_uris_have_empty_uri_summary
  bridge_new_specialized_parts_disjoint
  bridge_new_complete_finish_selected_exact
  bridge_new_complete_source_step_matches_selected
  bridge_new_complete_source_step_has_unique_finish
  bridge_par_handles_for_slot_without_par
  bridge_expr_structural_missing_par_step_none
  bridge_emethod_missing_target_step_none
  bridge_structural_no_score_group_1_empty
  bridge_structural_no_score_group_2_empty
  bridge_structural_no_score_group_3_empty
  bridge_structural_no_score_group_4_empty
  bridge_structural_no_score_group_5_empty
  bridge_structural_no_score_child_finish_selected_none
  bridge_evar_missing_var_step_none
  bridge_evar_missing_var_finish_selected_none
  bridge_other_summary_child_finish_selected_none
  bridge_emap_complete_key_handles_exact
  bridge_emap_complete_key_handles_bounded
  bridge_canonical_varint_end_unique
  bridge_canonical_field_tag_and_end_unique
  bridge_par_locally_free_range_is_source_bounded
  bridge_par_identity_is_source_slice
  bridge_par_locally_free_range_unique_at
  bridge_par_identity_from_source_is_unique
  bridge_empty_par_body_has_empty_identity
  bridge_locally_free_only_par_has_empty_identity
  bridge_emap_pairs_match_admission_source
  bridge_emap_complete_matches_admission
  bridge_emap_finish_source_complete_and_incomplete
  bridge_emap_incomplete_finish_has_no_score_step
  bridge_emap_complete_finish_source_exact
  bridge_emap_incomplete_pair_has_no_key_score
  bridge_emap_complete_pair_has_exact_key_score
  bridge_emap_duplicate_identity_rejects_fixed_point
  bridge_emap_pair_out_of_order_rejects_fixed_point
  bridge_key_value_pair_source_eof_selected_none
  bridge_key_value_pair_parent_pop_preserves_source
  bridge_expr_structural_missing_par_finish_selected_none
  bridge_emethod_missing_target_finish_selected_none
  bridge_emap_incomplete_finish_selected_none
  bridge_score_source_step_preserves_exact
  bridge_raw_par_shape
  bridge_cost_atom_bytes_shape
  bridge_cost_bound_shape
  bridge_cost_atom_bound_forward_shape
  bridge_if_shape
  bridge_send_shape
  bridge_match_shape
  bridge_absent_specialized_constructor_shape
  bridge_absent_connective_shape
  bridge_receive_bind_shape
  bridge_match_case_shape
  bridge_bool_oneof_shape
  bridge_expr_atom_shape
  bridge_expr_numeric_shape
  bridge_expr_big_rational_shape
  bridge_expr_fixed_point_shape
  bridge_new_empty_collections_shape
  bridge_new_one_injection_shape
  bridge_new_one_absent_injection_shape
  bridge_new_one_uri_shape
  bridge_expr_payload_forward_shape
  bridge_connective_varref_shape
  bridge_unforgeable_bytes_shape
  bridge_empty_par_unattached_shape
  bridge_receive_present_condition_shape
  bridge_receive_absent_condition_shape
  bridge_score_source_step_preserves_shape
  bridge_score_source_step_preserves_driver_counts
  bridge_score_source_step_preserves_par_source_trace
  bridge_message_driver_step_preserves_exact
  bridge_message_driver_step_preserves_par_source_trace
  bridge_source_step_preserves_exact
  bridge_source_step_preserves_par_source_trace
  bridge_trace_edge_is_source_step
  bridge_initial_reachable
  bridge_source_step_extends_reachability
  bridge_trace_prefix_preserves_exact
  bridge_trace_prefix_preserves_par_source_trace
  bridge_reachable_has_par_source_trace
  bridge_reachable_emap_keys_have_source_identity
  bridge_reachable_has_exact_binding
  bridge_reachable_component_score_eof_source_finish_unique
  bridge_first_difference_exact
  bridge_root_first_difference_exact
  bridge_cost_token_stack_root_claim_exact
  bridge_source_checked_root_has_exact_claim
  bridge_root_exact
  bridge_var_instance_descriptor_rows_pinned
  bridge_var_level_score_step_exact
  bridge_var_score_step_exact
  bridge_var_score_step_is_production_var_score
  bridge_var_frame_required_slots_generated
  bridge_var_level_score_step_succeeds_from_empty
  bridge_var_score_absent_witness
  bridge_var_score_bound_zero_witness
  bridge_var_score_free_minus_one_witness
  bridge_var_score_wildcard_witness
  bridge_epathmap_connective_used_descriptor_pinned
  bridge_pathmap_score_append_exact
  bridge_pathmap_score_append_is_production
  bridge_epathmap_owner_score_step_exact
  bridge_pathmap_score_append_succeeds
  bridge_epathmap_witness_snapshot_score_length
  bridge_epathmap_frame_required_slots_generated
  bridge_epathmap_witness_owner_wf
  bridge_epathmap_owner_score_modeled_witness
  bridge_ezipper_connective_used_descriptor_pinned
  bridge_epathmap_owner_score_snapshot_readback
  bridge_ezipper_score_step_exact
  bridge_ezipper_score_step_is_production
  bridge_ezipper_frame_required_slots_generated
  bridge_ezipper_witness_before_wf
  bridge_ezipper_witness_owner_scores
  bridge_ezipper_witness_child_completes
  bridge_ezipper_score_after_owner_witness
  bridge_score_finish_selection_group_6_sound
  bridge_score_source_step_group_6_top
  bridge_score_finish_selection_group_6_empty_off_programs
  bridge_score_finish_selection_group_6_earlier_groups_empty
  bridge_score_source_step_group_6_preserves_exact
  bridge_score_source_step_group_6_preserves_shape
  bridge_score_source_step_group_6_preserves_driver_counts
  bridge_score_source_step_group_6_preserves_par_source_trace
  bridge_no_score_child_group_6_empty
  bridge_structural_no_score_group_6_empty
  bridge_score_finish_selection_group_6_var_child_before_wf
  bridge_score_finish_selection_group_6_var_child_witness
  bridge_score_finish_selection_group_6_owner_witness
  bridge_score_finish_selection_group_6_ezipper_witness
)
proofs_rpb3_score_arena_source_refinement=(
  runtime_children_len_projects
  runtime_score_len_projects
  runtime_push_refines_score_append
  runtime_append_attempt_refines_score_append
  runtime_reservation_failure_preserves_next_handle
  runtime_kind_valid_projects
  runtime_success_preserves_append_trace
  runtime_failed_attempt_preserves_append_trace
  runtime_take_of_take
  runtime_append_trace_replays_exact
  runtime_append_trace_handle_exact
  runtime_operational_attempt_preserves_replay_or_appends
  indexed_bytes_and_node_witness
)
proofs_rpb3_score_cursor_refinement=(
  cursor_work_push
  cursor_work_empty
  cursor_reversed_children_bytes
  cursor_slice_head_exact
  cursor_escape_pending_exact
  cursor_escape_finished_exact
  cursor_escape_head_exact
  cursor_non_score_step_conserves
  cursor_children_prefix_same
  cursor_score_step_conserves
  flat_indexed_next_projects
  flat_indexed_score_step_projects
  frozen_owned_subview_projects
  frozen_owned_next_projects
  frozen_owned_score_step_projects
  frozen_page_prefix_byte_exact
  frozen_multi_byte_at_exact
  frozen_multi_locate_exact
  frozen_multi_next_projects
  frozen_multi_score_step_projects
  frozen_two_leaf_crossing_witness
  composed_leaf_next_projects
  composed_leaf_score_step_projects
  composed_pending_push
  composed_branch_full_expand_at
  composed_branch_full_expand_preserves
  composed_range_concat
  composed_branch_selected_expand_preserves
  composed_discard_exhausted_conserves
  composed_active_next_conserves
  composed_branch_expand_windows_wf
  composed_branch_pop_conserves
  composed_source_branch_children_exact
  composed_source_branch_pop_conserves
  cursor_initial_transcript
  cursor_literal_byte_exact
  cursor_integer_score_expands_exactly
  first_difference_loop_prefix
  equal_scores_have_no_difference
  first_difference_loop_exact
  score_handle_first_difference_exact
)
proofs_rpb3_score_finish_groups=(
  finish_eof_prestate_top_wf
  finish_routes_raw_par
  finish_routes_cost_atom_absent
  finish_routes_cost_atom_bytes
  finish_routes_cost_bound
  finish_routes_cost_atom_bound_forward
  finish_routes_cost_signature
  finish_routes_cost_region
  finish_routes_cost_token_stack
  finish_routes_bundle
  finish_routes_if
  finish_routes_send
  finish_routes_match_component
  finish_routes_absent_specialized
  finish_routes_absent_connective
  finish_routes_receive_bind
  finish_routes_match_case
  finish_routes_bool_oneof
  finish_routes_expr_atom
  finish_routes_expr_numeric
  finish_routes_expr_big_rational
  finish_routes_expr_fixed_point
  finish_routes_sys_auth_token
  finish_routes_new_empty_collections
  finish_routes_new_one_injection
  finish_routes_new_one_absent_injection
  finish_routes_new_one_uri
  finish_routes_new_collections
  finish_routes_expr_payload_forward
  finish_routes_expr_structural
  finish_routes_evar
  finish_routes_ordered_sequence
  finish_routes_emethod
  finish_routes_emap_finish
  finish_routes_connective_not
  finish_routes_connective_varref
  finish_routes_connective_collection
  finish_routes_unforgeable_bytes
  finish_routes_receive_present_condition
  finish_routes_receive_absent_condition
  finish_routes_nonsemantic
  finish_routes_var
  finish_routes_epathmap_owner
  finish_routes_ezipper
  finish_group_1_body
  finish_group_2_body
  finish_group_3_body
  finish_group_4_body
  finish_group_5_body
  finish_group_6_body
  finish_group_1_folds
  finish_group_2_folds
  finish_group_3_folds
  finish_group_4_folds
  finish_group_5_folds
  finish_group_6_folds
  finish_group_1_sources
  finish_group_2_sources
  finish_group_3_sources
  finish_group_4_sources
  finish_group_5_sources
  finish_group_6_sources
  finish_source_step_by_group
  finish_source_step_routes
  finish_group_1_covers
  finish_group_2_covers
  finish_group_3_covers
  finish_group_4_covers
  finish_group_5_covers
  finish_group_6_covers
  finish_group_1_disjoint
  finish_group_2_disjoint
  finish_group_3_disjoint
  finish_group_4_disjoint
  finish_group_5_disjoint
  finish_group_6_disjoint
  finish_classes_partition
  finish_uncovered_shape
  finish_coverage_counterexample
  finish_witness_prestate
  finish_witness_leaf_fires
  finish_witness_cost_token_stack_fires
  finish_witness_absent_specialized_fires
  finish_witness_sys_auth_token_fires
  finish_witness_ordered_sequence_fires
  finish_witness_nonsemantic_fires
  finish_witness_var_fires
  finish_witness_owner_fires
  finish_witness_ezipper_fires
  finish_nonvacuous_class_1
  finish_nonvacuous_class_2
  finish_nonvacuous_class_3
  finish_nonvacuous_class_4
  finish_nonvacuous_class_5
  finish_nonvacuous_class_6_var
  finish_nonvacuous_class_6_owner
  finish_nonvacuous_class_6_ezipper
)
proofs_rpb3_score_finish_unique=(
  finish_selected_unfolds
  finish_selection_folds
  bridge_score_finish_never_ambiguous
  bridge_score_finish_unique
  bridge_score_finish_epathmap_returns
  finish_epathmap_nested_shape
  finish_epathmap_nested_witness
  finish_epathmap_owner_product
  finish_owner_score_agrees
  finish_owner_top_shape
  finish_owner_top_scores
  finish_owner_top_instance
  finish_ezipper_top_instance
  finish_ezipper_scored_instance
  finish_payload_parent_scored_shape
  finish_payload_parent_complete_instance
  finish_payload_parent_incomplete_instance
  finish_value_child_instance
)
proofs_rpb3_score_transcript=(
  empty_arena_well_formed
  empty_append_certificate
  certified_interval_entries_valid
  integer_transcript_exact
  escape_zeroes_length
  string_and_bytes_transcripts_exact
  children_transcript_length
  node_transcript_exact
  kind_encoded_len_matches_transcript
  prepare_score_entry_matches_checked_append
  checked_append_exact
  checked_append_preserves_certificate
  checked_append_attempt_preserves_certificate
  checked_append_attempt_preserves_certified_interval
  certified_empty_arena_wf
  certified_append_attempt_preserves_wf
  append_integer_exact
  append_string_exact
  append_bytes_exact
  append_node_exact
)
proofs_rpb3_wire_cursor_source_refinement=(
  wire_shift_factor_matches_table
  wire_multiplier_matches_shift
  wire_payload_matches_mod
  wire_continuation_matches
  wire_or_shift_is_sum
  wire_or_group_matches_sum
  wire_accumulation_bound
  wire_source_or_loop_matches
  wire_source_or_read_matches
  wire_tenth_group_overflow
  wire_prefix_same_value
  wire_subrange_prefix
  wire_varint_loop_refines
  wire_varint_read_refines
  wire_source_field_step_matches_model
  wire_field_step_refines
  wire_source_next_field_or_matches_model
  wire_source_next_field_refines
  wire_runtime_subview_exact
  wire_runtime_varint_refines
  wire_concrete_next_raw_projects
  wire_concrete_initial_refines
  wire_concrete_next_field_or_matches_model
  wire_runtime_borrowed_window_projects
  wire_runtime_message_pre_read_projects
  wire_runtime_borrowed_message_pre_read_projects
  wire_runtime_frozen_index_projects
  wire_runtime_composed_tree_projects
  wire_runtime_frozen_message_pre_read_projects
  wire_runtime_composed_message_pre_read_projects
  wire_frozen_owned_segment_view_wf
  wire_frozen_owned_segment_subview_preserves_wf
  wire_frozen_owned_segment_subview_preserves_lookup
  wire_frozen_owned_segment_coalesce_preserves_lookup
  wire_frozen_one_leaf_at_matches_paired_slot
  wire_frozen_owned_segment_view_at_matches_pair
  wire_frozen_one_leaf_total
  wire_source_node_postorder_total
  wire_source_tree_postorder_total
  wire_source_tree_postorder_at_total
  wire_source_live_view_clone_preserves
  wire_source_live_view_subview_preserves
  wire_source_indexed_clone_preserves
  wire_source_indexed_subview_preserves
  wire_source_indexed_subview_with_end_preserves
  wire_source_indexed_projects
  wire_source_live_view_same_root_coalesce_preserves
  wire_source_live_view_lookup_total
  wire_source_live_view_projects
  wire_source_live_borrowed_original_coalesce_preserves
  wire_source_live_frozen_original_coalesce_preserves
  wire_source_old_lookup_after_append
  wire_source_append_branch2_projects
  wire_source_append_branch3_projects
  wire_runtime_frozen_one_leaf_pre_read_projects
  wire_runtime_composed_postorder_pre_read_projects
  wire_runtime_varint_continue_read_refines
  wire_runtime_next_field_from_source
  wire_runtime_end_is_exact
  wire_empty_par_field_is_read
)
proof_declaration_regex='pub(\(crate\))? proof fn [A-Za-z0-9_]+'
check_proof_inventory() {
  local module="$1" array_name="$2"
  local module_source="$model_dir/$module.rs"
  local -n listed_proofs="$array_name"
  local -A listed=() declared=()
  local proof declaration nonconforming
  if [[ ! -f "$module_source" ]]; then
    echo "rpb3-admission-cursor: gated module $module is missing" >&2
    exit 1
  fi
  nonconforming="$(rg -n '\bproof\s+fn\b' "$module_source" \
    | rg -v "^[0-9]+:$proof_declaration_regex" \
    | rg -v '^[0-9]+:\s*//' || true)"
  if [[ -n "$nonconforming" ]]; then
    printf '%s\n' "$nonconforming" >&2
    echo "rpb3-admission-cursor: non-public proof function in $module" >&2
    exit 1
  fi
  for proof in "${listed_proofs[@]}"; do
    if [[ -n "${listed[$proof]:-}" ]]; then
      echo "rpb3-admission-cursor: duplicate listed proof $proof in $module" >&2
      exit 1
    fi
    listed[$proof]=1
  done
  while IFS= read -r declaration; do
    proof="${declaration##* }"
    if [[ -z "${listed[$proof]:-}" ]]; then
      echo "rpb3-admission-cursor: unlisted proof $proof in $module" >&2
      exit 1
    fi
    declared[$proof]=1
  done < <(rg -o "^$proof_declaration_regex" "$module_source")
  for proof in "${listed_proofs[@]}"; do
    if [[ -z "${declared[$proof]:-}" ]]; then
      echo "rpb3-admission-cursor: listed proof $proof is not declared in $module" >&2
      exit 1
    fi
  done
  if [[ "${#declared[@]}" -ne "${#listed_proofs[@]}" ]]; then
    echo "rpb3-admission-cursor: proof count of $module changed: ${#declared[@]} declared, ${#listed_proofs[@]} listed" >&2
    exit 1
  fi
}
proofs_rpb3_integrated_dispatcher=(
  product_epm_receipts_carried_congruent
  product_map_step_is_functional
  live_source_continue_is_functional
  live_source_continue_indexed_is_functional
  live_source_map_key_is_functional
  live_runtime_continue_is_functional
  live_runtime_continue_indexed_is_functional
  live_runtime_map_key_is_functional
  integrated_score_finish_shape_is_functional
  live_runtime_root_score_is_functional
  live_runtime_message_child_score_is_functional
  integrated_map_pop_shape_is_functional
  live_runtime_message_child_complete_is_functional
  live_runtime_map_value_accept_is_functional
  live_runtime_map_entry_finish_is_functional
  live_runtime_message_push_is_functional
  live_runtime_message_push_indexed_is_functional
  live_runtime_message_epm_push_indexed_is_functional
  live_runtime_map_value_push_is_functional
  live_runtime_map_value_push_indexed_is_functional
  epm_continue_event_step_is_functional
  linked_epm_continue_is_functional
  product_epm_continue_is_functional
  live_runtime_epm_continue_is_functional
  accept_child_is_functional
  product_epm_child_return_is_functional
  live_runtime_epm_map_value_accept_is_functional
  live_runtime_epm_end_is_functional
  product_epm_finish_accept_is_functional
  live_runtime_epm_finish_accept_is_functional
  integrated_live_non_push_is_functional
  integrated_message_push_route_implies_observed
  integrated_map_value_push_route_implies_observed
  integrated_live_relation_non_push_refines_live_step
  integrated_live_relation_message_push_refines_live_step
  integrated_live_relation_map_value_push_is_route_step
  integrated_map_value_push_observed_is_live_step
  integrated_live_relation_map_value_push_refines_live_step
  integrated_live_relation_refines_live_step
  integrated_step_is_function
  same_act_source_view_preserves_event_count
  act_forward_frame_is_layout_wf
  integrated_epm_cursor_admission_preserves_wf
  integrated_act_forward_product_preserves_wf
  integrated_act_forward_preserves_wf
  integrated_step_preserves_wf
  integrated_kind_guards_are_exclusive
  product_linked_top_is_epathmap_frame
  product_map_message_step_requires_message_top
  live_runtime_message_push_is_message_top
  live_runtime_message_push_indexed_is_message_top
  live_runtime_message_epm_push_indexed_is_message_top
  live_source_continue_is_message_top
  live_source_continue_indexed_is_message_top
  live_runtime_continue_is_message_top
  live_runtime_continue_indexed_is_message_top
  integrated_live_step_establishes_kind_guard
  integrated_epm_raw_par_return_is_integrated_step
  integrated_epm_nested_return_is_integrated_step
  integrated_trace_wf
  integrated_trace_wf_append
  live_root_completion_is_exact
  integrated_accept_refines_root
  integrated_accepted_step_is_accepting_trace
  live_nested_epm_finish_accept_is_integrated_step
  boolean_root_canonical_path_complete_is_integrated_step
  act_unwind_witness_admission_is_wf
  act_unwind_witness_product_is_wf
  act_unwind_witness_state_is_wf
  act_unwind_witness_is_integrated_step
  act_forward_witness_single_byte_varint
  act_forward_witness_encoded_prefix
  act_forward_witness_trie_view
  act_forward_witness_act_magic
  act_forward_witness_root_semantic
  act_forward_witness_root_id
  act_forward_witness_record_matches
  act_forward_witness_producer_matches
  act_forward_witness_encoded_act_plan
  act_forward_witness_validated_plan_matches
  act_forward_witness_raw_expansion
  act_forward_witness_attachment
  act_forward_witness_attached_events_valid
  act_forward_witness_expanded_act_events
  act_forward_witness_concrete_act_layout
  act_forward_witness_admission_is_wf
  act_forward_witness_product_is_wf
  act_forward_witness_state_is_wf
  act_forward_witness_suffix_expansion
  act_forward_witness_source_records_match_snapshot
  act_forward_witness_source_structure
  act_forward_witness_plan_source_projects_frame
  act_forward_witness_work_is_prepared
  act_forward_witness_derived_placement
  act_forward_witness_line_work_placement
  act_forward_witness_indexed_line_source
  act_forward_witness_forward_capacity
  act_forward_witness_forward_source
  act_forward_witness_is_integrated_step
)
proofs_rpb3_plain_parser_source_refinement=(
  forward_payload_root_projection
  forward_payload_without_bytes_is_identity
  forward_capacity_narrows_to_shorter_width
  forward_capacity_step_feeds_reusable_byte
  forward_payload_prefix_matches_byte_run
  forward_payload_equals_plain_byte_run
  unready_obligation_requests_no_child
  forward_virtual_step_is_scalar_payload_edge
  forward_virtual_step_meets_payload_edge_clauses
  forward_virtual_suffix_is_scalar_payload_edges
  payload_checkpoint_parser_is_plain_byte_prefix
  fresh_forward_run_starts_at_pre_batch_path
  fresh_forward_run_ends_at_published_path
  fresh_forward_batch_is_scalar_payload_edges
  coalesced_forward_batch_is_scalar_payload_edges
  forward_virtual_path_projection
  forward_virtual_path_template_swap
  payload_run_unwind_selects_scalar_prefix
  fresh_forward_then_unwind_restores_scalar_prefix
  saved_image_matches_restored_parser_iff
  counter_reread_preserves_parser_source_match
  rebased_saved_image_matches_forward_payload
  payload_run_rebased_images_match_checkpoints
  edge_return_is_scalar_edge_path_step
  leave_return_selects_scalar_leave_path
  scalar_edge_then_leave_restores_checkpoint
  edge_return_then_leave_return_round_trip
  scalar_edge_checkpoint_has_saved_image
  canonical_skip_virtual_step_is_internal_edge
  canonical_skip_virtual_prefix_is_wf
  canonical_skip_virtual_suffix_scans_to_end
  canonical_bulk_skip_is_scalar_edge_scan
  opaque_and_owner_parser_witness
  opaque_payload_run_is_wf
  epm_forward_path_witness
  saved_handle_image_witness
  plain_tag_edge_round_trip_witness
  canonical_internal_edge_advances_slot_counts
  opaque_bulk_event_premises_hold
  canonical_skip_virtual_slot_count
  canonical_opaque_bulk_skip_witness
  dropped_byte_or_field_claims_are_refuted
  payload_edge_agreements_are_necessary
)
gated_modules=(
  rpb3_act_source_product_integration
  rpb3_admission_cursor
  rpb3_canonical_path_common_product
  rpb3_canonical_path_driver_product_integration
  rpb3_canonical_path_driver_source_refinement
  rpb3_canonical_path_eof_owner_obligation_refinement
  rpb3_canonical_path_epm_return_producer
  rpb3_canonical_path_forward_capacity_source
  rpb3_canonical_path_historical_checkpoint_source
  rpb3_canonical_path_owner_health_source
  rpb3_canonical_path_persistent_state_source
  rpb3_canonical_path_raw_par_complete_macro_refinement
  rpb3_canonical_path_rejected_nested_return_refinement
  rpb3_canonical_path_root_complete_source_correspondence
  rpb3_canonical_path_root_source_refinement
  rpb3_canonical_path_terminal_driver_observation
  rpb3_epathmap_canonical_path_complete_macro_refinement
  rpb3_epathmap_end_source_selector
  rpb3_epathmap_event_refinement
  rpb3_epathmap_nested_complete_macro_refinement
  rpb3_epathmap_program31_complete_macro_refinement
  rpb3_epathmap_push_macro_refinement
  rpb3_epathmap_terminal_nested_return_composition
  rpb3_epathmap_terminal_source_observation
  rpb3_epm_active_line_work_source
  rpb3_epm_batch_cursor_composition
  rpb3_epm_batch_cursor_source_extraction
  rpb3_epm_frame_path_invariant
  rpb3_epm_loop_continuation_source
  rpb3_epm_loop_observation_bridge
  rpb3_epm_nested_accept_checked_stats_refinement
  rpb3_epm_nested_source_refinement
  rpb3_epm_obligation_return_live
  rpb3_epm_obligation_return_source
  rpb3_epm_obligation_return_witness
  rpb3_epm_parser_escape_invariant
  rpb3_epm_path_invariant_integrated
  rpb3_epm_push_live_action_refinement
  rpb3_epm_push_source_action_refinement
  rpb3_epm_push_transport_source_refinement
  rpb3_epm_restore_run_expansion
  rpb3_epm_score_replay_refinement
  rpb3_epm_source_accept_refinement
  rpb3_epm_unwind_event_lift
  rpb3_epm_unwind_product_lift
  rpb3_ezipper_source_refinement
  rpb3_finish_macro_refinement
  rpb3_generated_descriptor_projection
  rpb3_generic_message_eof_source_refinement
  rpb3_indexed_region_source_projection
  rpb3_integrated_admission
  rpb3_integrated_dispatcher
  rpb3_integrated_source_trace_composition
  rpb3_live_canonical_path_descendant_action_lift
  rpb3_live_canonical_path_map_finish_action_lift
  rpb3_live_canonical_path_pop_action_lift
  rpb3_live_canonical_path_push_action_lift
  rpb3_live_canonical_path_root_terminal_trace
  rpb3_live_canonical_path_score_action_lift
  rpb3_live_canonical_path_source_refinement
  rpb3_live_canonical_path_terminal_refinement
  rpb3_live_canonical_path_trace_refinement
  rpb3_map_eof_indexed_view_identity
  rpb3_map_frame_refinement
  rpb3_map_key_full_source_composition
  rpb3_map_key_source_correspondence
  rpb3_map_push_backing_identity
  rpb3_map_push_checked_receipt_projection
  rpb3_map_push_finite_observation
  rpb3_map_push_fresh_child_extraction
  rpb3_map_push_live_child_projection
  rpb3_map_push_live_parent_bounds
  rpb3_map_push_receipt_source_extraction
  rpb3_map_push_scoped_identity_classes
  rpb3_map_push_selected_subview_extraction
  rpb3_message_driver_source_integration
  rpb3_no_score_child_eof_coverage
  rpb3_path_parser_refinement
  rpb3_plain_parser_source_refinement
  rpb3_runtime_frame_source_refinement
  rpb3_scalar_continue_source_refinement
  rpb3_score_admission_bridge
  rpb3_score_arena_source_refinement
  rpb3_score_cursor_refinement
  rpb3_score_finish_groups
  rpb3_score_finish_unique
  rpb3_score_transcript
  rpb3_source_observation
  rpb3_staged_admission_refinement
  rpb3_wire_cursor_source_refinement
)
for module in "${gated_modules[@]}"; do
  check_proof_inventory "$module" "proofs_$module"
done
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
  if ! rg -Fxq 'verification results:: 185 verified, 0 errors' "$score_store_log"; then
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
if [[ "$(rg -c "^$proof_declaration_regex" "$staged_source")" -ne "${#staged_proofs[@]}" ]]; then
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
scalar_source_proof_count="$(rg -c "^$proof_declaration_regex" "$scalar_source")"
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
indexed_region_source_proof_count="$(rg -c "^$proof_declaration_regex" "$indexed_region_source")"
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
  bridge_new_complete_source_step_has_unique_finish \
  bridge_score_finish_selection_group_6_sound \
  bridge_score_source_step_group_6_top \
  bridge_score_finish_selection_group_6_empty_off_programs \
  bridge_score_finish_selection_group_6_var_child_witness \
  bridge_score_finish_selection_group_6_owner_witness \
  bridge_score_finish_selection_group_6_ezipper_witness; do
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
  runtime_noscore_ezipper_absent_modeled_witness \
  runtime_noscore_emap_incomplete_modeled_witness \
  runtime_push_indexed_checked_storage \
  source_checked_frame_request_push_indexed_refines \
  runtime_epm_source_score_finish_is_checked \
  runtime_epm_root_modeled_witness \
  runtime_epm_child_modeled_witness \
  source_checked_frame_request_push_fresh_refines \
  source_checked_frame_request_push_indexed_fresh_refines \
  source_checked_fresh_root_refines \
  source_initial_indexed_cursor_rejects_consumed \
  runtime_epm_owner_generic_agrees \
  runtime_epm_owner_generic_route_exact \
  runtime_epm_owner_generic_root_witness \
  runtime_epm_owner_generic_child_witness; do
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
live_canonical_path_proof_count="$(rg -c "^$proof_declaration_regex" "$live_canonical_path_source")"
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
for proof in "${proofs_rpb3_live_canonical_path_pop_action_lift[@]}"; do
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
if ! rg -q '^verification results:: 5 verified, 0 errors' "$live_canonical_path_pop_log"; then
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
  if ! rg -q "^pub proof fn ${proof}\\(" "$epm_loop_continuation"; then
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

map_eof_indexed_view_proof_count="$(rg -c "^$proof_declaration_regex" "$map_eof_indexed_view_source")"
if [[ "$map_eof_indexed_view_proof_count" -ne "${#map_eof_indexed_view_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map EOF indexed-view proof inventory changed" >&2
  exit 1
fi
map_push_receipt_proof_count="$(rg -c "^$proof_declaration_regex" "$map_push_receipt_source")"
if [[ "$map_push_receipt_proof_count" -ne "${#map_push_receipt_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push receipt proof inventory changed" >&2
  exit 1
fi
map_push_backing_proof_count="$(rg -c "^$proof_declaration_regex" "$map_push_backing_source")"
if [[ "$map_push_backing_proof_count" -ne "${#map_push_backing_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push backing proof inventory changed" >&2
  exit 1
fi
map_push_scoped_classes_proof_count="$(rg -c "^$proof_declaration_regex" "$map_push_scoped_classes_source")"
if [[ "$map_push_scoped_classes_proof_count" -ne "${#map_push_scoped_classes_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push scoped identity proof inventory changed" >&2
  exit 1
fi
map_push_finite_observation_proof_count="$(rg -c "^$proof_declaration_regex" "$map_push_finite_observation_source")"
if [[ "$map_push_finite_observation_proof_count" -ne "${#map_push_finite_observation_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push finite observation proof inventory changed" >&2
  exit 1
fi
map_push_selected_subview_proof_count="$(rg -c "^$proof_declaration_regex" "$map_push_selected_subview_source")"
if [[ "$map_push_selected_subview_proof_count" -ne "${#map_push_selected_subview_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push selected subview proof inventory changed" >&2
  exit 1
fi
map_push_live_parent_bounds_proof_count="$(rg -c "^$proof_declaration_regex" "$map_push_live_parent_bounds_source")"
if [[ "$map_push_live_parent_bounds_proof_count" -ne "${#map_push_live_parent_bounds_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push live parent bounds proof inventory changed" >&2
  exit 1
fi
map_push_fresh_child_proof_count="$(rg -c "^$proof_declaration_regex" "$map_push_fresh_child_source")"
if [[ "$map_push_fresh_child_proof_count" -ne "${#map_push_fresh_child_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push fresh child proof inventory changed" >&2
  exit 1
fi
map_push_live_child_proof_count="$(rg -c "^$proof_declaration_regex" "$map_push_live_child_source")"
if [[ "$map_push_live_child_proof_count" -ne "${#map_push_live_child_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push live child proof inventory changed" >&2
  exit 1
fi
map_push_checked_receipt_proof_count="$(rg -c "^$proof_declaration_regex" "$map_push_checked_receipt_source")"
if [[ "$map_push_checked_receipt_proof_count" -ne "${#map_push_checked_receipt_proofs[@]}" ]]; then
  echo "rpb3-admission-cursor: map push checked receipt proof inventory changed" >&2
  exit 1
fi
source_observation_proof_count="$(rg -c "^$proof_declaration_regex" "$source_observation")"
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
integrated_proof_count="$(rg -c "^$proof_declaration_regex" "$integrated_source")"
integrated_expected_count="${#proofs_rpb3_integrated_admission[@]}"
if [[ "$integrated_proof_count" -ne "$integrated_expected_count" ]]; then
  echo "rpb3-admission-cursor: integrated proof inventory changed" >&2
  exit 1
fi
declare -A integrated_expected_proofs=()
for proof in "${proofs_rpb3_integrated_admission[@]}"; do
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
for proof in "${integrated_helper_proofs[@]}"; do
  proof_log="$evidence_dir/integrated-helper-$proof.log"
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
for proof in "${proofs_rpb3_integrated_dispatcher[@]}"; do
  proof_log="$evidence_dir/integrated-dispatcher-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_integrated_dispatcher --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: integrated dispatcher $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: integrated dispatcher $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
for proof in "${proofs_rpb3_epm_obligation_return_live[@]}"; do
  proof_log="$evidence_dir/epm-obligation-return-live-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_epm_obligation_return_live --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPathMap obligation return live $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPathMap obligation return live $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
epm_obligation_return_live_log="$evidence_dir/epm-obligation-return-live-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_obligation_return_live \
  "$integrated_source" \
  >"$epm_obligation_return_live_log" 2>&1; then
  cat "$epm_obligation_return_live_log"
  echo "rpb3-admission-cursor: EPathMap obligation return live module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 21 verified, 0 errors' "$epm_obligation_return_live_log"; then
  cat "$epm_obligation_return_live_log"
  echo "rpb3-admission-cursor: EPathMap obligation return live module did not verify" >&2
  exit 1
fi
for proof in "${proofs_rpb3_epm_parser_escape_invariant[@]}"; do
  proof_log="$evidence_dir/epm-parser-escape-invariant-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_epm_parser_escape_invariant --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPathMap parser escape invariant $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPathMap parser escape invariant $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
epm_parser_escape_invariant_log="$evidence_dir/epm-parser-escape-invariant-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_parser_escape_invariant \
  "$integrated_source" \
  >"$epm_parser_escape_invariant_log" 2>&1; then
  cat "$epm_parser_escape_invariant_log"
  echo "rpb3-admission-cursor: EPathMap parser escape invariant module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 7 verified, 0 errors' "$epm_parser_escape_invariant_log"; then
  cat "$epm_parser_escape_invariant_log"
  echo "rpb3-admission-cursor: EPathMap parser escape invariant module did not verify" >&2
  exit 1
fi
for proof in "${proofs_rpb3_epm_obligation_return_source[@]}"; do
  proof_log="$evidence_dir/epm-obligation-return-source-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_epm_obligation_return_source --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPathMap obligation return source $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPathMap obligation return source $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
epm_obligation_return_source_log="$evidence_dir/epm-obligation-return-source-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_obligation_return_source \
  "$integrated_source" \
  >"$epm_obligation_return_source_log" 2>&1; then
  cat "$epm_obligation_return_source_log"
  echo "rpb3-admission-cursor: EPathMap obligation return source module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 11 verified, 0 errors' "$epm_obligation_return_source_log"; then
  cat "$epm_obligation_return_source_log"
  echo "rpb3-admission-cursor: EPathMap obligation return source module did not verify" >&2
  exit 1
fi
for proof in "${proofs_rpb3_epm_frame_path_invariant[@]}"; do
  proof_log="$evidence_dir/epm-frame-path-invariant-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_epm_frame_path_invariant --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPathMap frame path invariant $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPathMap frame path invariant $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
epm_frame_path_invariant_log="$evidence_dir/epm-frame-path-invariant-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_frame_path_invariant \
  "$integrated_source" \
  >"$epm_frame_path_invariant_log" 2>&1; then
  cat "$epm_frame_path_invariant_log"
  echo "rpb3-admission-cursor: EPathMap frame path invariant module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 26 verified, 0 errors' "$epm_frame_path_invariant_log"; then
  cat "$epm_frame_path_invariant_log"
  echo "rpb3-admission-cursor: EPathMap frame path invariant module did not verify" >&2
  exit 1
fi
for proof in "${proofs_rpb3_plain_parser_source_refinement[@]}"; do
  proof_log="$evidence_dir/plain-parser-source-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_plain_parser_source_refinement --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: plain parser source $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: plain parser source $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
plain_parser_source_log="$evidence_dir/rpb3_plain_parser_source_refinement.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_plain_parser_source_refinement \
  "$integrated_source" >"$plain_parser_source_log" 2>&1; then
  cat "$plain_parser_source_log"
  echo "rpb3-admission-cursor: plain parser source module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 45 verified, 0 errors' "$plain_parser_source_log"; then
  cat "$plain_parser_source_log"
  echo "rpb3-admission-cursor: plain parser source module did not verify" >&2
  exit 1
fi
sha256sum "$plain_parser_source_log"
for proof in "${proofs_rpb3_epm_path_invariant_integrated[@]}"; do
  proof_log="$evidence_dir/epm-path-invariant-integrated-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_epm_path_invariant_integrated --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPathMap path invariant integrated $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPathMap path invariant integrated $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
epm_path_invariant_integrated_log="$evidence_dir/epm-path-invariant-integrated-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_path_invariant_integrated \
  "$integrated_source" \
  >"$epm_path_invariant_integrated_log" 2>&1; then
  cat "$epm_path_invariant_integrated_log"
  echo "rpb3-admission-cursor: EPathMap path invariant integrated module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 19 verified, 0 errors' "$epm_path_invariant_integrated_log"; then
  cat "$epm_path_invariant_integrated_log"
  echo "rpb3-admission-cursor: EPathMap path invariant integrated module did not verify" >&2
  exit 1
fi
for proof in "${proofs_rpb3_epm_obligation_return_witness[@]}"; do
  proof_log="$evidence_dir/epm-obligation-return-witness-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_epm_obligation_return_witness --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPathMap obligation return witness $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: EPathMap obligation return witness $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
epm_obligation_return_witness_log="$evidence_dir/epm-obligation-return-witness-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_epm_obligation_return_witness \
  "$integrated_source" \
  >"$epm_obligation_return_witness_log" 2>&1; then
  cat "$epm_obligation_return_witness_log"
  echo "rpb3-admission-cursor: EPathMap obligation return witness module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 105 verified, 0 errors' "$epm_obligation_return_witness_log"; then
  cat "$epm_obligation_return_witness_log"
  echo "rpb3-admission-cursor: EPathMap obligation return witness module did not verify" >&2
  exit 1
fi
for proof in "${proofs_rpb3_no_score_child_eof_coverage[@]}"; do
  proof_log="$evidence_dir/no-score-child-eof-coverage-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_no_score_child_eof_coverage --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: no-score child EOF coverage $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: no-score child EOF coverage $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
no_score_child_eof_coverage_log="$evidence_dir/no-score-child-eof-coverage-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_no_score_child_eof_coverage \
  "$integrated_source" \
  >"$no_score_child_eof_coverage_log" 2>&1; then
  cat "$no_score_child_eof_coverage_log"
  echo "rpb3-admission-cursor: no-score child EOF coverage module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 46 verified, 0 errors' "$no_score_child_eof_coverage_log"; then
  cat "$no_score_child_eof_coverage_log"
  echo "rpb3-admission-cursor: no-score child EOF coverage module did not verify" >&2
  exit 1
fi
for proof in "${proofs_rpb3_score_finish_groups[@]}"; do
  proof_log="$evidence_dir/score-finish-groups-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_score_finish_groups --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: score-finish groups $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: score-finish groups $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
score_finish_groups_log="$evidence_dir/score-finish-groups-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_score_finish_groups \
  "$integrated_source" \
  >"$score_finish_groups_log" 2>&1; then
  cat "$score_finish_groups_log"
  echo "rpb3-admission-cursor: score-finish groups module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 97 verified, 0 errors' "$score_finish_groups_log"; then
  cat "$score_finish_groups_log"
  echo "rpb3-admission-cursor: score-finish groups module did not verify" >&2
  exit 1
fi
for proof in "${proofs_rpb3_score_finish_unique[@]}"; do
  proof_log="$evidence_dir/score-finish-unique-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 \
    --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
    --verify-only-module rpb3_score_finish_unique --verify-function "$proof" \
    "$integrated_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: score-finish uniqueness $proof proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: score-finish uniqueness $proof did not verify" >&2
    exit 1
  fi
  sha256sum "$proof_log"
done
score_finish_unique_log="$evidence_dir/score-finish-unique-module.log"
if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
  --no-cheating --num-threads 1 --rlimit 30 --cfg rpb3_integrated --cfg rpb3_live_push \
  --verify-only-module rpb3_score_finish_unique \
  "$integrated_source" \
  >"$score_finish_unique_log" 2>&1; then
  cat "$score_finish_unique_log"
  echo "rpb3-admission-cursor: score-finish uniqueness module failed" >&2
  exit 1
fi
if ! rg -q '^verification results:: 18 verified, 0 errors' "$score_finish_unique_log"; then
  cat "$score_finish_unique_log"
  echo "rpb3-admission-cursor: score-finish uniqueness module did not verify" >&2
  exit 1
fi
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

promoted_module_proofs=(
  canonical_path_epm_internal_prefix_preserves
  nested_epm_internal_prefix_preserves
  program31_epm_internal_prefix_preserves
  linked_epm_internal_prefix_preserves
  epm_terminal_end_preserves_layout
  saved_parser_gate_survives_global_counter_restore
  forward_scalar_prefix_preserves_image
  epm_internal_edges_prefix_wf
  runtime_noscore_expr_structural_modeled_case
)
for proof in "${promoted_module_proofs[@]}"; do
  promoted_source="$model_dir/rpb3_integrated_admission.rs"
  case "$proof" in
    canonical_path_epm_internal_prefix_preserves)
      promoted_flags=(--cfg rpb3_integrated --verify-root)
      promoted_source="$model_dir/rpb3_epathmap_canonical_path_complete_macro_refinement.rs" ;;
    nested_epm_internal_prefix_preserves)
      promoted_flags=(--cfg rpb3_integrated --verify-root)
      promoted_source="$model_dir/rpb3_epathmap_nested_complete_macro_refinement.rs" ;;
    program31_epm_internal_prefix_preserves)
      promoted_flags=(--cfg rpb3_integrated --verify-root)
      promoted_source="$model_dir/rpb3_epathmap_program31_complete_macro_refinement.rs" ;;
    linked_epm_internal_prefix_preserves)
      promoted_flags=(--verify-root)
      promoted_source="$model_dir/rpb3_epathmap_push_macro_refinement.rs" ;;
    epm_terminal_end_preserves_layout)
      promoted_flags=(--cfg rpb3_integrated --cfg rpb3_live_push
        --verify-only-module rpb3_epathmap_terminal_source_observation) ;;
    saved_parser_gate_survives_global_counter_restore)
      promoted_flags=(--cfg rpb3_integrated --cfg rpb3_live_push
        --verify-only-module rpb3_epm_batch_cursor_composition) ;;
    forward_scalar_prefix_preserves_image)
      promoted_flags=(--cfg rpb3_integrated --cfg rpb3_live_push
        --verify-only-module rpb3_epm_loop_continuation_source) ;;
    epm_internal_edges_prefix_wf)
      promoted_flags=(--cfg rpb3_integrated --cfg rpb3_live_push --cfg rpb3_terminal
        --verify-only-module rpb3_integrated_source_trace_composition) ;;
    runtime_noscore_expr_structural_modeled_case)
      promoted_flags=(--cfg rpb3_integrated --cfg rpb3_runtime_standalone --verify-root)
      promoted_source="$model_dir/rpb3_runtime_frame_source_refinement.rs" ;;
    *)
      echo "rpb3-admission-cursor: no Verus flags for promoted proof $proof" >&2
      exit 1 ;;
  esac
  proof_log="$evidence_dir/promoted-$proof.log"
  if ! tlc_bounded timeout "${TLC_RUNTIME_MAX_SEC}s" "$verus_bin" \
    --no-cheating --num-threads 1 --rlimit 30 \
    "${promoted_flags[@]}" --verify-function "$proof" \
    "$promoted_source" >"$proof_log" 2>&1; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: promoted proof $proof failed" >&2
    exit 1
  fi
  if ! rg -q 'verification results:: 1 verified, 0 errors' "$proof_log"; then
    cat "$proof_log"
    echo "rpb3-admission-cursor: promoted proof $proof did not verify" >&2
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
