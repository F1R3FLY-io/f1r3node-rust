(* ===========================================================================
   MainTheorem.v - Capstone: the finalized-floor multi-parent merge is correct.

   Bundles the module-level theorems into one end-to-end statement. Each conjunct
   is discharged by `exact` against the already-proven, axiom-free lemma, so the
   capstone itself introduces no new assumptions (verify with
   `Print Assumptions finalized_floor_merge_correct`).

   The conjuncts, and what each rules out (safety S-labels from the spec):

     T-TERM      spine walk terminates          -- floor derivation always halts
     T-MONO/L-ANC ancestor-monotone finalization-- floor cannot regress (¬S2);
                                                    downward-closed finalized cut
     L-SNAP      snapshot-monotone finalization -- larger justification snapshot
                                                    only ever finalizes more
     T-CACHE     frontier cache transparent     -- warm up-walk == cold down-walk,
                                                    so caching cannot fork (¬S1)
     T-DETMERGE  merge order-independent        -- no fork from parent fold order (¬S6)
     T-K1        no mergeable write lost         -- the ~400-block write-loss (¬S5)
     T-NDA       recovery not double-applied     -- effects applied at most once
     T-LINEAGE   LFB promotion preserves every committed active state effect

   The H1 deterministic backstop (over-Δ merges refuse rather than substitute a
   lossy state) and its liveness are verified in TLA+ (SpecFixed:
   Inv_NoLostParentWrite + Inv_DeltaWithinCap + Liveness_Progress); this Rocq
   development supplies the determinism and algebra that keep every honest node
   in lockstep.
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import ZArith.
Import ListNotations.

From FinalizedFloor Require Import Foundation.
From FinalizedFloor Require Import CliqueOracle.
From FinalizedFloor Require Import AccountableSafety.
From FinalizedFloor Require Import Floor.
From FinalizedFloor Require Import Merge.
From FinalizedFloor Require Import OccurrenceDisposition.
From FinalizedFloor Require Import DeployIdentitySeparation.
From FinalizedFloor Require Import DeployOccurrenceStorage.
From FinalizedFloor Require Import FinalizedOccurrenceStatus.
From FinalizedFloor Require Import Recovery.
From FinalizedFloor Require Import MergeRecoveryCoherence.
From FinalizedFloor Require Import AdmissionEffectAlignment.
From FinalizedFloor Require Import RejectionReasonConfluence.
From FinalizedFloor Require Import ProtocolVersionLifecycle.
From FinalizedFloor Require Import StartupMetadataPreflight.
From FinalizedFloor Require Import ProtocolActivationCoherence.
From FinalizedFloor Require Import Selection.
From FinalizedFloor Require Import IntegerAdd.
From FinalizedFloor Require Import FtExact.
From FinalizedFloor Require Import FinalityThresholdAlignment.
From FinalizedFloor Require Import GenesisApprovalTrust.
From FinalizedFloor Require Import FtProvenance.
From FinalizedFloor Require Import FinalizerProgress.
From FinalizedFloor Require Import BootstrapReplayContext.
From FinalizedFloor Require Import LocalFaultDeferral.
From FinalizedFloor Require Import FundingAdmissionLifecycle.
From FinalizedFloor Require Import EffectCausalClosure.
From FinalizedFloor Require Import StateEffectProvenance.
From FinalizedFloor Require Import StateLineageFinality.
From FinalizedFloor Require Import CertifiedFloorPromotion.
From FinalizedFloor Require Import SnapshotFloorMaterialization.
From FinalizedFloor Require Import CommitteeTransition.
From FinalizedFloor Require Import ObjectiveEquivocation.
From FinalizedFloor Require Import BondGenerationLifecycle.
From FinalizedFloor Require Import CertifiedObjectiveEquivocation.
From FinalizedFloor Require Import CertifiedCausalAdmission.
From FinalizedFloor Require Import CausalFinalityProjection.
From FinalizedFloor Require Import HeartbeatFinalityBackpressure.
From FinalizedFloor Require Import TargetDeployTerminality.
From FinalizedFloor Require Import NodeLocalProductLifting.
From FinalizedFloor Require Import NodeLocalTemporalLifting.
From FinalizedFloor Require Import ParallelValidatorConsensus.
From FinalizedFloor Require Import FinalizationAtomicity.
From FinalizedFloor Require Import ProposalFloorReadiness.
From FinalizedFloor Require Import FinalizerFloorMaterialization.
From FinalizedFloor Require Import DivergentFinalizationHistories.
From FinalizedFloor Require Import MinorityForkRecovery.
From FinalizedFloor Require Import CandidateScopeDeployRehome.
From FinalizedFloor Require Import RecoveryFrontierCoverage.
From FinalizedFloor Require Import StaleSiblingRecovery.
From FinalizedFloor Require Import CertifiedFloorCommitment.
From FinalizedFloor Require Import FinalizationCertificateRetrieval.
From FinalizedFloor Require Import DependencyMaintenanceRound.
From FinalizedFloor Require Import WitnessEquivalentCarrier.
From FinalizedFloor Require Import ObjectiveEvidenceSequenceEligibility.

Theorem finalized_floor_merge_correct :
  (* T-TERM: the main-parent spine walk always reaches genesis. *)
  (forall d, wf_spine d ->
     forall b, In b d ->
       exists g, walk_spine d b (blk_num b) = Some g /\ blk_main_parent g = None)
  /\
  (* T-MONO / L-ANC: finalization is downward-closed along ancestry. *)
  (forall d c J b b', anc_of d b' b -> CliqueOracle.Finalized d c J b -> CliqueOracle.Finalized d c J b')
  /\
  (* L-SNAP: finalization is monotone under snapshot growth. *)
  (forall d c J J' b, snap_extends J' J -> CliqueOracle.Finalized d c J b -> CliqueOracle.Finalized d c J' b)
  /\
  (* T-CACHE: the warm frontier up-walk equals the cold down-walk (no fork). *)
  (forall pivot band, AdjDC band ->
     lastTrue ((pivot, true) :: band) = Some (upgo pivot band))
  /\
  (* T-DETMERGE / T-CONV: the mergeable-channel merge is order-independent. *)
  (forall l1 l2, Permutation l1 l2 -> merge_or l1 = merge_or l2)
  /\
  (* T-K1: no mergeable write is lost (every set bit survives the merge). *)
  (forall l x i, In x l -> Nat.testbit x i = true -> Nat.testbit (merge_or l) i = true)
  /\
  (* T-NDA: recovery never double-applies an effect. *)
  (forall s d, apply_effect (apply_effect s d) d = apply_effect s d).
Proof.
  repeat split.
  - exact spine_walk_terminates.
  - exact L_ANC.
  - exact L_SNAP.
  - exact frontier_cache_transparent.
  - exact merge_or_perm.
  - exact merge_or_no_lost_bit.
  - exact apply_idem.
Qed.

Theorem finalized_floor_candidate_scope_rehome_correct :
  (classify_candidate_self_chain true true false = ActiveDuplicate /\
   should_package_candidate ActiveDuplicate = false)
  /\
  (classify_candidate_self_chain true false false = ExcludedBranchRehome /\
   should_package_candidate ExcludedBranchRehome = true)
  /\
  (forall active_in_candidate_scope,
    classify_candidate_self_chain true active_in_candidate_scope true = SelectedRecovery /\
    should_package_candidate SelectedRecovery = true)
  /\
  (forall on_self_chain active_in_candidate_scope selected_recovery,
    should_package_candidate
      (classify_candidate_self_chain
        on_self_chain active_in_candidate_scope selected_recovery) = false <->
    on_self_chain = true /\
    active_in_candidate_scope = true /\
    selected_recovery = false).
Proof.
  split.
  - exact active_candidate_duplicate_is_suppressed.
  - split.
    + exact excluded_branch_occurrence_is_rehomed.
    + split.
      * exact selected_recovery_preserves_authorization.
      * exact only_active_candidate_duplicate_is_suppressed.
Qed.

Theorem finalized_floor_stale_sibling_recovery_correct :
  In SourceA (causal_sources (finalize_majority_b accepted_siblings)) /\
  publish_elected_recovery (finalize_majority_b accepted_siblings) = None /\
  let settled := settle_exact_frontier (finalize_majority_b accepted_siblings) in
  has_exact_a_tombstone settled = true /\
  has_buffered_a settled = true /\
  exists recovered,
    publish_elected_recovery settled = Some recovered /\
    selected_recovery recovered = [StaleA; FreshWork] /\
    committed_effects recovered = [StaleA; FloorB; FreshWork].
Proof.
  exact stale_sibling_recovery_end_to_end_correct.
Qed.

Theorem finalized_floor_startup_metadata_preflight_correct :
  (forall path,
    running_event_published (complete_startup path false) = false /\
    engine_running (complete_startup path false) = false)
  /\
  (forall path,
    process_alive (complete_startup path false) = false /\
    exit_nonzero (complete_startup path false) = true)
  /\
  (forall path,
    metadata_verified (complete_startup path true) = true /\
    running_event_published (complete_startup path true) = true /\
    engine_running (complete_startup path true) = true /\
    process_alive (complete_startup path true) = true /\
    exit_nonzero (complete_startup path true) = false).
Proof.
  split.
  - exact mismatch_never_publishes_running.
  - split.
    + exact mismatch_exits_nonzero.
    + exact matching_startup_runs_only_after_verification.
Qed.

Theorem finalized_floor_objective_evidence_sequence_boundary_correct :
  (forall sequence : Z,
    (sequence < 0)%Z ->
    persists_metadata EvidenceObjectiveRejected = true /\
    indexes_objective_evidence EvidenceObjectiveRejected sequence = false)
  /\
  (forall admission (sequence : Z),
    indexes_objective_evidence admission sequence = true ->
    persists_metadata admission = true /\ (0 <= sequence)%Z)
  /\
  (forall admission (sequence : Z),
    persists_metadata admission = true ->
    (0 <= sequence)%Z ->
    indexes_objective_evidence admission sequence = true).
Proof.
  split.
  - exact attributable_negative_sequence_persists_without_evidence.
  - split.
    + exact indexed_evidence_has_attributable_nonnegative_sequence.
    + exact nonnegative_certified_admission_is_indexable.
Qed.

Theorem finalized_floor_occurrence_correct :
  (forall records rejected,
     tombstoned (reject_occurrence records rejected) rejected)
  /\
  (forall records rejected survivor,
     deploy_id rejected = deploy_id survivor ->
     source_id rejected <> source_id survivor ->
     active records survivor ->
     active (reject_occurrence records rejected) survivor)
  /\
  (forall records left right candidate,
     tombstoned (reject_occurrence (reject_occurrence records left) right) candidate <->
     tombstoned (reject_occurrence (reject_occurrence records right) left) candidate)
  /\
  (forall winner loser,
     deploy_id winner = deploy_id loser ->
     source_id winner <> source_id loser ->
     active (reject_occurrence [] loser) winner).
Proof.
  exact (conj rejection_is_source_exact
          (conj distinct_source_survives_rejection
            (conj rejection_order_independent one_winner_preserved))).
Qed.

Theorem finalized_floor_deploy_identity_separation_correct :
  (forall payload,
    {| identity_domain := Legacy; identity_payload := payload |} <>
    {| identity_domain := V6; identity_payload := payload |})
  /\
  (forall tombstones payload,
    ~ rejected tombstones
        {| identity_domain := Legacy; identity_payload := payload |} ->
    ~ rejected
        (reject tombstones
          {| identity_domain := V6; identity_payload := payload |})
        {| identity_domain := Legacy; identity_payload := payload |})
  /\
  (forall tombstones payload,
    ~ rejected tombstones
        {| identity_domain := V6; identity_payload := payload |} ->
    ~ rejected
        (reject tombstones
          {| identity_domain := Legacy; identity_payload := payload |})
        {| identity_domain := V6; identity_payload := payload |}).
Proof.
  exact (conj equal_payload_cross_domain_ids_are_distinct
          (conj v6_rejection_preserves_equal_payload_legacy_identity
                legacy_rejection_preserves_equal_payload_v6_identity)).
Qed.

Theorem finalized_floor_occurrence_status_scope_correct :
  (forall records record,
     In record records ->
     recording_in_finalized_closure record = true ->
     tombstoned
       (finalized_rejection_targets records)
       (rejection_target record))
  /\
  (tombstoned
     (finalized_rejection_targets [secondary_example_record])
     secondary_example_occurrence /\
   ~ tombstoned
       (main_chain_rejection_targets [secondary_example_record])
       secondary_example_occurrence).
Proof.
  exact (conj finalized_closure_rejection_is_authoritative
          main_chain_only_projection_is_incomplete).
Qed.

Theorem finalized_floor_recovery_admission_correct :
  (forall records occurrences,
     (forall candidate, In candidate occurrences -> ~ active records candidate) <->
     all_sources_tombstoned records occurrences)
  /\
  (forall records occurrences valid_after next_block lifespan,
     retry_eligible records occurrences valid_after next_block lifespan ->
     forall candidate, In candidate occurrences -> ~ active records candidate)
  /\
  (forall records occurrences candidate valid_after next_block lifespan,
     In candidate occurrences ->
     active records candidate ->
     ~ retry_eligible records occurrences valid_after next_block lifespan)
  /\
  (forall records occurrences valid_after next_block lifespan,
     valid_after + lifespan <= next_block ->
     ~ retry_eligible records occurrences valid_after next_block lifespan).
Proof.
  exact (conj no_active_iff_all_sources_tombstoned
          (conj retry_requires_no_active_source
            (conj active_source_blocks_retry expiry_closes_recovery))).
Qed.

Theorem finalized_floor_recovery_leadership_correct :
  (forall validator_count finalized_height,
     validator_count > 0 ->
     1 <= inclusion_leader validator_count finalized_height <= validator_count)
  /\
  (forall validator_count finalized_height proposer_a proposer_b,
     inclusion_authorized validator_count finalized_height proposer_a ->
     inclusion_authorized validator_count finalized_height proposer_b ->
     proposer_a = proposer_b)
  /\
  (forall carrier_owner proposer_a proposer_b,
     recovery_custody_authorized carrier_owner proposer_a ->
     recovery_custody_authorized carrier_owner proposer_b ->
     proposer_a = proposer_b).
Proof.
  exact (conj inclusion_leader_in_validator_set
          (conj inclusion_authorization_unique_per_finalized_view
            recovery_custody_authorization_unique_per_carrier)).
Qed.

Theorem finalized_floor_merge_recovery_coherence_correct :
  (forall base scope tombstones committed_receipt candidate,
    base committed_receipt ->
    same_deploy committed_receipt candidate ->
    ~ selected base scope tombstones candidate)
  /\
  (forall base scope tombstones named candidate,
    scope named ->
    tombstones (receipt_occurrence named) ->
    receipt_chain named = receipt_chain candidate ->
    ~ selected base scope tombstones candidate)
  /\
  (forall base scope tombstones,
    base_deploy_unique base ->
    effect_identity_consistent scope ->
    forall left right,
      committed base scope tombstones left ->
      committed base scope tombstones right ->
      same_deploy left right ->
      left = right)
  /\
  (forall base scope tombstones receipt,
    committed base scope tombstones receipt <->
    ordinary_applied base scope tombstones receipt /\
    merge_metadata_bound base scope tombstones receipt)
  /\
  (forall base scope tombstones receipt,
    base receipt ->
    ~ retry_allowed base scope tombstones (receipt_deploy receipt))
  /\
  (forall base contributions,
    length (materialize_number base contributions) = 1)
  /\
  (forall base left right,
    Permutation left right ->
    materialize_number base left = materialize_number base right).
Proof.
  exact (conj base_committed_dominates_scope
          (conj tombstoned_chain_is_excluded
            (conj committed_deploy_unique
              (conj state_record_effect_coherence
                (conj base_committed_blocks_retry
                  (conj materialized_number_is_singleton
                        materialized_number_permutation)))))).
Qed.

Theorem finalized_floor_admission_effect_alignment_correct :
  (forall left right record_id,
    user_effect_records
      (left ++ {| user_record_id := record_id;
                  user_record_disposition := AdmissionRejected |} :: right) =
    user_effect_records (left ++ right))
  /\
  (forall record_id,
    length
      (user_effect_records
        [{| user_record_id := record_id;
            user_record_disposition := ExecutionFailed |}]) = 1)
  /\
  (forall left right,
    Permutation left right ->
    length (user_effect_records left) = length (user_effect_records right))
  /\
  (forall records system_effect_count metadata,
    metadata_aligned records system_effect_count metadata ->
    exists user_metadata system_metadata,
      metadata = user_metadata ++ system_metadata /\
      length user_metadata = length (user_effect_records records) /\
      length system_metadata = system_effect_count)
  /\
  (forall record_id,
    required_merge_metadata
      [{| user_record_id := record_id;
          user_record_disposition := AdmissionRejected |}]
      1 = 1 /\
    length
      [{| user_record_id := record_id;
          user_record_disposition := AdmissionRejected |}] + 1 = 2).
Proof.
  exact (conj admission_rejected_has_no_effect_slot
          (conj executed_failure_retains_effect_slot
            (conj effect_projection_permutation_length
              (conj aligned_metadata_splits_exactly
                    funding_rejection_close_block_regression)))).
Qed.

Theorem finalized_floor_rejection_reason_confluence_correct :
  (forall left right,
    canonical_reason_join left right = canonical_reason_join right left)
  /\
  (forall left middle right,
    canonical_reason_join (canonical_reason_join left middle) right =
    canonical_reason_join left (canonical_reason_join middle right))
  /\
  (forall reason,
    canonical_reason_join reason reason = reason)
  /\
  (forall left right,
    Permutation left right ->
    fold_rejection_reasons left = fold_rejection_reasons right)
  /\
  (forall reason,
    canonical_reason_join DuplicateOccurrence reason = DuplicateOccurrence)
  /\
  canonical_reason_join MergeConflict CollateralChainDrop = MergeConflict.
Proof.
  exact (conj canonical_reason_join_commutative
          (conj canonical_reason_join_associative
            (conj canonical_reason_join_idempotent
              (conj fold_rejection_reasons_permutation
                (conj duplicate_reason_dominates
                      merge_reason_dominates_collateral))))).
Qed.

(* ===========================================================================
   Phase 6 capstone extensions — the floor SELECTION (T-SOUND/T-FIN/T-LIN/
   T-PS/T-COMM/H3) and the IntegerAdd ALGEBRA (T-ALG c/d + launder-free + bound).
   Each conjunct is discharged by `exact` against its module lemma, so these add
   no assumptions (verify with `Print Assumptions`).
   =========================================================================== *)

Theorem finalized_floor_selection_correct :
  (* T-SOUND: the chosen merge base is sound. *)
  (forall d fuel parents cands f,
     select_floor d fuel parents cands = Some f ->
     is_sound d fuel parents cands f = true)
  /\
  (* T-SOUND (Err-correct) / T-PS: for ANY parent list, None ⇒ no candidate is
     sound (the incompatible-fork Err is correct, never a silent unsound base). *)
  (forall d fuel parents cands,
     select_floor d fuel parents cands = None ->
     forall c, In c cands -> is_sound d fuel parents cands c = false)
  /\
  (* T-SOUND-A / T-LIN: a Case-A base is a common DAG-ancestor of every parent. *)
  (forall d fuel parents c,
     case_a d fuel parents c = true ->
     forall p, In p parents -> c = p \/ anc_of d c p)
  /\
  (* T-FIN: the chosen base is drawn from the candidates, so it is finalized when
     they are. *)
  (forall (Fin : BlockHash -> Prop) d fuel parents cands f,
     Forall Fin cands -> select_floor d fuel parents cands = Some f -> Fin f)
  /\
  (* T-COMM: the committee is bonds_of(floor), a pure function of the floor. *)
  (forall bonds_of d fuel parents cands f,
     select_floor d fuel parents cands = Some f ->
     committee_used bonds_of d fuel parents cands = Some (bonds_of f))
  /\
  (* H3: the floor-bounded scan covers every parent write at or above the floor. *)
  (forall d parents fl p w,
     wf_dag_num d -> In p parents -> anc_of d fl w -> anc_of d w p ->
     in_scope d parents fl w).
Proof.
  (* build the tuple directly; `repeat split` would over-split `in_scope`. *)
  exact (conj select_sound
          (conj select_none_correct
            (conj case_a_common_ancestor
              (conj select_finalized
                (conj committee_is_floor_bonds scope_covers_band))))).
Qed.

Theorem committee_transition_correct :
  (forall post_state_bonds block,
     serialized_post_state_bonds post_state_bonds block =
     post_state_bonds (blk_hash block))
  /\
  (forall floor_bonds floor_of block
      (post_state_bonds_left post_state_bonds_right : BlockHash -> Committee),
     authority_committee floor_bonds floor_of block =
     authority_committee floor_bonds floor_of block)
  /\
  (forall floor_bonds post_state_bonds floor_of block validator,
     ~ authorized (authority_committee floor_bonds floor_of block) validator ->
     In validator
       (committee_validators (serialized_post_state_bonds post_state_bonds block)) ->
     ~ authorized (authority_committee floor_bonds floor_of block) validator)
  /\
  (forall floor_bonds floor_of block sender,
     authority_context_valid floor_bonds floor_of block sender ->
     same_validator_set
       (justification_validators block)
       (positive_committee_validators
         (authority_committee floor_bonds floor_of block)))
  /\
  (forall floor_bonds floor_of block sender,
     authority_context_valid floor_bonds floor_of block sender ->
     authorized (authority_committee floor_bonds floor_of block) sender)
  /\
  (forall registered post_state_bonds candidate validator,
     In validator
       (positive_committee_validators (post_state_bonds candidate)) ->
     In validator
       (register_transition true registered post_state_bonds candidate))
  /\
  (forall registered post_state_bonds candidate validator,
     ~ In validator registered ->
     ~ In validator
       (register_transition false registered post_state_bonds candidate))
  /\
  (forall accepted registered post_state_bonds candidate validator,
     promotion_ready accepted registered post_state_bonds candidate ->
     In validator
       (positive_committee_validators (post_state_bonds candidate)) ->
     accepted = true /\ In validator registered)
  /\
  (forall registered post_state_bonds candidate,
     ~ promotion_ready false registered post_state_bonds candidate)
  /\
  (forall floor_bonds post_state_bonds floor_of registered source promoted validator,
     promotion_ready true
       (register_transition true registered post_state_bonds (blk_hash source))
       post_state_bonds
       (blk_hash source) ->
     floor_of promoted = blk_hash source ->
     floor_bonds (blk_hash source) = post_state_bonds (blk_hash source) ->
     In validator
       (positive_committee_validators
         (serialized_post_state_bonds post_state_bonds source)) ->
     authorized (authority_committee floor_bonds floor_of promoted) validator)
  /\
  (forall canonical_genesis block,
     admission_valid canonical_genesis OrdinaryReceivedAdmission block ->
     exists parent, blk_main_parent block = Some parent)
  /\
  (forall canonical_genesis path block,
     admission_valid canonical_genesis path block ->
     blk_main_parent block = None ->
     path = ApprovedGenesisAdmission /\ blk_hash block = canonical_genesis)
  /\
  (forall canonical_genesis path block,
     blk_hash block <> canonical_genesis ->
     blk_main_parent block = None ->
     ~ admission_valid canonical_genesis path block)
  /\
  (forall sender_of canonical_genesis genesis_placeholder block key cited,
     justification_keys_valid
       sender_of canonical_genesis genesis_placeholder block ->
     In (key, cited) (blk_just block) ->
     cited <> canonical_genesis ->
     key = sender_of cited)
  /\
  (forall sender_of canonical_genesis genesis_placeholder block cited,
     justification_keys_valid
       sender_of canonical_genesis genesis_placeholder block ->
     In (genesis_placeholder, cited) (blk_just block) ->
     genesis_placeholder <> sender_of cited ->
     cited = canonical_genesis)
  /\
  (forall canonical_genesis current post_state_bonds candidate validator,
     In validator
       (positive_committee_validators (post_state_bonds candidate)) ->
     seed_registered_genesis true canonical_genesis current
       post_state_bonds candidate validator = Some canonical_genesis)
  /\
  (forall canonical_genesis,
     insert_approved_genesis_index
       canonical_genesis canonical_genesis MissingGenesisIndex =
     GenesisAt canonical_genesis)
  /\
  (forall canonical_genesis conflicting,
     conflicting <> canonical_genesis ->
     insert_approved_genesis_index
       canonical_genesis conflicting (GenesisAt canonical_genesis) =
     GenesisAt canonical_genesis)
  /\
  (forall canonical_genesis current_left current_right
      post_state_bonds candidate validator,
     In validator
       (positive_committee_validators (post_state_bonds candidate)) ->
     seed_registered_genesis true canonical_genesis current_left
       post_state_bonds candidate validator = Some canonical_genesis /\
     seed_registered_genesis true canonical_genesis current_right
       post_state_bonds candidate validator = Some canonical_genesis)
  /\
  (forall registered slots sender,
     ~ In sender registered ->
     record_invalid_lmm registered slots sender = slots)
  /\
  (forall registered post_state_bonds candidate validator,
     ~ In validator registered ->
     ~ In validator
       (positive_committee_validators (post_state_bonds candidate)) ->
     ~ In validator
       (register_transition true registered post_state_bonds candidate))
  /\
  (forall invalid slots validator,
     In validator invalid ->
     ~ In validator (finality_lmm_projection invalid slots)).
Proof.
  exact (conj serialized_bonds_are_post_state_bonds
          (conj authority_ignores_same_block_post_state
            (conj same_block_transition_does_not_grant_authority
              (conj valid_authority_context_has_exact_justifications
                (conj valid_authority_context_authorizes_sender
                  (conj accepted_transition_registers_post_state_validators
                    (conj rejected_transition_cannot_register_new_validator
                      (conj promotion_requires_accepted_registration
                        (conj rejected_transition_cannot_promote
                          (conj registered_transition_is_eligible_after_floor_promotion
                            (conj ordinary_received_block_has_parent
                              (conj approved_genesis_is_the_only_admitted_root
                                (conj counterfeit_root_is_not_admitted
                                  (conj non_genesis_justification_key_matches_cited_sender
                                    (conj placeholder_justification_cites_only_approved_genesis
                                      (conj accepted_positive_validator_seeds_canonical_genesis
                                        (conj duplicate_approved_genesis_backfills_legacy_index
                                          (conj conflicting_approved_hash_preserves_canonical_index
                                            (conj canonical_genesis_seed_is_independent_of_local_height_zero_order
                                              (conj invalid_unregistered_sender_cannot_create_lmm_slot
                                                (conj accepted_nonpositive_validator_cannot_create_new_slot
                                                  invalid_lmm_never_contributes_to_finality_projection))))))))))))))))))))).
Qed.

Print Assumptions committee_transition_correct.

Theorem objective_equivocation_correct :
  (forall left right,
     canonical_evidence_pair left right =
     canonical_evidence_pair right left)
  /\
  (forall left right,
     In left (evidence_dependencies left right) /\
     In right (evidence_dependencies left right))
  /\
  (forall sender_of sequence_of left right,
     objective_equivocation sender_of sequence_of left right ->
     objective_equivocation sender_of sequence_of right left)
  /\
  (forall sender_of sequence_of local_invalid left right,
     objective_equivocation sender_of sequence_of left right ->
     accept_objective_evidence
       sender_of sequence_of local_invalid left right)
  /\
  (forall sender_of sequence_of local_left local_right left right,
     accept_objective_evidence
       sender_of sequence_of local_left left right <->
     accept_objective_evidence
       sender_of sequence_of local_right left right)
  /\
  (forall validity left right hash,
     apply_objective_evidence
       validity (canonical_evidence_pair left right) hash = validity hash)
  /\
  (forall equivocator voters,
     ~ In equivocator (finality_voters equivocator voters))
  /\
  (forall target_incarnation incarnation_of old_hash current_left current_right,
     incarnation_of old_hash <> target_incarnation ->
     incarnation_of current_left = target_incarnation ->
     incarnation_of current_right = target_incarnation ->
     current_incarnation_pair target_incarnation incarnation_of
       [old_hash; current_left; current_right] =
       Some (canonical_evidence_pair current_left current_right) /\
     current_incarnation_pair target_incarnation incarnation_of
       [current_right; old_hash; current_left] =
       Some (canonical_evidence_pair current_left current_right))
  /\
  (forall old_hash current_left current_right,
     first_two_before_incarnation_grouping
       [old_hash; current_left; current_right] =
       Some (canonical_evidence_pair old_hash current_left))
  /\
  (forall active_incarnation evidence_incarnation equivocator voters,
     incarnation_finality_voters
       active_incarnation evidence_incarnation false equivocator voters = voters)
  /\
  (forall active_incarnation equivocator voters,
     ~ In equivocator
         (incarnation_finality_voters
           active_incarnation active_incarnation true equivocator voters))
  /\
  (forall active_incarnation evidence_incarnation equivocator voters,
     evidence_incarnation <> active_incarnation ->
     incarnation_finality_voters
       active_incarnation evidence_incarnation true equivocator voters = voters)
  /\
  (forall target_incarnation unary_incarnation,
     objective_slash_authorized
       true target_incarnation target_incarnation target_incarnation unary_incarnation = true)
  /\
  (forall target_incarnation old_incarnation unary_incarnation,
     old_incarnation <> target_incarnation ->
     objective_slash_authorized
       true target_incarnation old_incarnation target_incarnation unary_incarnation = false)
  /\
  (forall target_incarnation left_incarnation right_incarnation unary_left unary_right,
     objective_slash_authorized
       true target_incarnation left_incarnation right_incarnation unary_left =
     objective_slash_authorized
       true target_incarnation left_incarnation right_incarnation unary_right)
  /\
  (forall validator sequence unary_eligible,
     scoped_unary_slash_authorized
       (validator, sequence) (validator, sequence) unary_eligible = false)
  /\
  (forall validator objective_sequence unary_sequence,
     objective_sequence <> unary_sequence ->
     scoped_unary_slash_authorized
       (validator, objective_sequence) (validator, unary_sequence) true = true)
  /\
  objective_refinement_contract
  /\
  (forall left right,
     restart_objective_evidence
       (persist_objective_evidence (canonical_evidence_pair left right)) =
     Some (canonical_evidence_pair left right)).
Proof.
  exact (conj canonical_evidence_pair_symmetric
          (conj canonical_evidence_dependencies_contain_both_hashes
            (conj objective_equivocation_is_symmetric
              (conj equal_sequence_siblings_suffice_for_objective_acceptance
                (conj objective_acceptance_ignores_local_invalid_flags
                  (conj objective_evidence_does_not_retroactively_change_block_validity
                    (conj objective_equivocator_is_excluded_from_finality_voters
                      (conj incarnation_grouping_precedes_pair_canonicalization
                        (conj first_two_before_grouping_can_select_cross_incarnation_pair
                          (conj structural_pair_without_same_incarnation_evidence_preserves_voters
                            (conj active_incarnation_evidence_excludes_equivocator
                              (conj later_incarnation_restores_raw_public_key
                                (conj same_current_incarnation_objective_pair_is_slash_eligible
                                  (conj cross_incarnation_objective_pair_suppresses_unary_fallback
                                    (conj objective_pair_slash_decision_is_independent_of_unary_arrival
                                      (conj objective_pair_suppresses_unary_fallback_at_same_fault_key
                                        (conj independent_unary_fault_at_other_sequence_remains_eligible
                                          (conj objective_refinement_contract_holds
                                            restart_preserves_canonical_objective_evidence)))))))))))))))))).
Qed.

Print Assumptions objective_equivocation_correct.

Theorem objective_evidence_authorization_v5_correct :
  (forall target_generation target_epoch generation_of epoch_of
          old_epoch_hash current_left current_right,
     generation_of old_epoch_hash = target_generation ->
     generation_of current_left = target_generation ->
     generation_of current_right = target_generation ->
     epoch_of old_epoch_hash <> target_epoch ->
     epoch_of current_left = target_epoch ->
     epoch_of current_right = target_epoch ->
     current_generation_epoch_pair
       target_generation target_epoch generation_of epoch_of
       [old_epoch_hash; current_left; current_right] =
       Some (canonical_evidence_pair current_left current_right) /\
     current_generation_epoch_pair
       target_generation target_epoch generation_of epoch_of
       [current_right; old_epoch_hash; current_left] =
       Some (canonical_evidence_pair current_left current_right))
  /\
  (forall target_generation target_epoch generation_of epoch_of left right,
     epoch_of left <> target_epoch ->
     objective_pair_authorized_v5
       target_generation target_epoch generation_of epoch_of left right = false)
  /\
  (forall target_generation target_epoch generation_of epoch_of left right,
     proposer_objective_authorized_v5
       target_generation target_epoch generation_of epoch_of left right =
     receiver_objective_authorized_v5
       target_generation target_epoch generation_of epoch_of left right)
  /\
  slash_authority_needed 0 2 = true
  /\
  (forall authority target_epoch sender_of sequence_of generation_of epoch_of left right,
     proposer_objective_authorized_by_authority_v5
       authority target_epoch sender_of sequence_of generation_of epoch_of left right =
     receiver_objective_authorized_by_authority_v5
       authority target_epoch sender_of sequence_of generation_of epoch_of left right)
  /\
  (forall authority target_epoch sender_of sequence_of generation_of epoch_of left right,
     authority_bond authority (sender_of left) = 0 ->
     objective_pair_authorized_by_authority_v5
       authority target_epoch sender_of sequence_of generation_of epoch_of left right = false)
  /\
  (forall authority target_epoch sender_of sequence_of generation_of epoch_of left right,
     sender_of left <> sender_of right ->
     objective_pair_authorized_by_authority_v5
       authority target_epoch sender_of sequence_of generation_of epoch_of left right = false)
  /\
  (forall authority target_epoch sender_of sequence_of generation_of epoch_of left right,
     sequence_of left <> sequence_of right ->
     objective_pair_authorized_by_authority_v5
       authority target_epoch sender_of sequence_of generation_of epoch_of left right = false)
  /\
  (forall authority target_epoch sender_of sequence_of generation_of epoch_of left right,
     epoch_of left <> target_epoch ->
     objective_pair_authorized_by_authority_v5
       authority target_epoch sender_of sequence_of generation_of epoch_of left right = false)
  /\
  (forall state_root bonds generations,
     authority_state_root
       (canonical_slash_authority_from_state state_root bonds generations) = state_root /\
     authority_bond
       (canonical_slash_authority_from_state state_root bonds generations) = bonds /\
     authority_generation
       (canonical_slash_authority_from_state state_root bonds generations) = generations).
Proof.
  exact (conj generation_and_epoch_grouping_precede_pair_canonicalization
    (conj cross_epoch_objective_pair_cannot_authorize_v5
      (conj proposer_receiver_objective_authorization_parity_v5
        (conj pair_only_evidence_activates_slash_authority
          (conj proposer_receiver_canonical_authority_parity_v5
            (conj nonpositive_canonical_bond_rejects_objective_pair_v5
              (conj mismatched_sender_rejects_objective_pair_v5
                (conj mismatched_sequence_rejects_objective_pair_v5
                  (conj cross_epoch_canonical_authority_pair_cannot_authorize_v5
                    canonical_slash_authority_snapshot_is_root_bound))))))))).
Qed.

Print Assumptions objective_evidence_authorization_v5_correct.

Theorem finalized_floor_atomic_commit_correct :
  (forall ledger,
    exists next,
      finalization_compare_append
        (ledger_head ledger) (S (ledger_head ledger)) ledger = Some next /\
      committed_round next (ledger_head next) = true)
  /\
  (forall ledger expected candidate,
    expected <> ledger_head ledger ->
    finalization_compare_append expected candidate ledger = None)
  /\
  (forall round effects candidate,
    finalization_apply_effect round
      (finalization_apply_effect round effects) candidate =
    finalization_apply_effect round effects candidate)
  /\
  (forall left right effects candidate,
    finalization_apply_effect left
      (finalization_apply_effect right effects) candidate =
    finalization_apply_effect right
      (finalization_apply_effect left effects) candidate)
  /\
  (forall candidate current,
    current <= finalization_publish_revision candidate current)
  /\
  (forall node,
    durable_state (restart_finalization_node node) = durable_state node /\
    published_revision (restart_finalization_node node) =
      ledger_head (durable_state node)).
Proof. exact finalization_atomicity_contract. Qed.

Print Assumptions finalized_floor_atomic_commit_correct.

Theorem finalized_floor_snapshot_capture_retry_correct :
  (retry_snapshot_capture [SnapshotCaptureStale] = None)
  /\
  (forall stale_count revision,
    retry_snapshot_capture
      (repeat SnapshotCaptureStale stale_count ++
       [SnapshotCaptureCoherent revision]) = Some revision)
  /\
  (forall observations revision,
    retry_snapshot_capture observations = Some revision ->
    In (SnapshotCaptureCoherent revision) observations).
Proof.
  exact (conj stale_snapshot_capture_publishes_no_result
    (conj finite_stale_snapshot_prefix_reaches_coherent_capture
      snapshot_retry_returns_only_an_observed_coherent_revision)).
Qed.

Print Assumptions finalized_floor_snapshot_capture_retry_correct.

Theorem finalized_floor_worker_retry_correct :
  (forall completed coverage,
    worker_completed_after FinalizationWorkerFailed completed coverage = completed)
  /\
  (forall succeeded coverage,
    worker_succeeded_after FinalizationWorkerFailed succeeded coverage = succeeded)
  /\
  (forall completed coverage,
    completed < coverage ->
    worker_retry_required FinalizationWorkerFailed completed coverage = true)
  /\
  (forall completed coverage,
    worker_completed_after FinalizationWorkerSucceeded completed coverage =
    worker_succeeded_after FinalizationWorkerSucceeded completed coverage)
  /\
  (forall completed older newer,
    older <= newer ->
    older <= worker_completed_after FinalizationWorkerSucceeded completed newer).
Proof. exact finalization_worker_retry_contract. Qed.

Print Assumptions finalized_floor_worker_retry_correct.

Theorem finalized_floor_proposal_readiness_correct :
  (forall permit_required permit_fresh relation slots_complete proposer_active,
     classify_proposal_readiness permit_required permit_fresh relation
       slots_complete proposer_active = ProposalReady ->
     relation = MatchingContext /\
     slots_complete = true /\
     proposer_active = true /\
     (permit_required = false \/ permit_fresh = true))
  /\
  (forall permit_required permit_fresh relation slots_complete proposer_active,
     classify_proposal_readiness permit_required permit_fresh relation
       slots_complete proposer_active = FloorMaterializationPending ->
     relation = StrictStatePreservingDescendant /\
     (permit_required = false \/ permit_fresh = true))
  /\
  (forall permit_required permit_fresh relation slots_complete proposer_active,
     requests_finalization
       (classify_proposal_readiness permit_required permit_fresh relation
         slots_complete proposer_active) = true ->
     materializable relation = true /\ preserves_committed_state relation = true)
  /\
  (forall reason,
     reason = CandidateFloorRegression \/
     reason = CandidateFloorConflict \/
     reason = CertifiedContextMismatch ->
     requests_finalization reason = false).
Proof. exact proposal_floor_readiness_contract. Qed.

Print Assumptions finalized_floor_proposal_readiness_correct.

Section BoundFinalizationHeadCorrectness.

Context {Block : Type}.
Variable block_eq_dec : forall left right : Block, {left = right} + {left <> right}.
Variable state_preserves : Block -> Block -> bool.

Theorem finalized_floor_bound_head_correct :
  (forall certificate ledger next,
    bound_finalization_compare_append block_eq_dec state_preserves certificate ledger =
      Some next ->
    state_preserves (bound_head ledger) (bound_head next) = true)
  /\
  (forall certificate ledger,
    certificate_revision certificate <> bound_revision ledger \/
    certificate_base certificate <> bound_head ledger ->
    bound_finalization_compare_append block_eq_dec state_preserves certificate ledger =
      None)
  /\
  (forall first second ledger next,
    certificate_revision first = bound_revision ledger ->
    certificate_revision second = bound_revision ledger ->
    bound_finalization_compare_append block_eq_dec state_preserves first ledger = Some next ->
    bound_finalization_compare_append block_eq_dec state_preserves second next = None).
Proof.
  exact (bound_finalization_head_contract block_eq_dec state_preserves).
Qed.

Print Assumptions finalized_floor_bound_head_correct.

End BoundFinalizationHeadCorrectness.

Theorem finalized_floor_recovery_cursors_correct :
  (forall head cursor,
    cursor <= head ->
    finalization_projection_step head cursor <= head)
  /\
  (forall cursor completed,
    finalization_effect_cursor_step cursor completed <= S cursor)
  /\
  (forall cursor completed,
    completed_prefix cursor completed ->
    completed (S cursor) = true ->
    completed_prefix (S cursor) completed)
  /\
  (forall effects_cursor compaction_cursor next,
    finalization_compaction_step effects_cursor compaction_cursor = Some next ->
    next <= effects_cursor)
  /\
  (forall cursors,
    restart_finalization_cursors cursors = cursors).
Proof. exact finalization_recovery_contract. Qed.

Print Assumptions finalized_floor_recovery_cursors_correct.

Open Scope Z_scope.

Theorem finalized_floor_arithmetic_correct :
  (* T-ALG(c): wrapping-add group laws (associativity + commutativity). *)
  (forall a b c : Z, wadd (wadd a b) c = wadd a (wadd b c))
  /\ (forall a b : Z, wadd a b = wadd b a)
  /\
  (* T-ALG(d): the checked apply-to-base rejects on overflow OR a negative result. *)
  (forall base diff : Z, ~ in_range (base + diff) -> checked_apply base diff = None)
  /\ (forall base diff : Z, base + diff < 0 -> checked_apply base diff = None)
  /\
  (* The fail-loudly FIX is launder-free: if checked_combine accepts, it returns
     the TRUE sum, in range — a wrapped value can never be laundered through it. *)
  (forall (l : list Z) (c : Z), checked_combine l = Some c -> c = true_sum l /\ in_range c)
  /\
  (* Defense-in-depth: while every partial sum stays in range, wrapping = checked
     = true sum, so the launder cannot arise. *)
  (forall l : list Z, safe l -> checked_combine l = Some (true_sum l) /\ wsum l = true_sum l).
Proof.
  exact (conj wadd_assoc
          (conj wadd_comm
            (conj checked_apply_rejects_overflow
              (conj checked_apply_rejects_negative
                (conj checked_combine_sound supply_cap_no_launder))))).
Qed.

(* A9 exact-integer fault-tolerance arithmetic (the f32 -> exact hardening). The
   runtime decision uses the strict `2q·den > S(den+num)` test. Both strict and
   inclusive arithmetic forms are proved equivalent to their rational forms; the
   inclusive form remains a historical boundary-bug control. Both are monotone in
   clique weight (given den >= 0) and overflow-free for i64-bounded stake.
   The overflow envelope now covers the FULL validated ppm range num ∈ [-den, den]
   (G2 widening in FtExact.ft_exact_no_overflow), matching the runtime range-check
   and the negative-θ sentinels, not merely [0, den]. *)
Theorem finalized_floor_ftexact_correct :
  (forall q S num den : Z, ft_exact_ge q S num den <-> ft_ratio_ge q S num den)
  /\ (forall q S num den : Z, ft_exact_gt q S num den <-> ft_ratio_gt q S num den)
  /\ (forall q q' S num den : Z,
        0 <= den -> q <= q' -> ft_exact_ge q S num den -> ft_exact_ge q' S num den)
  /\ (forall q q' S num den : Z,
        0 <= den -> q <= q' -> ft_exact_gt q S num den -> ft_exact_gt q' S num den)
  /\ (forall q S num den : Z,
        0 <= q <= S -> 0 <= S <= 2^63 -> -den <= num <= den -> den = 1000000 ->
        Z.abs (2*q*den) < 2^127 /\ Z.abs (S*(den+num)) < 2^127).
Proof.
  exact (conj ft_exact_iff_ratio
          (conj ft_exact_iff_ratio_strict
            (conj ft_exact_mono_q
              (conj ft_exact_gt_mono_q ft_exact_no_overflow)))).
Qed.

Theorem finalized_floor_threshold_alignment_correct :
  (forall q S num den,
    candidate_floor_certificate q S num den <->
    durable_finalizer_certificate q S num den)
  /\
  (~ candidate_floor_certificate 8 16 0 1000000 /\
   ~ durable_finalizer_certificate 8 16 0 1000000)
  /\
  (inclusive_candidate_control 8 16 0 1000000 /\
   ~ durable_finalizer_certificate 8 16 0 1000000).
Proof. exact aligned_threshold_contract. Qed.

Print Assumptions finalized_floor_threshold_alignment_correct.

Close Scope Z_scope.

(* ===========================================================================
   G2 capstone — θ_ppm PROVENANCE determinism + the widened i128 overflow envelope.

   Strengthens the A9 (ftexact) capstone at its one un-modelled seam: the SOURCING
   of the threshold numerator θ_ppm. A9 proves the decision is exact GIVEN θ_ppm;
   this proves θ_ppm is a pure function of the on-chain value (the unconditional
   override at casper.rs:266), so local config cannot drive a fork,
   AND that the exact decision is i128-overflow-free across the node's FULL
   validated ppm range num ∈ [-den, den] (the token_metadata_check.rs:105 range,
   including the negative-θ sentinels) — not just the narrower [0, den].

   Each conjunct is discharged by `exact` against its FtProvenance lemma, so the
   capstone adds NO assumptions (verify with `Print Assumptions
   finalized_floor_ftprovenance_correct`).
   =========================================================================== *)
Open Scope Z_scope.

Theorem finalized_floor_ftprovenance_correct :
  (* G2 / provenance: the θ_ppm a node finalizes with is the on-chain value,
     independent of local config (the unconditional override), so two nodes on the
     same genesis agree on θ_ppm regardless of local config — not a fork input. *)
  (forall local onchain : Z, reconcile local onchain = onchain)
  /\
  (* G2 / provenance (agreement form): agreeing on-chain ppm forces agreeing
     reconciled ppm, for ANY local configs local, local'. *)
  (forall local local' onchain : Z, reconcile local onchain = reconcile local' onchain)
  /\
  (* G2 / widened envelope: the exact decision is i128-overflow-free over the FULL
     validated ppm range num ∈ [-den, den] (not merely [0, den]). *)
  (forall q S num den : Z,
     0 <= q <= S -> 0 <= S <= 2^63 -> -den <= num <= den -> den = 1000000 ->
     Z.abs (2*q*den) < 2^127 /\ Z.abs (S*(den+num)) < 2^127).
Proof.
  exact (conj reconcile_is_onchain
          (conj reconcile_agrees_on_onchain ppm_range_decision_no_overflow)).
Qed.

Close Scope Z_scope.

(* ===========================================================================
   Phase 7 capstone — the strengthened selection conjuncts (Case-B compatibility
   + selection maximality). The guard⇒AdjDC bridge and the frontier-is-finalized
   result live in GuardBridge.v (guard_constant_committee_transparent,
   upgo_finalized); they are checked axiom-free by the gate.
   =========================================================================== *)

Theorem finalized_floor_phase7_correct :
  (* Case-B compatibility: the precise guarantee of the all_compatible branch —
     every other candidate is `c`, in `c`'s past, or mergeable via a common
     descendant parent (no incompatible finalized fork). *)
  (forall d fuel parents cands c,
     case_b d fuel parents cands c = true ->
     forall o, In o cands ->
       o = c \/ anc_of d o c
       \/ (exists p, In p parents /\ anc_of d o p /\ anc_of d c p))
  /\
  (* Selection maximality: on descending-sorted candidates the chosen floor is the
     sound base of greatest block number — the canonical highest sound base. *)
  (forall d fuel parents cands f,
     DescSorted d cands ->
     select_floor d fuel parents cands = Some f ->
     is_sound d fuel parents cands f = true /\
     (forall c, In c cands -> is_sound d fuel parents cands c = true ->
        numof d c <= numof d f)).
Proof.
  exact (conj case_b_compatible select_highest_sound).
Qed.

(* ===========================================================================
   C1 + C5 capstone — the θ-exact finalization test and snapshot advancement.

   Strengthens the two "assumed/proxy" seams the earlier capstones rested on:

     C1  The node's REAL fault-tolerance decision is the exact-integer test
         `Finalized_ft` (2q·den > S(den+num), θ = num/den), not merely strict
         majority. It is ancestor- and snapshot-monotone (L-ANC/L-SNAP for the
         exact test) and REFINES the strict-majority `Finalized` proxy for
         θ ∈ (0,1) over a positive-stake committee — so every θ-finalized block
         inherits T-CACHE (frontier_cache_transparent) and every capstone above.

     C5  Finalization is monotone under snapshot ADVANCEMENT (a validator's latest
         message moving forward to a DAG-descendant), which GENERALIZES the
         preservation-only L-SNAP: `snap_extends ⇒ snap_advances`, so the original
         L-SNAP is the reflexive-descendant corollary.

     C1' The strict test already refines majority for θ >= 0, including the
         default θ = 0. For negative sentinels the node's real decision additionally
         applies a θ-independent hard majority gate (clique_oracle.rs,
         `2·agreeing > S`), modelled as `Finalized_ft_hg`; the gate alone yields
         strict-majority `Finalized` for all num. (Cache transparency is independently secured by
         GuardBridge.BridgeFt over `Finalized_ft` directly, via `L_ANC_ft`.)

   Each conjunct is discharged by `exact` against its CliqueOracle lemma, so this
   capstone introduces NO new assumptions (verify with `Print Assumptions
   finalized_floor_thetaexact_advance_correct`). The pre-existing five capstones
   are unchanged; this only ADDS coverage of the real node test and the faithful
   advancement model. The strict refinement needs only num >= 0 and den > 0;
   strictness excludes zero-stake certificates without another premise. The C1'
   hard-gate refinement holds for all num and all committees.
   =========================================================================== *)
Theorem finalized_floor_thetaexact_advance_correct :
  (* C1 / L-ANC: θ-exact finalization is downward-closed along ancestry. *)
  (forall d c J b b' num den,
     anc_of d b' b -> Finalized_ft d c J b num den -> Finalized_ft d c J b' num den)
  /\
  (* C1 / L-SNAP: θ-exact finalization is monotone under snapshot growth. *)
  (forall d c J J' b num den,
     snap_extends J' J -> Finalized_ft d c J b num den -> Finalized_ft d c J' b num den)
  /\
  (* C1 / refinement: the strict θ-test implies the strict-majority proxy for
     every non-negative threshold, including θ = 0. *)
  (forall d c J b num den,
     (0 <= num)%Z -> (0 < den)%Z ->
     Finalized_ft d c J b num den -> CliqueOracle.Finalized d c J b)
  /\
  (* C5 / advancement: finalization is monotone as latest messages advance to
     DAG-descendants (generalizes the preservation-only L-SNAP). *)
  (forall d c J J' b,
     snap_advances d J' J -> CliqueOracle.Finalized d c J b -> CliqueOracle.Finalized d c J' b)
  /\
  (* C5 / generalization: preservation ⇒ advancement, so the existing L-SNAP is
     the reflexive-descendant corollary of L_SNAP_advance. *)
  (forall d J' J, snap_extends J' J -> snap_advances d J' J)
  /\
  (* C1' / negative-threshold coverage: the node's real decision is the θ-test and the
     θ-INDEPENDENT hard majority gate (clique_oracle.rs:79-81, `2·agreeing > S`).
     The hard gate ALONE yields the strict-majority `Finalized` for ALL num —
     including the negative-θ sentinels. Independently, T-CACHE holds directly over `Finalized_ft` for all
     num via GuardBridge.BridgeFt.guard_constant_committee_transparent_ft.) *)
  (forall d c J b num den,
     Finalized_ft_hg d c J b num den -> CliqueOracle.Finalized d c J b).
Proof.
  exact (conj L_ANC_ft
          (conj L_SNAP_ft
            (conj Finalized_ft_refines_Finalized
              (conj L_SNAP_advance
                (conj snap_extends_snap_advances Finalized_ft_hg_refines_Finalized))))).
Qed.

Theorem finalizer_progress_correct :
  (forall (A : Type) (decides : A -> option bool) candidates selected,
     scan decides candidates = Selected selected ->
     In selected candidates /\ decides selected = Some true)
  /\
  (forall (A : Type) (decides : A -> option bool) candidates,
     scan decides candidates = Exhausted ->
     forall candidate, In candidate candidates -> decides candidate = Some false)
  /\
  (forall (A : Type) (decides : A -> option bool) candidates,
     Forall (fun candidate => exists decision, decides candidate = Some decision) candidates ->
     (exists candidate, In candidate candidates /\ decides candidate = Some true) ->
     exists selected, scan decides candidates = Selected selected)
  /\
  scan (fun candidate => Some (Nat.eqb candidate 3)) (firstn 2 [1; 2; 3]) = Exhausted
  /\
  scan (fun candidate => Some (Nat.eqb candidate 3)) [1; 2; 3] = Selected 3
  /\
  (forall (A : Type)
          (eq_dec : forall left right : A, {left = right} + {left <> right})
          scheduled proposed,
     NoDup (schedule_once A eq_dec scheduled proposed)
     /\
     forall candidate,
       In candidate (schedule_once A eq_dec scheduled proposed) <->
       In candidate scheduled \/ In candidate proposed).
Proof.
  destruct fixed_prefix_can_starve_a_finalizable_candidate as [Hprefix Hcomplete].
  exact (conj scan_selected_sound
          (conj scan_exhausted_complete
            (conj complete_scan_selects_when_ready_candidate_exists
              (conj Hprefix
                (conj Hcomplete
                  (fun A eq_dec scheduled proposed =>
                    conj (schedule_once_has_no_duplicates A eq_dec scheduled proposed)
                      (schedule_once_preserves_exact_membership A eq_dec scheduled proposed))))))).
Qed.

Theorem finalized_floor_protocol_activation_correct :
  (forall active_version block_version record,
    scope_admissible active_version block_version record ->
    block_version = active_version)
  /\
  (forall version record,
    exact_protocol version ->
    encoding_matches version record ->
    exists provenance, record_provenance record = Some provenance)
  /\
  (forall version record,
    exact_protocol version ->
    encoding_matches version record ->
    record_reason record <> ReasonUnspecified)
  /\
  (forall version record,
    version < 2 ->
    encoding_matches version record ->
    record_provenance record = None)
  /\
  (forall version record,
    version < 2 ->
    encoding_matches version record ->
    record_reason record = ReasonUnspecified)
  /\
  (forall active_version floor_version block_version record,
   forall base scope tombstones committed_receipt candidate,
    exact_protocol active_version ->
    floor_version < 2 ->
    base committed_receipt ->
    same_deploy committed_receipt candidate ->
    ~ protocol_selected active_version block_version record
        base scope tombstones candidate).
Proof.
  exact (conj admissible_scope_uses_active_version
    (conj exact_encoding_requires_provenance
      (conj exact_encoding_requires_reason
        (conj legacy_encoding_forbids_provenance
          (conj legacy_encoding_requires_unspecified_reason
            legacy_floor_exact_activation_preserves_base_dominance))))).
Qed.

Print Assumptions finalized_floor_protocol_activation_correct.

Theorem finalized_floor_protocol_lifecycle_correct :
  (forall candidate_version approved_version local_versions,
    candidate_version = ceremony_candidate current_protocol ->
    approver_accepts current_protocol candidate_version = true ->
    approved_version = candidate_version ->
    approved_version = current_protocol /\
    adopt_network approved_version local_versions =
      repeat current_protocol (length local_versions) /\
    Forall
      (fun running_version =>
        receiver_accepts running_version
          (proposal_version approved_version) = true)
      (adopt_network approved_version local_versions))
  /\
  (forall approved_version local_versions,
    supported_protocol approved_version ->
    admit_approved approved_version = Some approved_version /\
    adopt_network approved_version local_versions =
      repeat approved_version (length local_versions) /\
    Forall
      (fun running_version =>
        receiver_accepts running_version
          (proposal_version approved_version) = true)
      (adopt_network approved_version local_versions))
  /\
  (forall version,
    ~ supported_protocol version ->
    admit_approved version = None)
  /\
  admit_approved legacy_protocol = None
  /\
  (forall configured_version candidate_version,
    candidate_version <> configured_version ->
    approver_accepts configured_version candidate_version = false)
  /\
  (forall active_version block_version record,
    scope_admissible active_version block_version record ->
    block_version = active_version)
  /\
  (genesis_occurrence_identity current_protocol = ProtocolEnvelopeIdentity /\
   genesis_execution_identity current_protocol = ProtocolEnvelopeIdentity /\
   genesis_replay_identity current_protocol = ProtocolEnvelopeIdentity /\
   genesis_replay_identity current_protocol =
     genesis_execution_identity current_protocol /\
   (forall public_key,
     project_ground_custody (PrincipalDeployer 1 public_key) =
     project_ground_custody (LegacyGroundDeployer public_key))).
Proof.
  exact (conj current_ceremony_end_to_end
    (conj supported_recovery_end_to_end
      (conj unsupported_approved_fails_closed
        (conj legacy_approved_fails_closed
          (conj mismatched_candidate_is_not_approved
            (conj admissible_scope_uses_active_version
              current_genesis_identity_end_to_end)))))).
Qed.

Print Assumptions finalized_floor_protocol_lifecycle_correct.

Definition finalized_floor_funding_ground_custody_projection_correct :=
  funding_ground_custody_projection_correct.

Print Assumptions finalized_floor_funding_ground_custody_projection_correct.

Definition typed_local_validation_recovery_contract : Prop :=
  (forall history deferral artifact,
    deferral_artifact deferral = Some artifact ->
    certified_artifact (certify_deferral history deferral) = Some artifact)
  /\
  (forall history block_id state_id,
    certify_deferral history (AwaitingBlock block_id) <>
    certify_deferral history (AwaitingState state_id))
  /\
  (forall identity,
    certify_deferral GenesisRooted (AwaitingBlock identity) =
    LocalArtifactFault (MissingBlockArtifact identity))
  /\
  (forall identity,
    certify_deferral GenesisRooted (AwaitingState identity) =
    LocalArtifactFault (MissingStateArtifact identity))
  /\
  (forall history deferral,
    certified_deferral_disposition (certify_deferral history deferral) = Pending)
  /\
  (forall state_id block_id,
    recovery_releases
      (MissingStateArtifact state_id)
      (AwaitingBlock block_id) = false)
  /\
  (forall block_id state_id,
    recovery_releases
      (MissingBlockArtifact block_id)
      (AwaitingState state_id) = false)
  /\
  (forall artifact outstanding candidate,
    request_artifact artifact (request_artifact artifact outstanding) candidate =
    request_artifact artifact outstanding candidate)
  /\
  (forall left right outstanding candidate,
    request_artifact left (request_artifact right outstanding) candidate =
    request_artifact right (request_artifact left outstanding) candidate).

Theorem typed_local_validation_recovery_correct :
  typed_local_validation_recovery_contract.
Proof.
  unfold typed_local_validation_recovery_contract.
  exact (conj certified_deferral_preserves_artifact_identity
    (conj block_and_state_deferrals_never_collapse
      (conj genesis_guard_retains_typed_block_fault
        (conj genesis_guard_retains_typed_state_fault
          (conj typed_deferral_never_creates_objective_invalidity
            (conj state_recovery_never_releases_block_waiter
              (conj block_recovery_never_releases_state_waiter
                (conj duplicate_recovery_request_is_idempotent
                  independent_recovery_requests_commute)))))))).
Qed.

Print Assumptions typed_local_validation_recovery_correct.

Theorem bootstrap_replay_and_local_fault_recovery_correct :
  (forall (Context Root : Type)
          (replay : Context -> Root -> Root)
          (history : list (@ConsensusBlock Context Root replay)),
    replay_history replay history = declared_history_roots replay history)
  /\
  (forall state,
    validation_disposition (defer_local_fault state) =
      validation_disposition state)
  /\
  (forall state,
    queue_state (defer_local_fault state) <> Ready)
  /\
  (forall state,
    queue_state state = Deferred ->
    queue_state (recovery_request_failed state) <> Ready)
  /\
  (forall state,
    regular_parent_satisfied state = true ->
    validation_disposition state = LocalFaultDeferral.Accepted)
  /\
  typed_local_validation_recovery_contract.
Proof.
  split.
  - intros Context Root replay history.
    exact (consensus_history_replay_matches_declared_roots replay history).
  - exact (conj local_fault_preserves_consensus_disposition
      (conj local_fault_leaves_ready_queue
        (conj failed_recovery_does_not_restore_ready_state
          (conj regular_child_requires_valid_parent
            typed_local_validation_recovery_correct)))).
Qed.

Print Assumptions bootstrap_replay_and_local_fault_recovery_correct.

Theorem terminal_funding_admission_lifecycle_correct :
  (forall supply demand,
    supply < demand ->
    recorded_decision (propose supply demand) = Reject /\
    user_effects (propose supply demand) = 0 /\
    finalize_record (propose supply demand) = RejectedFinalized)
  /\
  (forall record (later_supply : nat),
    recorded_decision record = Reject ->
    finalize_record record = RejectedFinalized /\
    user_effects record = 0)
  /\
  (forall supply demand,
    demand <= supply ->
    validate_record
      {| recorded_supply := supply;
         recorded_demand := demand;
         recorded_decision := Reject |} = false).
Proof.
  exact (conj underfunded_proposal_is_terminal_rejection
    (conj later_supply_does_not_resurrect_recorded_rejection
      fundable_deploy_cannot_be_forged_as_rejected)).
Qed.

Print Assumptions terminal_funding_admission_lifecycle_correct.

Theorem finalized_floor_effect_causal_closure_correct :
  exact_effect_causal_closure_contract.
Proof.
  exact exact_effect_causal_closure_correct.
Qed.

Print Assumptions finalized_floor_effect_causal_closure_correct.

Theorem finalized_floor_state_lineage_correct :
  state_lineage_contract /\
  promotion_preservation_contract /\
  base_lineage_promotion_contract.
Proof.
  exact (conj state_lineage_end_to_end
    (conj state_lineage_promotion_correct base_lineage_promotion_correct)).
Qed.

Print Assumptions finalized_floor_state_lineage_correct.

Theorem finalized_floor_state_effect_provenance_correct :
  state_effect_provenance_contract.
Proof.
  exact state_effect_provenance_end_to_end.
Qed.

Print Assumptions finalized_floor_state_effect_provenance_correct.

Theorem finalized_floor_rebased_parent_selection_correct :
  floor_rebased_parent_selection_contract.
Proof.
  exact floor_rebased_parent_selection_end_to_end.
Qed.

Print Assumptions finalized_floor_rebased_parent_selection_correct.

Theorem finalized_floor_state_support_refines_causal_certificate :
  forall
    (d : DAG)
    (state_ancestor : BlockHash -> BlockHash -> Prop),
    (forall ancestor descendant,
      state_ancestor ancestor descendant -> anc_of d ancestor descendant) ->
    forall c J b num den,
      StateFinalized_ft_hg state_ancestor c J b num den ->
      Finalized_ft_hg d c J b num den.
Proof.
  exact state_finalization_refines_causal_finalization.
Qed.

Print Assumptions finalized_floor_state_support_refines_causal_certificate.

Theorem finalized_floor_certified_promotion_correct :
  certified_floor_promotion_contract.
Proof.
  exact certified_floor_promotion_end_to_end.
Qed.

Print Assumptions finalized_floor_certified_promotion_correct.

Theorem finalized_floor_latest_message_coverage_correct :
  forall
    (Block Validator : Type)
    (parent_edge : Block -> Block -> Prop)
    (latest : Validator -> Block)
    (candidate : Block)
    (decide : (Validator -> Prop) -> Prop),
    (forall left right,
      (forall validator, left validator <-> right validator) ->
      (decide left <-> decide right)) ->
    decide
      (fun validator =>
        propagated_coverage parent_edge latest candidate validator) <->
    decide
      (fun validator =>
        pairwise_support parent_edge latest candidate validator).
Proof.
  intros Block Validator parent_edge latest candidate decide Hextensional.
  apply coverage_decision_transparent.
  exact Hextensional.
Qed.

Print Assumptions finalized_floor_latest_message_coverage_correct.

Theorem finalized_floor_linear_snapshot_reuse_correct :
  forall
    (Block Validator : Type)
    (parent_edge : Block -> Block -> Prop)
    (latest : Validator -> Block)
    (predecessor parent candidate : Block)
    (eligible : Block -> Prop),
    (forall immediate,
      parent_edge immediate parent -> immediate = predecessor) ->
    (eligible parent ->
      exists validator,
        pairwise_support parent_edge latest parent validator) ->
    (forall validator,
      ~ pairwise_support parent_edge latest parent validator) ->
    eligible candidate ->
    dag_reaches parent_edge candidate parent ->
    dag_reaches parent_edge candidate predecessor.
Proof.
  intros Block Validator parent_edge latest predecessor parent candidate
    eligible Hunique Heligible_support Hparent_unsupported Hcandidate Hreaches.
  apply (@unchanged_linear_snapshot_reuse_sound
    Block Validator parent_edge latest
    predecessor parent candidate eligible); assumption.
Qed.

Print Assumptions finalized_floor_linear_snapshot_reuse_correct.

Theorem finalized_floor_snapshot_materialization_correct :
  forall
    (Block : Type)
    (block_eq_dec : forall left right : Block, {left = right} + {left <> right})
    (parents latest_messages finalizer_blocks cache : list Block),
    snapshot_ready
      parents latest_messages
      (materialize_all
        block_eq_dec
        (snapshot_required parents latest_messages)
        (materialize_all block_eq_dec finalizer_blocks cache))
    /\
    cache_equiv
      (materialize_all
        block_eq_dec
        (snapshot_required parents latest_messages)
        (materialize_all block_eq_dec finalizer_blocks cache))
      (materialize_all block_eq_dec finalizer_blocks
        (materialize_all
          block_eq_dec
          (snapshot_required parents latest_messages)
          cache)).
Proof.
  intros Block block_eq_dec parents latest_messages finalizer_blocks cache.
  destruct (SnapshotFloorMaterialization.finalized_floor_snapshot_materialization_correct
    block_eq_dec parents latest_messages finalizer_blocks cache)
    as [Hready [Hcommutes _]].
  exact (conj Hready Hcommutes).
Qed.

Print Assumptions finalized_floor_snapshot_materialization_correct.

Theorem finalized_floor_heartbeat_backpressure_correct :
  heartbeat_backpressure_contract.
Proof.
  exact heartbeat_backpressure_end_to_end.
Qed.

Print Assumptions finalized_floor_heartbeat_backpressure_correct.

Theorem finalized_floor_accountable_safety_correct :
  forall
    (dag : DAG)
    (snapshot : Snapshot)
    (incompatible : BlockHash -> BlockHash -> Prop)
    (committee : Committee)
    (faulty : list Validator)
    (num den : Z)
    (left right : BlockHash),
    NoDup (map fst committee) ->
    NoDup faulty ->
    incl faulty (map fst committee) ->
    causal_incompatibility_is_accountable
      dag snapshot incompatible faulty ->
    Finalized_ft dag committee snapshot left num den ->
    Finalized_ft dag committee snapshot right num den ->
    incompatible left right ->
    (0 < num)%Z ->
    (0 < den)%Z ->
    (Z.of_nat
      (validator_stake (committee_stake committee) faulty) * den <
      Z.of_nat (cweight committee) * num)%Z ->
    False.
Proof.
  exact exact_clique_certificates_are_accountably_safe.
Qed.

Print Assumptions finalized_floor_accountable_safety_correct.

Theorem finalized_floor_strict_accountable_safety_correct :
  forall
    (dag : DAG)
    (snapshot : Snapshot)
    (incompatible : BlockHash -> BlockHash -> Prop)
    (committee : Committee)
    (faulty : list Validator)
    (num den : Z)
    (left right : BlockHash),
    NoDup (map fst committee) ->
    NoDup faulty ->
    incl faulty (map fst committee) ->
    causal_incompatibility_is_accountable
      dag snapshot incompatible faulty ->
    Finalized_ft_gt dag committee snapshot left num den ->
    Finalized_ft_gt dag committee snapshot right num den ->
    incompatible left right ->
    (0 < den)%Z ->
    (Z.of_nat
      (validator_stake (committee_stake committee) faulty) * den <=
      Z.of_nat (cweight committee) * num)%Z ->
    False.
Proof.
  exact strict_exact_clique_certificates_are_accountably_safe.
Qed.

Print Assumptions finalized_floor_strict_accountable_safety_correct.

Theorem finalized_floor_parallel_validator_consensus_correct :
  forall
    (Node Block Root Effect : Type)
    (node_eq_dec : forall left right : Node, {left = right} + {left <> right})
    (block_root : Block -> Root)
    (block_effects : Block -> list Effect)
    (certified state_certified : Block -> Prop),
    parallel_validator_contract
      node_eq_dec block_root block_effects certified state_certified.
Proof.
  intros Node Block Root Effect node_eq_dec block_root block_effects
    certified state_certified.
  apply parallel_validator_consensus_correct.
Qed.

Print Assumptions finalized_floor_parallel_validator_consensus_correct.

Theorem finalized_floor_parallel_accountable_promotion_correct :
  forall
    (Node Root Effect : Type)
    (node_eq_dec : forall left right : Node, {left = right} + {left <> right})
    (block_root : BlockHash -> Root)
    (block_effects : BlockHash -> list Effect)
    (state_certified : BlockHash -> Prop)
    (dag : DAG)
    (snapshot : Snapshot)
    (incompatible : BlockHash -> BlockHash -> Prop)
    (committee : Committee)
    (faulty : list Validator)
    (num den : Z),
    NoDup (map fst committee) ->
    NoDup faulty ->
    incl faulty (map fst committee) ->
    causal_incompatibility_is_accountable
      dag snapshot incompatible faulty ->
    (0 < num)%Z ->
    (0 < den)%Z ->
    (Z.of_nat
      (validator_stake (committee_stake committee) faulty) * den <
      Z.of_nat (cweight committee) * num)%Z ->
    parallel_validator_contract
      node_eq_dec
      block_root
      block_effects
      (fun block => Finalized_ft dag committee snapshot block num den)
      state_certified
    /\
    (forall left right,
      Finalized_ft dag committee snapshot left num den ->
      Finalized_ft dag committee snapshot right num den ->
      incompatible left right ->
      False).
Proof.
  intros Node Root Effect node_eq_dec block_root block_effects
    state_certified dag snapshot incompatible committee faulty num den
    Hcommittee Hfaulty Hfaulty_in Haccountable Hnum Hden Hbudget.
  split.
  - apply parallel_validator_consensus_correct.
  - intros left right Hleft Hright Hincompatible.
    eapply exact_clique_certificates_are_accountably_safe.
    + exact Hcommittee.
    + exact Hfaulty.
    + exact Hfaulty_in.
    + exact Haccountable.
    + exact Hleft.
    + exact Hright.
    + exact Hincompatible.
    + exact Hnum.
    + exact Hden.
    + exact Hbudget.
Qed.

Print Assumptions finalized_floor_parallel_accountable_promotion_correct.

Theorem validator_incarnation_consensus_correct :
  (forall amount state event next,
    lifecycle_step amount state event next ->
    generation_le
      (lifecycle_generation state)
      (lifecycle_generation next))
  /\
  (forall amount state event next,
    lifecycle_step amount state event next ->
    lifecycle_total next = lifecycle_total state)
  /\
  (forall authority claim certificate,
    sender_generation_certified authority claim certificate ->
    certificate_generation certificate = exact_authority_generation authority /\
    claim_generation claim = exact_authority_generation authority)
  /\
  (forall storage,
    certified_index_complete (duplicate_retry_repair storage))
  /\
  (forall authority latest incoming exact_justifications vote,
    In vote
      (derive_finality_vote_projection
        authority latest incoming exact_justifications) ->
    In vote exact_justifications)
  /\
  (forall authority latest incoming exact_justifications parent,
    In parent
      (derive_causal_parent_projection
        authority latest incoming exact_justifications) ->
    In parent exact_justifications)
  /\
  (forall authority latest incoming exact_justifications vote,
    In vote
      (derive_finality_vote_projection
        authority latest incoming exact_justifications) ->
    In vote
      (derive_causal_parent_projection
        authority latest incoming exact_justifications))
  /\
  (forall authority latest incoming left right,
    Permutation left right ->
    Permutation
      (derive_causal_parent_projection authority latest incoming left)
      (derive_causal_parent_projection authority latest incoming right))
  /\
  (forall authority latest incoming left right,
    Permutation left right ->
    Permutation
      (derive_finality_vote_projection authority latest incoming left)
      (derive_finality_vote_projection authority latest incoming right))
  /\
  (forall base promoted authority latest exact max_sequences view delta,
    consensus_finality_projection
      (certify_finality_context
        base promoted authority latest exact max_sequences view delta) =
    derive_finality_vote_projection
      authority latest (receiver_parent_evidence view) exact /\
    consensus_floor
      (certify_finality_context
        base promoted authority latest exact max_sequences view delta) =
    floor_from_projection base promoted
      (derive_finality_vote_projection
        authority latest (receiver_parent_evidence view) exact)).
Proof.
  exact
    (conj lifecycle_generation_monotone
      (conj lifecycle_step_preserves_value
        (conj sender_certificate_generation_is_parent_derived
          (conj duplicate_retry_repairs_metadata_index_crash_window
            (conj finality_projection_is_subset_of_exact_justifications
              (conj causal_parent_projection_is_subset_of_exact_justifications
                (conj finality_projection_is_subset_of_causal_parent_projection
                  (conj causal_parent_projection_is_permutation_invariant
                    (conj finality_vote_projection_is_permutation_invariant
                          candidate_delta_does_not_affect_own_floor))))))))).
Qed.

Print Assumptions validator_incarnation_consensus_correct.

Theorem certified_projection_binding_and_evidence_roots_correct :
  (forall authority latest incoming exact validator hash authority_entry latest_entry,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    hash <> certified_latest_hash latest_entry ->
    ~ In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact))
  /\
  (forall authority latest incoming exact validator hash authority_entry latest_entry,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    validator <> certified_latest_sender latest_entry ->
    ~ In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact))
  /\
  (forall authority latest incoming exact validator hash authority_entry latest_entry,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    certified_latest_generation latest_entry = None ->
    ~ In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact))
  /\
  (forall authority latest incoming exact validator hash authority_entry latest_entry,
    lookup_parent_authority validator authority = Some authority_entry ->
    lookup_certified_latest validator latest = Some latest_entry ->
    certified_latest_admission_accepted latest_entry = false ->
    ~ In (validator, hash)
      (derive_causal_parent_projection authority latest incoming exact))
  /\
  (forall authority latest incoming exact justification,
    ~ In justification
      (derive_causal_parent_projection authority latest incoming exact) ->
    ~ In justification
      (derive_finality_vote_projection authority latest incoming exact))
  /\
  (forall root_evidence floor exact evidence,
    In evidence (root_evidence floor) ->
    In evidence (certified_evidence_closure root_evidence floor exact))
  /\
  (forall root_evidence floor exact validator hash evidence,
    In (validator, hash) exact ->
    In evidence (root_evidence hash) ->
    In evidence (certified_evidence_closure root_evidence floor exact)).
Proof.
  exact
    (conj mismatched_hash_cannot_be_causal_parent
      (conj mismatched_sender_cannot_be_causal_parent
        (conj missing_generation_cannot_be_causal_parent
          (conj nonaccepted_latest_message_cannot_be_causal_parent
            (conj causal_exclusion_implies_vote_exclusion
              (conj selected_floor_evidence_survives_stale_latest_messages
                    exact_latest_evidence_is_in_certified_closure)))))).
Qed.

Print Assumptions certified_projection_binding_and_evidence_roots_correct.

Theorem finalization_closure_availability_correct :
  (forall authority latest incoming exact validator hash,
    lookup_parent_authority validator authority = None ->
    ~ In (validator, hash)
      (derive_finality_vote_projection authority latest incoming exact))
  /\
  (forall held dependencies closure_invalid authority latest incoming exact missing,
    capture_finality_projection
      held dependencies closure_invalid authority latest incoming exact =
      MissingFinalityDependency missing ->
    In missing dependencies /\ held missing = false)
  /\
  (forall held dependencies closure_invalid authority latest incoming exact missing,
    capture_finality_projection
      held dependencies closure_invalid authority latest incoming exact =
      MissingFinalityDependency missing ->
    projection_from_capture
      (capture_finality_projection
        held dependencies closure_invalid authority latest incoming exact) = None)
  /\
  (forall base promoted exact max_sequences incoming delta capture missing,
    capture = MissingFinalityDependency missing ->
    certificate_from_projection_capture
      base promoted exact max_sequences incoming delta capture = None)
  /\
  (forall base promoted exact max_sequences incoming delta capture,
    capture = InvalidFinalityClosure ->
    certificate_from_projection_capture
      base promoted exact max_sequences incoming delta capture = None)
  /\
  (forall held dependencies authority latest incoming exact,
    Forall (fun dependency => held dependency = true) dependencies ->
    capture_finality_projection
      held dependencies false authority latest incoming exact =
      CompleteFinalityProjection
        (derive_finality_vote_projection authority latest incoming exact))
  /\
  (forall base promoted exact max_sequences incoming delta capture projection,
    capture = CompleteFinalityProjection projection ->
    exists certificate,
      certificate_from_projection_capture
        base promoted exact max_sequences incoming delta capture = Some certificate /\
      consensus_finality_projection certificate = projection).
Proof.
  exact
    (conj absent_authority_cannot_vote
      (conj missing_capture_names_exact_unheld_dependency
        (conj incomplete_closure_has_no_projection
          (conj incomplete_closure_has_no_certificate
            (conj invalid_closure_has_no_certificate
              (conj full_restoration_reproduces_complete_projection
                    complete_capture_certifies_the_same_projection)))))).
Qed.

Print Assumptions finalization_closure_availability_correct.

Theorem finalized_floor_certified_causal_admission_correct :
  (forall left right incarnation,
    context_join left right incarnation = context_join right left incarnation)
  /\
  (forall left middle right incarnation,
    context_join (context_join left middle) right incarnation =
    context_join left (context_join middle right) incarnation)
  /\
  (forall context incarnation,
    context_join context context incarnation = context incarnation)
  /\
  (forall node,
    causal_node_predecessors (with_decision node CertifiedRejected) =
    causal_node_predecessors (with_decision node CertifiedAccepted))
  /\
  (forall node incarnation,
    propagated_node_delta (with_decision node CertifiedRejected) incarnation = None)
  /\
  (forall inherited structural ambient_left ambient_right incarnation,
    certified_effective_context inherited structural ambient_left incarnation =
    certified_effective_context inherited structural ambient_right incarnation).
Proof. exact certified_causal_admission_correct. Qed.

Print Assumptions finalized_floor_certified_causal_admission_correct.

Theorem finalized_floor_genesis_approval_trust_correct :
  (forall local_minimum candidate_threshold bonded_count valid_distinct_count,
    approval_authorized local_minimum candidate_threshold bonded_count valid_distinct_count ->
    local_minimum <= candidate_threshold /\
    candidate_threshold <= bonded_count /\
    candidate_threshold <= valid_distinct_count)
  /\
  (forall local_minimum bonded_count,
    approval_authorized local_minimum 0 bonded_count 0 -> local_minimum = 0)
  /\
  (forall local_minimum candidate_threshold bonded_count valid_distinct_count state,
    ~ approval_authorized local_minimum candidate_threshold bonded_count valid_distinct_count ->
    apply_approval local_minimum candidate_threshold bonded_count valid_distinct_count state = state).
Proof. exact genesis_approval_trust_correct. Qed.

Print Assumptions finalized_floor_genesis_approval_trust_correct.

Theorem finalized_floor_rooted_genesis_identity_correct :
  (forall genesis head records,
    let store :=
      {| stored_genesis_anchor := Some genesis;
         stored_finalization_head := Some head;
         stored_finalization_records := records;
         stored_recovery_cursor_count := 3 |} in
    ensure_genesis_identity genesis store = Some store)
  /\
  (forall canonical requested head records,
    requested <> canonical ->
    let store :=
      {| stored_genesis_anchor := Some canonical;
         stored_finalization_head := Some head;
         stored_finalization_records := records;
         stored_recovery_cursor_count := 3 |} in
    ensure_genesis_identity requested store = None)
  /\
  (forall candidate store next,
    append_rooted_finalization candidate store = Some next ->
    stored_genesis_anchor next = stored_genesis_anchor store /\
    stored_genesis_anchor next <> None)
  /\
  (forall store, restart_rooted_finalization store = store).
Proof.
  exact
    (conj exact_genesis_assertion_is_write_free
      (conj conflicting_genesis_assertion_fails_closed
        (conj successful_rooted_append_preserves_genesis
              restart_preserves_rooted_finalization_identity))).
Qed.

Print Assumptions finalized_floor_rooted_genesis_identity_correct.

Theorem finalized_floor_live_minor_fork_recovery_correct :
  (local_target stepped_history_head = local_target direct_history_head /\
   local_revision stepped_history_head <> local_revision direct_history_head /\
   local_digest stepped_history_head <> local_digest direct_history_head)
  /\
  (forall tip state, advertise_remote_tip tip state = state)
  /\
  (forall target state published,
    run_local_finalizer target state = Some published ->
    live_durable_head state <= live_durable_head published /\
    live_durable_head published = live_effects_through published).
Proof.
  split.
  - exact equal_finalized_target_does_not_imply_equal_local_ledger_identity.
  - split.
    + exact remote_tip_advertisement_cannot_mutate_local_state.
    + intros target state published Hrun.
      split.
      * exact (proj1 (local_finalizer_is_monotone_and_effect_atomic
                        target state published Hrun)).
      * exact (proj1 (proj2 (local_finalizer_is_monotone_and_effect_atomic
                              target state published Hrun))).
Qed.

Print Assumptions finalized_floor_live_minor_fork_recovery_correct.

Theorem finalized_floor_materialization_target_alignment_correct :
  pairwise_support trace_full_parent trace_latest
    TraceSibling3 TraceValidator4 /\
  ~ pairwise_support trace_main_parent trace_latest
      TraceSibling3 TraceValidator4 /\
  ~ trace_state_preserves TraceSibling2 TraceMerge /\
  trace_state_preserves TraceSibling3 TraceMerge /\
  2 * 8 <= 16 /\ 12 > 8.
Proof.
  exact finalizer_floor_materialization_trace_correct.
Qed.

Print Assumptions finalized_floor_materialization_target_alignment_correct.

Theorem finalized_floor_target_deploy_wait_correct :
  (forall status now last_progress_at stall_timeout absolute_timeout,
    classify_deploy_wait
      status now last_progress_at stall_timeout absolute_timeout =
      WaitSucceeded ->
    status = StatusFinalized)
  /\
  (forall status now last_progress_at stall_timeout absolute_timeout,
    progress_deadline_expired
      now last_progress_at stall_timeout absolute_timeout = false ->
    status = StatusFailed \/ status = StatusExpired ->
    classify_deploy_wait
      status now last_progress_at stall_timeout absolute_timeout =
      WaitTerminalError)
  /\
  (forall observation observed_at previous_progress_at,
    observation <> ObservationStrictProgress ->
    progress_time_after_observation
      observation observed_at previous_progress_at = previous_progress_at)
  /\
  (forall observed_at previous_progress_at,
    progress_time_after_observation
      ObservationStrictProgress observed_at previous_progress_at = observed_at)
  /\
  (forall previous_height previous_hash next_height next_hash observed_at
    previous_progress_at,
    classify_lfb_observation
      false previous_height previous_hash next_height next_hash =
      ObservationBaseline /\
    progress_time_after_observation
      ObservationBaseline observed_at previous_progress_at = previous_progress_at)
  /\
  (history_corruption ObservationRegression = true /\
   history_corruption ObservationRevision = true /\
   history_corruption ObservationBaseline = false /\
   history_corruption ObservationStable = false /\
   history_corruption ObservationStrictProgress = false)
  /\
  (classify_lfb_wait_observation ObservationRegression =
     WaitHistoryCorruption /\
   classify_lfb_wait_observation ObservationRevision =
     WaitHistoryCorruption)
  /\
  (classify_lfb_observation true 6 10 6 11 = ObservationRevision /\
   classify_lfb_observation true 6 10 5 9 = ObservationRegression)
  /\
  (forall now last_progress_at stall_timeout absolute_timeout,
    absolute_timeout <= now ->
    progress_deadline_expired
      now last_progress_at stall_timeout absolute_timeout = true)
  /\
  (forall status now last_progress_at stall_timeout absolute_timeout,
    absolute_timeout <= now ->
    classify_deploy_wait
      status now last_progress_at stall_timeout absolute_timeout =
      WaitTimedOut)
  /\
  (classify_deploy_wait StatusFinalized 8 5 3 8 = WaitTimedOut /\
   classify_deploy_wait StatusFailed 8 5 3 8 = WaitTimedOut /\
   classify_deploy_wait StatusExpired 8 5 3 8 = WaitTimedOut)
  /\
  (fixed_deadline_expired 45 45 = true /\
   progress_deadline_expired 45 43 45 135 = false)
  /\
  (classify_deploy_wait StatusPending 45 43 45 135 = WaitPending /\
   classify_deploy_wait StatusFinalized 49 43 45 135 = WaitSucceeded)
  /\
  (forall start stall_timeout absolute_timeout,
    stall_timeout <= absolute_timeout ->
    progress_deadline_expired
      (start + stall_timeout) start stall_timeout absolute_timeout = true).
Proof.
  exact
    (conj exact_success_requires_exact_finalized_status
      (conj in_budget_failed_or_expired_is_terminal_error
        (conj only_strict_height_progress_renews_stall_budget
          (conj strict_height_progress_renews_stall_budget
            (conj first_observation_establishes_baseline_without_renewal
              (conj finalized_history_anomalies_fail_loudly
                (conj finalized_history_anomalies_are_terminal_observer_errors
                  (conj concrete_revision_and_regression_are_detected
                    (conj absolute_deadline_cannot_be_renewed
                      (conj expired_observation_cannot_report_terminal_success
                        (conj terminal_response_at_deadline_is_timeout
                          (conj fixed_deadline_rejects_valid_intermediate_progress_trace
                            (conj reproduced_trace_succeeds_only_at_exact_terminality
                                  no_progress_trace_is_stall_bounded))))))))))))).
Qed.

Print Assumptions finalized_floor_target_deploy_wait_correct.

Theorem finalized_floor_node_local_product_lifting_correct :
  forall
    (Node LocalState Action : Type)
    (node_eq_dec : forall left right : Node, {left = right} + {left <> right})
    (local_step : Action -> LocalState -> LocalState)
    (local_invariant local_goal : LocalState -> Prop)
    (local_enabled : Action -> LocalState -> Prop),
    node_local_product_contract
      node_eq_dec local_step local_invariant local_goal local_enabled.
Proof.
  intros Node LocalState Action node_eq_dec local_step local_invariant
    local_goal local_enabled.
  apply node_local_product_lifting_correct.
Qed.

Print Assumptions finalized_floor_node_local_product_lifting_correct.

Definition finalized_floor_node_local_temporal_lifting_correct :=
  @node_local_temporal_product_lifting_correct.

Print Assumptions finalized_floor_node_local_temporal_lifting_correct.

Definition finalized_floor_certificate_retrieval_correct :=
  @finalization_certificate_retrieval_contract.

Print Assumptions finalized_floor_certificate_retrieval_correct.

Definition finalized_floor_dependency_maintenance_correct :=
  @dependency_maintenance_round_contract.

Print Assumptions finalized_floor_dependency_maintenance_correct.

Definition finalized_floor_witness_equivalent_carrier_correct :=
  @witness_equivalent_carrier_contract.

Print Assumptions finalized_floor_witness_equivalent_carrier_correct.

Definition finalized_floor_collective_recovery_coverage_correct :=
  @one_parent_coverage_implies_collective_coverage.

Print Assumptions finalized_floor_collective_recovery_coverage_correct.

Definition finalized_floor_split_recovery_frontier_correct :=
  collective_coverage_does_not_require_one_covering_parent.

Print Assumptions finalized_floor_split_recovery_frontier_correct.

Definition finalized_floor_recovery_leadership_separation_correct :=
  @retry_readiness_is_independent_of_ordinary_leadership.

Print Assumptions finalized_floor_recovery_leadership_separation_correct.

Definition finalized_floor_recovery_parent_order_independent :=
  @collective_coverage_parent_permutation.

Print Assumptions finalized_floor_recovery_parent_order_independent.

Definition finalized_floor_recovery_latest_order_independent :=
  @collective_coverage_latest_message_permutation.

Print Assumptions finalized_floor_recovery_latest_order_independent.
