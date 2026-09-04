From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
Import ListNotations.

From ForkChoice Require Import Foundation.

Definition covered_by_otherb
  (ancestorb : BlockHash -> BlockHash -> bool)
  (parents : list BlockHash)
  (candidate : BlockHash) : bool :=
  existsb
    (fun other => negb (Nat.eqb candidate other) && ancestorb candidate other)
    parents.

Definition reachability_maximal_antichain
  (ancestorb : BlockHash -> BlockHash -> bool)
  (parents : list BlockHash) : list BlockHash :=
  filter (fun candidate => negb (covered_by_otherb ancestorb parents candidate)) parents.

Definition secondary_candidateb
  (ancestorb : BlockHash -> BlockHash -> bool)
  (protected candidate : BlockHash) : bool :=
  negb (Nat.eqb candidate protected) && negb (ancestorb candidate protected).

Definition secondary_candidates
  (ancestorb : BlockHash -> BlockHash -> bool)
  (protected : BlockHash)
  (candidates : list BlockHash) : list BlockHash :=
  filter (secondary_candidateb ancestorb protected) candidates.

Definition protected_parent_frontier
  (ancestorb : BlockHash -> BlockHash -> bool)
  (protected : BlockHash)
  (candidates : list BlockHash) : list BlockHash :=
  protected
  :: reachability_maximal_antichain
       ancestorb
       (secondary_candidates ancestorb protected candidates).

Definition parent_covers_tipb
  (ancestorb : BlockHash -> BlockHash -> bool)
  (tip parent : BlockHash) : bool :=
  Nat.eqb tip parent || ancestorb tip parent.

Definition causal_coverageb
  (ancestorb : BlockHash -> BlockHash -> bool)
  (tips parents : list BlockHash) : bool :=
  forallb
    (fun tip => existsb (parent_covers_tipb ancestorb tip) parents)
    tips.

Theorem retained_parent_was_an_input :
  forall ancestorb parents parent,
    In parent (reachability_maximal_antichain ancestorb parents) ->
    In parent parents.
Proof.
  intros ancestorb parents parent Hparent.
  apply filter_In in Hparent. exact (proj1 Hparent).
Qed.

Theorem retained_parents_are_pairwise_uncovered :
  forall ancestorb parents left right,
    In left (reachability_maximal_antichain ancestorb parents) ->
    In right (reachability_maximal_antichain ancestorb parents) ->
    left <> right ->
    ancestorb left right = false.
Proof.
  intros ancestorb parents left right Hleft Hright Hneq.
  apply filter_In in Hleft. destruct Hleft as [_ Hmaximal].
  apply filter_In in Hright. destruct Hright as [Hright _].
  apply negb_true_iff in Hmaximal.
  unfold covered_by_otherb in Hmaximal.
  destruct (ancestorb left right) eqn:Hancestor; [| reflexivity].
  exfalso.
  assert (Hexists :
    existsb
      (fun other => negb (Nat.eqb left other) && ancestorb left other)
      parents = true).
  { apply existsb_exists. exists right. split; [exact Hright |].
    apply Nat.eqb_neq in Hneq. rewrite Hneq, Hancestor. reflexivity. }
  rewrite Hexists in Hmaximal. discriminate.
Qed.

Theorem causal_coverage_guard_is_sound :
  forall ancestorb tips parents,
    causal_coverageb ancestorb tips parents = true ->
    forall tip,
      In tip tips ->
      exists parent,
        In parent parents /\
        (tip = parent \/ ancestorb tip parent = true).
Proof.
  intros ancestorb tips parents Hcoverage tip Htip.
  unfold causal_coverageb in Hcoverage.
  apply forallb_forall with (x := tip) in Hcoverage; [| exact Htip].
  apply existsb_exists in Hcoverage.
  destruct Hcoverage as [parent [Hparent Hcovers]].
  exists parent. split; [exact Hparent |].
  unfold parent_covers_tipb in Hcovers.
  apply orb_true_iff in Hcovers. destruct Hcovers as [Heq | Hancestor].
  - left. apply Nat.eqb_eq. exact Heq.
  - right. exact Hancestor.
Qed.

