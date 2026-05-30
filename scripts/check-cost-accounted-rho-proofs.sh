#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROOF_ROOT="$ROOT/formal/rocq/cost_accounted_rho"
THEORIES="$PROOF_ROOT/theories"
VERIFICATION_DOCS=(
  "$ROOT/docs/theory/cost-accounted-rho-verification.md"
  "$ROOT/docs/theory/cost-accounting-migration.md"
  "$ROOT/docs/theory/cost-accounting-use-cases.md"
  "$ROOT/docs/theory/cost-accounting-threat-model.md"
)

echo "Checking cost-accounted rho proof hygiene..."

SANITIZED_THEORIES="$(mktemp -d)"
for proof in "$THEORIES"/*.v; do
  perl -0pe 's/\(\*.*?\*\)//gs' "$proof" > "$SANITIZED_THEORIES/$(basename "$proof")"
done

assumptions="$(mktemp)"
trap 'rm -rf "$SANITIZED_THEORIES"; rm -f "$assumptions"' EXIT

if rg -n '(^|[[:space:]])(Admitted\.|admit\.)|^[[:space:]]*(Conjecture|Parameter)[[:space:]]' "$SANITIZED_THEORIES"; then
  echo "error: found an admitted proof or unsupported declaration" >&2
  exit 1
fi

if rg -n '^[[:space:]]*Axiom[[:space:]]+[A-Za-z0-9_]+[[:space:]]*:' "$SANITIZED_THEORIES"; then
  echo "error: found an axiom in the cost-accounted rho theories" >&2
  exit 1
fi

if rg -n 'TODO|FIXME|deferred|future work|placeholder|not formally proven|open work' "$SANITIZED_THEORIES" "${VERIFICATION_DOCS[@]}"; then
  echo "error: found an incompletion marker in proof theories or verification docs" >&2
  exit 1
fi

echo "Compiling and checking Rocq theories..."
(
  cd "$PROOF_ROOT"
  rocq makefile -f _CoqProject -o Makefile >/dev/null
  make -j"${ROCQ_JOBS:-2}" >/dev/null
  proof_modules=()
  while IFS= read -r proof; do
    module="${proof#theories/}"
    module="${module%.v}"
    proof_modules+=("CostAccountedRho.${module}")
  done < <(rg -N '^[[:space:]]*theories/[A-Za-z0-9_]+\.v[[:space:]]*$' _CoqProject | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
  rocqchk -Q theories CostAccountedRho "${proof_modules[@]}" >/dev/null 2>&1
)

if ! rocq repl -Q "$THEORIES" CostAccountedRho > "$assumptions" 2>&1 <<'EOF'
From CostAccountedRho Require Import TranslationFaithfulness Bisimulation Replication Settlement SlashingComposition MergeableChannelAccounting RuntimeBudgetRefinement MultiSignerRefinement LinearLogicResources LLIdentities MintingInjection MintingHalt UseCaseAdequacy SystemStructEquiv SyntacticSugar WalletNaming ChannelSeparation TokenConservation.
Print Assumptions mint_inject_not_ca_step.
Print Assumptions user_ca_step_does_not_mint.
Print Assumptions admin_trans_mint_adds_exactly.
Print Assumptions supply_write_injective_in_pk.
Print Assumptions supply_wallet_disjoint.
Print Assumptions epoch_mint_idempotent_on_balance.
Print Assumptions user_ca_step_does_not_increase_balance.
Print Assumptions halted_validator_supply_not_increased.
Print Assumptions halted_validator_not_minted.
Print Assumptions credit_implies_not_halted.
Print Assumptions translation_faithful.
Print Assumptions translation_strong_bisimilar_generic.
Print Assumptions compound_gate_per_step_reverse.
Print Assumptions backward_reflection_phased_gate.
Print Assumptions well_reflected_backward_reflection.
Print Assumptions recursively_metered_backward_reflection.
Print Assumptions preplicate_bang_encoding_body_barbs_sound.
Print Assumptions replication_encoding_forward_barb_sound.
Print Assumptions charged_plus_refund_eq_escrow.
Print Assumptions post_evaluation_settlement_no_mint.
Print Assumptions slash_preserves_fee_settlement_inputs.
Print Assumptions slash_preserves_settled_amount.
Print Assumptions slash_after_evaluation_cannot_add_fuel.
Print Assumptions cost_invalid_block_evidence_does_not_change_user_cost.
Print Assumptions slash_system_effect_is_unmetered_for_user_budget.
Print Assumptions current_cost_evidence_epoch_sound.
Print Assumptions parent_pre_state_authorizes_current_cost_evidence.
Print Assumptions parent_pre_state_authorization_requires_parent_bond.
Print Assumptions ambient_bond_does_not_authorize_without_parent_pre_state.
Print Assumptions recovered_rejected_slash_requires_current_cost_evidence.
Print Assumptions stale_recovered_slash_not_authorized.
Print Assumptions parent_pre_state_authorized_slash_preserves_cost_boundary.
Print Assumptions zero_bond_slash_noop_preserves_cost_boundary.
Print Assumptions bitmask_diff_merge_round_trip.
Print Assumptions mergeable_channel_bitmask_fold_permutation.
Print Assumptions integer_add_diff_merge_round_trip.
Print Assumptions mergeable_channel_delta_preserves_type.
Print Assumptions non_numeric_channel_not_mergeable_payload_match.
Print Assumptions mergeable_channel_accounting_preserves_user_budget.
Print Assumptions mergeable_channel_accounting_preserves_fee_settlement_inputs.
Print Assumptions rb_total_remaining_conservation.
Print Assumptions rb_reserve_oop_commits_limit.
Print Assumptions rb_reserve_first_oop_commits_boundary.
Print Assumptions rb_reserve_many_conservation.
Print Assumptions rb_reserve_many_oop_count_le_one.
Print Assumptions rb_reserve_many_unmetered_no_cost.
Print Assumptions rb_replay_payload_user_trace_change_detected.
Print Assumptions rb_replay_payload_system_trace_change_detected.
Print Assumptions rb_replay_payload_canonical_user_trace_permutation.
Print Assumptions rb_full_replay_payload_signature_change_detected.
Print Assumptions rb_full_replay_payload_system_kind_change_detected.
Print Assumptions rb_full_replay_payload_genesis_change_detected.
Print Assumptions rb_diagnostic_cap_preserves_budget_observables.
Print Assumptions rb_finalize_trace_window_preserves_budget_observables.
Print Assumptions rb_cost_trace_change_detected.
Print Assumptions rb_cost_trace_event_count_success_and_oop.
Print Assumptions rb_post_activation_cost_trace_present_matches_count.
Print Assumptions rb_post_activation_cost_trace_commitment_valid.
Print Assumptions rb_empty_cost_trace_commitment_can_be_valid.
Print Assumptions rb_cost_accounted_replay_requires_commitment.
Print Assumptions rb_legacy_replay_accepts_absent_commitment.
Print Assumptions rb_oop_trace_survives_boundary.
Print Assumptions rb_oversized_weight_rejection_preserves_trace.
Print Assumptions rb_zero_weight_admission_rejection_preserves_trace.
Print Assumptions rb_oversized_weight_admission_rejection_preserves_trace.
Print Assumptions rb_oversized_source_path_admission_rejection_preserves_trace.
Print Assumptions rb_oversized_primitive_descriptor_admission_rejection_preserves_trace.
Print Assumptions rb_admitted_success_has_admissible_event.
Print Assumptions rb_admitted_success_has_positive_bounded_weight.
Print Assumptions rb_trace_cap_rejection_preserves_trace.
Print Assumptions rb_repeated_oop_boundary_frontier.
Print Assumptions rb_repeated_oop_preserves_first_boundary.
Print Assumptions rb_trace_cap_frontier_preserves_budget_and_trace.
Print Assumptions rb_multi_deploy_settlement_frontier.
Print Assumptions rb_nonbillable_frame_preserves_trace.
Print Assumptions rb_block_auth_payload_replay_payload_change_detected.
Print Assumptions rb_replay_cache_key_payload_change_detected.
Print Assumptions rb_full_replay_payload_user_cost_change_detected.
Print Assumptions rb_full_replay_payload_user_cost_trace_change_detected.
Print Assumptions rb_full_replay_payload_user_cost_trace_event_count_change_detected.
Print Assumptions rb_full_replay_payload_user_cost_trace_present_change_detected.
Print Assumptions rb_full_replay_payload_missing_cost_trace_change_detected.
Print Assumptions rb_full_replay_payload_user_failed_change_detected.
Print Assumptions rb_full_replay_payload_user_error_change_detected.
Print Assumptions rb_full_replay_payload_system_error_change_detected.
Print Assumptions rb_full_replay_payload_slash_fields_change_detected.
Print Assumptions rb_full_replay_payload_slash_target_epoch_change_detected.
Print Assumptions rb_sum_settlement_app.
Print Assumptions low_deploy_price_violation_sound.
Print Assumptions low_deploy_price_violation_complete.
Print Assumptions unauthorized_fee_settlement_sound.
Print Assumptions unauthorized_budget_mutation_sound.
Print Assumptions stale_cost_evidence_sound.
Print Assumptions stale_cost_evidence_complete.
Print Assumptions rb_trace_entry_kind_domain_separated.
Print Assumptions rb_trace_entry_deploy_change_detected.
Print Assumptions rb_trace_entry_source_path_change_detected.
Print Assumptions rb_trace_entry_redex_change_detected.
Print Assumptions rb_trace_entry_local_index_change_detected.
Print Assumptions rb_trace_entry_billable_kind_change_detected.
Print Assumptions rb_trace_entry_primitive_descriptor_change_detected.
Print Assumptions rb_trace_entry_weight_change_detected.
Print Assumptions rb_trace_duplicate_multiplicity_detected.
Print Assumptions rb_cost_accounted_replay_rejects_absent_commitment.
Print Assumptions rb_reset_from_token_retention_bound_zero.
Print Assumptions rb_unmetered_reserve_preserves_trace.
Print Assumptions uc_ca_001_budget_conservation.
Print Assumptions uc_ca_002_weighted_event_refines_unit_token_expansion.
Print Assumptions uc_ca_004_parallel_terminal_cost_determinism.
Print Assumptions uc_ca_005_well_reflected_replay_step_sound.
Print Assumptions uc_ca_009_charged_plus_refund_equals_escrow.
Print Assumptions uc_ca_010_replay_cost_mismatch_sound.
Print Assumptions uc_ca_012_slashing_preserves_settlement_accounting.
Print Assumptions uc_ca_013_runtime_budget_conserves_consumed_remaining.
Print Assumptions uc_ca_014_weighted_runtime_event_refines_unit_count.
Print Assumptions uc_ca_018_replay_payload_user_trace_change_detected.
Print Assumptions uc_ca_019_replay_payload_system_trace_change_detected.
Print Assumptions uc_ca_020_replay_payload_user_trace_permutation_equiv.
Print Assumptions uc_ca_021_replay_payload_system_trace_permutation_equiv.
Print Assumptions uc_ca_022_replay_payload_signature_change_detected.
Print Assumptions uc_ca_023_replay_payload_system_kind_change_detected.
Print Assumptions uc_ca_024_reservation_batch_preserves_budget_conservation.
Print Assumptions uc_ca_025_reservation_batch_has_at_most_one_oop.
Print Assumptions uc_ca_026_unmetered_batch_no_cost.
Print Assumptions uc_ca_027_settlement_exhaustion_and_zero_price.
Print Assumptions uc_ca_028_slashing_after_evaluation_cannot_add_fuel.
Print Assumptions uc_ca_029_diagnostic_log_cap_preserves_budget_observables.
Print Assumptions uc_ca_030_replay_payload_genesis_change_detected.
Print Assumptions uc_ca_031_finalization_reads_completed_cost_trace.
Print Assumptions uc_ca_032_cost_trace_canonicalization_and_sensitivity.
Print Assumptions uc_ca_033_replay_payload_full_field_sensitivity.
Print Assumptions uc_ca_034_multi_deploy_budget_isolation_and_settlement_sum.
Print Assumptions uc_ca_035_unmetered_system_mode_restoration.
Print Assumptions uc_ca_036_diagnostic_retention_is_non_consensus.
Print Assumptions uc_ca_037_trace_mismatch_preserves_settlement_accounting.
Print Assumptions uc_ca_038_legacy_metering_quarantine.
Print Assumptions uc_ca_039_post_activation_cost_trace_required.
Print Assumptions uc_ca_040_full_replay_payload_authenticates_cost_trace_fields.
Print Assumptions uc_ca_041_concurrent_finalization_trace_completeness.
Print Assumptions uc_ca_042_oop_trace_survives_failed_deploy_boundary.
Print Assumptions uc_ca_043_mixed_deploy_block_trace_and_settlement_isolation.
Print Assumptions uc_ca_044_oversized_weight_rejection_preserves_trace.
Print Assumptions uc_ca_045_nonbillable_frames_do_not_enter_cost_trace.
Print Assumptions uc_ca_046_zero_event_post_activation_trace_commitment.
Print Assumptions uc_ca_047_block_authenticates_cost_trace_payload.
Print Assumptions uc_ca_048_replay_cache_key_authenticates_cost_trace_payload.
Print Assumptions uc_ca_049_legacy_replay_quarantines_absent_cost_trace.
Print Assumptions uc_ca_050_billable_reservation_enters_cost_trace.
Print Assumptions uc_ca_051_parallel_trace_and_cost_determinism.
Print Assumptions uc_ca_052_cost_trace_mismatch_slashing_boundary.
Print Assumptions uc_ca_053_cost_trace_domain_separation_and_multiplicity.
Print Assumptions uc_ca_054_activation_replay_rejects_absent_commitment.
Print Assumptions uc_ca_055_unauthorized_settlement_and_budget_mutation_are_cost_invalid.
Print Assumptions uc_ca_056_low_deploy_price_is_cost_invalid_evidence.
Print Assumptions uc_ca_057_stale_cost_invalid_evidence_is_rejected.
Print Assumptions uc_ca_058_refund_cannot_replenish_runtime_fuel.
Print Assumptions uc_ca_059_deterministic_billable_descriptor_sensitivity.
Print Assumptions uc_ca_060_reset_clears_retained_trace_after_finalization.
Print Assumptions uc_ca_061_system_mode_cannot_leak_into_user_metering.
Print Assumptions uc_ca_062_block_validation_authenticates_cost_fields.
Print Assumptions uc_ca_063_threaded_oop_boundary_ownership.
Print Assumptions uc_ca_064_external_nondeterminism_requires_replay_evidence.
Print Assumptions uc_ca_065_zero_weight_billable_event_rejected.
Print Assumptions uc_ca_066_oversized_billable_event_rejected.
Print Assumptions uc_ca_067_trace_cap_rejection_preserves_budget.
Print Assumptions uc_ca_068_admitted_success_has_positive_bounded_weight.
Print Assumptions uc_ca_069_producer_routing_search_frontier.
Print Assumptions uc_ca_070_trace_slot_linearizability_frontier.
Print Assumptions uc_ca_071_replay_mutation_frontier.
Print Assumptions uc_ca_072_multi_deploy_settlement_frontier.
Print Assumptions uc_ca_073_slashing_composition_frontier.
Print Assumptions uc_ca_074_resource_exhaustion_frontier.
Print Assumptions uc_ca_141_typed_mergeable_channel_type_preservation.
Print Assumptions uc_ca_142_bitmask_or_diff_merge_round_trip.
Print Assumptions uc_ca_143_bitmask_or_fold_order_independent.
Print Assumptions uc_ca_144_integer_add_diff_merge_round_trip.
Print Assumptions uc_ca_145_mergeable_channel_accounting_preserves_cost_boundary.
Print Assumptions uc_ca_146_recovered_slash_requires_current_cost_evidence.
Print Assumptions uc_ca_147_parent_pre_state_slash_authorization_preserves_cost_boundary.
Print Assumptions uc_ca_148_slash_target_epoch_is_replay_authenticated.
Print Assumptions uc_ca_149_zero_bond_slash_noop_preserves_cost_boundary.
Print Assumptions pos_map_currentdeploys_invariant.
Print Assumptions pos_refund_no_cross_attribution.
Print Assumptions pos_precharge_failure_atomic.
Print Assumptions fifo_drain_conservation.
Print Assumptions ll_tensor_min_required_matches_runtime.
Print Assumptions ll_threshold_min_required_matches_runtime.
Print Assumptions ll_plus_left_min_required_matches_runtime.
Print Assumptions ll_plus_right_min_required_matches_runtime.
Print Assumptions ll_with_min_required_matches_runtime.
Print Assumptions ll_bang_min_required_matches_runtime.
Print Assumptions ll_whynot_min_required_matches_runtime.
Print Assumptions ll_lolly_min_required_matches_runtime.
Print Assumptions ll_all_required_uses_all_atoms.
Print Assumptions ll_threshold_validity_bounds_runtime_quorum.
Print Assumptions ll_sig_algebra_required_complete.
Print Assumptions ll_sig_algebra_consumed_matches_presented.
Print Assumptions ll_sig_algebra_threshold_valid_bounds_bridge.
Print Assumptions dill_linear_identity.
Print Assumptions dill_tensor_combines_linear_contexts.
Print Assumptions dill_unrestricted_claim_uses_no_linear_witness.
Print Assumptions dill_lolly_modus_ponens_consumes_input_context.
Print Assumptions dill_whynot_intro_uses_no_linear_witness.
Print Assumptions ll_plus_left_consumes_chosen_branch.
Print Assumptions ll_plus_right_consumes_chosen_branch.
Print Assumptions ll_with_requires_both_branches_available.
Print Assumptions ll_bang_reuse_no_extra_linear_cost.
Print Assumptions ll_whynot_consumes_no_linear_witness.
Print Assumptions ll_lolly_resource_flow_conservative.
Print Assumptions ll_threshold_quorum_sound.
Print Assumptions ll_linear_no_contraction.
Print Assumptions ll_linear_no_weakening.
Print Assumptions ll_linear_atom_contraction_changes_count.
Print Assumptions ll_consume_linear_once_atom_exhausts.
Print Assumptions ll_no_double_spend_single_witness.
Print Assumptions ll_double_spend_requires_duplicate_witness.
Print Assumptions ll_unrestricted_reuse_preserves_context.
Print Assumptions ll_unrestricted_can_be_reused.
Print Assumptions ll_linear_cut_consumes_cut_witness.
Print Assumptions ll_unrestricted_cut_preserves_linear_zone.
Print Assumptions core_demand_invariant_under_extension.
Print Assumptions extension_demand_ge_core.
Print Assumptions delta_s_tensor_additive.
Print Assumptions funding_decidable.
Print Assumptions sigma_s_balance_eq_stack_count.
Print Assumptions funding_check_balance_sound.
Print Assumptions funding_check_balance_sound_against_stack.
Print Assumptions competing_funding_at_most_one_succeeds.
Print Assumptions admit_prefix_maximal.
Print Assumptions reject_both_sound.
Print Assumptions reject_both_from_first_overshoot.
Print Assumptions settlement_conserves.
Print Assumptions accept_commit_conserves.
Print Assumptions sig_free_names_quote.
Print Assumptions sse_par_unit.
Print Assumptions token_decomp.
Print Assumptions uniform_sugar_translation_equiv.
Print Assumptions lollipop_sugar_translation_equiv.
Print Assumptions wallet_name_injective.
Print Assumptions domain_name_injective.
Print Assumptions wallet_quarantine_domain_disjoint.
Print Assumptions wallet_funding_slot_domain_disjoint.
Print Assumptions quarantine_funding_slot_domain_disjoint.
Print Assumptions lane_pool_disjoint.
Print Assumptions lane_key_not_app_channel.
Print Assumptions rb_pool_total_cost_eq_sum.
Print Assumptions rb_lane_reconcile_preserves_valid.
Print Assumptions rb_pool_reconcile_preserves_valid.
Print Assumptions rb_pool_total_cost_permutation_invariant.
Print Assumptions rb_pool_reconciled_total_cost_permutation_invariant.
Print Assumptions rb_pool_singleton_eq_scalar.
Print Assumptions rb_pool_total_cost_metered_eq_consumed_sum.
Quit.
EOF
then
  echo "error: failed to query headline theorem assumptions" >&2
  sed -n '1,160p' "$assumptions" >&2
  exit 1
fi

closed_count="$( (rg -o 'Closed under the global context' "$assumptions" || true) | wc -l | tr -d ' ')"
expected_closed_count="$(rg -c '^[[:space:]]*Print Assumptions ' "$0")"
if [ "$closed_count" -ne "$expected_closed_count" ]; then
  echo "error: headline theorems have unexpected assumptions" >&2
  sed -n '/Print Assumptions/,$p' "$assumptions" >&2
  exit 1
fi

echo "Proof hygiene check passed."
