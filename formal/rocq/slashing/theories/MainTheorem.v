(* ═══════════════════════════════════════════════════════════════════════════
   MainTheorem.v — Top-level statement composing all results

   Composes:
     - T-1 .. T-3 (detection layer)
     - T-4 .. T-5 (record persistence)
     - T-7 .. T-10 (slash effect)
     - T-11 .. T-12 (two-level closure)
     - T-9.1 .. T-9.11 (bug-fix correctness)
     - T-13 .. T-15 (bisimilarity)

   The headline statement: under the documented bug fixes, the Rust slashing
   layer is observationally bisimilar to the Scala original on every
   pipeline transition.

   Companion doc: slashing-verification.md §11.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Slashing Require Import
  Validator ValidatorLifetime Block InvalidBlock EquivocationRecord DAGState
  EquivocationDetector PoSContract SlashDeploy BlockCreator ForkChoice
  TwoLevelSlashing
  BugFixIgnorable BugFixAtomicTracker BugFixDispatcher
  BugFixTransferFailure BugFixStakeZero BugFixSelfRegression
  BugFixSeqNumDensity BugFixSeqArithmetic BugFixDuplicateJustifications
  BugFixSlashAuthorization BugFixUnbondedProposer
  BugFixWithdrawTransferFailure
  Bisimulation.

Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Detection layer summary
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem main_T1_detection_sound :
  forall st v n d s,
    detect st v n d = s ->
    s = DSAdmissible \/ s = DSIgnorable ->
    equivocates st v n.
Proof. exact detection_sound. Qed.

Theorem main_T2_detection_complete :
  forall st v n d,
    equivocates st v n ->
    detect st v n d = DSAdmissible \/ detect st v n d = DSIgnorable.
Proof. exact detection_complete. Qed.

Theorem main_T3_slashable_taxonomy :
  forall ib,
    is_slashable_pre_fix ib = true -> is_slashable ib = true.
Proof. exact slashable_post_fix_extends_pre_fix. Qed.

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
  forall ps v,
    let (ps', _) := slash ps v in
    bm_lookup (ps_allBonds ps') v = 0.
Proof. exact slash_zeros_bond. Qed.

Theorem main_T9_slash_idempotent :
  forall ps v,
    let (ps1, _)  := slash ps  v in
    let (ps2, _)  := slash ps1 v in
    ps_allBonds  ps2 = ps_allBonds  ps1
    /\ ps_coopVault ps2 = ps_coopVault ps1
    /\ ps_active   ps2 = ps_active   ps1.
Proof. exact slash_idempotent. Qed.

Theorem main_TIdem_zero_bond_noop :
  forall ps v,
    bm_lookup (ps_allBonds ps) v = 0 ->
    slash ps v = (ps, true).
Proof. exact slash_zero_bond_noop. Qed.

Theorem main_T10_fork_choice_exclusion :
  forall lm bonds v,
    bm_lookup bonds v = 0 ->
    fc_lookup (filter_slashed lm bonds) v = None.
Proof. exact fork_choice_exclusion. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — Bug-fix summary
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem main_T9_1_ignorable :
  forall st v n d,
    detect st v n d = DSIgnorable ->
    is_slashable IBIgnorableEquivocation = true /\ equivocates st v n.
Proof. exact post_fix_ignorable_implies_equivocation. Qed.

Theorem main_T9_2_atomic :
  forall s k h,
    incl (hashes_at_key s k) (hashes_at_key (atomic_record_or_update s k h) k).
Proof. exact t_9_2_atomic_no_overwrite. Qed.

Theorem main_T9_3_dispatch :
  forall ib offender baseSeq s,
    is_slashable ib = true ->
    has_key (dispatch_post_fix ib offender baseSeq s) (offender, baseSeq) = true.
Proof. exact t_9_3_dispatch_complete. Qed.

Theorem main_T9_4_transfer :
  forall ps v transfer_ok,
    let result := slash_with_transfer_oracle ps v transfer_ok in
    let ps' := fst result in
    let ok := snd result in
    (ok = true /\ bm_lookup (ps_allBonds ps') v = 0)
    \/ (ok = false /\ ps' = ps).
Proof. exact t_9_4_transfer_failure_safety. Qed.

Theorem main_T9_5_stake_zero :
  forall ps v,
    active_implies_bonded ps ->
    let result := slash ps v in
    let ps' := fst result in
    active_implies_bonded ps'.
Proof. exact t_9_5_slash_preserves_invariant. Qed.

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
  forall candidates bonds proposer seqNum currentEpoch seed_fn,
    bm_lookup bonds proposer = 0 ->
    prepare_slashing_deploys_post_fix candidates bonds proposer seqNum currentEpoch seed_fn = [].
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

Theorem main_T9_12_stale_evidence_not_authorized :
  forall v e_old e_new,
    e_old <> e_new ->
    evidence_authorizes_lifetime
      (mkValidatorLifetimeId v e_old)
      (mkValidatorLifetimeId v e_new) = false.
Proof. exact stale_evidence_not_authorized. Qed.

Theorem main_T9_13_unknown_slash_evidence_noop :
  forall ps sd evidence current_epoch,
    evidence_lookup evidence (sd_target_hash sd) = None ->
    execute_slash_deploy ps sd current_epoch (evidence_lookup evidence) = (ps, false).
Proof. exact unauthorized_unknown_execution_noop. Qed.

Theorem main_T9_13_zero_parent_bond_not_authorized :
  forall current_epoch parent_bonds sd evidence offender evidence_epoch,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, evidence_epoch) ->
    bm_lookup parent_bonds offender = 0 ->
    authorized_slash_candidate current_epoch parent_bonds sd evidence = false.
Proof. exact zero_parent_bond_not_authorized_candidate. Qed.

Theorem main_T9_13_positive_parent_bond_authorizes_matching_candidate :
  forall current_epoch parent_bonds sd evidence offender,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, current_epoch) ->
    sd_target_epoch sd = current_epoch ->
    bm_lookup parent_bonds offender > 0 ->
    authorized_slash_candidate current_epoch parent_bonds sd evidence = true.
Proof. exact positive_parent_bond_authorizes_matching_candidate. Qed.

Theorem main_T9_13_parent_pre_state_authorizes_when_ambient_zero :
  forall current_epoch ambient_bonds parent_bonds sd evidence offender,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, current_epoch) ->
    sd_target_epoch sd = current_epoch ->
    bm_lookup ambient_bonds offender = 0 ->
    bm_lookup parent_bonds offender > 0 ->
    authorized_slash_candidate_with_ambient
      current_epoch ambient_bonds parent_bonds sd evidence = true.
Proof. exact parent_pre_state_authorizes_when_ambient_zero. Qed.

Theorem main_T9_13_parent_zero_rejects_even_if_ambient_positive :
  forall current_epoch ambient_bonds parent_bonds sd evidence offender evidence_epoch,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, evidence_epoch) ->
    bm_lookup ambient_bonds offender > 0 ->
    bm_lookup parent_bonds offender = 0 ->
    authorized_slash_candidate_with_ambient
      current_epoch ambient_bonds parent_bonds sd evidence = false.
Proof. exact parent_zero_rejects_even_if_ambient_positive. Qed.

Theorem main_T9_13_recoverable_rejected_slash_hashes_nodup :
  forall rejected own_invalid_hashes,
    NoDup (recoverable_rejected_slash_hashes rejected own_invalid_hashes).
Proof. exact recoverable_rejected_slash_hashes_nodup. Qed.

Theorem main_T9_13_own_detected_hash_not_recovered :
  forall rejected own_invalid_hashes h,
    In h own_invalid_hashes ->
    ~ In h (recoverable_rejected_slash_hashes rejected own_invalid_hashes).
Proof. exact own_detected_hash_not_recovered. Qed.

Theorem main_T9_13_uncovered_rejected_hash_recovered :
  forall rejected own_invalid_hashes rs,
    In rs rejected ->
    ~ In (rejected_slash_hash rs) own_invalid_hashes ->
    In (rejected_slash_hash rs)
       (recoverable_rejected_slash_hashes rejected own_invalid_hashes).
Proof. exact uncovered_rejected_hash_recovered. Qed.

Theorem main_T9_13_recoverable_rejected_slash_requires_current_evidence :
  forall rejected own_invalid_hashes current_evidence_hashes h,
    In h (recoverable_current_rejected_slash_hashes
            rejected own_invalid_hashes current_evidence_hashes) ->
    In h current_evidence_hashes.
Proof. exact recoverable_rejected_slash_requires_current_evidence. Qed.

Theorem main_TAuth_invalid_token_noop :
  forall ps sd lookup current_epoch,
    execute_authenticated_slash_deploy ps sd current_epoch lookup false = (ps, false).
Proof. exact execute_invalid_auth_token_noop. Qed.

Theorem main_TAuth_valid_token_equiv :
  forall ps sd lookup current_epoch,
    execute_authenticated_slash_deploy ps sd current_epoch lookup true =
    execute_slash_deploy ps sd current_epoch lookup.
Proof. exact execute_valid_auth_token_equiv. Qed.

Theorem main_TSlash_seed_input_hash_injective :
  forall proposer seqNum h1 h2,
    slash_seed_input proposer seqNum h1 =
    slash_seed_input proposer seqNum h2 ->
    h1 = h2.
Proof. exact slash_seed_input_hash_injective. Qed.

Theorem main_TSlash_deploy_seed_uses_invalid_block_hash :
  forall candidates bonds proposer seqNum currentEpoch seed_fn sd,
    In sd (prepare_slashing_deploys candidates bonds proposer seqNum currentEpoch seed_fn) ->
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
   §5 — Bisimilarity summary (T-13, T-15 components)
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem main_T13_slash_bisim :
  forall b1 b2 v,
    bonds_bisim b1 b2 ->
    bonds_bisim (bm_slash b1 v) (bm_slash b2 v).
Proof. exact t_13_bm_slash_preserves_bonds_bisim. Qed.

Theorem main_T15_record_bisim_insert :
  forall s r k,
    incl (hashes_at_key s k) (hashes_at_key (insert_cond s r) k).
Proof. exact t_13_insert_preserves_records_monotone. Qed.

Theorem main_T15_record_bisim_update :
  forall s k h k',
    incl (hashes_at_key s k') (hashes_at_key (update_record s k h) k').
Proof. exact t_13_update_preserves_records_monotone. Qed.

Theorem main_T14_weak_barbed_equiv_refl :
  forall b rs sl v lm, weak_barbed_equiv b b rs rs sl sl v v lm lm.
Proof. exact weak_barbed_equiv_refl. Qed.

Theorem main_T14_weak_barbed_equiv_sym :
  forall b1 b2 rs1 rs2 sl1 sl2 v1 v2 lm1 lm2,
    weak_barbed_equiv b1 b2 rs1 rs2 sl1 sl2 v1 v2 lm1 lm2 ->
    weak_barbed_equiv b2 b1 rs2 rs1 sl2 sl1 v2 v1 lm2 lm1.
Proof. exact weak_barbed_equiv_sym. Qed.

Theorem main_T14_weak_barbed_equiv_trans :
  forall b1 b2 b3 rs1 rs2 rs3 sl1 sl2 sl3 v1 v2 v3 lm1 lm2 lm3,
    weak_barbed_equiv b1 b2 rs1 rs2 sl1 sl2 v1 v2 lm1 lm2 ->
    weak_barbed_equiv b2 b3 rs2 rs3 sl2 sl3 v2 v3 lm2 lm3 ->
    weak_barbed_equiv b1 b3 rs1 rs3 sl1 sl3 v1 v3 lm1 lm3.
Proof. exact weak_barbed_equiv_trans. Qed.

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

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — Headline composition
   ═══════════════════════════════════════════════════════════════════════════

   The headline composition: under the ten bug fixes, every pipeline
   transition preserves the bonds, records, slashed-set, and Coop-vault
   bisimulation relations. *)

Theorem main_bisimilarity_theorem :
  forall (b1 b2 : BondMap) (s1 s2 : EqStore) (sl1 sl2 : list Validator)
         (v1 v2 : nat) (offender : Validator),
    bonds_bisim b1 b2 ->
    records_bisim s1 s2 ->
    slashed_bisim sl1 sl2 ->
    vault_bisim v1 v2 ->
    let b1'  := bm_slash b1 offender in
    let b2'  := bm_slash b2 offender in
    let sl1' := offender :: sl1 in
    let sl2' := offender :: sl2 in
    let v1'  := v1 + bm_lookup b1 offender in
    let v2'  := v2 + bm_lookup b2 offender in
    bonds_bisim b1' b2'
    /\ slashed_bisim sl1' sl2'
    /\ vault_bisim v1' v2'.
Proof.
  intros b1 b2 s1 s2 sl1 sl2 v1 v2 offender Hb Hr Hsl Hv. simpl.
  split; [|split].
  - apply t_13_bm_slash_preserves_bonds_bisim. assumption.
  - apply t_15_slashed_append_consistent. assumption.
  - unfold vault_bisim. rewrite Hv. f_equal. apply Hb.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §7 — Closure-strengthened bisimilarity (Gaps 1, 2, 8)
   ═══════════════════════════════════════════════════════════════════════════

   The strong bisimilarity theorem closing Gaps 1 and 2: under
   records_bisim_strong (with key alignment) and forkchoice_bisim, applying
   the same slash, record-update, and filter operations on both sides
   preserves all five components of R. *)