Theorem compaction_and_coverage_guard_preserve_every_causal_tip :
  forall ancestorb tips candidates,
    causal_coverageb
      ancestorb
      tips
      (reachability_maximal_antichain ancestorb candidates) = true ->
    forall tip,
      In tip tips ->
      exists parent,
        In parent (reachability_maximal_antichain ancestorb candidates) /\
        (tip = parent \/ ancestorb tip parent = true).
Proof. intros; eapply causal_coverage_guard_is_sound; eauto. Qed.

Theorem protected_frontier_has_exact_head :
  forall ancestorb protected candidates,
    hd_error (protected_parent_frontier ancestorb protected candidates) = Some protected.
Proof. reflexivity. Qed.

Theorem protected_frontier_tail_excludes_head :
  forall ancestorb protected candidates parent,
    In parent (tl (protected_parent_frontier ancestorb protected candidates)) ->
    parent <> protected.
Proof.
  intros ancestorb protected candidates parent Hparent.
  simpl in Hparent.
  apply retained_parent_was_an_input in Hparent.
  unfold secondary_candidates in Hparent.
  apply filter_In in Hparent. destruct Hparent as [_ Heligible].
  unfold secondary_candidateb in Heligible.
  apply andb_true_iff in Heligible. destruct Heligible as [Hneq _].
  apply negb_true_iff in Hneq. apply Nat.eqb_neq in Hneq. exact Hneq.
Qed.

Theorem protected_frontier_tail_was_candidate :
  forall ancestorb protected candidates parent,
    In parent (tl (protected_parent_frontier ancestorb protected candidates)) ->
    In parent candidates.
Proof.
  intros ancestorb protected candidates parent Hparent.
  simpl in Hparent.
  apply retained_parent_was_an_input in Hparent.
  unfold secondary_candidates in Hparent.
  apply filter_In in Hparent. exact (proj1 Hparent).
Qed.

Theorem protected_frontier_tail_is_pairwise_uncovered :
  forall ancestorb protected candidates left right,
    In left (tl (protected_parent_frontier ancestorb protected candidates)) ->
    In right (tl (protected_parent_frontier ancestorb protected candidates)) ->
    left <> right ->
    ancestorb left right = false.
Proof.
  intros ancestorb protected candidates left right Hleft Hright Hneq.
  simpl in Hleft, Hright.
  eapply retained_parents_are_pairwise_uncovered; eauto.
Qed.

Theorem protected_frontier_is_duplicate_free :
  forall ancestorb protected candidates,
    NoDup candidates ->
    NoDup (protected_parent_frontier ancestorb protected candidates).
Proof.
  intros ancestorb protected candidates Hnodup.
  unfold protected_parent_frontier.
  apply NoDup_cons.
  - intro Hin.
    assert (Hneq : protected <> protected).
    { eapply protected_frontier_tail_excludes_head.
      simpl. exact Hin. }
    contradiction.
  - unfold reachability_maximal_antichain, secondary_candidates.
    apply NoDup_filter. apply NoDup_filter. exact Hnodup.
Qed.

Theorem protected_compaction_and_coverage_guard_preserve_every_causal_tip :
  forall ancestorb protected tips candidates,
    causal_coverageb
      ancestorb
      tips
      (protected_parent_frontier ancestorb protected candidates) = true ->
    forall tip,
      In tip tips ->
      exists parent,
        In parent (protected_parent_frontier ancestorb protected candidates) /\
        (tip = parent \/ ancestorb tip parent = true).
Proof. intros; eapply causal_coverage_guard_is_sound; eauto. Qed.

Definition linear_ancestorb (ancestor descendant : BlockHash) : bool :=
  ancestor <? descendant.

Theorem generic_compaction_can_erase_protected_head :
  reachability_maximal_antichain linear_ancestorb [0; 1] = [1]
  /\ protected_parent_frontier linear_ancestorb 0 [0; 1] = [0; 1].
Proof. split; reflexivity. Qed.

Theorem protected_frontier_preserves_descendant_coverage :
  causal_coverageb linear_ancestorb [0; 1]
    (protected_parent_frontier linear_ancestorb 0 [0; 1]) = true.
Proof. reflexivity. Qed.

Print Assumptions retained_parents_are_pairwise_uncovered.
Print Assumptions compaction_and_coverage_guard_preserve_every_causal_tip.
