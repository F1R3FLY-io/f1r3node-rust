#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROOF_ROOT="$ROOT/formal/rocq/cost_accounted_rho"
THEORIES="$PROOF_ROOT/theories"
SLASHING_ROOT="$ROOT/formal/rocq/slashing"
VALIDATOR_ROOT="$ROOT/formal/rocq/validator"
VALIDATOR_THEORIES="$VALIDATOR_ROOT/theories"
VERIFICATION_DOCS=(
  "$ROOT/docs/casper/theory/cost-accounted-rho-verification.md"
  "$ROOT/docs/casper/theory/cost-accounting-migration.md"
  "$ROOT/docs/casper/theory/cost-accounting-use-cases.md"
  "$ROOT/docs/casper/theory/cost-accounting-threat-model.md"
)
WORK_ROOT="$ROOT/target/verification/cost-accounted-rho/proofs"
mkdir -p "$WORK_ROOT"

echo "Checking cost-accounted rho proof hygiene..."

proof_module_count="$(find "$THEORIES" -maxdepth 1 -type f -name '*.v' | wc -l | tr -d ' ')"
listed_module_count="$(rg -N '^[[:space:]]*theories/[A-Za-z0-9_]+\.v[[:space:]]*$' \
  "$PROOF_ROOT/_CoqProject" | wc -l | tr -d ' ')"
proof_line_count="$(find "$THEORIES" -maxdepth 1 -type f -name '*.v' -print0 \
  | xargs -0 wc -l | awk 'END {print $1}')"
proof_term_count="$(rg -o '\b(Qed|Defined)\.' "$THEORIES" --glob '*.v' \
  | wc -l | tr -d ' ')"

if [[ "$proof_module_count" != "$listed_module_count" ]]; then
  echo "error: _CoqProject lists $listed_module_count of $proof_module_count Rocq modules" >&2
  exit 1
fi

verification_doc_plain="$(perl -0pe \
  's/(?<=\d),(?=\d)//g; s/\n[[:space:]]*/ /g' \
  "${VERIFICATION_DOCS[0]}")"
required_scale_claims=(
  "across $proof_module_count modules and $proof_line_count lines of development"
  "All $proof_term_count \`Qed.\`/\`Defined.\` proof terms"
  "development spans $proof_module_count modules and $proof_line_count lines, with $proof_term_count \`Qed.\`"
  "| Rocq source files                                | $proof_module_count modules"
  "| Total lines of Rocq                              | $proof_line_count"
  "| Proven lemmas and theorems (\`Qed.\` / \`Defined.\`) | $proof_term_count"
  "foundational 32-module subgraph of the $proof_module_count-module formalization"
  "current $proof_module_count-module catalog"
)
for claim in "${required_scale_claims[@]}"; do
  if ! rg -q -F "$claim" <<<"$verification_doc_plain"; then
    echo "error: stale or missing Rocq scale claim: $claim" >&2
    exit 1
  fi
done

SANITIZED_THEORIES="$(mktemp -d "$WORK_ROOT/sanitized.XXXXXX")"
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

assumptions="$(mktemp "$WORK_ROOT/assumptions.XXXXXX.log")"
trap 'rm -rf "$SANITIZED_THEORIES"; rm -f "$assumptions"' EXIT

if rg -n '(^|[[:space:]])(Admitted\.|admit\.)|^[[:space:]]*(Conjecture|Parameter)[[:space:]]' "$SANITIZED_THEORIES"; then
  echo "error: found an admitted proof or unsupported declaration" >&2
  exit 1
fi

if rg -n '^[[:space:]]*Axiom[[:space:]]+[A-Za-z0-9_]+[[:space:]]*:' "$SANITIZED_THEORIES"; then
  echo "error: found an axiom in the cost-accounted rho theories" >&2
  exit 1
fi

if rg -n 'TODO|FIXME|deferred|future work|not formally proven|open work' "$SANITIZED_THEORIES" "${VERIFICATION_DOCS[@]}"; then
  echo "error: found an incompletion marker in proof theories or verification docs" >&2
  exit 1
fi

# "placeholder" as an incompletion marker is rejected except for two modeled
# domain terms: unsigned threshold-cosigner slots and the historical unresolved
# PoS template literal that the fail-closed compiler model must name and refute.
# A genuine "placeholder proof / stub / section" carries neither vocabulary and
# is still rejected.
placeholder_hits="$(rg -n 'placeholder' "$SANITIZED_THEORIES" "${VERIFICATION_DOCS[@]}" \
  | rg -iv 'cosigner|threshold|signer|`sig`|\bsig\b|wallet|funding_sig|template|PoS|controller|literal|unresolved' || true)"
