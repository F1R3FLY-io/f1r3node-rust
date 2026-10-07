(* ===========================================================================
   ConflictSoundness.v - THE GAP-3 proof: removing the single-value-cell
   (consume-then-produce-by-channel) conflict predicate is SOUND -- everything it
   would have flagged is already caught by the RETAINED double-consume /
   same-IO-event race detector, EXCEPT on number/foldable channels, which are
   intrinsically mergeable and therefore safe to leave unflagged.

   `conflicts` (rspace++/src/rspace/merger/merging_logic.rs:262-479) retains:
     Check #1  races_for_same_io_event -- the SAME non-persistent Produce/Consume
               destroyed in COMM in BOTH branches (produces_consumed /
               consumes_produced intersection), minus both-mergeable;
     Check #2  potential_comms;
     Check #3  produce_touch_base_join.
   The REMOVED predicate was a single-value-cell check: two branches that both
   consume-then-produce on a shared channel (a write-write on one storage cell),
   which EXEMPTED number channels (they fold, so they never truly conflict).

   ---------------------------------------------------------------------------
   Why the removal is sound (the subsumption argument)
   ---------------------------------------------------------------------------
   A single-value cell holds exactly one datum; to UPDATE it a branch must CONSUME
   the cell's current (base) datum and PRODUCE a new one. So "branch does a
   consume-then-produce on channel ch" DEFINITIONALLY includes "branch consumed
   the base datum on ch" (`consumes ch` -- the base Produce destroyed in COMM,
   which lands in produces_consumed). If BOTH branches update the same non-number
   cell, they both consumed the SAME base datum -> a produces_consumed race, which
   is EXACTLY Check #1 (races_for_same_io_event). If the cell is a number channel,
   it is in produces_mergeable, so Check #1 (minus both-mergeable) correctly does
   NOT flag it -- and the removed predicate also exempted it. Either way, nothing
   is lost.

   ---------------------------------------------------------------------------
   Model (abstract branch event-sets; NO hidden hypothesis)
   ---------------------------------------------------------------------------
   A Branch carries two boolean channel-indexed fields:
     consumes ch : the base datum on ch was DESTROYED in COMM inside the branch
                   (models produces_consumed; non-persistent, since a persistent
                   produce is never destroyed -- the retained detector's
                   `!persistent` filter is thereby respected);
     produces ch : the branch produced a NEW datum on ch.
   svc_update ch := consumes ch && produces ch  (the consume-then-produce cell
   update). The single-value-cell semantics -- "updating consumes the base" -- is
   captured DEFINITIONALLY by this conjunction, so the subsumption needs NO
   hypothesis (unlike a stated `svc_update -> consumes` assumption): it is
   `andb_prop`. `is_number_ch` is an abstract PARAMETER (any number-channel
   classifier), not an axiom; the side-condition it induces is EXPLICIT in every
   statement.

   Headline `conflict_removal_sound`; core lemma `removed_subset_retained`.

   Stdlib-only, axiom-free. `Print Assumptions conflict_removal_sound`
   => "Closed under the global context".
   =========================================================================== *)

From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
Import ListNotations.

Definition Channel := nat.

