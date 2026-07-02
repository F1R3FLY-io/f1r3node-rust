(* ===========================================================================
   GuardBridge.v - the seams the Rust ENFORCES, DERIVED (not axiomatized) as the
   premises the Rocq development assumes. The "Rocq assumes what Rust enforces"
   pattern (as in finalized_floor/GuardBridge.v): each seam is a Section
   Variable/Hypothesis reflecting a Rust-checked invariant; on Section close it
   becomes a PREMISE of the closed term, never an axiom.

   Four seams:

   (1) validation_implies_wf_dag  - `validate.rs::block_number` (:703) enforces
       `num child = 1 + max(parent numbers)` along every edge, and looks up every
       parent (:684). From that validation predicate we DERIVE Foundation.wf_dag
       (parents present & strictly older; main parent = head). [NOTE: we model
       the FAITHFUL `1 + max parent num` (validate.rs:698-703), which is stronger
       than the task's shorthand `1 + main-parent num` and is exactly what makes
       EVERY parent - not just the main one - strictly older, so wf_dag follows.]

   (2) weight_block_structural   - `weight_from_validator_by_dag` reads only the
       RESOLVED main-parent weight map, so weight is pure: this IS
       Score.weight_is_pure, re-exported as the seam.

   (3) main_parent_first_deterministic - `snapshot.rs` (:136-142) orders parents
       by (is_main DESC, hash ASC); that is a TOTAL order, so the ordering is
       deterministic - via TieBreak.output_indep_of_input_perm.

   (4) honest_forkchoice_parents_validate - the REFRAMED T-VALID: a validator
       does NOT recompute fork choice; it RANGE-CHECKS the declared parents
       (snapshot.rs:124-126: "validators replay declared parents, not fork
       choice"). A proposer's depth-filtered parents (all within `mpd`) therefore
       ALWAYS pass the validator's buffered bound (`mpd + buf`), since mpd <=
       mpd + buf. No honest proposer is ever rejected, and no fork-choice
       recomputation is needed for validation.

   ---------------------------------------------------------------------------
   Rocq                             | Rust
   ---------------------------------+------------------------------------------
   validated_block / ...wf_dag      | validate.rs:681-703 block_number
   weight_block_structural          | proto_util.rs:160 (pure fn of main parent)
   main_parent_first_deterministic  | snapshot.rs:136-142 sort_by (is_main,hash)
   honest_forkchoice_parents_validate| snapshot.rs:124-126 (no recompute)
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import Lia.
Import ListNotations.

From ForkChoice Require Import Foundation.
From ForkChoice Require Import Score.
From ForkChoice Require Import TieBreak.
From ForkChoice Require Import Filter.
From ForkChoice Require Import Rank.
From ForkChoice Require Import Bound.

(* ===========================================================================
   Seam (1) - validation ENFORCES the well-formed DAG
   =========================================================================== *)

(* Max number over a parent list (validate.rs:698 fold(-1, max) then +1). *)
Definition maxpn (d : DAG) (parents : list BlockHash) : nat :=
  fold_right (fun ph acc => Nat.max (numof d ph) acc) 0 parents.

Lemma in_le_fold_max :
  forall d parents ph, In ph parents -> numof d ph <= maxpn d parents.
Proof.
  intros d parents ph Hin. unfold maxpn.
  induction parents as [| q qs IH]; simpl in *.
  - contradiction.
  - destruct Hin as [Heq | Hin].
    + subst q. apply Nat.le_max_l.
    + eapply Nat.le_trans; [apply IH; exact Hin | apply Nat.le_max_r].
Qed.

Lemma hd_error_in :
  forall (A : Type) (l : list A) (x : A), hd_error l = Some x -> In x l.
Proof.
  intros A l x H. destruct l as [| y ys]; simpl in H.
  - discriminate.
  - injection H as ->. left; reflexivity.
Qed.

(* The validation predicate mirroring validate.rs::block_number. *)
Definition validated_block (d : DAG) (b : Block) : Prop :=
  (forall ph, In ph (blk_parents b) -> exists p, lookup d ph = Some p)
  /\ match blk_main_parent b with
     | None    => blk_parents b = [] /\ blk_num b = 0
     | Some mph => hd_error (blk_parents b) = Some mph
                   /\ blk_num b = S (maxpn d (blk_parents b))
     end.

Section Bridge.
  Variable d : DAG.
  Hypothesis all_validated : forall b, In b d -> validated_block d b.

  (* DERIVED: every validated DAG is well-formed (parents present & strictly
     older; main parent = head). No axiom - `all_validated` is a premise. *)
  Theorem validation_implies_wf_dag : wf_dag d.
  Proof.
    intros b Hb. destruct (all_validated b Hb) as [Hpres Hmain].
    split.
    - (* every parent: present with strictly smaller number *)
      intros ph Hph. destruct (Hpres ph Hph) as [p Hlp]. exists p. split; [exact Hlp |].
      assert (Hnp : numof d ph = blk_num p) by (unfold numof; rewrite Hlp; reflexivity).
      assert (Hbound : numof d ph <= maxpn d (blk_parents b)) by (apply in_le_fold_max; exact Hph).
      destruct (blk_main_parent b) as [mph |] eqn:Emp.
      + destruct Hmain as [_ Hnum]. lia.
      + destruct Hmain as [Hnil _]. rewrite Hnil in Hph. destruct Hph.
    - (* main-parent clause *)
      destruct (blk_main_parent b) as [mph |] eqn:Emp.
      + destruct Hmain as [Hhd Hnum]. split; [| exact Hhd].
        assert (Hmphin : In mph (blk_parents b)) by (apply (hd_error_in _ (blk_parents b) mph); exact Hhd).
        destruct (Hpres mph Hmphin) as [p Hlp]. exists p. split; [exact Hlp |].
        assert (Hnp : numof d mph = blk_num p) by (unfold numof; rewrite Hlp; reflexivity).
        assert (Hbound : numof d mph <= maxpn d (blk_parents b)) by (apply in_le_fold_max; exact Hmphin).
        lia.
      + destruct Hmain as [_ Hnum]. exact Hnum.
  Qed.
End Bridge.

(* ===========================================================================
   Seam (2) - weight is a pure function of the resolved main-parent bonds
   =========================================================================== *)

Corollary weight_block_structural :
  forall d1 d2 h v,
    Score.resolve_main d1 h = Score.resolve_main d2 h ->
    Score.weight d1 h v = Score.weight d2 h v.
Proof. exact Score.weight_is_pure. Qed.

(* ===========================================================================
   Seam (3) - main-parent-first ordering is deterministic (a total order)
   =========================================================================== *)

(* A parent as a tie-break entry: score = 1 if it is the ghost main parent, else
   0; hash = the block hash. (is_main DESC, hash ASC) = TieBreak's (score,hash). *)
Definition parent_entry (main : BlockHash) (h : BlockHash) : entry :=
  ((if Nat.eqb h main then 1 else 0), h).

Lemma map_ehash_parent_entry :
  forall main l, map ehash (map (parent_entry main) l) = l.
Proof.
  intros main l. induction l as [| x xs IH]; simpl.
  - reflexivity.
  - f_equal. exact IH.
Qed.

Theorem main_parent_first_deterministic :
  forall main (l l' : list BlockHash),
    NoDup l -> Permutation l l' ->
    sort (map (parent_entry main) l) = sort (map (parent_entry main) l').
Proof.
  intros main l l' Hnd Hperm.
  apply output_indep_of_input_perm.
  - rewrite (map_ehash_parent_entry main l). exact Hnd.
  - apply Permutation_map. exact Hperm.
Qed.

(* ===========================================================================
   Seam (4) - honest fork-choice parents pass the validator's bound-check
   =========================================================================== *)

(* A parent at height `pn` is within depth `mpd` of the tip `maxn`. *)
Definition within_depth (maxn mpd pn : nat) : bool := Nat.leb (maxn - pn) mpd.
Definition parents_ok (maxn mpd : nat) (nums : list nat) : bool :=
  forallb (within_depth maxn mpd) nums.
Definition prop_filter (maxn mpd : nat) (nums : list nat) : list nat :=
  filter (within_depth maxn mpd) nums.

Lemma within_depth_mono :
  forall maxn mpd buf pn,
    within_depth maxn mpd pn = true -> within_depth maxn (mpd + buf) pn = true.
Proof.
  intros maxn mpd buf pn H. unfold within_depth in *.
  apply Nat.leb_le in H. apply Nat.leb_le. lia.
Qed.

(* The reframed T-VALID: the proposer's depth-filtered parents (all within `mpd`)
   ALWAYS pass the validator's buffered acceptance (`mpd + buf`). Validators
   range-check declared parents; they never recompute fork choice. *)
Theorem honest_forkchoice_parents_validate :
  forall maxn mpd buf nums,
    parents_ok maxn (mpd + buf) (prop_filter maxn mpd nums) = true.
Proof.
  intros maxn mpd buf nums. unfold parents_ok. rewrite forallb_forall.
  intros x Hx. unfold prop_filter in Hx. apply filter_In in Hx.
  destruct Hx as [_ Hp]. apply within_depth_mono. exact Hp.
Qed.

(* ===========================================================================
   Pipeline seams: the whole estimator pipeline (filter -> score -> rank -> cap)
   composes with the bridges above. These re-exports make the Filter/Rank/Bound
   dependencies substantive - each names the exact Rust-enforced fact.
   =========================================================================== *)

(* Filter: the fork-choice input excludes slashed validators (T-10). *)
Corollary honest_lms_exclude_slashed :
  forall lms inv v h, In v inv -> ~ In (v, h) (filter_inv lms inv).
Proof. exact invalid_excluded. Qed.

(* Rank: the fork-choice descent always terminates at a tip. *)
Corollary forkchoice_descent_reaches_tip :
  forall d score is_scored h,
    wf_dag d -> wf_lookup d ->
    best_child d score is_scored (rank d score is_scored (S (dag_max_num d)) h) = None.
Proof. exact rank_terminates. Qed.

(* Bound: a positive/unlimited cap of the honest depth-filtered parents STILL
   passes the validator's bound (firstn is a prefix; forallb survives it). *)
Lemma forallb_firstn :
  forall (A : Type) (p : A -> bool) (n : nat) (l : list A),
    forallb p l = true -> forallb p (firstn n l) = true.
Proof.
  intros A p n. induction n as [| k IH]; intros l H; simpl.
  - reflexivity.
  - destruct l as [| x xs]; simpl in *; [reflexivity |].
    apply andb_true_iff in H. destruct H as [Hx Hxs].
    apply andb_true_iff. split; [exact Hx | apply IH; exact Hxs].
Qed.

Corollary capped_parents_validate :
  forall maxn mpd buf n unlimited nums,
    parents_ok maxn (mpd + buf) (cap_tips n unlimited (prop_filter maxn mpd nums)) = true.
Proof.
  intros maxn mpd buf n unlimited nums. unfold cap_tips. destruct unlimited.
  - apply honest_forkchoice_parents_validate.
  - unfold parents_ok. apply forallb_firstn.
    exact (honest_forkchoice_parents_validate maxn mpd buf nums).
Qed.