if [ -n "$placeholder_hits" ]; then
  printf '%s\n' "$placeholder_hits" >&2
  echo "error: found a non-domain 'placeholder' incompletion marker in proof theories or verification docs" >&2
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
From CostAccountedRho Require Import CostAccountedSyntax TranslationFaithfulness Bisimulation Replication Settlement SlashingComposition MergeableChannelAccounting RuntimeBudgetRefinement AtomicCommAccounting MultiSignerRefinement LinearLogicResources LLIdentities MintingInjection MintingHalt UseCaseAdequacy SystemStructEquiv SyntacticSugar WalletNaming ChannelSeparation TokenConservation FuelEventDecomposition Exchange BoundedLedger GSLTOSLFCapstone Rule45ContinuationAdequacy CAReduction WrappingSubjectReduction SignatureMonoid CATokenConservation CAStrongNormalization CAConfluence CAStepDeterminism CACostDeterminism CAModulus ContinuedGSLTCapstone CAGradedTransition CATranslation CostMonad CATranslationLemmas CATranslationFaithfulness CABisimulation CASettlement CAMintingInjection CAExchange CAEconomicCapstone CALocatedPurses CAGradedAdequacy CAAdjunctions CATypeDiscipline CAOSLFSpatialModal CAGradedImageFinite CAGradedSuccPairs CAGradedCompleteness CAInternalisation CAGradedLimit CAForceSeparation CAJoinConservation CategoryInterface CACategory CACostFunctor CACostFunctorCI CACostMonadCat CAAdjunctionI CACostMonadInstances CASimulationBicat CAAdjunctionII CAProperSubcategory CAAbstractCapstone CAUntypedLambda CAUntypedLambdaCI EndToEndAuthority.
From CostAccountedRho Require Import LocatedAuthoritySettlement RuntimeBoundAuthority PayloadSortPersistence SettlementMergeVisibility StructuralAuthorityBound RuntimeAuthorityScope StackTransferConservation StackIntroductionAtomicity EvaluationTransactionIsolation MergeableEvidenceAuthentication ParallelStackMaterialization StateBoundFrontierExpansion VaultBackedCostLifecycle AtomicVaultSettlementRefinement WalletFundedLollipop FundingSlotBootstrap PoSVaultAuthority PhysicalSettlementWorklist ReplayRootMaterialization CanonicalRevRedemption RedemptionCustodyAtomicity RedemptionMintResumption VaultBackedByteAccounting MultiShardConcurrency DeterministicParallelReduction.
From CostAccountedRho Require Import ThresholdEnvelopeAuthority ReplayAdmissionPublication.
Print Assumptions unmatched_introduction_costs_zero.
Print Assumptions committed_comm_costs_exactly_one.
Print Assumptions trigger_side_does_not_change_cost.
Print Assumptions join_arity_does_not_multiply_cost.
Print Assumptions comm_trace_cost_is_comm_count.
Print Assumptions comm_trace_cost_permutation_invariant.
Print Assumptions replaying_same_comm_trace_has_same_cost.
Print Assumptions rejected_comm_is_atomic.
Print Assumptions funded_comm_debits_before_commit.
Print Assumptions rejected_event_is_state_atomic.
Print Assumptions admitted_event_debits_exactly.
Print Assumptions debit_preserves_unselected_purse.
Print Assumptions explicit_regions_do_not_debit_ambient_purse.
Print Assumptions debit_conserves_each_purse.
Print Assumptions admitted_settlement_conserves.
Print Assumptions plan_permutation_preserves_authority.
Print Assumptions replay_preserves_authority.
Print Assumptions replay_preserves_purse_debit.
Print Assumptions continuation_requires_outer_event.
Print Assumptions cross_deploy_slot_identity_is_replay_stable.
Print Assumptions full_guilty_confiscation_is_rejected.
Print Assumptions burned_redemption_conserves_canonical_rev.
Print Assumptions burned_redemption_removes_circulating_claims.
Print Assumptions failed_evaluation_publishes_nothing.
Print Assumptions unauthorized_generation_is_effect_free.
Print Assumptions full_guilty_penalty_is_effect_free.
Print Assumptions exact_retry_is_idempotent.
Print Assumptions conflicting_retry_is_effect_free.
Print Assumptions vindication_restores_exact_phase.
Print Assumptions guilty_restores_exact_phase.
Print Assumptions resolution_conserves_canonical_rev.
Print Assumptions resolution_preserves_physical_custody.
Print Assumptions distinct_validator_resolutions_commute.
Print Assumptions redemption_preserves_mint_ledger.
Print Assumptions redemption_preserves_vault_balance.
Print Assumptions redemption_removes_only_the_target_halt.
Print Assumptions redemption_cannot_remint_recorded_epoch.
Print Assumptions redemption_enables_exactly_one_fresh_epoch_credit.
Print Assumptions redemption_fresh_epoch_replay_is_idempotent.
Print Assumptions atomic_join_debits_all_or_none.
Print Assumptions bound_authority_is_excluded_from_static_capacity.
Print Assumptions resolved_authority_is_static.
Print Assumptions runtime_resolution_eliminates_bound_levels.
Print Assumptions new_bound_slot_becomes_the_persisted_continuation_authority.
Print Assumptions replay_preserves_the_resolved_slot_identity.
Print Assumptions deterministic_parallel_reduction_end_to_end.
Print Assumptions persistent_path_append_projection.
Print Assumptions persistent_child_path_projection.
Print Assumptions persistent_path_append_allocates_one_node.
Print Assumptions persistent_path_order_refines_sequence_order.
Print Assumptions equal_path_projections_preserve_all_comparisons.
Print Assumptions reserve_success_is_conserving.
Print Assumptions settlement_requires_complete_reservation.
Print Assumptions settlement_is_conserving_and_refunds_exactly.
Print Assumptions mint_is_the_only_supply_growth.
Print Assumptions independent_credit_is_unbacked.
Print Assumptions lollipop_reservation_is_all_or_nothing.
Print Assumptions lollipop_insufficient_continuation_payer_rejects_atomically.
Print Assumptions native_apply_refines_reserve_then_settle.
Print Assumptions native_apply_success_is_visible_and_conserving.
Print Assumptions native_apply_insufficient_bound_has_no_result.
Print Assumptions native_apply_rejects_realized_over_bound.
Print Assumptions aggregate_native_apply_is_order_independent.
Print Assumptions aggregate_native_apply_rejects_overdraw.
Print Assumptions complete_funding_implies_protocol_only_funding.
Print Assumptions migrated_recovery_fixture_complete_boundary.
Print Assumptions pre_state_materialized_before_snapshot.
Print Assumptions next_snapshot_pre_state_is_materialized.
Print Assumptions all_snapshots_use_ordinary_runtime.
Print Assumptions accepted_post_state_is_exact.
Print Assumptions independent_validator_replay_agrees.
Print Assumptions eager_second_snapshot_is_not_materialized_by_genesis.
Print Assumptions replay_runtime_is_not_an_ordinary_snapshot_source.
Print Assumptions outer_address_is_canonical.
Print Assumptions slot_address_is_canonical.
Print Assumptions located_addresses_are_injective.
Print Assumptions public_addresses_are_not_draw_capabilities.
Print Assumptions public_address_alone_never_authorizes_draw.
Print Assumptions retained_slot_capability_authorizes_draw.
Print Assumptions funding_success_is_exact.
Print Assumptions funding_success_is_conserving.
Print Assumptions funding_success_is_not_minting.
Print Assumptions insufficient_sponsor_rejects_both_purses_atomically.
Print Assumptions continuation_activation_requires_prior_funding.
Print Assumptions continuation_activation_requires_gateway_authentication.
Print Assumptions continuation_activation_requires_outer_authority.
Print Assumptions fully_authorized_continuation_activates.
Print Assumptions settlement_requires_prior_funding.
Print Assumptions settlement_requires_gateway_authentication.
Print Assumptions settlement_requires_outer_authority.
Print Assumptions settlement_requires_retained_slot_capability.
Print Assumptions insufficient_outer_purse_rejects_atomically.
Print Assumptions insufficient_slot_purse_rejects_atomically.
Print Assumptions outer_realized_over_bound_rejects_atomically.
Print Assumptions slot_realized_over_bound_rejects_atomically.
Print Assumptions insufficient_gateway_fee_rejects_atomically.
Print Assumptions settlement_success_is_component_exact.
Print Assumptions settlement_success_refunds_both_unused_bounds.
Print Assumptions settlement_success_is_conserving.
Print Assumptions wallet_funding_then_lollipop_is_conserving.
Print Assumptions wallet_funding_then_lollipop_is_component_exact.
Print Assumptions replay_uses_identical_staged_settlement.
Print Assumptions unresolved_templates_fail_closed.
Print Assumptions complete_templates_compile.
Print Assumptions authenticated_install_binds_pos_generator.
Print Assumptions unauthorized_transfer_is_effect_free.
Print Assumptions authenticated_transfer_moves_exactly_one.
Print Assumptions transfer_conserves_custody.
Print Assumptions literal_placeholder_denies_the_authenticated_generator.
Print Assumptions candidate_stack_does_not_inflate_creator_preflight_capacity.
Print Assumptions certification_execution_replay_share_authenticated_environment.
Print Assumptions empty_certification_environment_diverges_on_deployer_identity.
Print Assumptions worklist_solutions_refine_recursive.
Print Assumptions worklist_first_preserves_canonical_candidate_order.
Print Assumptions worklist_success_is_recursive_success.
Print Assumptions worklist_failure_is_recursive_failure.
Print Assumptions storage_preserves_complete_payload.
Print Assumptions free_capture_preserves_complete_payload.
Print Assumptions execution_preserves_complete_payload.
Print Assumptions replay_preserves_complete_payload.
Print Assumptions free_capture_preserves_authority.
Print Assumptions free_capture_preserves_stack_order.
Print Assumptions free_capture_preserves_conditionals.
Print Assumptions settlement_pop_is_event_visible.
Print Assumptions replay_reproduces_settlement_removal.
Print Assumptions same_linear_instance_conflicts.
Print Assumptions distinct_linear_instances_do_not_conflict.
Print Assumptions soft_checkpoint_returns_current_segment.
Print Assumptions soft_checkpoint_clears_active_segment.
Print Assumptions consecutive_soft_checkpoints_are_disjoint.
Print Assumptions checkpoint_segments_reconstruct_execution_trace.
Print Assumptions settlement_extends_trace_prefix.
Print Assumptions realized_authority_never_exceeds_structural_demand.
Print Assumptions runtime_unit_has_zero_demand.
Print Assumptions runtime_unit_is_left_neutral.
Print Assumptions runtime_unit_is_right_neutral.
Print Assumptions concurrent_scope_entry_is_order_independent.
Print Assumptions first_scope_exit_preserves_other_owner_a.
Print Assumptions first_scope_exit_preserves_other_owner_b.
Print Assumptions final_scope_exit_deactivates_accounting.
Print Assumptions duplicate_stack_transfer_is_rejected_atomically.
Print Assumptions underfunded_stack_transfer_is_rejected_atomically.
Print Assumptions funded_fresh_stack_transfer_is_exact.
Print Assumptions funded_stack_transfer_conserves.
Print Assumptions funded_stack_transfer_produces_every_debited_cell.
Print Assumptions funded_stack_transfer_records_one_fresh_event.
Print Assumptions stack_transfer_is_all_or_none.
Print Assumptions replay_stack_transfer_is_identical.
Print Assumptions authorized_mint_is_the_only_supply_increase.
Print Assumptions preparation_is_capacity_conserving_and_invisible.
Print Assumptions byte_rejection_after_preparation_restores_exact_state.
Print Assumptions commit_after_visible_produce_is_complete_and_conserving.
Print Assumptions preparation_cannot_oversubscribe_capacity.
Print Assumptions abort_preserves_unrelated_commit.
Print Assumptions enclosing_deploy_failure_restores_linear_capacity.
Print Assumptions enclosing_deploy_failure_preserves_attempted_byte_cost.
Print Assumptions every_matched_produce_is_causally_extracted.
Print Assumptions matched_produces_precede_their_comm.
Print Assumptions parser_failure_cannot_reuse_prior_witness.
Print Assumptions reducer_failure_retains_exact_attempted_work.
Print Assumptions rejected_play_restores_its_base_state.
Print Assumptions rejected_replay_restores_its_base_state.
Print Assumptions rejected_replay_discards_its_prevalidation_checkpoint.
Print Assumptions rollback_preserves_attempted_work.
Print Assumptions rejected_replay_publishes_no_mergeable_evidence.
Print Assumptions accepted_replay_publishes_mergeable_evidence.
Print Assumptions published_mergeable_evidence_requires_final_state_match.
Print Assumptions complete_key_is_injective.
Print Assumptions distinct_pre_states_have_distinct_keys.
Print Assumptions distinct_post_states_have_distinct_keys.
Print Assumptions distinct_creators_have_distinct_keys.
Print Assumptions distinct_sequences_have_distinct_keys.
Print Assumptions distinct_payloads_have_distinct_keys.
Print Assumptions legacy_key_alias_witness.
Print Assumptions local_replay_publishes_exact_evidence.
Print Assumptions peer_response_cannot_publish_evidence.
Print Assumptions peer_response_cannot_overwrite_local_replay.
Print Assumptions distinct_replays_preserve_both_entries.
Print Assumptions distinct_insertions_commute_pointwise.
Print Assumptions opposite_arrival_orders_agree_on_every_lookup.
Print Assumptions complete_key_deletion_removes_exact_execution.
Print Assumptions complete_key_deletion_preserves_distinct_execution.
Print Assumptions deletion_after_distinct_replays_preserves_survivor.
Print Assumptions deletion_is_idempotent.
Print Assumptions deletion_commutes_with_distinct_insertion_pointwise.
Print Assumptions retirement_requires_concrete_latest_witness.
Print Assumptions retirement_requires_every_safety_guard.
Print Assumptions vacuous_latest_guard_is_unsafe.
Print Assumptions full_dag_retirement_accepts_every_parent_path.
Print Assumptions main_spine_only_retirement_is_incomplete.
Print Assumptions initial_materialization_conserves.
Print Assumptions parent_reduction_requires_materialized_initial_purse.
Print Assumptions parent_reduction_conserves.
Print Assumptions nested_materialization_requires_parent_commit.
Print Assumptions nested_materialization_conserves.
Print Assumptions premature_parent_reduction_is_rejected.
Print Assumptions premature_nested_materialization_is_rejected.
Print Assumptions phased_evaluation_reaches_exact_state.
Print Assumptions reducer_scheduling_preference_is_irrelevant.
Print Assumptions phased_evaluation_conserves.
Print Assumptions replay_reproduces_materialization_and_reduction.
Print Assumptions authenticated_capacity_append_monotone.
Print Assumptions positive_backing_strictly_expands_capacity.
Print Assumptions authenticated_prefix_is_bounded_by_total.
Print Assumptions frontier_retry_count_is_finite.
Print Assumptions speculative_exhaustion_does_not_publish_state.
Print Assumptions discovered_backing_conserves_total_supply.
Print Assumptions candidate_created_supply_cannot_expand_prestate_capacity.
Print Assumptions replay_uses_the_same_authenticated_capacity.
Print Assumptions admitted_trace_fits_budget.
Print Assumptions introduction_consumes_no_comm_execution_unit.
Print Assumptions communication_consumes_one_execution_unit.
Print Assumptions communication_execution_unit_is_independent_of_join_arity.
Print Assumptions trigger_side_does_not_change_byte_debit.
Print Assumptions trigger_arrival_order_does_not_change_total_debit.
Print Assumptions join_transfer_includes_every_participant.
Print Assumptions adding_join_participant_adds_exact_transfer_cost.
Print Assumptions stable_persistent_identity_is_charged_once_across_retries.
Print Assumptions nonpersistent_identity_preserves_attempt_multiplicity.
Print Assumptions v1_debit_is_canonical_encoded_footprint.
Print Assumptions trace_debit_is_product_sum.
Print Assumptions trace_debit_permutation_invariant.
Print Assumptions persistent_introduction_is_charged_once_and_each_delivery_is_charged.
Print Assumptions peek_neither_charges_nor_refunds.
Print Assumptions rejected_byte_event_is_atomic.
Print Assumptions run_byte_trace_acceptance_is_exact.
Print Assumptions run_byte_trace_preserves_hard_ceiling.
Print Assumptions accepted_permutations_have_identical_settlement.
Print Assumptions replay_byte_trace_accepts_iff_exact.
Print Assumptions replay_acceptance_binds_event_kind_and_amount.
Print Assumptions replay_rejects_changed_event_kind.
Print Assumptions top_up_is_a_conserving_transfer.
Print Assumptions top_up_commutes_with_running_settlement.
Print Assumptions top_up_does_not_expand_inflight_reservation.
Print Assumptions empty_registry_resolves_to_deploy_payer.
Print Assumptions fallback_resolution_is_atomically_pinned.
Print Assumptions registered_resolution_returns_the_committed_payer.
Print Assumptions fallback_pin_rejects_a_late_conflicting_registration.
Print Assumptions inserted_introduction_authority_is_stable.
Print Assumptions same_payer_registration_is_idempotent.
Print Assumptions conflicting_registration_is_rejected_without_overwrite.
Print Assumptions deploy_reset_discards_every_registered_introduction_authority.
Print Assumptions authority_neutral_stack_keeps_storage_neutral_and_charges_sponsor.
Print Assumptions lollipop_receiver_introduction_charges_outer_not_continuation.
Print Assumptions continuation_created_introduction_charges_continuation.
Print Assumptions stored_interaction_authority_cannot_redirect_introduction_charge.
Print Assumptions unmetered_introduction_has_no_authority_debit.
Print Assumptions candidate_created_stack_cannot_supply_prestate_byte_capacity.
Print Assumptions single_cell_authority_settlement_matches_processed_trace.
Print Assumptions byte_trace_settlement_conserves_rev.
Print Assumptions byte_trace_refines_single_comm_execution.
Print Assumptions admitted_witness_is_funded.
Print Assumptions left_branch_fits_reservation.
Print Assumptions pointwise_funded_realized_is_funded.
Print Assumptions left_branch_fits_pointwise_max.
Print Assumptions pointwise_settlement_conserves.
Print Assumptions pointwise_refund_is_unused_reservation.
Print Assumptions exact_settlement_conserves.
Print Assumptions local_fault_never_slashes.
Print Assumptions validation_origin_independent.
Print Assumptions every_origin_replays_checkpoint_and_checks_bonds.
Print Assumptions finality_parent_permutation_invariant.
Print Assumptions genesis_allocation_total_permutation.
Print Assumptions permutation_genesis_replay_agrees.
Print Assumptions genesis_replay_agreement_preserves_admission.
Print Assumptions duplicate_genesis_allocations_combine.
Print Assumptions genesis_system_vault_funding_is_exact.
Print Assumptions committed_genesis_system_vault_funding_is_idempotent.
Print Assumptions genesis_system_vault_replay_agrees.
Print Assumptions admission_requires_verified_genesis.
Print Assumptions genesis_unit_execution_replay_agrees.
Print Assumptions genesis_unit_execution_rejects_funder_replay.
Print Assumptions capacity_exactly_characterizes_funding.
Print Assumptions exhausted_execution_cannot_be_certified.
Print Assumptions state_bound_certificate_funds_committed_cost.
Print Assumptions state_bound_chain_preserves_adjacent_roots.
Print Assumptions admitted_costs_are_funded.
Print Assumptions state_bound_exact_settlement_conserves.
Print Assumptions cost_accounted_calculus_is_gslt_with_oslf_logic.
Print Assumptions oslf_spatial_modal_logic_sound.
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
Print Assumptions admission_sig_algebra_valid_sound.
Print Assumptions admission_sig_algebra_scalar_policy_sound.
Print Assumptions admission_sig_algebra_quorum_sound.
(* DR-25: untyped-lambda R1-only cost instance (CAUntypedLambda + CAUntypedLambdaCI) *)
Print Assumptions lca_only_beta_r1.
Print Assumptions lca_contact_requires_token.
Print Assumptions lca_stack_inert.
Print Assumptions lca_funded_nonredex_stuck.
Print Assumptions lca_step_needs_fuel.
Print Assumptions lca_step_decreases.
Print Assumptions lca_funded_run_bounded.
Print Assumptions lca_well_founded.
Print Assumptions lca_SN_funded.
Print Assumptions omega_pure_diverges.
Print Assumptions lca_omega_funded_one_step.
Print Assumptions lca_omega_funded_halts.
Print Assumptions lca_beta_r1_erasure.
Print Assumptions lca_graded_step_sound.
Print Assumptions lca_step_gradable.
Print Assumptions Lambda_ciGSLT_nonvacuous.
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
Print Assumptions exchange_preserves_stack_identity_and_order.
Print Assumptions exchange_preserves_resource_multiset.
Print Assumptions exchange_preserves_resource_cell_count.
Print Assumptions exchange_resource_join_requires_both.
Print Assumptions exchange_resource_join_one_sided_is_inert.
Print Assumptions ca_exchange_total_conserved.
Print Assumptions ca_exchange_preserves_stack_identity_and_order.
Print Assumptions ca_exchange_preserves_resource_cell_count.
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
Print Assumptions join_key_atoms_perm.
Print Assumptions join_authority_conserved_operational.
Print Assumptions reverse_curry_iso.
Print Assumptions join_demand_partition_invariant.
Print Assumptions join_no_weakening.
Print Assumptions rho_object_nonvacuous.
Print Assumptions graded_bisim_trans.
Print Assumptions cost_is_endofunctor.
Print Assumptions cost_obj_closure.
Print Assumptions CostCI.
Print Assumptions cost_ci_preserves_bisim.
Print Assumptions cost_ci_preserves_step.
Print Assumptions cost_ci_preserves_quote_faithful.
Print Assumptions cost_is_monad.
Print Assumptions cost_monad_instance.
Print Assumptions free_forget_adjunction.
Print Assumptions cost_kleisli_adjunction.
Print Assumptions proper_subcategory.
Print Assumptions U_not_eso.
Print Assumptions sim_2cells_form_setoid.
Print Assumptions internalisation_adjoint_retraction.
Print Assumptions internalisation_retraction_param.
Print Assumptions rho_internalisable.
Print Assumptions rho_internalises_by_interpreter.
Print Assumptions continued_gslt_cost_abstract_capstone.
Print Assumptions consumed_fuel_count_eq_token_drop.
Print Assumptions consumed_comm_count_determined_by_endpoints.
Print Assumptions mint_inject_not_ca_step.
Print Assumptions user_ca_step_does_not_mint.
Print Assumptions admin_trans_mint_adds_exactly.
Print Assumptions system_vault_credit_injective_in_pk.
Print Assumptions system_vault_credit_names_distinct.
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
Print Assumptions debit_plus_refund_eq_reservation.
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
Print Assumptions canonical_slash_candidate_requires_current_cost_evidence.
Print Assumptions stale_canonical_slash_candidate_not_authorized.
Print Assumptions missing_evidence_cannot_select_canonical_slash_candidate.
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
Print Assumptions integer_diff_total_permutation.
Print Assumptions integer_total_result_permutation.
Print Assumptions integer_selection_application_agree.
Print Assumptions widened_total_ignores_invalid_prefix_when_final_result_fits.
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
Print Assumptions rb_diagnostic_refinement_requires_commitment.
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
Print Assumptions rb_diagnostic_refinement_rejects_absent_commitment.
Print Assumptions rb_reset_from_token_retention_bound_zero.
Print Assumptions rb_unmetered_reserve_preserves_trace.
Print Assumptions rb_meter_identity_unmetered_reserve_many_preserves_state.
Print Assumptions rb_meter_identity_unmetered_child_many_preserves_state.
Print Assumptions rb_meter_identity_metered_reserve_uses_next_identity.
Print Assumptions rb_meter_identity_scoped_unmetered_work_preserves_next_metered_identity.
Print Assumptions rb_meter_identity_scoped_unmetered_children_preserve_next_metered_identity.
Print Assumptions uc_ca_001_budget_conservation.
Print Assumptions uc_ca_002_weighted_event_refines_unit_token_expansion.
Print Assumptions uc_ca_004_parallel_terminal_cost_determinism.
Print Assumptions uc_ca_005_well_reflected_replay_step_sound.
Print Assumptions uc_ca_009_debit_plus_refund_equals_reservation.
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
Print Assumptions uc_ca_027_settlement_exhaustion_and_fee_only.
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
Print Assumptions uc_ca_043_matched_unmatched_deploy_trace_and_settlement_isolation.
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
Print Assumptions uc_ca_146_canonical_slash_candidate_requires_current_evidence.
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
Print Assumptions exact_spend_check_sound_complete.
Print Assumptions exact_linear_check_sound.
Print Assumptions exact_linear_check_complete.
Print Assumptions linear_forbids_contraction.
Print Assumptions linear_forbids_weakening.
Print Assumptions modal_poststate_is_exact.
Print Assumptions modal_spend_preserves_other_surface.
Print Assumptions located_observation_isolates_other_surface.
Print Assumptions spatial_is_commutative.
Print Assumptions spatial_local_sufficiency_composes.
Print Assumptions conservative_sufficiency_is_sound.
Print Assumptions upper_bound_cannot_assert_modal_spend.
Print Assumptions upper_bound_insufficient_supply_rejects.
Print Assumptions authenticated_supply_excludes_candidate_credit.
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
Print Assumptions fee_transfer_conserves.
Print Assumptions fee_recipient_credit_eq_client_debit.
Print Assumptions fee_transfer_zero_is_noop.
Print Assumptions native_fee_credit_is_backed.
Print Assumptions native_fee_transfer_conserves_holding.
Print Assumptions native_fee_transfer_zero_is_noop.
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
Print Assumptions system_vault_name_injective.
Print Assumptions domain_name_injective.
Print Assumptions system_vault_quarantine_domain_disjoint.
Print Assumptions system_vault_funding_slot_domain_disjoint.
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
(* item 2494 / 2505 — the i64-bounded ledger: over/underflow are MODELED
   (conserved OR deterministically rejected), the nat model is the in-range
   restriction, no existing guarantee weakened. *)
