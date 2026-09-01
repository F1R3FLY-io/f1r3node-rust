(* ═══════════════════════════════════════════════════════════════════════════
   MainTheorem.v — Top-level statement composing all results

   Composes:
     - T-1 .. T-3 (detection layer)
     - T-4 .. T-5 (record persistence)
     - T-7 .. T-10 (slash effect)
     - T-11 .. T-12 (two-level closure)
     - T-9.1 .. T-9.15 (bug-fix correctness)

   The headline statement: main_slashing_algorithm_correct — for every
   detected admissible/ignorable equivocation, the slash effect zeros the
   offender's bond, records the witnessing hash, excludes the offender from
   fork choice, halts minting, and quarantines the stake without changing the
   Coop vault. Redemption performs the later adjudicated transfer.

   Companion doc: slashing-verification.md §11.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Slashing Require Import
  Validator ValidatorLifetime BondGenerationLifecycle Block InvalidBlock
  EquivocationRecord DAGState
  EquivocationDetector DetectorProduct PoSContract SlashDeploy BlockCreator ForkChoice
  TwoLevelSlashing
  BugFixIgnorable BugFixAtomicTracker BugFixDispatcher
  BugFixTransferFailure BugFixStakeZero BugFixSelfRegression
  BugFixSeqNumDensity BugFixSeqArithmetic BugFixDuplicateJustifications
  BugFixSlashAuthorization BugFixUnbondedProposer
  BugFixWithdrawTransferFailure ProtocolV5DependencyReadiness.

Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Detection layer summary
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem main_T1_detection_sound :
  forall cj lm d s,
    detect cj lm d = s ->
    s = DSAdmissible \/ s = DSIgnorable ->
    equivocates_ptr cj lm = true.
Proof. exact detection_sound. Qed.

Theorem main_T2_detection_complete :
  forall cj lm d,
    equivocates_ptr cj lm = true ->
    detect cj lm d = DSAdmissible \/ detect cj lm d = DSIgnorable.
Proof. exact detection_complete. Qed.

Theorem main_T2_detector_product_locality :
  forall (Validator LocalState : Type)
         (validator_eq_dec : forall x y : Validator, {x = y} + {x <> y})
         (local_step : LocalState -> LocalState -> Prop)
         (local_invariant : LocalState -> Prop),
    (forall before after,
        local_invariant before ->
        local_step before after ->
        local_invariant after) ->
    forall before after,
      @detector_product_invariant Validator LocalState local_invariant before ->
      @detector_product_steps Validator LocalState local_step before after ->
      @detector_product_invariant Validator LocalState local_invariant after.
Proof.
  intros Validator LocalState validator_eq_dec local_step local_invariant.
  exact (@detector_product_steps_preserve_pointwise_invariant
           Validator LocalState validator_eq_dec local_step local_invariant).
Qed.

Theorem main_T3_slashable_taxonomy :
  forall ib,
    is_slashable ib = true <->
    ib = IBAdmissibleEquivocation \/ ib = IBIgnorableEquivocation.
Proof. exact slashable_current_exact. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Record persistence summary
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem main_T4_record_monotone :
  forall s k h k',
    incl (hashes_at_key s k') (hashes_at_key (update_record s k h) k').
Proof. exact t_4_record_monotone_update. Qed.

Theorem main_T5_record_unique :
  forall s r,
    unique_keys s ->
    unique_keys (insert_cond s r).
Proof. exact t_5_insert_cond_preserves_unique. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — Slash effect summary
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem main_T7_slash_zeros_bond :
  forall psc v,
    let (psc', _) := slashC psc v in
    bm_lookup (ps_allBonds (psc_pos psc')) v = 0.
Proof. exact slashC_zeros_bond. Qed.

Theorem main_T9_slash_idempotent :
  forall psc v,
    let (psc1, _) := slashC psc v in
    let (psc2, _) := slashC psc1 v in
    ps_allBonds (psc_pos psc2) = ps_allBonds (psc_pos psc1)
    /\ ps_coopVault (psc_pos psc2) = ps_coopVault (psc_pos psc1)
    /\ ps_active (psc_pos psc2) = ps_active (psc_pos psc1)
    /\ psc_mintingHalted psc2 = psc_mintingHalted psc1
    /\ psc_quarantined psc2 = psc_quarantined psc1.
Proof. exact slashC_idempotent. Qed.

Theorem main_TIdem_zero_bond_noop :
  forall psc v,
    bm_lookup (ps_allBonds (psc_pos psc)) v = 0 ->
    slashC psc v = (psc, true).
Proof. exact slashC_zero_bond_noop. Qed.

Theorem main_T10_fork_choice_exclusion :
  forall lm bonds v,
    bm_lookup bonds v = 0 ->
    fc_lookup (filter_slashed lm bonds) v = None.
Proof. exact fork_choice_exclusion. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — Bug-fix summary
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem main_T9_1_ignorable :
  forall cj lm d,
    detect cj lm d = DSIgnorable ->
    is_slashable IBIgnorableEquivocation = true /\ equivocates_ptr cj lm = true.
Proof. exact post_fix_ignorable_implies_equivocation. Qed.

(* Bug fix #1 — every current slashable variant is an objective equivocation. *)
Theorem main_T9_1_slashable_attributable :
  forall ib,
    is_slashable ib = true ->
    ib = IBAdmissibleEquivocation \/ ib = IBIgnorableEquivocation.
Proof. exact bug_fix_ignorable_safety. Qed.

Theorem main_T9_1_unauthorized_rejection_has_no_evidence :
  is_slashable IBUnauthorizedSlashDeploy = false.
Proof. exact unauthorized_current_not_slashable. Qed.

(* FV audit #6 remediation — unbonded-window record pollution fork.
   The stake-0 / unbonded offender now resolves to EquivocationOblivious
   (equivocation_detector.rs:280,311), which makes the caller's stamping arm
   unreachable, so an unbonded offender's witness set can never be polluted and
   the fork cannot arise. These three capstones pin the fix:

   (1a-i)   the unbonded discovery status is always Oblivious;
   (1a-ii)  stamping an unbonded offender's record is a no-op (empty witness);
   (1a-iii) two nodes stamping candidate hashes in EITHER order reach the SAME
            record (= the original r), so the observation-order-dependent
            NeglectedEquivocation divergence is impossible. *)
Theorem main_T9_1a_unbonded_oblivious :
  forall d, discovery_status_bonded 0 d = EDOblivious.
Proof. exact unbonded_offender_oblivious. Qed.

Theorem main_T9_1a_unbonded_no_stamp :
  forall r d h, stamp_on_status r (discovery_status_bonded 0 d) h = r.
Proof. exact unbonded_stamp_noop. Qed.

Theorem main_T9_1a_unbonded_order_independent :
  forall r d h1 h2,
    let st := discovery_status_bonded 0 d in
    stamp_on_status (stamp_on_status r st h1) st h2
    = stamp_on_status (stamp_on_status r st h2) st h1
    /\ stamp_on_status (stamp_on_status r st h1) st h2 = r.
Proof. exact unbonded_witness_order_independent. Qed.

Theorem main_T9_2_atomic :
  forall s k h,
    incl (hashes_at_key s k) (hashes_at_key (atomic_record_or_update s k h) k).
Proof. exact t_9_2_atomic_no_overwrite. Qed.

Theorem main_T9_3_dispatch :
  forall ib offender baseSeq s,
    is_slashable ib = true ->
    has_key (dispatch_post_fix ib offender baseSeq s) (offender, baseSeq) = true.
Proof. exact t_9_3_dispatch_complete. Qed.

Theorem main_T9_3_certified_rejection_persists_without_false_evidence :
  forall offender baseSeq s,
    rejection_persisted
      (dispatch_certified_rejection IBInvalidSequenceNumber offender baseSeq s) = true
    /\ rejection_buffered
      (dispatch_certified_rejection IBInvalidSequenceNumber offender baseSeq s) = false
    /\ rejection_evidence
      (dispatch_certified_rejection IBInvalidSequenceNumber offender baseSeq s) = s.
Proof. exact invalid_sequence_persists_without_evidence. Qed.

Theorem main_T9_3_every_certified_rejection_is_terminal :
  forall ib offender baseSeq s,
    rejection_persisted
      (dispatch_certified_rejection ib offender baseSeq s) = true
    /\ rejection_buffered
      (dispatch_certified_rejection ib offender baseSeq s) = false.
Proof. exact every_certified_rejection_is_terminal. Qed.

Theorem main_T9_3_non_slashable_rejection_preserves_evidence :
  forall ib offender baseSeq s,
    is_slashable ib = false ->
    rejection_evidence
      (dispatch_certified_rejection ib offender baseSeq s) = s.
Proof. exact certified_non_slashable_rejection_preserves_evidence. Qed.

Theorem main_T9_3_block_exception_is_local_fault :
  classify_block_outcome BOException = None.
Proof. exact block_exception_is_not_objective_invalidity. Qed.

Theorem main_T9_4_transfer :
  forall ps v transfer_ok,
    let result := slash_with_transfer_oracle ps v transfer_ok in
    let ps' := fst result in
    let ok := snd result in
    (ok = true /\ bm_lookup (ps_allBonds ps') v = 0)
    \/ (ok = false /\ ps' = ps).
Proof. exact t_9_4_transfer_failure_safety. Qed.

Theorem main_T9_5_stake_zero :
  forall psc v,
    active_implies_bonded_stage_c psc ->
    let result := slashC psc v in
    let psc' := fst result in
    active_implies_bonded_stage_c psc'.
Proof. exact t_9_5_slashC_preserves_invariant. Qed.

Theorem main_T9_6_self_regression :
  forall blk_sn latest cited,
    cited < latest ->
    has_self_regression blk_sn latest cited = true.
Proof. exact t_9_6_self_regression_detected. Qed.

Theorem main_T9_7_seqnum_density :
  forall blocks sender baseSeq b,
    In b blocks ->
    Forall (canonical_candidate_prop sender baseSeq) blocks ->
    exists b', canonical_child_post_fix blocks sender baseSeq = Some b'.
Proof. exact t_9_7_canonical_finds_visible_descendant_with_gap. Qed.

Theorem main_T9_7_seqnum_density_dense_subsumption :
  forall b sender baseSeq,
    block_sender b = sender ->
    block_seq b = S baseSeq ->
    canonical_child_post_fix [b] sender baseSeq = Some b.
Proof. exact t_9_7_canonical_dense_subsumes_pre_fix. Qed.

Theorem main_T9_7_seqnum_density_same_branch_stability :
  forall prefix chain sender baseSeq b,
    Forall (canonical_candidate_prop sender baseSeq) prefix ->
    canonical_child_post_fix chain sender baseSeq = Some b ->
    canonical_child_post_fix (prefix ++ chain) sender baseSeq = Some b.
Proof. exact t_9_7_canonical_prefix_stability. Qed.

Theorem main_T9_7_seqnum_density_memoized_equivalent :
  forall chain sender baseSeq cached,
    canonical_cache_consistent chain sender baseSeq cached ->
    canonical_child_memoized cached chain sender baseSeq =
    canonical_child_post_fix chain sender baseSeq.
Proof. exact t_9_7_canonical_memoized_equivalent. Qed.

Theorem main_T9_8_unbonded :
  forall candidates bonds generations proposer seqNum currentEpoch seed_fn,
    bm_lookup bonds proposer = 0 ->
    prepare_slashing_deploys_post_fix
      candidates bonds generations proposer seqNum currentEpoch seed_fn = [].
Proof. exact t_9_8_unbonded_proposer_no_slash. Qed.

Theorem main_T9_9_self_correcting :
  forall hn hs,
    rejects_neglected_post_fix hn hs = true <-> (hn = true /\ hs = false).
Proof. exact t_9_9_post_fix_rejection_iff. Qed.

Theorem main_T9_10_withdraw_transfer_failure :
  forall psw v transfer_ok,
    let psw' := withdraw_with_transfer_oracle psw v transfer_ok in
    (transfer_ok = true /\ wm_contains (psw_withdrawers psw') v = false)
    \/ (transfer_ok = false /\ psw' = psw).
Proof. exact t_9_10_withdraw_transfer_failure_safety. Qed.

Theorem main_T9_10_failure_preserves_total_funds :
  forall psw v,
    total_funds (withdraw_with_transfer_oracle psw v false) = total_funds psw.
Proof. exact t_9_10_failure_preserves_total_funds. Qed.

Theorem main_T9_10_withdraw_independence :
  forall psw v u ok_v ok_u,
    v <> u ->
    let psw1 := withdraw_with_transfer_oracle
                  (withdraw_with_transfer_oracle psw v ok_v) u ok_u in
    let psw2 := withdraw_with_transfer_oracle
                  (withdraw_with_transfer_oracle psw u ok_u) v ok_v in
    psw_withdrawers psw1 = psw_withdrawers psw2
    /\ psw_rewards psw1 = psw_rewards psw2.
Proof. exact t_9_10_withdraw_independence. Qed.

Theorem main_T9_12_stale_generation_evidence_not_authorized :
  forall validator evidence_generation target_generation,
    evidence_generation <> target_generation ->
    evidence_authorizes_lifetime
      (mkValidatorLifetimeId validator evidence_generation)
      (mkValidatorLifetimeId validator target_generation) = false.
Proof. exact stale_generation_evidence_not_authorized. Qed.

Theorem main_T9_12_epoch_advance_preserves_lifetime :
  forall validator generation (old_epoch new_epoch : Epoch),
    same_lifetime
      (mkValidatorLifetimeId validator generation)
      (mkValidatorLifetimeId validator generation).
Proof. exact same_generation_different_epoch_same_lifetime. Qed.

Theorem main_T9_12_fresh_bond_preserves_generation_count_relation :
  forall state next,
    generation_matches_successful_bonds state ->
    fresh_bond state = Some next ->
    generation_matches_successful_bonds next.
Proof. exact fresh_bond_preserves_generation_count_relation. Qed.

Theorem main_T9_12_successful_rebond_strictly_increases_generation :
  forall generation successful_bonds next,
    fresh_bond
      (mkValidatorBondLifecycle
        (Some generation) successful_bonds WithdrawnPhase) = Some next ->
    generation <
      match lifecycle_generation next with
      | Some next_generation => next_generation
      | None => 0
      end.
Proof. exact successful_rebond_strictly_increases_generation. Qed.

Theorem main_T9_12_generation_changes_only_on_successful_fresh_bond :
  forall state,
    lifecycle_generation (request_withdraw state) =
      lifecycle_generation state /\
    lifecycle_generation (begin_withdraw state) =
      lifecycle_generation state /\
    lifecycle_generation (complete_withdraw state) =
      lifecycle_generation state /\
    lifecycle_generation (advance_epoch state) =
      lifecycle_generation state.
Proof. exact generation_changes_only_on_successful_fresh_bond. Qed.

Theorem main_T9_12_stale_generation_slash_is_noninterfering :
  forall state current_generation target_generation,
    lifecycle_generation state = Some current_generation ->
    target_generation <> current_generation ->
    slash_lifecycle state target_generation = (state, false).
Proof. exact stale_generation_slash_is_noninterfering. Qed.

Theorem main_T9_12_current_generation_locked_slash_quarantines :
  forall generation successful_bonds phase,
    phase = BondedPhase \/
    phase = PendingWithdrawPhase \/
    phase = WithdrawingPhase ->
    slash_lifecycle
      (mkValidatorBondLifecycle
        (Some generation) successful_bonds phase)
      generation =
    (mkValidatorBondLifecycle
      (Some generation) successful_bonds QuarantinedPhase,
     true).
Proof. exact current_generation_locked_slash_quarantines. Qed.

Theorem main_T9_12_generation_guard_refines_current_pos_slash :
  forall psc offender generation successful_bonds phase,
    phase = BondedPhase \/
    phase = PendingWithdrawPhase \/
    phase = WithdrawingPhase ->
    bm_lookup (ps_allBonds (psc_pos psc)) offender > 0 ->
    let result := generation_scoped_slash_effect
      psc
      (mkValidatorBondLifecycle (Some generation) successful_bonds phase)
      offender generation in
    let psc_after := fst (fst result) in
    let lifecycle_after := snd (fst result) in
    bm_lookup (ps_allBonds (psc_pos psc_after)) offender = 0 /\
    qs_lookup (psc_quarantined psc_after) offender =
      Some (bm_lookup (ps_allBonds (psc_pos psc)) offender) /\
    halted_mem (psc_mintingHalted psc_after) offender = true /\
    ps_coopVault (psc_pos psc_after) = ps_coopVault (psc_pos psc) /\
    lifecycle_phase lifecycle_after = QuarantinedPhase /\
    lifecycle_generation lifecycle_after = Some generation.
Proof. exact generation_scoped_slash_locked_zeros_and_quarantines. Qed.

Theorem main_T9_12_generation_exhaustion_rejects_rebond :
  forall generation successful_bonds,
    checked_next_generation generation = None ->
    fresh_bond
      (mkValidatorBondLifecycle
        (Some generation) successful_bonds WithdrawnPhase) = None.
Proof. exact unavailable_next_generation_rejects_rebond. Qed.

Theorem main_T9_13_unknown_slash_evidence_noop :
  forall ps sd evidence current_epoch canonical_generations,
    evidence_lookup evidence (sd_target_hash sd) = None ->
    execute_slash_deploy ps sd current_epoch canonical_generations
      (evidence_lookup evidence) = (ps, false).
Proof. exact unauthorized_unknown_execution_noop. Qed.

Theorem main_T9_13_zero_canonical_bond_not_authorized :
  forall current_epoch canonical_bonds canonical_generations sd evidence
         offender evidence_generation evidence_epoch,
    evidence_lookup evidence (sd_target_hash sd) =
      Some (offender, (evidence_generation, evidence_epoch)) ->
    bm_lookup canonical_bonds offender = 0 ->
    authorized_slash_candidate current_epoch canonical_bonds
      canonical_generations sd evidence = false.
Proof. exact zero_canonical_bond_not_authorized_candidate. Qed.

Theorem main_T9_13_matching_lifetime_current_window_authorized :
  forall current_epoch canonical_bonds canonical_generations sd evidence
         offender generation,
    evidence_lookup evidence (sd_target_hash sd) =
      Some (offender, (generation, current_epoch)) ->
    sd_target_activation_epoch sd = current_epoch ->
    sd_target_bond_generation sd = generation ->
    gm_lookup canonical_generations offender = Some generation ->
    bm_lookup canonical_bonds offender > 0 ->
    authorized_slash_candidate current_epoch canonical_bonds
      canonical_generations sd evidence = true.
Proof. exact matching_generation_current_window_positive_bond_authorized. Qed.

Theorem main_T9_13_canonical_pre_state_authorizes_when_ambient_differs :
  forall current_epoch ambient_bonds ambient_generations
         canonical_bonds canonical_generations sd evidence offender generation,
    evidence_lookup evidence (sd_target_hash sd) =
      Some (offender, (generation, current_epoch)) ->
    sd_target_activation_epoch sd = current_epoch ->
    sd_target_bond_generation sd = generation ->
    gm_lookup canonical_generations offender = Some generation ->
    bm_lookup ambient_bonds offender = 0 ->
    gm_lookup ambient_generations offender <> Some generation ->
    bm_lookup canonical_bonds offender > 0 ->
    authorized_slash_candidate_with_ambient
      current_epoch ambient_bonds ambient_generations canonical_bonds
      canonical_generations sd evidence = true.
Proof. exact canonical_pre_state_authorizes_when_ambient_differs. Qed.

Theorem main_T9_13_canonical_zero_rejects_even_if_ambient_positive :
  forall current_epoch ambient_bonds ambient_generations
         canonical_bonds canonical_generations sd evidence offender
         evidence_generation evidence_epoch,
    evidence_lookup evidence (sd_target_hash sd) =
      Some (offender, (evidence_generation, evidence_epoch)) ->
    bm_lookup ambient_bonds offender > 0 ->
    bm_lookup canonical_bonds offender = 0 ->
    authorized_slash_candidate_with_ambient
      current_epoch ambient_bonds ambient_generations canonical_bonds
      canonical_generations sd evidence = false.
Proof. exact canonical_zero_rejects_even_if_ambient_positive. Qed.

Theorem main_T9_13_stale_activation_epoch_not_authorized :
  forall current_epoch canonical_bonds canonical_generations sd evidence
         offender evidence_generation old_epoch,
    evidence_lookup evidence (sd_target_hash sd) =
      Some (offender, (evidence_generation, old_epoch)) ->
    old_epoch <> current_epoch ->
    authorized_slash_candidate current_epoch canonical_bonds
      canonical_generations sd evidence = false.
Proof. exact stale_activation_epoch_not_authorized_candidate. Qed.

Theorem main_T9_13_stale_generation_not_authorized :
  forall current_epoch canonical_bonds canonical_generations sd evidence
         offender evidence_generation,
    evidence_lookup evidence (sd_target_hash sd) =
      Some (offender, (evidence_generation, current_epoch)) ->
    evidence_generation <> sd_target_bond_generation sd ->
    authorized_slash_candidate current_epoch canonical_bonds
      canonical_generations sd evidence = false.
Proof. exact stale_generation_not_authorized_candidate. Qed.

Theorem main_T9_13_proposer_receiver_authorization_parity :
  forall current_epoch proposer_ambient_bonds receiver_ambient_bonds
         proposer_ambient_generations receiver_ambient_generations
         canonical_bonds canonical_generations sd evidence,
    authorized_slash_candidate_for_origin
      OriginProposer current_epoch proposer_ambient_bonds
      proposer_ambient_generations canonical_bonds canonical_generations
      sd evidence =
    authorized_slash_candidate_for_origin
      OriginReceiver current_epoch receiver_ambient_bonds
      receiver_ambient_generations canonical_bonds canonical_generations
      sd evidence.
Proof. exact proposer_receiver_authorization_parity. Qed.

Theorem main_T9_13_same_pre_state_root_same_authorization :
  forall current_epoch proposer_ambient_bonds receiver_ambient_bonds
         proposer_ambient_generations receiver_ambient_generations
         bond_state generation_state
         proposer_root receiver_root sd evidence,
    proposer_root = receiver_root ->
    authorized_slash_candidate_at_root
      OriginProposer current_epoch proposer_ambient_bonds
      proposer_ambient_generations bond_state generation_state
      proposer_root sd evidence =
    authorized_slash_candidate_at_root
      OriginReceiver current_epoch receiver_ambient_bonds
      receiver_ambient_generations bond_state generation_state
      receiver_root sd evidence.
Proof. exact same_pre_state_root_same_authorization. Qed.

Theorem main_T9_13_merge_rejected_hint_subsumed_by_authorized_scan :
  forall rejectedHints candidates bonds generations currentEpoch candidate,
    In candidate rejectedHints ->
    In candidate candidates ->
    candidate_authorized bonds generations currentEpoch candidate = true ->
    In candidate
      (selected_slash_candidates candidates bonds generations currentEpoch).
Proof. exact merge_rejected_hint_subsumed_by_authorized_scan. Qed.

Theorem main_T9_13_zero_bond_candidate_not_selected :
  forall candidates bonds generations currentEpoch validator hash
         targetEpoch targetGeneration,
    bm_lookup bonds validator = 0 ->
    ~ In (validator, hash, targetEpoch, targetGeneration)
        (selected_slash_candidates candidates bonds generations currentEpoch).
Proof. exact zero_bond_candidate_not_selected. Qed.

Theorem main_T9_13_selected_target_keys_nodup :
  forall candidates bonds generations currentEpoch,
    NoDup (map candidate_key candidates) ->
    NoDup
      (map candidate_key
        (selected_slash_candidates candidates bonds generations currentEpoch)).
Proof. exact selected_target_keys_nodup. Qed.

(* Bug fix #3 — the FULL §9.8 seven-rule receive gate. The core three-conjunct
   `authorized_slash_candidate` above (evidence/target epoch = current, positive
   bond; unknown/valid evidence folded into evidence_lookup) is completed by the
   issuer==sender rule (1) and the block-level (offender,epoch) NoDup rule (7),
   faithfully mirroring validate_received_slash_deploys (slashing_authorization.rs
   :342-508). *)
Theorem main_T9_13_issuer_mismatch_rejected :
  forall block_sender current_epoch canonical_bonds canonical_generations
         sd evidence,
    sd_issuer sd <> block_sender ->
    received_slash_deploy_authorized block_sender current_epoch canonical_bonds
      canonical_generations sd evidence = false.
Proof. exact issuer_mismatch_not_authorized. Qed.

Theorem main_T9_13_duplicate_target_rejected :
  forall block_sender current_epoch canonical_bonds canonical_generations
         evidence sd1 sd2 rest k,
    received_slash_deploy_authorized block_sender current_epoch canonical_bonds
      canonical_generations sd1 evidence = true ->
    received_slash_deploy_authorized block_sender current_epoch canonical_bonds
      canonical_generations sd2 evidence = true ->
    slash_target_key evidence sd1 = Some k ->
    slash_target_key evidence sd2 = Some k ->
    validate_block_slash_deploys block_sender current_epoch canonical_bonds
      canonical_generations evidence (sd1 :: sd2 :: rest) = false.
Proof. exact duplicate_target_rejected. Qed.

Theorem main_T9_13_authorized_block_validates :
  forall block_sender current_epoch canonical_bonds canonical_generations
         evidence sd k,
    received_slash_deploy_authorized block_sender current_epoch canonical_bonds
      canonical_generations sd evidence = true ->
    slash_target_key evidence sd = Some k ->
    validate_block_slash_deploys block_sender current_epoch canonical_bonds
      canonical_generations evidence [sd] = true.
Proof. exact single_authorized_deploy_validates. Qed.

Theorem main_T9_13_slash_target_is_dependency :
  forall deploys sd,
    In sd deploys ->
    In (sd_target_hash sd) (slash_evidence_dependencies deploys).
Proof. exact every_slash_target_is_a_dependency. Qed.

Theorem main_T9_13_missing_local_evidence_waits :
  forall available deploys sd,
    In sd deploys ->
    ~ In (sd_target_hash sd) available ->
    receive_slash_dependency available deploys sd = SlashDependencyWaiting.
Proof. exact unavailable_declared_slash_waits_for_evidence. Qed.

Theorem main_T9_13_missing_local_evidence_not_unauthorized :
  forall available deploys sd,
    In sd deploys ->
    ~ In (sd_target_hash sd) available ->
    receive_slash_dependency available deploys sd <>
      SlashDependencyRejectedForLocalAbsence.
Proof. exact unavailable_declared_slash_not_rejected_as_unauthorized. Qed.

Theorem main_T9_13_tracker_witness_not_slash_evidence :
  forall available tracker_witnesses deploys sd,
    In sd deploys ->
    In (sd_target_hash sd) tracker_witnesses ->
    ~ In (sd_target_hash sd) available ->
    receive_slash_dependency_with_tracker
      available tracker_witnesses deploys sd = SlashDependencyWaiting.
Proof. exact tracker_witness_does_not_satisfy_slash_evidence_dependency. Qed.

Theorem main_T9_13_tracker_witness_not_processed_block :
  forall dag buffered tracker_witnesses hash,
    In hash tracker_witnesses ->
    ~ In hash dag ->
    ~ In hash buffered ->
    block_is_processed_with_tracker
      dag buffered tracker_witnesses hash = false.
Proof. exact tracker_witness_does_not_mark_block_processed. Qed.

Theorem main_TAuth_invalid_token_noop :
  forall ps sd lookup current_epoch generations,
    execute_authenticated_slash_deploy
      ps sd current_epoch generations lookup false = (ps, false).
Proof. exact execute_invalid_auth_token_noop. Qed.

Theorem main_TAuth_valid_token_equiv :
  forall ps sd lookup current_epoch generations,
    execute_authenticated_slash_deploy
      ps sd current_epoch generations lookup true =
    execute_slash_deploy ps sd current_epoch generations lookup.
Proof. exact execute_valid_auth_token_equiv. Qed.

Theorem main_TSlash_seed_input_hash_injective :
  forall proposer seqNum h1 h2,
    slash_seed_input proposer seqNum h1 =
    slash_seed_input proposer seqNum h2 ->
    h1 = h2.
Proof. exact slash_seed_input_hash_injective. Qed.

Theorem main_TSlash_deploy_seed_uses_invalid_block_hash :
  forall candidates bonds generations proposer seqNum currentEpoch seed_fn sd,
    In sd (prepare_slashing_deploys
      candidates bonds generations proposer seqNum currentEpoch seed_fn) ->
    sd_seed sd = seed_fn proposer seqNum (sd_target_hash sd).
Proof. exact deploy_seed_uses_invalid_block_hash. Qed.

Theorem main_T9_14_checked_pred_positive :
  forall n,
    n > 0 ->
    checked_pred n = Some (n - 1).
Proof. exact checked_pred_total_positive. Qed.

Theorem main_T9_15_duplicate_justifications_rejected :
  forall v h1 h2 rest,
    unique_justification_validators
      (mkJustification v h1 :: mkJustification v h2 :: rest) = false.
Proof. exact duplicate_head_rejected. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — Two-level closure and detector-bound summary (T-11, T-12, T-9.11, T-6)
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem main_T12_bft_quorum :
  forall (universe : list Validator) (closure : list Validator) (F : nat),
    NoDup universe ->
    NoDup closure ->
    incl closure universe ->
    length closure <= F ->
    length universe - length closure >= length universe - F.
Proof. exact t_12_bft_quorum_preservation. Qed.

Theorem main_T12_closure_depth_bound :
  forall universe g s0,
    NoDup universe ->
    NoDup s0 ->
    incl s0 universe ->
    slash_fixed_point universe g (slash_iter universe g s0 (length universe)).
Proof. exact closure_depth_bound_at_universe_size. Qed.

Theorem main_T12_evidence_monotone :
  forall universe g1 g2 s1 s2 n,
    graph_incl g1 g2 ->
    incl s1 s2 ->
    incl (slash_iter universe g1 s1 n)
         (slash_iter universe g2 s2 n).
Proof. exact slash_iter_initial_graph_monotone. Qed.

Theorem main_T12_no_seed_empty_closure :
  forall universe g n,
    slash_iter universe g [] n = [].
Proof. exact slash_iter_empty_initial_empty. Qed.

Theorem main_T12_reports_do_not_suppress_direct :
  forall universe g s0 n,
    incl s0 (slash_iter universe g s0 n).
Proof. exact slash_iter_monotone. Qed.

Theorem main_T12_unreported_visible_edge_remains_active :
  forall visible reported v offender,
    In offender (visible v) ->
    ~ In offender (reported v) ->
    In offender (visible_unreported_graph visible reported v).
Proof. exact unreported_visible_edge_remains_active. Qed.

Theorem main_T12_report_growth_antitone :
  forall universe visible reported_before reported_after s0 n,
    (forall v offender, In offender (reported_before v) -> In offender (reported_after v)) ->
    incl (view_closure universe visible reported_after s0 n)
         (view_closure universe visible reported_before s0 n).
Proof. exact view_closure_reports_antimonotone. Qed.

Theorem main_T12_view_merge_overapproximates_left :
  forall universe g1 g2 s0 n,
    incl (slash_iter universe g1 s0 n)
         (slash_iter universe (union_neglect_graph g1 g2) s0 n).
Proof. exact graph_union_closure_overapproximates_left. Qed.

Theorem main_T12_view_merge_overapproximates_right :
  forall universe g1 g2 s0 n,
    incl (slash_iter universe g2 s0 n)
         (slash_iter universe (union_neglect_graph g1 g2) s0 n).
Proof. exact graph_union_closure_overapproximates_right. Qed.

Theorem main_T12_view_merge_commutative :
  forall universe g1 g2 s0 n v,
    In v (slash_iter universe (union_neglect_graph g1 g2) s0 n) <->
    In v (slash_iter universe (union_neglect_graph g2 g1) s0 n).
Proof. exact graph_union_closure_commutative. Qed.

Theorem main_T12_validator_renaming_equiv :
  forall universe g h s0 t0 n rho sigma v,
    incl s0 universe ->
    incl t0 universe ->
    validator_renaming_maps_universe universe rho ->
    validator_renaming_maps_universe universe sigma ->
    validator_renaming_inverse_on universe rho sigma ->
    validator_renaming_inverse_on universe sigma rho ->
    validator_set_renaming_incl rho s0 t0 ->
    validator_set_renaming_incl sigma t0 s0 ->
    neglect_graph_renaming_incl universe rho g h ->
    neglect_graph_renaming_incl universe sigma h g ->
    In v universe ->
    In v (slash_iter universe g s0 n) <->
    In (rho v) (slash_iter universe h t0 n).
Proof. exact slash_iter_validator_renaming_equiv. Qed.

Theorem main_T9_11_detector_traversal_fuel_bound :
  forall fuel step current,
    length (detector_traversal_fuel fuel step current) <= fuel.
Proof. exact detector_traversal_fuel_length_bound. Qed.

Theorem main_T9_11_detector_branch_traversal_fixed_bound :
  forall domain g seen,
    NoDup domain ->
    NoDup seen ->
    incl seen domain ->
    branch_traversal_fixed domain g
      (branch_traversal_after domain g seen (length domain)).
Proof. exact branch_traversal_fixed_after_domain_bound. Qed.

Theorem main_T12_temporal_retention_boundary :
  forall gossip_delay inclusion_delay,
    temporal_retention_safe
      gossip_delay inclusion_delay (gossip_delay + inclusion_delay).
Proof. exact temporal_retention_boundary_exact. Qed.

Theorem main_T12_temporal_retention_under_window :
  forall gossip_delay inclusion_delay retention_window,
    retention_window < gossip_delay + inclusion_delay ->
    ~ temporal_retention_safe gossip_delay inclusion_delay retention_window.
Proof. exact temporal_retention_under_window_projection_risk. Qed.

Theorem main_T9_2_n_threads :
  forall ops s k,
    incl (hashes_at_key s k)
         (hashes_at_key (apply_schedule s ops) k).
Proof. exact t_9_2_atomic_n_threads_arbitrary. Qed.

Theorem main_T4_record_lifecycle_retains_hash :
  forall s k h_old h_new,
    In h_old (hashes_at_key s k) ->
    In h_old (hashes_at_key (update_record s k h_new) k).
Proof. exact record_lifecycle_update_retains_detected_hash. Qed.

Theorem main_T9_6_dag :
  forall (blocks : list Block) (sender : Validator) (cited : nat) (b : Block),
    In b blocks ->
    block_sender b = sender ->
    block_seq b > cited ->
    has_self_regression 0 (ds_latest_seq blocks sender) cited = true.
Proof. exact t_9_6_self_regression_in_dag. Qed.

Theorem main_T6_detect_neglected_sound :
  forall st v n d records,
    detect_neglected st v n d records = DSNeglected ->
    d = true /\ has_key records (v, pred n) = true.
Proof. exact detect_neglected_sound. Qed.

Theorem main_T6_detect_neglected_complete :
  forall st v n records,
    has_key records (v, pred n) = true ->
    detect_neglected st v n true records = DSNeglected.
Proof. exact detect_neglected_complete. Qed.

Theorem main_T9_21_protocol_ready_decidable :
  forall (Hash : Type)
         (hash_eq_dec : forall left right : Hash, {left = right} + {left <> right})
         metadata origins,
    protocol_readyb hash_eq_dec metadata origins = true <->
    protocol_all_admitted hash_eq_dec metadata origins.
Proof.
  intros.
  apply protocol_readyb_spec.
Qed.

Theorem main_T9_21_objective_pair_complete :
  forall (Hash : Type)
         (hash_eq_dec : forall left right : Hash, {left = right} + {left <> right})
         origins first second,
    In (ProtocolObjectivePair first second) (protocol_slash_evidence origins) ->
    In first (protocol_dependencies hash_eq_dec origins)
    /\ In second (protocol_dependencies hash_eq_dec origins).
Proof.
  intros.
  eapply protocol_objective_pair_is_complete.
  exact H.
Qed.

Theorem main_T9_21_header_pair_complete :
  forall (Hash : Type)
         (hash_eq_dec : forall left right : Hash, {left = right} + {left <> right})
         origins first second,
    In (first, second) (protocol_header_evidence origins) ->
    In first (protocol_dependencies hash_eq_dec origins)
    /\ In second (protocol_dependencies hash_eq_dec origins).
Proof.
  intros.
  eapply protocol_header_pair_is_complete.
  exact H.
Qed.

Theorem main_T9_21_invalid_index_noninterference :
  forall (Hash : Type)
         (hash_eq_dec : forall left right : Hash, {left = right} + {left <> right})
         metadata invalid_left invalid_right tracker origins,
    protocol_direct_ready hash_eq_dec metadata invalid_left tracker origins <->
    protocol_direct_ready hash_eq_dec metadata invalid_right tracker origins.
Proof.
  intros.
  apply protocol_invalid_index_noninterference.
Qed.

Theorem main_T9_21_tracker_noninterference :
  forall (Hash : Type)
         (hash_eq_dec : forall left right : Hash, {left = right} + {left <> right})
         metadata invalid_index tracker_left tracker_right origins,
    protocol_direct_ready hash_eq_dec metadata invalid_index tracker_left origins <->
    protocol_direct_ready hash_eq_dec metadata invalid_index tracker_right origins.
Proof.
  intros.
  apply protocol_tracker_noninterference.
Qed.

Theorem main_T9_21_direct_buffer_parity :
  forall (Hash : Type)
         (hash_eq_dec : forall left right : Hash, {left = right} + {left <> right})
         metadata invalid_index tracker origins,
    protocol_direct_ready hash_eq_dec metadata invalid_index tracker origins <->
    protocol_buffer_ready hash_eq_dec metadata invalid_index tracker origins.
Proof.
  intros.
  apply protocol_direct_buffer_readiness_equal.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — Headline composition
   ═══════════════════════════════════════════════════════════════════════════

   The headline statement of the development: for every detected admissible
   or ignorable equivocation, applying the slash effect and the atomic record
   update yields (i) a confirmed equivocation, (ii) the witnessing hash
   retained in the record store, (iii) the offender's bond zeroed, (iv) the
   offender excluded from fork choice, (v) the stake quarantined, (vi) minting
   halted, and (vii) the Coop vault unchanged before adjudication. *)

Theorem main_slashing_algorithm_correct :
  forall cj lmh d status v n psc lm records witness,
    detect cj lmh d = status ->
    status = DSAdmissible \/ status = DSIgnorable ->
    let result := slashC psc v in
    let psc' := fst result in
    let records' := atomic_record_or_update records (v, pred n) witness in
    equivocates_ptr cj lmh = true
    /\ In witness (hashes_at_key records' (v, pred n))
    /\ bm_lookup (ps_allBonds (psc_pos psc')) v = 0
    /\ fc_lookup (filter_slashed lm (ps_allBonds (psc_pos psc'))) v = None
    /\ (bm_lookup (ps_allBonds (psc_pos psc)) v > 0 ->
        qs_lookup (psc_quarantined psc') v =
          Some (bm_lookup (ps_allBonds (psc_pos psc)) v)
        /\ halted_mem (psc_mintingHalted psc') v = true
        /\ ps_coopVault (psc_pos psc') = ps_coopVault (psc_pos psc)).
Proof.
  intros cj lmh d status v n psc lm records witness Hd Hstatus.
  pose proof (@detection_sound cj lmh d status Hd Hstatus) as Heq.
  pose proof (slashC_zeros_bond psc v) as Hzero.
  pose proof (slash_quarantines_stake psc v) as Hquarantine.
  pose proof (slashC_halts psc v) as Hhalt.
  destruct (slashC psc v) as [psc' ok] eqn:Hslash.
  simpl in Hzero, Hquarantine, Hhalt |- *.
  split.
  - exact Heq.
  - split.
    + apply t_9_2_atomic_records_hash.
    + split.
      * exact Hzero.
      * split.
        -- apply fork_choice_exclusion. exact Hzero.
        -- intro Hbond.
           destruct (Hquarantine Hbond) as [Hstake Hcoop].
           split.
           ++ exact Hstake.
           ++ split.
              ** apply Hhalt. exact Hbond.
              ** exact Hcoop.
Qed.
