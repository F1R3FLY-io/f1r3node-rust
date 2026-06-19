From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
From Slashing Require Import Validator Block.
Import ListNotations.

Set Implicit Arguments.

Fixpoint has_validator (v : Validator) (js : list Justification) : bool :=
  match js with
  | [] => false
  | j :: rest =>
      if validator_eq_dec (j_validator j) v then true else has_validator v rest
  end.

Fixpoint unique_justification_validators (js : list Justification) : bool :=
  match js with
  | [] => true
  | j :: rest =>
      negb (has_validator (j_validator j) rest)
      && unique_justification_validators rest
  end.

Theorem duplicate_head_rejected :
  forall v h1 h2 rest,
    unique_justification_validators
      (mkJustification v h1 :: mkJustification v h2 :: rest) = false.
Proof.
  intros. simpl.
  destruct (validator_eq_dec v v) as [_ | Hbad]; [reflexivity|contradiction].
Qed.

Theorem unique_tail_of_unique :
  forall j rest,
    unique_justification_validators (j :: rest) = true ->
    unique_justification_validators rest = true.
Proof.
  intros j rest H.
  simpl in H.
  apply andb_true_iff in H.
  destruct H as [_ Htail].
  assumption.
Qed.

Theorem accepted_implies_head_not_in_tail :
  forall j rest,
    unique_justification_validators (j :: rest) = true ->
    has_validator (j_validator j) rest = false.
Proof.
  intros j rest H.
  simpl in H.
  apply andb_true_iff in H.
  destruct H as [Hhead _].
  apply negb_true_iff in Hhead.
  assumption.
Qed.