Print Assumptions checked_add_i64_conserved_or_rejected.
Print Assumptions checked_add_i64_never_wraps.
Print Assumptions checked_add_i64_none_iff_overflow.
Print Assumptions checked_add_i64_some_in_range.
Print Assumptions checked_sub_nonneg_conserved_or_rejected.
Print Assumptions checked_add_i64_matches_nat.
Print Assumptions vault_credit_conserved_or_rejected.
Print Assumptions bounded_settlement_conserved_or_rejected.
Print Assumptions bounded_fee_transfer_conserved_or_rejected.
(* item 2509 — consensus-core vs diagnostic-only split of the full replay payload. *)
Print Assumptions rb_full_replay_payload_equiv_split.
Print Assumptions rb_full_replay_payload_equiv_implies_consensus.
Print Assumptions rb_full_replay_payload_consensus_coarser_than_full.
From CostAccountedRho Require Import BlockHeapLifecycle.
Print Assumptions positive_interval_counter_is_bounded.
Print Assumptions trim_is_requested_exactly_at_the_boundary.
Print Assumptions every_block_default_reclaims_retained_heap.
Print Assumptions block_reclamation_is_semantically_invisible.
Print Assumptions reclamation_choice_does_not_change_semantic_commits.
Print Assumptions safe_finish_preserves_retained_counter_bound.
Print Assumptions default_boundary_bounds_resident_heap.
Print Assumptions missing_boundary_reclamation_exceeds_two_slot_envelope.
From CostAccountedRho Require Import MultiShardConcurrency.
Print Assumptions foreign_action_trace_preserves_protected_shard.
Print Assumptions action_trace_preserves_per_shard_conservation.
Print Assumptions action_trace_preserves_per_shard_root_alignment.
Print Assumptions distinct_shard_actions_commute_pointwise.
Print Assumptions admitted_serial_commits_have_no_lost_updates.
Print Assumptions admitted_serial_commits_preserve_conservation.
Print Assumptions shared_worker_capstone.
From CostAccountedRho Require Import FundingSlotBootstrap.
Print Assumptions scaffold_install_is_conserving.
Print Assumptions eager_located_install_rejects_new_zero_purses.
Print Assumptions staged_scaffold_install_needs_no_candidate_purse_supply.
Print Assumptions dual_purse_funding_is_exact_and_conserving.
Print Assumptions insufficient_dual_purse_funding_is_atomic.
Print Assumptions rejected_dual_purse_funding_preserves_registry.
Print Assumptions accepted_dual_purse_funding_creates_both_vaults.
Print Assumptions eager_target_creation_breaks_rejection_atomicity.
Print Assumptions slot_only_funding_cannot_satisfy_positive_outer_bound.
Print Assumptions dual_funding_establishes_local_sufficiency.
Print Assumptions activation_requires_gateway_authentication.
Print Assumptions activation_requires_both_located_purses.
Print Assumptions activated_lollipop_settlement_is_exact_and_conserving.
Print Assumptions witnesses_are_exactly_selected.
Print Assumptions every_witness_is_a_policy_member.
Print Assumptions funding_is_exact_selected_projection.
Print Assumptions unsigned_policy_member_is_not_funded.
Print Assumptions native_and_ethereum_schemes_share_ground_authority.
Print Assumptions policy_rejects_duplicate_ground_owner.
Print Assumptions different_presence_changes_commitment_preimage.
Print Assumptions equal_commitment_preimage_has_equal_authority_projection.
Print Assumptions accepted_threshold_meets_quorum.
Print Assumptions verified_partition_covers_every_candidate.
Print Assumptions verified_partition_has_no_dual_disposition.
Print Assumptions processed_evidence_is_exactly_admitted.
Print Assumptions count_equality_does_not_establish_identity_equality.
Print Assumptions primary_signature_identity_is_not_injective.
Print Assumptions absent_publication_inserts_complete_evidence.
Print Assumptions identical_publication_is_idempotent.
Print Assumptions conflicting_publication_preserves_existing_store.
Print Assumptions peer_bytes_leave_store_unchanged.
Print Assumptions durable_proof_precedes_cache_entry.
Print Assumptions crash_exposes_before_or_after_transaction.
Print Assumptions validators_with_equal_artifacts_publish_equal_evidence.
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
Print Assumptions validator_contract_P1_epoch_advance.
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
Print Assumptions main_T9_3_block_exception_is_local_fault.
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
Print Assumptions main_T9_12_stale_generation_evidence_not_authorized.
Print Assumptions main_T9_12_epoch_advance_preserves_lifetime.
Print Assumptions main_T9_13_unknown_slash_evidence_noop.
Print Assumptions main_T9_13_zero_canonical_bond_not_authorized.
Print Assumptions main_T9_13_matching_lifetime_current_window_authorized.
Print Assumptions main_T9_13_canonical_pre_state_authorizes_when_ambient_differs.
Print Assumptions main_T9_13_canonical_zero_rejects_even_if_ambient_positive.
Print Assumptions main_T9_13_proposer_receiver_authorization_parity.
Print Assumptions main_T9_13_same_pre_state_root_same_authorization.
Print Assumptions main_T9_13_merge_rejected_hint_subsumed_by_authorized_scan.
Print Assumptions main_T9_13_zero_bond_candidate_not_selected.
Print Assumptions main_T9_13_selected_target_keys_nodup.
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
Print Assumptions redeem_full_guilty_rejected.
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