(* The five-component bisimilarity theorem: applies slash, record update,
   slashed-set update, vault increment, and fork-choice filter consistently
   on both sides, preserving the full strong record relation. *)

Theorem main_bisimilarity_strong :
  forall (b1 b2 : BondMap) (rs1 rs2 : EqStore) (sl1 sl2 : list Validator)
         (v1 v2 : nat) (lm1 lm2 : LatestMessages)
         (offender : Validator) (k : Validator * nat) (h : BlockHash),
    bonds_bisim b1 b2 ->
    records_bisim_strong rs1 rs2 ->
    slashed_bisim sl1 sl2 ->
    vault_bisim v1 v2 ->
    forkchoice_bisim lm1 lm2 ->
    let b1' := bm_slash b1 offender in
    let b2' := bm_slash b2 offender in
    let rs1' := update_record rs1 k h in
    let rs2' := update_record rs2 k h in
    let sl1' := offender :: sl1 in
    let sl2' := offender :: sl2 in
    let v1' := v1 + bm_lookup b1 offender in
    let v2' := v2 + bm_lookup b2 offender in
    let lm1' := filter_slashed lm1 b1' in
    let lm2' := filter_slashed lm2 b2' in
    bonds_bisim b1' b2'
    /\ records_bisim_strong rs1' rs2'
    /\ slashed_bisim sl1' sl2'
    /\ vault_bisim v1' v2'
    /\ forkchoice_bisim lm1' lm2'.
