From Stdlib Require Import List Bool.Bool.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Inductive frontier_parent : Type :=
| LeftParent
| RightParent
| CoveringParent.

Inductive latest_message : Type :=
| LeftLatest
| RightLatest.

Definition parent_covers (parent : frontier_parent) (latest : latest_message) : bool :=
  match parent, latest with
  | LeftParent, LeftLatest => true
  | RightParent, RightLatest => true
  | CoveringParent, _ => true
  | _, _ => false
  end.

Definition collectively_covers
  (parents : list frontier_parent)
  (latest_messages : list latest_message) : bool :=
  forallb
    (fun latest => existsb (fun parent => parent_covers parent latest) parents)
    latest_messages.

Definition one_parent_covers
  (parents : list frontier_parent)
  (latest_messages : list latest_message) : bool :=
  existsb
    (fun parent => forallb (fun latest => parent_covers parent latest) latest_messages)
    parents.

Definition retry_ready
  (gate_open owner_custody : bool)
  (parents : list frontier_parent)
  (latest_messages : list latest_message) : bool :=
  gate_open && owner_custody && collectively_covers parents latest_messages.

Record packaging_decision : Type := {
  ordinary_publisher : nat;
  retry_authorized : bool
}.

Definition decide_packaging
  (ordinary_leader : nat)
  (gate_open owner_custody : bool)
  (parents : list frontier_parent)
  (latest_messages : list latest_message) : packaging_decision :=
  {| ordinary_publisher := ordinary_leader;
     retry_authorized :=
       retry_ready gate_open owner_custody parents latest_messages |}.

Theorem one_parent_coverage_implies_collective_coverage :
  forall parents latest_messages,
    one_parent_covers parents latest_messages = true ->
    collectively_covers parents latest_messages = true.
Proof.
  intros parents latest_messages Hone.
  unfold one_parent_covers in Hone.
  apply existsb_exists in Hone as [parent [Hparent Hall]].
  unfold collectively_covers.
  apply forallb_forall.
  intros latest Hlatest.
  apply existsb_exists.
  exists parent.
  split.
  - exact Hparent.
  - apply forallb_forall with (x := latest) in Hall.
    + exact Hall.
    + exact Hlatest.
Qed.

Theorem collective_coverage_does_not_require_one_covering_parent :
  collectively_covers
    [LeftParent; RightParent]
    [LeftLatest; RightLatest] = true /\
  one_parent_covers
    [LeftParent; RightParent]
    [LeftLatest; RightLatest] = false.
Proof.
  split; reflexivity.
Qed.

Theorem collective_coverage_is_order_independent_for_split_frontier :
  collectively_covers
    [LeftParent; RightParent]
    [LeftLatest; RightLatest] =
  collectively_covers
    [RightParent; LeftParent]
    [RightLatest; LeftLatest].
Proof.
  reflexivity.
Qed.

Theorem collective_coverage_parent_permutation :
  forall parents_a parents_b latest_messages,
    Permutation parents_a parents_b ->
    collectively_covers parents_a latest_messages = true <->
    collectively_covers parents_b latest_messages = true.
Proof.
  intros parents_a parents_b latest_messages Hpermutation.
  split; intro Hcoverage.
  - unfold collectively_covers in *.
    apply forallb_forall.
    intros latest Hlatest.
    apply forallb_forall with (x := latest) in Hcoverage.
    + apply existsb_exists in Hcoverage as [parent [Hparent Hcovers]].
      apply existsb_exists.
      exists parent.
      split.
      * eapply Permutation_in.
        -- exact Hpermutation.
        -- exact Hparent.
      * exact Hcovers.
    + exact Hlatest.
  - unfold collectively_covers in *.
    apply forallb_forall.
    intros latest Hlatest.
    apply forallb_forall with (x := latest) in Hcoverage.
    + apply existsb_exists in Hcoverage as [parent [Hparent Hcovers]].
      apply existsb_exists.
      exists parent.
      split.
      * eapply Permutation_in.
        -- apply Permutation_sym.
           exact Hpermutation.
        -- exact Hparent.
      * exact Hcovers.
    + exact Hlatest.
Qed.

Theorem collective_coverage_latest_message_permutation :
  forall parents latest_a latest_b,
    Permutation latest_a latest_b ->
    collectively_covers parents latest_a = true <->
    collectively_covers parents latest_b = true.
Proof.
  intros parents latest_a latest_b Hpermutation.
  unfold collectively_covers.
  split; intro Hcoverage.
  - apply forallb_forall.
    intros latest Hlatest.
    apply forallb_forall with (x := latest) in Hcoverage.
    + exact Hcoverage.
    + eapply Permutation_in.
      * apply Permutation_sym.
        exact Hpermutation.
      * exact Hlatest.
  - apply forallb_forall.
    intros latest Hlatest.
    apply forallb_forall with (x := latest) in Hcoverage.
    + exact Hcoverage.
    + eapply Permutation_in.
      * exact Hpermutation.
      * exact Hlatest.
Qed.

Theorem retry_readiness_requires_gate_custody_and_collective_coverage :
  forall gate_open owner_custody parents latest_messages,
    retry_ready gate_open owner_custody parents latest_messages = true ->
    gate_open = true /\
    owner_custody = true /\
    collectively_covers parents latest_messages = true.
Proof.
  intros gate_open owner_custody parents latest_messages Hready.
  unfold retry_ready in Hready.
  apply andb_true_iff in Hready as [Hauthority Hcoverage].
  apply andb_true_iff in Hauthority as [Hgate Hcustody].
  repeat split; assumption.
Qed.

Theorem retry_readiness_is_independent_of_ordinary_leadership :
  forall gate_open owner_custody parents latest_messages (leader_a leader_b : nat),
    retry_authorized
      (decide_packaging
        leader_a gate_open owner_custody parents latest_messages) =
    retry_authorized
      (decide_packaging
        leader_b gate_open owner_custody parents latest_messages).
Proof.
  reflexivity.
Qed.

Theorem split_frontier_owner_retry_is_ready :
  retry_ready
    true
    true
    [LeftParent; RightParent]
    [LeftLatest; RightLatest] = true.
Proof.
  reflexivity.
Qed.

Print Assumptions one_parent_coverage_implies_collective_coverage.
Print Assumptions collective_coverage_does_not_require_one_covering_parent.
Print Assumptions collective_coverage_parent_permutation.
Print Assumptions collective_coverage_latest_message_permutation.
Print Assumptions retry_readiness_requires_gate_custody_and_collective_coverage.
