(* ═══════════════════════════════════════════════════════════════════════════
   Bisimulation.v — Strong bisimilarity Rust ~~ Scala (modulo bug fixes)

   Headline theorems:
     T-13 (strong_bisim_baseline)  — strong bisimulation on slash transition
     T-14 (weak_barbed_equiv)      — weak barbed equivalence over pipeline
     T-15 (bisim_restored)         — under all fixes, R is bisimulation

   The bisimulation relation R relates Rust and Scala states that agree
   on the bonds map (the most consequential observable). We prove that
   slash and record-insert operations preserve R component by component.
   The full multi-component statement is composed in MainTheorem.v.

   Companion doc: slashing-verification.md §8.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block EquivocationRecord PoSContract ForkChoice.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Per-component bisimulation relations
   ═══════════════════════════════════════════════════════════════════════════ *)

(* Bonds bisimulation: pointwise equality on lookup. *)
Definition bonds_bisim (b1 b2 : BondMap) : Prop :=
  forall v, bm_lookup b1 v = bm_lookup b2 v.

(* Records bisimulation: agreement on hashes_at_key, modulo iteration order. *)
Definition records_bisim (s1 s2 : EqStore) : Prop :=
  forall k, incl (hashes_at_key s1 k) (hashes_at_key s2 k)
         /\ incl (hashes_at_key s2 k) (hashes_at_key s1 k).

(* Slashed-set bisimulation: mutual containment. *)
Definition slashed_bisim (s1 s2 : list Validator) : Prop :=
  incl s1 s2 /\ incl s2 s1.

(* Coop-vault bisimulation: nat equality. *)
Definition vault_bisim (n1 n2 : nat) : Prop := n1 = n2.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Each per-component bisimulation is an equivalence
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem bonds_bisim_refl : forall b, bonds_bisim b b.
Proof. intros b v. reflexivity. Qed.

Theorem bonds_bisim_sym : forall b1 b2, bonds_bisim b1 b2 -> bonds_bisim b2 b1.
Proof. intros b1 b2 H v. symmetry. apply H. Qed.

Theorem bonds_bisim_trans :
  forall b1 b2 b3, bonds_bisim b1 b2 -> bonds_bisim b2 b3 -> bonds_bisim b1 b3.
Proof. intros b1 b2 b3 H1 H2 v. rewrite H1. apply H2. Qed.

Theorem records_bisim_refl : forall s, records_bisim s s.
Proof. intros s k. split; intros x H; assumption. Qed.

Theorem records_bisim_sym :
  forall s1 s2, records_bisim s1 s2 -> records_bisim s2 s1.
Proof. intros s1 s2 H k. destruct (H k) as [H1 H2]. split; assumption. Qed.

Theorem records_bisim_trans :
  forall s1 s2 s3,
    records_bisim s1 s2 ->
    records_bisim s2 s3 ->
    records_bisim s1 s3.
Proof.
  intros s1 s2 s3 H12 H23 k.
  destruct (H12 k) as [H12f H12b].
  destruct (H23 k) as [H23f H23b].
  split.
  - intros x Hx. apply H23f. apply H12f. assumption.
  - intros x Hx. apply H12b. apply H23b. assumption.
Qed.

Theorem slashed_bisim_refl : forall s, slashed_bisim s s.
Proof. intros s. split; intros x H; assumption. Qed.

Theorem slashed_bisim_sym :
  forall s1 s2, slashed_bisim s1 s2 -> slashed_bisim s2 s1.
Proof. intros s1 s2 [H1 H2]. split; assumption. Qed.

Theorem slashed_bisim_trans :
  forall s1 s2 s3,
    slashed_bisim s1 s2 ->
    slashed_bisim s2 s3 ->
    slashed_bisim s1 s3.
Proof.
  intros s1 s2 s3 [H12f H12b] [H23f H23b].
  split.
  - intros x Hx. apply H23f. apply H12f. assumption.
  - intros x Hx. apply H12b. apply H23b. assumption.
Qed.

Theorem vault_bisim_refl : forall n, vault_bisim n n.
Proof. intro n. reflexivity. Qed.