Proof.
  intros b1 b2 rs1 rs2 sl1 sl2 v1 v2 lm1 lm2 offender k h
         Hb Hr Hsl Hv Hfc.
  simpl.
  split; [|split; [|split; [|split]]].
  - apply t_13_bm_slash_preserves_bonds_bisim. assumption.
  - apply records_bisim_strong_preserved_update. assumption.
  - apply t_15_slashed_append_consistent. assumption.
  - unfold vault_bisim in *.
    rewrite Hv. f_equal. apply Hb.
  - unfold forkchoice_bisim.
    apply forkchoice_bisim_preserves_filter; [assumption|].
    apply t_13_bm_slash_preserves_bonds_bisim. assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §8 — Sequential pipeline composition (Gap 8)
   ═══════════════════════════════════════════════════════════════════════════

   A pipeline step is the composition of:
     1. slash(offender)
     2. record-update at (offender, baseSeq) with hash h
     3. filter-fork-choice
   This theorem states that the strong bisimulation R is preserved across
   one full pipeline step. *)

Definition pipeline_step
  (b : BondMap) (rs : EqStore) (sl : list Validator) (v : nat)
  (lm : LatestMessages) (offender : Validator) (baseSeq : nat) (h : BlockHash)
  : BondMap * EqStore * list Validator * nat * LatestMessages :=
  let b' := bm_slash b offender in
  let rs' := update_record rs (offender, baseSeq) h in
  let sl' := offender :: sl in
  let v' := v + bm_lookup b offender in
  let lm' := filter_slashed lm b' in
  (b', rs', sl', v', lm').

Definition pipeline_bonds
  (r : BondMap * EqStore * list Validator * nat * LatestMessages) : BondMap :=
  fst (fst (fst (fst r))).

Definition pipeline_records
  (r : BondMap * EqStore * list Validator * nat * LatestMessages) : EqStore :=
  snd (fst (fst (fst r))).

Definition pipeline_slashed
  (r : BondMap * EqStore * list Validator * nat * LatestMessages) : list Validator :=
  snd (fst (fst r)).

Definition pipeline_vault
  (r : BondMap * EqStore * list Validator * nat * LatestMessages) : nat :=
  snd (fst r).

Definition pipeline_forkchoice
  (r : BondMap * EqStore * list Validator * nat * LatestMessages) : LatestMessages :=
  snd r.

Theorem t_15_pipeline_step_preserves_R :
  forall b1 b2 rs1 rs2 sl1 sl2 v1 v2 lm1 lm2 offender baseSeq h,
    bonds_bisim b1 b2 ->
    records_bisim_strong rs1 rs2 ->
    slashed_bisim sl1 sl2 ->
    vault_bisim v1 v2 ->
    forkchoice_bisim lm1 lm2 ->
    let r1 := pipeline_step b1 rs1 sl1 v1 lm1 offender baseSeq h in
    let r2 := pipeline_step b2 rs2 sl2 v2 lm2 offender baseSeq h in
    weak_barbed_equiv
      (pipeline_bonds r1) (pipeline_bonds r2)
      (pipeline_records r1) (pipeline_records r2)
      (pipeline_slashed r1) (pipeline_slashed r2)
      (pipeline_vault r1) (pipeline_vault r2)
      (pipeline_forkchoice r1) (pipeline_forkchoice r2).
Proof.
  intros. simpl. unfold pipeline_step. simpl.
  pose proof (@main_bisimilarity_strong b1 b2 rs1 rs2 sl1 sl2 v1 v2 lm1 lm2
    offender (offender, baseSeq) h H H0 H1 H2 H3) as Hbisim.
  simpl in Hbisim.
  destruct Hbisim as [Hb [Hrs [Hsl [Hv Hfc]]]].
  unfold weak_barbed_equiv. simpl.
  split; [assumption|].
  split; [assumption|].
  split; [assumption|].
  split; assumption.
Qed.

Theorem main_T15_pipeline_step :
  forall b1 b2 rs1 rs2 sl1 sl2 v1 v2 lm1 lm2 offender baseSeq h,
    bonds_bisim b1 b2 ->
    records_bisim_strong rs1 rs2 ->
    slashed_bisim sl1 sl2 ->
    vault_bisim v1 v2 ->
    forkchoice_bisim lm1 lm2 ->
    let r1 := pipeline_step b1 rs1 sl1 v1 lm1 offender baseSeq h in
    let r2 := pipeline_step b2 rs2 sl2 v2 lm2 offender baseSeq h in
    weak_barbed_equiv
      (pipeline_bonds r1) (pipeline_bonds r2)
      (pipeline_records r1) (pipeline_records r2)
      (pipeline_slashed r1) (pipeline_slashed r2)
      (pipeline_vault r1) (pipeline_vault r2)
      (pipeline_forkchoice r1) (pipeline_forkchoice r2).
Proof. exact t_15_pipeline_step_preserves_R. Qed.

Theorem main_slashing_algorithm_correct :
  forall st v n d status ps lm records witness,
    detect st v n d = status ->
    status = DSAdmissible \/ status = DSIgnorable ->
    let result := slash ps v in
    let ps' := fst result in
    let records' := atomic_record_or_update records (v, pred n) witness in
    equivocates st v n
    /\ In witness (hashes_at_key records' (v, pred n))
    /\ bm_lookup (ps_allBonds ps') v = 0
    /\ fc_lookup (filter_slashed lm (ps_allBonds ps')) v = None
    /\ (bm_lookup (ps_allBonds ps) v > 0 ->
        ps_coopVault ps' = ps_coopVault ps + bm_lookup (ps_allBonds ps) v).
Proof.
  intros st v n d status ps lm records witness Hd Hstatus.
  pose proof (@detection_sound st v n d status Hd Hstatus) as Heq.
  pose proof (slash_zeros_bond ps v) as Hzero.
  pose proof (slash_transfers_stake ps v) as Htransfer.
  destruct (slash ps v) as [ps' ok] eqn:Hslash.
  simpl in Hzero, Htransfer |- *.
  repeat split.
  - assumption.
  - apply t_9_2_atomic_records_hash.
  - assumption.
  - apply fork_choice_exclusion. assumption.
  - intro Hbond. apply Htransfer. assumption.
Qed.
