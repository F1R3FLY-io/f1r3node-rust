#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROOF_ROOT="$ROOT/formal/rocq/cost_accounted_rho"
THEORIES="$PROOF_ROOT/theories"
SLASHING_ROOT="$ROOT/formal/rocq/slashing"
VALIDATOR_ROOT="$ROOT/formal/rocq/validator"
VALIDATOR_THEORIES="$VALIDATOR_ROOT/theories"
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
# Validator behavioral-contract aggregation (Workstream E, stage E5): a thin
# subtree that NAMES the contract by re-exporting already-proven obligations.
# Subject it to the same Admitted/Axiom/incompletion-marker hygiene gate.
for proof in "$VALIDATOR_THEORIES"/*.v; do
  perl -0pe 's/\(\*.*?\*\)//gs' "$proof" > "$SANITIZED_THEORIES/validator__$(basename "$proof")"
done
# Slashing development (Stage-C two-effect slash + redemption; #14): the
# validator-contract dependency compiled below. It was previously compiled but
# NOT axiom-gated; subject its theories to the same Admitted/Axiom/incompletion
# hygiene scan as the cost-accounted + validator trees.
for proof in "$SLASHING_ROOT/theories"/*.v; do
  perl -0pe 's/\(\*.*?\*\)//gs' "$proof" > "$SANITIZED_THEORIES/slashing__$(basename "$proof")"
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

echo "Compiling the Slashing development (validator contract dependency)..."
(
  cd "$SLASHING_ROOT"
  rocq makefile -f _CoqProject -o Makefile >/dev/null
  make -j"${ROCQ_JOBS:-2}" >/dev/null
)

echo "Compiling and checking the validator contract aggregation..."
(
  cd "$VALIDATOR_ROOT"
  rocq makefile -f _CoqProject -o Makefile >/dev/null 2>&1
  make -j"${ROCQ_JOBS:-2}" >/dev/null
  rocqchk -Q ../cost_accounted_rho/theories CostAccountedRho \
          -Q ../slashing/theories Slashing \
          -Q theories Validator Validator.Contract >/dev/null 2>&1
)

if ! rocq repl -Q "$THEORIES" CostAccountedRho > "$assumptions" 2>&1 <<'EOF'
From CostAccountedRho Require Import TranslationFaithfulness Bisimulation Replication Settlement SlashingComposition MergeableChannelAccounting RuntimeBudgetRefinement MultiSignerRefinement LinearLogicResources LLIdentities MintingInjection MintingHalt UseCaseAdequacy SystemStructEquiv SyntacticSugar WalletNaming ChannelSeparation TokenConservation FuelEventDecomposition Exchange GSLTOSLFCapstone Rule45ContinuationAdequacy CAReduction WrappingSubjectReduction SignatureMonoid CATokenConservation CAStrongNormalization CAConfluence CAStepDeterminism CACostDeterminism CAModulus ContinuedGSLTCapstone CAGradedTransition CATranslation CostMonad CATranslationLemmas CATranslationFaithfulness CABisimulation CASettlement CAMintingInjection CAExchange CAEconomicCapstone CALocatedPurses CAGradedAdequacy CAAdjunctions CATypeDiscipline CAGradedImageFinite CAGradedSuccPairs CAGradedCompleteness CAInternalisation CAGradedLimit CAForceSeparation CAJoinConservation.
Print Assumptions cost_accounted_calculus_is_gslt_with_oslf_logic.
Print Assumptions sig_monoid_comm.
Print Assumptions sig_monoid_assoc.
Print Assumptions sig_monoid_unit_l.
Print Assumptions sig_monoid_unit_r.
Print Assumptions tok_concat_assoc.
Print Assumptions tok_concat_unit_r.
Print Assumptions token_size_concat.
Print Assumptions tok_concat_not_commutative.
Print Assumptions continuation_seal_is_cost_irrelevant.
Print Assumptions rule45_result_cost_independent_of_seal.
Print Assumptions subject_reduction_wrapping.
Print Assumptions no_leak_requires_token.
Print Assumptions no_leak_stack_inert.
Print Assumptions gap2_split_combined_keeps_own_seal.
Print Assumptions gap2_split_split_keeps_own_seal.
Print Assumptions st_token_count_subst_invariant.
Print Assumptions ca_step_needs_fuel.
Print Assumptions funded_step_decreases.
Print Assumptions closed_deref_zero_ca.
Print Assumptions deref_subst_closed_ca.
Print Assumptions linear_subst_many_fuel_le.
Print Assumptions extract_sends_join_sends.
Print Assumptions signed_sends_injective.
Print Assumptions signed_sends_fuel.
Print Assumptions ca_step_join2_det.
Print Assumptions ca_SN_funded.
Print Assumptions st_total_fuel_can_increase_off_funded.
Print Assumptions ca_local_confluence.
Print Assumptions ca_step_rule1_det.
Print Assumptions ca_step_deterministic.
Print Assumptions single_token_path_unique.
Print Assumptions newman_funded.
Print Assumptions ca_cost_deterministic_funded.
Print Assumptions funded_run_bounded.
Print Assumptions continued_gslt_cost_capstone.
Print Assumptions graded_step_sound.
Print Assumptions graded_step_complete.
Print Assumptions graded_iff_step.
Print Assumptions gdia_complete.
Print Assumptions N_tr_closed.
Print Assumptions T_tr_closed.
Print Assumptions cost_map_id.
Print Assumptions cost_map_compose.
Print Assumptions cost_left_unit.
Print Assumptions cost_right_unit.
Print Assumptions cost_assoc.
Print Assumptions cost_eta_natural.
Print Assumptions cost_mu_natural.
Print Assumptions cost_monad_not_idempotent.
Print Assumptions lift_st_to_proc.
Print Assumptions subst_st_to_proc.
Print Assumptions lift_lift_comm.
Print Assumptions lift_subst_ca.
Print Assumptions lift_lift_compose_proc.
Print Assumptions lift_proc_S_compose.
Print Assumptions lift_lift_comm_proc.
Print Assumptions Nt_lift_inv.
Print Assumptions Nt_subst_inv.
Print Assumptions Tt_lift_inv.
Print Assumptions Tt_subst_inv.
Print Assumptions trd_bridge.
Print Assumptions st_trd_zero.
Print Assumptions rule1_reachable.
Print Assumptions rule2_reachable.
Print Assumptions rule5_reachable.
Print Assumptions Split_closed.
Print Assumptions Split_fires.
Print Assumptions rho_reachable_par_l.
Print Assumptions rule3_reachable.
Print Assumptions rule4_reachable.
Print Assumptions ca_translation_progresses.
Print Assumptions ca_single_gate_bisimilar.
Print Assumptions ca_funded_reachable_monotone.
Print Assumptions ca_post_evaluation_settlement_no_mint.
Print Assumptions mint_inject_st_not_ca_step.
Print Assumptions ca_admin_fuel_classified.
Print Assumptions ca_exchange_total_conserved.
Print Assumptions ca_exchange_is_step_not_mint.
Print Assumptions ca_economic_conservation.
Print Assumptions local_sufficiency_composes.
Print Assumptions draw_disjoint.
Print Assumptions graded_adequacy_sound.
Print Assumptions cost_forget_install.
Print Assumptions cost_install_forget_alters.
Print Assumptions ca_linear_no_contraction.
Print Assumptions ca_lolly_consumes_input.
Print Assumptions graded_image_finite.
Print Assumptions graded_image_finite_pairs.
Print Assumptions graded_dichotomy.
Print Assumptions graded_finitary_adequacy.
Print Assumptions ca_internalisation_retraction.
Print Assumptions ca_eta_is_weak_bisim_section.
Print Assumptions graded_limit_adequacy.
Print Assumptions graded_bisim_refines_approximants.
Print Assumptions graded_bisim_implies_hml.
Print Assumptions graded_bisim_n_monotone.
Print Assumptions graded_coinductive_completeness_modulo.
Print Assumptions graded_coinductive_hml_completeness_modulo.
Print Assumptions gated_translation_stuck.
Print Assumptions ca_force_overgating_separation.
Print Assumptions ca_force_overgating_nonvacuous.
Print Assumptions join_authority_conserved.
Print Assumptions reverse_curry_iso.
Print Assumptions join_demand_partition_invariant.
Print Assumptions join_no_weakening.
Print Assumptions consumed_fuel_count_eq_token_drop.
Print Assumptions consumed_comm_count_determined_by_endpoints.
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
Print Assumptions slash_two_effect_is_unmetered_for_user_budget.
Print Assumptions redeem_system_effect_is_unmetered_for_user_budget.
Print Assumptions redeem_preserves_fee_settlement_inputs.
Print Assumptions redeem_conserving_effect_preserves_tracked_funds.
Print Assumptions slash_two_effect_preserves_user_cost_observables.
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
Print Assumptions compound_demand_splits_to_components.
Print Assumptions funding_decidable.
Print Assumptions sigma_s_balance_eq_stack_count.
Print Assumptions funding_check_balance_sound.
Print Assumptions funding_check_balance_sound_against_stack.
Print Assumptions strict_reject_when_underfunded.
Print Assumptions strict_absent_pool_rejects_positive_demand.
Print Assumptions competing_funding_at_most_one_succeeds.
Print Assumptions admit_prefix_maximal.
Print Assumptions reject_both_sound.
Print Assumptions reject_both_from_first_overshoot.
Print Assumptions settlement_conserves.
Print Assumptions accept_commit_conserves.
Print Assumptions compound_split_debit_conserves.
Print Assumptions compound_split_debit_no_underflow.
Print Assumptions multi_settlement_conserves.
Print Assumptions compound_debit_is_block_settlement_instance.
Print Assumptions fee_collection_conserves.
Print Assumptions fee_collect_then_convert_conserves.
Print Assumptions fee_convert_credit_is_backed.
Print Assumptions fee_convert_conserves_holding.
Print Assumptions fee_convert_zero_is_noop.
Print Assumptions exchange_conserves_per_channel.
Print Assumptions exchange_total_conserved.
Print Assumptions exchange_requires_both_inputs.
Print Assumptions exchange_is_ca_step_not_amint.
Print Assumptions exchange_mints_nothing.
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

# Validator behavioral contract (Workstream E, stage E5): assert every
# validator_contract_* clause is axiom-free. Each clause is a re-export of an
# already-axiom-free obligation (S1-S4 from CostAccountedRho, P1/P2 from
# Slashing), so it inherits "Closed under the global context". Append to the
# SAME assumptions file so the closed-count invariant below covers these
# Print Assumptions lines too (the $0 grep counts them).
if ! rocq repl -Q "$THEORIES" CostAccountedRho \
               -Q "$SLASHING_ROOT/theories" Slashing \
               -Q "$VALIDATOR_THEORIES" Validator >> "$assumptions" 2>&1 <<'EOF'
From Validator Require Import Contract.
Print Assumptions validator_contract_S1.
Print Assumptions validator_contract_S2.
Print Assumptions validator_contract_S3.
Print Assumptions validator_contract_S4.
Print Assumptions validator_contract_P1.
Print Assumptions validator_contract_P1_effect.
Print Assumptions validator_contract_P2.
Print Assumptions validator_contract_P3.
Quit.
EOF
then
  echo "error: failed to query validator contract assumptions" >&2
  sed -n '1,160p' "$assumptions" >&2
  exit 1
fi

# Slashing development headline theorems (#14 StageC formal hardening). The
# slashing tree was compiled above (validator-contract dependency) but not
# axiom-gated. Print Assumptions its headline results — the MainTheorem.v
# composition (T-1..T-12 + T-9.x, incl. the top-level
# main_slashing_algorithm_correct), the ValidatorRedemption.v redemption set
# (incl. redeem_burned_stays_halted, the spec's TERMINAL-Burned anchor: a burned
# validator stays halted, faithful to "minting contingent on good behaviour",
# cost-accounted-rho.tex l.2368-2369 / l.3108-3109), and the un-composed
# BugFixAtomicBufferDagTransition.v T-9.20. Appended to the SAME $assumptions
# file so the closed-count invariant below counts them. The in-tree hygiene scan
# above rejects Admitted/Axiom in the slashing sources; these Print Assumptions
# additionally reject any IMPORTED (library) axiom the regex cannot see.
if ! rocq repl -Q "$SLASHING_ROOT/theories" Slashing >> "$assumptions" 2>&1 <<'EOF'
From Slashing Require Import MainTheorem ValidatorRedemption BugFixAtomicBufferDagTransition.
Print Assumptions main_T1_detection_sound.
Print Assumptions main_T2_detection_complete.
Print Assumptions main_T3_slashable_taxonomy.
Print Assumptions main_T4_record_monotone.
Print Assumptions main_T5_record_unique.
Print Assumptions main_T7_slash_zeros_bond.
Print Assumptions main_T9_slash_idempotent.
Print Assumptions main_TIdem_zero_bond_noop.
Print Assumptions main_T10_fork_choice_exclusion.
Print Assumptions main_T9_1_ignorable.
Print Assumptions main_T9_2_atomic.
Print Assumptions main_T9_3_dispatch.
Print Assumptions main_T9_4_transfer.
Print Assumptions main_T9_5_stake_zero.
Print Assumptions main_T9_6_self_regression.
Print Assumptions main_T9_7_seqnum_density.
Print Assumptions main_T9_7_seqnum_density_dense_subsumption.
Print Assumptions main_T9_7_seqnum_density_same_branch_stability.
Print Assumptions main_T9_7_seqnum_density_memoized_equivalent.
Print Assumptions main_T9_8_unbonded.
Print Assumptions main_T9_9_self_correcting.
Print Assumptions main_T9_10_withdraw_transfer_failure.
Print Assumptions main_T9_10_failure_preserves_total_funds.
Print Assumptions main_T9_10_withdraw_independence.
Print Assumptions main_T9_12_stale_evidence_not_authorized.
Print Assumptions main_T9_13_unknown_slash_evidence_noop.
Print Assumptions main_T9_13_zero_parent_bond_not_authorized.
Print Assumptions main_T9_13_positive_parent_bond_authorizes_matching_candidate.
Print Assumptions main_T9_13_parent_pre_state_authorizes_when_ambient_zero.
Print Assumptions main_T9_13_parent_zero_rejects_even_if_ambient_positive.
Print Assumptions main_T9_13_recoverable_rejected_slash_hashes_nodup.
Print Assumptions main_T9_13_own_detected_hash_not_recovered.
Print Assumptions main_T9_13_uncovered_rejected_hash_recovered.
Print Assumptions main_T9_13_recoverable_rejected_slash_requires_current_evidence.
Print Assumptions main_TAuth_invalid_token_noop.
Print Assumptions main_TAuth_valid_token_equiv.
Print Assumptions main_TSlash_seed_input_hash_injective.
Print Assumptions main_TSlash_deploy_seed_uses_invalid_block_hash.
Print Assumptions main_T9_14_checked_pred_positive.
Print Assumptions main_T9_15_duplicate_justifications_rejected.
Print Assumptions main_T12_bft_quorum.
Print Assumptions main_T12_closure_depth_bound.
Print Assumptions main_T12_evidence_monotone.
Print Assumptions main_T12_no_seed_empty_closure.
Print Assumptions main_T12_reports_do_not_suppress_direct.
Print Assumptions main_T12_unreported_visible_edge_remains_active.
Print Assumptions main_T12_report_growth_antitone.
Print Assumptions main_T12_view_merge_overapproximates_left.
Print Assumptions main_T12_view_merge_overapproximates_right.
Print Assumptions main_T12_view_merge_commutative.
Print Assumptions main_T12_validator_renaming_equiv.
Print Assumptions main_T9_11_detector_traversal_fuel_bound.
Print Assumptions main_T9_11_detector_branch_traversal_fixed_bound.
Print Assumptions main_T12_temporal_retention_boundary.
Print Assumptions main_T12_temporal_retention_under_window.
Print Assumptions main_T9_2_n_threads.
Print Assumptions main_T4_record_lifecycle_retains_hash.
Print Assumptions main_T9_6_dag.
Print Assumptions main_T6_detect_neglected_sound.
Print Assumptions main_T6_detect_neglected_complete.
Print Assumptions main_slashing_algorithm_correct.
Print Assumptions redeem_vindicated_restores.
Print Assumptions redeem_guilty_redistributes.
Print Assumptions redeem_burned_conserves.
Print Assumptions redeem_burned_stays_halted.
Print Assumptions redeem_requires_quarantine.
Print Assumptions redeem_authorized_only.
Print Assumptions slash_then_redeem_conserves_total.
Print Assumptions slash_then_redeem_burned_reduces_total_by_bond.
Print Assumptions redeem_other_unchanged.
Print Assumptions t_9_20_recon.
Print Assumptions t_9_20_reconcile_idempotent.
Print Assumptions t_9_20_step_idempotent_on_projection.
Quit.
EOF
then
  echo "error: failed to query slashing development assumptions" >&2
  sed -n '1,200p' "$assumptions" >&2
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