Section Conflict.
  (* Abstract number/foldable-channel classifier. A PARAMETER (not an axiom): the
     results hold for ANY classifier. The persistent/number-channel side-condition
     is thereby EXPLICIT in each statement below. *)
  Variable is_number_ch : Channel -> bool.

  Record Branch := {
    consumes : Channel -> bool;   (* base datum on ch destroyed in COMM (non-persistent) *)
    produces : Channel -> bool    (* a new datum produced on ch *)
  }.

  (* single-value-cell update = consume-then-produce on a channel (the per-channel
     condition of the REMOVED predicate). Consuming the base is a DEFINITIONAL
     conjunct, so no separate semantic hypothesis is needed. *)
  Definition svc_update (br : Branch) (ch : Channel) : bool :=
    consumes br ch && produces br ch.

  (* The REMOVED predicate: both branches consume-then-produce on a shared channel
     (a single-value-cell write-write). Number-channel exemption is expressed by
     the soundness disjunct below (matching the code's "exempting number
     channels"). *)
  Definition removed_fires (a b : Branch) : Prop :=
    exists ch, svc_update a ch = true /\ svc_update b ch = true.

  (* The RETAINED double-consume / same-IO-event race (Check #1 on the shared base
     datum): both branches destroyed the same base produce on a NON-number channel
     (the "minus both-mergeable" filter = `is_number_ch ch = false`). *)
  Definition retained_conflict (a b : Branch) : Prop :=
    exists ch, consumes a ch = true /\ consumes b ch = true /\ is_number_ch ch = false.

  (* The number-channel exemption witness: the shared single-value-cell channel is
     a number/foldable channel -- intrinsically mergeable, safe to leave unflagged. *)
  Definition is_number_channel (a b : Branch) : Prop :=
    exists ch, svc_update a ch = true /\ svc_update b ch = true /\ is_number_ch ch = true.

  (* THE core soundness lemma: whatever the removed predicate fires on is either a
     retained conflict OR a number channel. NO premise, NO hidden hypothesis. *)
  Theorem removed_subset_retained :
    forall a b, removed_fires a b -> retained_conflict a b \/ is_number_channel a b.
  Proof.
    intros a b [ch [Ha Hb]].
    destruct (is_number_ch ch) eqn:E.
    - (* number channel: exempt (intrinsically mergeable) *)
      right. exists ch. split; [exact Ha | split; [exact Hb | exact E]].
    - (* non-number: consume-then-produce => the base datum was consumed in both,
         i.e. exactly the retained produces_consumed race *)
      left. exists ch. unfold svc_update in Ha, Hb.
      apply andb_prop in Ha. destruct Ha as [Hca _].
      apply andb_prop in Hb. destruct Hb as [Hcb _].
      split; [exact Hca | split; [exact Hcb | exact E]].
  Qed.

  (* The code's ACTUAL predicate had the number-channel exemption baked in. With
     that filter, the removed check is FULLY subsumed by the retained detector --
     nothing is lost by dropping it. *)
  Definition removed_fires_exempt (a b : Branch) : Prop :=
    exists ch, svc_update a ch = true /\ svc_update b ch = true /\ is_number_ch ch = false.

  (* THE headline: the removal is sound (both the general disjunctive form and the
     number-filtered subsumption the code relies on). *)
  Theorem conflict_removal_sound :
    (forall a b, removed_fires a b -> retained_conflict a b \/ is_number_channel a b)
    /\ (forall a b, removed_fires_exempt a b -> retained_conflict a b).
  Proof.
    split.
    - exact removed_subset_retained.
    - intros a b [ch [Ha [Hb Hn]]]. exists ch.
      unfold svc_update in Ha, Hb.
      apply andb_prop in Ha. destruct Ha as [Hca _].
      apply andb_prop in Hb. destruct Hb as [Hcb _].
      split; [exact Hca | split; [exact Hcb | exact Hn]].
  Qed.

End Conflict.

(* ===========================================================================
   Section Overfill - THE §3c produce-only single-value-cell over-fill proof
   (RCA-asi-devnet-finality-halt).

   Section Conflict (GAP-3) proved the REMOVED consume-then-produce predicate is
   subsumed, modelling a cell update as `svc_update := consumes && produces` -- an
   update that ALWAYS consumes the base. But a PRODUCE-ONLY write (a produce that
   does NOT consume the base) also lands in the cell: producing 5e9 onto a single
   NUMBER cell [0] WITHOUT consuming the base leaves it holding TWO data [0, 5e9],
   tripping the RhoVM IntegerAdd single-value invariant at read time -> finality
   halt. Because `svc_update` requires `consumes = true`, Section Conflict's model
   is VACUOUS for a produce-only write and cannot see this over-fill. This section
   models the produce-only case with cardinality arithmetic and proves Kevin's §3c
   guard (`check_single_value_cell_not_overfilled`, rholang_merging_logic.rs:194,
   wired on the NON-mergeable else-path at dag_merger.rs:965) closes exactly this
   gap -- and, via svc_guard_not_subsumed_exhibit, that it is NOT redundant with
   the retained double-consume detector.

   Model (per channel; is_number_ch / numeric_base / base_card / added / removed
   are abstract PARAMETERS, not axioms -- the results hold for ANY of them):
     is_number_ch ch : the channel's writes were folded as a mergeable NUMBER
                       channel this merge (calculate_number_channel_merge path);
                       the §3c guard runs only when this is FALSE (the else-arm).
     numeric_base ch : the base holds a single NUMERIC datum
                       (try_get_number_with_rnd is Some).
     base_card ch    : |base data|  (the guard engages only at cardinality 1).
     added ch        : |changes.added|   (produced data).
     removed ch      : |changes.removed| (base data consumed by the write).
   cell_after ch = kept ch + added ch = (base_card - removed) + added is the
   post-merge cardinality the guard computes as `result_len`
   (`multiset_diff(base_binary, removed).len() + added.len()`). The guard rejects
   (Err) iff a NON-number single-number base would then hold > 1 value.

   Stdlib-only, axiom-free. `Print Assumptions svc_invariant_iff_both_detectors`
   => "Closed under the global context".
   =========================================================================== *)

Section Overfill.
  Variable is_number_ch : Channel -> bool.
  Variable numeric_base : Channel -> bool.
  Variable base_card : Channel -> nat.
  Variable added : Channel -> nat.
  Variable removed : Channel -> nat.

  (* A single-value NUMBER cell: numeric base of cardinality exactly one. *)
  Definition is_single_number_base (ch : Channel) : bool :=
    numeric_base ch && (base_card ch =? 1).

  (* Post-merge cardinality: base data surviving the removes, plus the adds. *)
  Definition kept (ch : Channel) : nat := base_card ch - removed ch.
  Definition cell_after (ch : Channel) : nat := kept ch + added ch.

  (* The §3c guard fires: a NON-number single-number cell whose post-merge
     cardinality exceeds one (`result_len > 1` -> Err). *)
  Definition svc_guard_active (ch : Channel) : bool :=
    negb (is_number_ch ch) && is_single_number_base ch && (1 <? cell_after ch).

  (* A produce-only write: it adds at least one datum and consumes NO base
     (removed = 0). This is the write Section Conflict's `svc_update` cannot model
     (svc_update needs consumes = true). *)
  Definition produce_only (ch : Channel) : bool :=
    (0 <? added ch) && (removed ch =? 0).

  (* A produce-only over-fill: a produce-only write onto a NON-number
     single-number cell -- the exact finality-halt scenario. *)
  Definition svc_overfill (ch : Channel) : bool :=
    negb (is_number_ch ch) && is_single_number_base ch && produce_only ch.

  (* The write consumed the base datum (its removes are non-empty) -- the bridge
     between "branch consumed the base" (Section Conflict's `consumes`) and the
     cardinality model's `removed`. *)
  Definition consumed_by_merge (ch : Channel) : bool := 0 <? removed ch.

  (* The retained double-consume race on a shared NON-number channel (Check #1,
     restated in this section's vocabulary; cf. Section Conflict's
     retained_conflict): both branches destroyed the base in COMM. *)
  Definition retained_ch (a b : Branch) (ch : Channel) : bool :=
    consumes a ch && consumes b ch && negb (is_number_ch ch).

  (* THE §3c soundness lemma: every produce-only over-fill is caught by the guard.
     cell_after = (base_card - removed) + added = (1 - 0) + added = 1 + added > 1
     since added > 0. Boolean reflection + lia; no hidden hypothesis. *)
  Theorem svc_guard_catches_overfill :
    forall ch, svc_overfill ch = true -> svc_guard_active ch = true.
  Proof.
    intros ch H.
    unfold svc_overfill in H.
    apply andb_true_iff in H. destruct H as [Hpre Hpo].
    apply andb_true_iff in Hpre. destruct Hpre as [Hnum Hsnb].
    unfold produce_only in Hpo.
    apply andb_true_iff in Hpo. destruct Hpo as [Hadd Hrem].
    apply Nat.ltb_lt in Hadd. apply Nat.eqb_eq in Hrem.
    assert (Hcard : base_card ch = 1).
    { unfold is_single_number_base in Hsnb.
      apply andb_true_iff in Hsnb. destruct Hsnb as [_ Hc].
      apply Nat.eqb_eq in Hc. exact Hc. }
    unfold svc_guard_active.
    apply andb_true_iff; split.
    - apply andb_true_iff; split; [exact Hnum | exact Hsnb].
    - apply Nat.ltb_lt. unfold cell_after, kept. lia.
  Qed.

  (* THE non-subsumption bridge: a produce-only over-fill is NOT caught by the
     retained double-consume detector. The hypothesis (an EXPLICIT bridge, not an
     axiom) says branch `a` consuming the base implies the merge removes some base
     (`consumed_by_merge`); a produce-only write removes NOTHING, so `a` did NOT
     consume the base, hence `retained_ch` cannot fire. This is why the §3c guard
     is a genuinely SEPARATE detector, not redundant with Section Conflict. *)
  Theorem overfill_not_retained :
    forall a b ch,
      (consumes a ch = true -> consumed_by_merge ch = true) ->
      svc_overfill ch = true ->
      retained_ch a b ch = false.
  Proof.
    intros a b ch Hbridge Hover.
    unfold retained_ch.
    destruct (consumes a ch) eqn:Eca.
    - exfalso.
      specialize (Hbridge eq_refl).
      unfold consumed_by_merge in Hbridge.
      apply Nat.ltb_lt in Hbridge.
      unfold svc_overfill in Hover.
      apply andb_true_iff in Hover. destruct Hover as [_ Hpo].
      unfold produce_only in Hpo.
      apply andb_true_iff in Hpo. destruct Hpo as [_ Hrem].
      apply Nat.eqb_eq in Hrem. lia.
    - reflexivity.
  Qed.

  (* The single-value-cell invariant is protected by EXACTLY the union of the two
     detectors: a break (a retained double-consume race, OR a non-number
     single-number cell left holding > 1 value) is flagged iff the retained
     detector OR the §3c guard fires. Neither alone suffices -- the retained
     detector misses the produce-only over-fill (svc_guard_not_subsumed_exhibit);
     together they are complete. *)
  Definition svc_broken (a b : Branch) (ch : Channel) : Prop :=
    retained_ch a b ch = true
    \/ (is_single_number_base ch = true /\ cell_after ch > 1 /\ is_number_ch ch = false).

  Definition svc_flagged (a b : Branch) (ch : Channel) : bool :=
    retained_ch a b ch || svc_guard_active ch.

  Theorem svc_invariant_iff_both_detectors :
    forall a b ch, svc_broken a b ch <-> svc_flagged a b ch = true.
  Proof.
    intros a b ch. unfold svc_broken, svc_flagged, svc_guard_active.
    split.
    - intros [Hr | [Hsnb [Hgt Hnum]]].
      + apply orb_true_intro. left. exact Hr.
      + apply orb_true_intro. right.
        apply andb_true_iff; split.
        * apply andb_true_iff; split.
          -- rewrite Hnum. reflexivity.
          -- exact Hsnb.
        * apply Nat.ltb_lt. exact Hgt.
    - intro Hflag. apply orb_prop in Hflag. destruct Hflag as [Hr | Hg].
      + left. exact Hr.
      + right.
        apply andb_true_iff in Hg. destruct Hg as [Hpre Hlt].
        apply andb_true_iff in Hpre. destruct Hpre as [Hnnum Hsnb].
        apply Nat.ltb_lt in Hlt.
        split; [exact Hsnb | split].
        * exact Hlt.
        * destruct (is_number_ch ch) eqn:E; [discriminate Hnnum | reflexivity].
  Qed.

End Overfill.

(* A concrete constant witness that the §3c guard is NOT redundant with the
   retained double-consume detector: instantiate the section at a produce-only
   write (added = 1, removed = 0) onto a single NUMBER cell (base_card = 1,
   numeric, non-number-merged), with both branches consuming NOTHING. The input
   is svc_broken (the cell would hold cell_after = 2 > 1) AND retained_ch = false
   (the retained detector is blind to it) AND svc_guard_active = true (only the
   §3c guard catches it). This is the mechanized statement of the finality-halt
   RCA: without §3c the produce-only over-fill escapes every retained check. *)
Definition ex_branch : Branch :=
  {| consumes := fun _ => false; produces := fun _ => true |}.

Example svc_guard_not_subsumed_exhibit :
  svc_broken (fun _ => false) (fun _ => true) (fun _ => 1) (fun _ => 1) (fun _ => 0)
             ex_branch ex_branch 0
  /\ retained_ch (fun _ => false) ex_branch ex_branch 0 = false
  /\ svc_guard_active (fun _ => false) (fun _ => true) (fun _ => 1) (fun _ => 1) (fun _ => 0) 0
     = true.
Proof.
  refine (conj _ (conj _ _)).
  - vm_compute. right. refine (conj _ (conj _ _)).
    + reflexivity.
    + lia.
    + reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
Qed.

Print Assumptions svc_invariant_iff_both_detectors.