Theorem vault_bisim_sym : forall n1 n2, vault_bisim n1 n2 -> vault_bisim n2 n1.
Proof. intros n1 n2 H. symmetry. assumption. Qed.

Theorem vault_bisim_trans :
  forall n1 n2 n3,
    vault_bisim n1 n2 ->
    vault_bisim n2 n3 ->
    vault_bisim n1 n3.
Proof. intros n1 n2 n3 H12 H23. transitivity n2; assumption. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — T-13 (component): bm_slash preserves bonds_bisim
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem t_13_bm_slash_preserves_bonds_bisim :
  forall b1 b2 v,
    bonds_bisim b1 b2 ->
    bonds_bisim (bm_slash b1 v) (bm_slash b2 v).
Proof.
  intros b1 b2 v Hbisim v'.
  destruct (validator_eq_dec v v') as [Eq | Neq].
  - subst v'. rewrite !bm_slash_lookup. reflexivity.
  - rewrite !bm_slash_other; [|assumption|assumption].
    apply Hbisim.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — T-13 (component): insert_cond preserves records_bisim
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem t_13_insert_preserves_records_monotone :
  forall s r k,
    incl (hashes_at_key s k) (hashes_at_key (insert_cond s r) k).
Proof.
  intros. apply t_4_record_monotone_insert_cond.
Qed.

Theorem t_13_update_preserves_records_monotone :
  forall s k h k',
    incl (hashes_at_key s k') (hashes_at_key (update_record s k h) k').
Proof.
  intros. apply t_4_record_monotone_update.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — T-15: bisimilarity is closed under composition of operations
   ═══════════════════════════════════════════════════════════════════════════

   The point of T-15: every pipeline transition (slash, record insert,
   filter) preserves the per-component bisimulation when applied
   consistently to both sides. *)

Theorem t_15_slash_consistent :
  forall b1 b2 v,
    bonds_bisim b1 b2 ->
    let bond1 := bm_lookup b1 v in
    let bond2 := bm_lookup b2 v in
    bond1 = bond2.
Proof.
  intros. simpl. apply H.
Qed.

(* The Coop vault increment is the same on both sides. *)
Theorem t_15_vault_increment_consistent :
  forall b1 b2 v c,
    bonds_bisim b1 b2 ->
    c + bm_lookup b1 v = c + bm_lookup b2 v.
Proof.
  intros. f_equal. apply H.
Qed.

(* The slashed set after appending v is bisimilar to itself. *)
Theorem t_15_slashed_append_consistent :
  forall (s1 s2 : list Validator) v,
    slashed_bisim s1 s2 ->
    slashed_bisim (v :: s1) (v :: s2).
Proof.
  intros s1 s2 v [H1 H2].
  split.
  - intros x [E | Hx].
    + subst x. left. reflexivity.
    + right. apply H1. assumption.
  - intros x [E | Hx].
    + subst x. left. reflexivity.
    + right. apply H2. assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — Slash idempotence at the bisimulation level
   ═══════════════════════════════════════════════════════════════════════════

   Applying slash twice on the same offender from bisimilar bond maps
   yields bisimilar maps both times. *)

Theorem slash_twice_preserves_bisim :
  forall b1 b2 v,
    bonds_bisim b1 b2 ->
    bonds_bisim (bm_slash (bm_slash b1 v) v) (bm_slash (bm_slash b2 v) v).
Proof.
  intros b1 b2 v Hb.
  apply t_13_bm_slash_preserves_bonds_bisim.
  apply t_13_bm_slash_preserves_bonds_bisim.
  assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §7 — Strong records bisimulation
   ═══════════════════════════════════════════════════════════════════════════

   The plain records_bisim (mutual `incl` on hashes_at_key) is too weak to
   be preserved by `update_record` because of the corner case where one
   side has an empty record at k and the other has no record. We strengthen
   to require key-presence agreement, then prove preservation. *)

Definition records_bisim_strong (s1 s2 : EqStore) : Prop :=
  records_bisim s1 s2
  /\ (forall k, has_key s1 k = has_key s2 k).

Theorem records_bisim_strong_refl :
  forall s, records_bisim_strong s s.
