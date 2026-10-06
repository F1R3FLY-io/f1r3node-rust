From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
Import ListNotations.

From FinalizedFloor Require Import OccurrenceDisposition.

Record located_rejection := {
  rejection_target : occurrence;
  recording_id : nat;
  recording_in_finalized_closure : bool;
  recording_on_main_chain : bool
}.

Fixpoint finalized_rejection_targets
  (records : list located_rejection) : list occurrence :=
  match records with
  | [] => []
  | record :: tail =>
      if recording_in_finalized_closure record
      then rejection_target record :: finalized_rejection_targets tail
      else finalized_rejection_targets tail
  end.

Fixpoint main_chain_rejection_targets
  (records : list located_rejection) : list occurrence :=
  match records with
  | [] => []
  | record :: tail =>
      if recording_in_finalized_closure record && recording_on_main_chain record
      then rejection_target record :: main_chain_rejection_targets tail
      else main_chain_rejection_targets tail
  end.

Theorem finalized_closure_rejection_is_authoritative :
  forall records record,
    In record records ->
    recording_in_finalized_closure record = true ->
    tombstoned (finalized_rejection_targets records) (rejection_target record).
Proof.
  intros records record Hin Hfinalized.
  unfold tombstoned.
  induction records as [|head tail IH].
  - inversion Hin.
  - simpl in Hin. destruct Hin as [Heq | Hin].
    + subst head. simpl. rewrite Hfinalized. simpl. left. reflexivity.
    + simpl. destruct (recording_in_finalized_closure head) eqn:Hhead.
      * simpl. right. apply IH. exact Hin.
      * apply IH. exact Hin.
Qed.

Definition secondary_example_occurrence : occurrence :=
  {| deploy_id := 1; source_id := 2 |}.

Definition secondary_example_record : located_rejection :=
  {| rejection_target := secondary_example_occurrence;
     recording_id := 3;
     recording_in_finalized_closure := true;
     recording_on_main_chain := false |}.

Theorem main_chain_only_projection_is_incomplete :
  tombstoned
    (finalized_rejection_targets [secondary_example_record])
    secondary_example_occurrence /\
  ~ tombstoned
      (main_chain_rejection_targets [secondary_example_record])
      secondary_example_occurrence.
Proof.
  split; simpl; tauto.
Qed.

Print Assumptions finalized_closure_rejection_is_authoritative.
Print Assumptions main_chain_only_projection_is_incomplete.
