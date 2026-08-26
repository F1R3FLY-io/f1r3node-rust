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

Print Assumptions retained_parents_are_pairwise_uncovered.
Print Assumptions compaction_and_coverage_guard_preserve_every_causal_tip.
