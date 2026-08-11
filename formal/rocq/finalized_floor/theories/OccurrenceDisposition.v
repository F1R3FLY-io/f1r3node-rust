From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
Import ListNotations.

Record occurrence := {
  deploy_id : nat;
  source_id : nat
}.

Definition occurrence_eq_dec : forall x y : occurrence, {x = y} + {x <> y}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition tombstoned (records : list occurrence) (candidate : occurrence) : Prop :=
  In candidate records.

Definition active (records : list occurrence) (candidate : occurrence) : Prop :=
  ~ tombstoned records candidate.

Definition reject_occurrence
  (records : list occurrence)
  (candidate : occurrence) : list occurrence :=
  if in_dec occurrence_eq_dec candidate records
  then records
  else candidate :: records.

Lemma reject_occurrence_membership :
  forall records rejected candidate,
    tombstoned (reject_occurrence records rejected) candidate <->
    candidate = rejected \/ tombstoned records candidate.
Proof.
  intros records rejected candidate.
  unfold tombstoned, reject_occurrence.
  destruct (in_dec occurrence_eq_dec rejected records) as [Hin | Hnotin].
  - split.
    + intro Hcandidate. right. exact Hcandidate.
    + intros [Heq | Hcandidate].
      * subst candidate. exact Hin.
      * exact Hcandidate.
  - simpl. split.
    + intros [Heq | Hcandidate].
      * left. symmetry. exact Heq.
      * right. exact Hcandidate.
    + intros [Heq | Hcandidate].
      * left. symmetry. exact Heq.
      * right. exact Hcandidate.
Qed.

Theorem rejection_is_source_exact :
  forall records rejected,
    tombstoned (reject_occurrence records rejected) rejected.
Proof.
  intros records rejected.
  apply reject_occurrence_membership.
  left. reflexivity.
Qed.

Theorem distinct_source_survives_rejection :
  forall records rejected survivor,
    deploy_id rejected = deploy_id survivor ->
    source_id rejected <> source_id survivor ->
    active records survivor ->
    active (reject_occurrence records rejected) survivor.
Proof.
  intros records rejected survivor _ Hsource Hactive Htombstoned.
  apply reject_occurrence_membership in Htombstoned.
  destruct Htombstoned as [Heq | Hold].
  - apply Hsource. now rewrite Heq.
  - exact (Hactive Hold).
Qed.

Theorem rejection_order_independent :
  forall records left right candidate,
    tombstoned (reject_occurrence (reject_occurrence records left) right) candidate <->
    tombstoned (reject_occurrence (reject_occurrence records right) left) candidate.
Proof.
  intros records left right candidate.
  repeat rewrite reject_occurrence_membership.
  tauto.
Qed.

Theorem one_winner_preserved :
  forall winner loser,
    deploy_id winner = deploy_id loser ->
    source_id winner <> source_id loser ->
    active (reject_occurrence [] loser) winner.
Proof.
  intros winner loser Hdeploy Hsource.
  apply distinct_source_survives_rejection.
  - symmetry. exact Hdeploy.
  - intro Heq. apply Hsource. symmetry. exact Heq.
  - unfold active, tombstoned. simpl. tauto.
Qed.