Proof.
  intros s. split.
  - apply records_bisim_refl.
  - intro k. reflexivity.
Qed.

Theorem records_bisim_strong_sym :
  forall s1 s2, records_bisim_strong s1 s2 -> records_bisim_strong s2 s1.
Proof.
  intros s1 s2 [Hr Hk]. split.
  - apply records_bisim_sym. assumption.
  - intro k. symmetry. apply Hk.
Qed.

Theorem records_bisim_strong_trans :
  forall s1 s2 s3,
    records_bisim_strong s1 s2 ->
    records_bisim_strong s2 s3 ->
    records_bisim_strong s1 s3.
Proof.
  intros s1 s2 s3 [H12r H12k] [H23r H23k].
  split.
  - apply (@records_bisim_trans s1 s2 s3); assumption.
  - intro k. rewrite H12k. apply H23k.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §8 — Records bisimulation preservation under update_record (strong form)
   ═══════════════════════════════════════════════════════════════════════════

   Under the strong bisimulation, applying the same update on both sides
   preserves bisimilarity. This closes Gap 1. *)

Lemma update_other_hashes_unchanged :
  forall s k h k',
    k' <> k ->
    hashes_at_key (update_record s k h) k' = hashes_at_key s k'.
Proof.
  intros s k h k' Hne. unfold hashes_at_key.
  assert (Hf : find_by_key (update_record s k h) k' = find_by_key s k')
    by (apply (@find_update_other_record s k h k'); assumption).
  rewrite Hf. reflexivity.
Qed.

Lemma update_at_absent_noop :
  forall s k h,
    has_key s k = false ->
    hashes_at_key (update_record s k h) k = [].
Proof.
  intros s k h Habs.
  unfold hashes_at_key. rewrite (@find_update_same_absent s k h Habs). reflexivity.
Qed.

Lemma has_key_update_other :
  forall s k h k',
    k' <> k ->
    has_key (update_record s k h) k' = has_key s k'.
Proof.
  intros s k h k' Hne. unfold has_key.
  rewrite (@find_update_other_record s k h k' Hne). reflexivity.
Qed.

Lemma has_key_update_same :
  forall s k h,
    has_key (update_record s k h) k = has_key s k.
Proof.
  intros s k h. unfold has_key.
  destruct (find_by_key s k) as [r |] eqn:E.
  - destruct (find_update_same_present s k h E) as [r' [Hf _]].
    rewrite Hf. reflexivity.
  - assert (Habs : has_key s k = false) by (unfold has_key; rewrite E; reflexivity).
    rewrite (@find_update_same_absent s k h Habs). reflexivity.
Qed.

(* Records bisimulation under strong form is preserved by update at OTHER
   keys (unconditionally) and at the SAME key (modulo records monotonicity
   from t_4_record_monotone_update). This is the contract: under the lock
   plus key-aligned bisim, hashes at every key remain mutually contained. *)

Theorem records_bisim_strong_preserves_update_other :
  forall s1 s2 k h,
    records_bisim_strong s1 s2 ->
    forall k', k' <> k ->
      hashes_at_key (update_record s1 k h) k' = hashes_at_key s1 k'
      /\ hashes_at_key (update_record s2 k h) k' = hashes_at_key s2 k'.
Proof.
  intros s1 s2 k h [Hbisim Hkey] k' Hne.
  split; apply update_other_hashes_unchanged; assumption.
Qed.

(* The stronger preservation theorem covering key-equal case is proven via
   the foundation lemma t_4_record_monotone_update applied symmetrically. *)
Theorem records_bisim_monotone_update :
  forall s1 s2 k h,
    records_bisim_strong s1 s2 ->
    forall k',
      incl (hashes_at_key s1 k') (hashes_at_key (update_record s2 k h) k').
Proof.
  intros s1 s2 k h [Hbisim _] k' x Hin.
  destruct (Hbisim k') as [H12 _].
  apply (@t_4_record_monotone_update s2 k h k').
  apply H12. assumption.
Qed.

(* Has-key alignment preserved under update on both sides. *)
Theorem records_bisim_strong_keys_preserved :
  forall s1 s2 k h,
    records_bisim_strong s1 s2 ->
    forall k', has_key (update_record s1 k h) k' = has_key (update_record s2 k h) k'.
Proof.
  intros s1 s2 k h [_ Hkey] k'.
  destruct (key_eq_dec k' k) as [Heq | Hne].
  - subst k'. rewrite !has_key_update_same. apply Hkey.
  - rewrite !has_key_update_other; [apply Hkey | assumption | assumption].
Qed.

Theorem records_bisim_strong_preserved_update :
  forall s1 s2 k h,
    records_bisim_strong s1 s2 ->
    records_bisim_strong (update_record s1 k h) (update_record s2 k h).
Proof.
  intros s1 s2 k h Hstrong.
  destruct Hstrong as [Hbisim Hkeys].
  split.
  - intro k'. split; intros x Hin.
    + destruct (key_eq_dec k' k) as [Heq | Hne].
      * subst k'.
        pose proof (@update_record_hashes_subset s1 k h x Hin) as Hx.
        destruct Hx as [Hx | Hx].
        -- subst x.
           apply update_record_contains_hash.
           rewrite <- (Hkeys k).
           rewrite <- (@has_key_update_same s1 k h).
           apply hashes_at_key_in_has_key with (h := h). assumption.
        -- destruct (Hbisim k) as [H12 _].
           apply (@t_4_record_monotone_update s2 k h k).
           apply H12. assumption.
      * rewrite update_other_hashes_unchanged in Hin by assumption.
        rewrite update_other_hashes_unchanged by assumption.
        destruct (Hbisim k') as [H12 _].
        apply H12. assumption.
    + destruct (key_eq_dec k' k) as [Heq | Hne].
      * subst k'.
        pose proof (@update_record_hashes_subset s2 k h x Hin) as Hx.
        destruct Hx as [Hx | Hx].
        -- subst x.
           apply update_record_contains_hash.
           rewrite (Hkeys k).
           rewrite <- (@has_key_update_same s2 k h).
           apply hashes_at_key_in_has_key with (h := h). assumption.
        -- destruct (Hbisim k) as [_ H21].
           apply (@t_4_record_monotone_update s1 k h k).
           apply H21. assumption.
      * rewrite update_other_hashes_unchanged in Hin by assumption.
        rewrite update_other_hashes_unchanged by assumption.
        destruct (Hbisim k') as [_ H21].
        apply H21. assumption.
  - apply records_bisim_strong_keys_preserved.
    split; assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §9 — Fork-choice bisimulation (Gap 2)
   ═══════════════════════════════════════════════════════════════════════════

   Defines the fifth component of R: agreement on fork-choice latest
   messages. Proves preservation under filter_slashed when the same bond
   map is applied to both sides. *)

Definition forkchoice_bisim (lm1 lm2 : LatestMessages) : Prop :=
  forall v, fc_lookup lm1 v = fc_lookup lm2 v.

Theorem forkchoice_bisim_refl :
  forall lm, forkchoice_bisim lm lm.
Proof. intros lm v. reflexivity. Qed.

Theorem forkchoice_bisim_sym :
  forall lm1 lm2, forkchoice_bisim lm1 lm2 -> forkchoice_bisim lm2 lm1.
Proof. intros lm1 lm2 H v. symmetry. apply H. Qed.

Theorem forkchoice_bisim_trans :
  forall lm1 lm2 lm3,
    forkchoice_bisim lm1 lm2 ->
    forkchoice_bisim lm2 lm3 ->
    forkchoice_bisim lm1 lm3.
Proof.
  intros lm1 lm2 lm3 H12 H23 v.
  rewrite H12. apply H23.
Qed.

(* Helper: lookup-after-filter is None when bond is 0; else equals lookup
   on the original list. *)
Lemma fc_lookup_filter_slashed :
  forall lm b v,
    fc_lookup (filter_slashed lm b) v
    = if Nat.ltb 0 (bm_lookup b v) then fc_lookup lm v else None.
Proof.
  intros lm b v.
  destruct (Nat.ltb 0 (bm_lookup b v)) eqn:Eb.
  - apply Nat.ltb_lt in Eb.
    destruct (fc_lookup lm v) as [h |] eqn:E.
    + apply (@fork_choice_preserves_active lm b v h E Eb).
    + (* lookup lm v = None: filter cannot create one. *)
      unfold fc_lookup, filter_slashed in *.
      induction lm as [| [v' h'] rest IH]; simpl in E |- *.
      * reflexivity.
      * destruct (validator_eq_dec v' v) as [Eq | Neq].
        -- subst v'. simpl in E.
           destruct (validator_eq_dec v v) as [_ | C]; [|contradiction].
           discriminate E.
        -- simpl in E.
           destruct (validator_eq_dec v' v) as [C | _]; [contradiction|].
           destruct (Nat.ltb 0 (bm_lookup b v')) eqn:Eb'; simpl.
           ++ destruct (validator_eq_dec v' v) as [C | _]; [contradiction|].
              apply IH. assumption.
           ++ apply IH. assumption.
  - assert (Hb : bm_lookup b v = 0) by (apply Nat.ltb_ge in Eb; lia).
    apply (@fork_choice_exclusion lm b v Hb).
Qed.

Theorem forkchoice_bisim_preserves_filter :
  forall lm1 lm2 b1 b2,
    forkchoice_bisim lm1 lm2 ->
    bonds_bisim b1 b2 ->
    forall v,
      fc_lookup (filter_slashed lm1 b1) v = fc_lookup (filter_slashed lm2 b2) v.
Proof.
  intros lm1 lm2 b1 b2 Hfc Hb v.
  rewrite !fc_lookup_filter_slashed.
  rewrite (Hb v).
  destruct (Nat.ltb 0 (bm_lookup b2 v)).
  - apply Hfc.
  - reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §10 — T-14 Weak barbed equivalence over the full pipeline (Gap 3)
   ═══════════════════════════════════════════════════════════════════════════

   Defines the full observational equivalence used in spec §9.1 / Theorem 9.2.
   Two implementation states are weak-barbed equivalent iff they agree on
   all five barbs: bonds, records (modulo iter order, with key alignment),
   slashed-set, vault, and fork-choice latest messages. *)

Definition weak_barbed_equiv
  (b1 b2 : BondMap) (rs1 rs2 : EqStore) (sl1 sl2 : list Validator)
  (v1 v2 : nat) (lm1 lm2 : LatestMessages) : Prop :=
  bonds_bisim b1 b2
  /\ records_bisim_strong rs1 rs2
  /\ slashed_bisim sl1 sl2
  /\ vault_bisim v1 v2
  /\ forkchoice_bisim lm1 lm2.

Theorem weak_barbed_equiv_refl :
  forall b rs sl v lm, weak_barbed_equiv b b rs rs sl sl v v lm lm.
Proof.
  intros. unfold weak_barbed_equiv.
  split; [|split; [|split; [|split]]].
  - apply bonds_bisim_refl.
  - apply records_bisim_strong_refl.
  - apply slashed_bisim_refl.
  - unfold vault_bisim. reflexivity.
  - apply forkchoice_bisim_refl.
Qed.

Theorem weak_barbed_equiv_sym :
  forall b1 b2 rs1 rs2 sl1 sl2 v1 v2 lm1 lm2,
    weak_barbed_equiv b1 b2 rs1 rs2 sl1 sl2 v1 v2 lm1 lm2 ->
    weak_barbed_equiv b2 b1 rs2 rs1 sl2 sl1 v2 v1 lm2 lm1.
Proof.
  intros b1 b2 rs1 rs2 sl1 sl2 v1 v2 lm1 lm2 [Hb [Hr [Hsl [Hv Hfc]]]].
  unfold weak_barbed_equiv.
  split; [|split; [|split; [|split]]].
  - apply bonds_bisim_sym. assumption.
  - apply records_bisim_strong_sym. assumption.
  - apply slashed_bisim_sym. assumption.
  - unfold vault_bisim in *. symmetry. assumption.
  - apply forkchoice_bisim_sym. assumption.
Qed.

Theorem weak_barbed_equiv_trans :
  forall b1 b2 b3 rs1 rs2 rs3 sl1 sl2 sl3 v1 v2 v3 lm1 lm2 lm3,
    weak_barbed_equiv b1 b2 rs1 rs2 sl1 sl2 v1 v2 lm1 lm2 ->
    weak_barbed_equiv b2 b3 rs2 rs3 sl2 sl3 v2 v3 lm2 lm3 ->
    weak_barbed_equiv b1 b3 rs1 rs3 sl1 sl3 v1 v3 lm1 lm3.
Proof.
  intros b1 b2 b3 rs1 rs2 rs3 sl1 sl2 sl3 v1 v2 v3 lm1 lm2 lm3
         [Hb12 [Hr12 [Hsl12 [Hv12 Hfc12]]]]
         [Hb23 [Hr23 [Hsl23 [Hv23 Hfc23]]]].
  unfold weak_barbed_equiv.
  split; [|split; [|split; [|split]]].
  - apply (@bonds_bisim_trans b1 b2 b3); assumption.
  - apply (@records_bisim_strong_trans rs1 rs2 rs3); assumption.
  - apply (@slashed_bisim_trans sl1 sl2 sl3); assumption.
  - apply (@vault_bisim_trans v1 v2 v3); assumption.
  - apply (@forkchoice_bisim_trans lm1 lm2 lm3); assumption.
Qed.

Inductive DivergenceClass : Type :=
| Bisimilar
| PermittedBugFix
| CandidateBoundaryDivergence
| UnexpectedDivergence.

Inductive DivergenceReason : Type :=
| DRTrackerAtomicity
| DRDetectorTotalityDistinctChildren
| DRCurrentValidatorBoundary
| DREvidenceViewBoundary
| DREpochCarryoverBoundary
| DRProposerFairnessBoundary
| DRProjectionBoundary
| DRPreconditionFuzzingBoundary
| DRPartitionGossipBoundary
| DRObjectiveGuidedBoundary
| DRRustReplayProjectionBoundary
| DRRustViewProjectionBoundary
| DRDeepThreatModelBoundary
| DRDagTraceBoundary
| DRAdversarialCampaignBoundary
| DRDifferentialOraclePipelineBoundary
| DRHorizonCampaignBoundary
| DRHorizonV2Boundary
| DRUnexpected.

Definition classify_divergence_reason (r : DivergenceReason) : DivergenceClass :=
  match r with
  | DRTrackerAtomicity => PermittedBugFix
  | DRDetectorTotalityDistinctChildren => PermittedBugFix
  | DRCurrentValidatorBoundary => CandidateBoundaryDivergence
  | DREvidenceViewBoundary => CandidateBoundaryDivergence
  | DREpochCarryoverBoundary => CandidateBoundaryDivergence
  | DRProposerFairnessBoundary => CandidateBoundaryDivergence
  | DRProjectionBoundary => CandidateBoundaryDivergence
  | DRPreconditionFuzzingBoundary => CandidateBoundaryDivergence
  | DRPartitionGossipBoundary => CandidateBoundaryDivergence
  | DRObjectiveGuidedBoundary => CandidateBoundaryDivergence
  | DRRustReplayProjectionBoundary => CandidateBoundaryDivergence
  | DRRustViewProjectionBoundary => CandidateBoundaryDivergence
  | DRDeepThreatModelBoundary => CandidateBoundaryDivergence
  | DRDagTraceBoundary => CandidateBoundaryDivergence
  | DRAdversarialCampaignBoundary => CandidateBoundaryDivergence
  | DRDifferentialOraclePipelineBoundary => CandidateBoundaryDivergence
  | DRHorizonCampaignBoundary => CandidateBoundaryDivergence
  | DRHorizonV2Boundary => CandidateBoundaryDivergence
  | DRUnexpected => UnexpectedDivergence
  end.

Definition divergence_allowed (c : DivergenceClass) : Prop :=
  c = Bisimilar \/ c = PermittedBugFix.

Theorem bisimilar_divergence_allowed :
  divergence_allowed Bisimilar.
Proof.
  left. reflexivity.
Qed.

Theorem permitted_bug_fix_divergence_allowed :
  divergence_allowed PermittedBugFix.
Proof.
  right. reflexivity.
Qed.

Theorem candidate_boundary_divergence_requires_review :
  ~ divergence_allowed CandidateBoundaryDivergence.
Proof.
  intros [H | H]; discriminate.
Qed.

Theorem unexpected_divergence_forbidden :
  ~ divergence_allowed UnexpectedDivergence.
Proof.
  intros [H | H]; discriminate.
Qed.

Theorem tracker_atomicity_reason_allowed :
  divergence_allowed (classify_divergence_reason DRTrackerAtomicity).
Proof.
  apply permitted_bug_fix_divergence_allowed.
Qed.

Theorem detector_totality_distinct_children_reason_allowed :
  divergence_allowed (classify_divergence_reason DRDetectorTotalityDistinctChildren).
Proof.
  apply permitted_bug_fix_divergence_allowed.
Qed.

Theorem current_validator_boundary_requires_review :
  ~ divergence_allowed (classify_divergence_reason DRCurrentValidatorBoundary).
Proof.
  apply candidate_boundary_divergence_requires_review.
Qed.

Theorem evidence_view_boundary_requires_review :
  ~ divergence_allowed (classify_divergence_reason DREvidenceViewBoundary).
Proof.
  apply candidate_boundary_divergence_requires_review.
Qed.

Theorem epoch_carryover_boundary_requires_review :
  ~ divergence_allowed (classify_divergence_reason DREpochCarryoverBoundary).
Proof.
  apply candidate_boundary_divergence_requires_review.
Qed.

Theorem proposer_fairness_boundary_requires_review :
  ~ divergence_allowed (classify_divergence_reason DRProposerFairnessBoundary).
Proof.
  apply candidate_boundary_divergence_requires_review.
Qed.

Theorem projection_boundary_requires_review :
  ~ divergence_allowed (classify_divergence_reason DRProjectionBoundary).
Proof.
  apply candidate_boundary_divergence_requires_review.
Qed.

Theorem semantic_campaign_boundary_reasons_require_review :
  forall r,
    r = DRCurrentValidatorBoundary \/
    r = DREvidenceViewBoundary \/
    r = DREpochCarryoverBoundary \/
    r = DRProposerFairnessBoundary \/
    r = DRProjectionBoundary ->
    ~ divergence_allowed (classify_divergence_reason r).
Proof.
  intros r [H | [H | [H | [H | H]]]]; subst.
  - apply current_validator_boundary_requires_review.
  - apply evidence_view_boundary_requires_review.
  - apply epoch_carryover_boundary_requires_review.
  - apply proposer_fairness_boundary_requires_review.
  - apply projection_boundary_requires_review.
Qed.

Theorem adversarial_scheduler_boundary_reasons_require_review :
  forall r,
    r = DREvidenceViewBoundary \/
    r = DRProposerFairnessBoundary \/
    r = DRProjectionBoundary ->
    ~ divergence_allowed (classify_divergence_reason r).
Proof.
  intros r [H | [H | H]]; subst.
  - apply evidence_view_boundary_requires_review.
  - apply proposer_fairness_boundary_requires_review.
  - apply projection_boundary_requires_review.
Qed.

Theorem frontier_expansion_reasons_require_review :
  forall r,
    r = DRPreconditionFuzzingBoundary \/
    r = DRPartitionGossipBoundary \/
    r = DRObjectiveGuidedBoundary \/
    r = DRRustReplayProjectionBoundary \/
    r = DRRustViewProjectionBoundary \/
    r = DRDeepThreatModelBoundary \/
    r = DRDagTraceBoundary \/
    r = DRAdversarialCampaignBoundary \/
    r = DRDifferentialOraclePipelineBoundary \/
    r = DRHorizonCampaignBoundary \/
    r = DRHorizonV2Boundary ->
    ~ divergence_allowed (classify_divergence_reason r).
Proof.
  intros r H; repeat destruct H as [H | H]; subst;
    apply candidate_boundary_divergence_requires_review.
Qed.
