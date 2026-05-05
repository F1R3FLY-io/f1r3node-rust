(* ═══════════════════════════════════════════════════════════════════════════
   MainTheorem.v — Top-level statement composing all results

   Composes:
     - T-1 .. T-3 (detection layer)
     - T-4 .. T-5 (record persistence)
     - T-7 .. T-10 (slash effect)
     - T-11 .. T-12 (two-level closure)
     - T-9.1 .. T-9.10 (bug-fix correctness)
     - T-13 .. T-15 (bisimilarity)

   The headline statement: under the nine bug fixes, the Rust slashing
   layer is observationally bisimilar to the Scala original on every
   pipeline transition.

   Companion doc: slashing-verification.md §11.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Slashing Require Import
  Validator Block InvalidBlock EquivocationRecord DAGState
  EquivocationDetector PoSContract SlashDeploy BlockCreator ForkChoice
  TwoLevelSlashing
  BugFixIgnorable BugFixAtomicTracker BugFixDispatcher
  BugFixTransferFailure BugFixStakeZero BugFixSelfRegression
  BugFixSeqNumDensity BugFixUnbondedProposer
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
    block_sender b = sender ->
    block_seq b > baseSeq ->
    exists b', find_descendant_post_fix blocks sender baseSeq = Some b'.
Proof. exact t_9_7_finds_descendant_with_gap. Qed.

Theorem main_T9_8_unbonded :
  forall ilm bonds proposer seqNum seed_fn,
    bm_lookup bonds proposer = 0 ->
    prepare_slashing_deploys_post_fix ilm bonds proposer seqNum seed_fn = [].
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

Theorem main_T9_2_n_threads :
  forall ops s k,
    incl (hashes_at_key s k)
         (hashes_at_key (apply_schedule s ops) k).
Proof. exact t_9_2_atomic_n_threads_arbitrary. Qed.

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

   The headline composition: under the nine bug fixes, every pipeline
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
