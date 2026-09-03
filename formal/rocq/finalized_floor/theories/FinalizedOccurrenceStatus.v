From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Lia.
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

Record terminal_occurrence_summary := {
  frozen_occurrence : occurrence
}.

Definition exact_active
  (active_state : list occurrence)
  (candidate : occurrence) : Prop :=
  In candidate active_state.

Definition freeze_active_occurrence
  (active_state : list occurrence)
  (candidate : occurrence) : option terminal_occurrence_summary :=
  if in_dec occurrence_eq_dec candidate active_state
  then Some {| frozen_occurrence := candidate |}
  else None.

Theorem terminal_summary_freezes_only_exact_active_occurrence :
  forall active_state candidate summary,
    freeze_active_occurrence active_state candidate = Some summary ->
    frozen_occurrence summary = candidate /\
    exact_active active_state candidate.
Proof.
  intros active_state candidate summary Hfreeze.
  unfold freeze_active_occurrence in Hfreeze.
  destruct (in_dec occurrence_eq_dec candidate active_state) as [Hactive | Hinactive].
  - inversion Hfreeze. subst summary. split.
    + reflexivity.
    + exact Hactive.
  - discriminate.
Qed.

Theorem exactly_inactive_occurrence_cannot_be_frozen :
  forall active_state candidate,
    ~ exact_active active_state candidate ->
    freeze_active_occurrence active_state candidate = None.
Proof.
  intros active_state candidate Hinactive.
  unfold freeze_active_occurrence.
  destruct (in_dec occurrence_eq_dec candidate active_state) as [Hactive | Hmissing].
  - contradiction.
  - reflexivity.
Qed.

Theorem rejection_evidence_cannot_override_exact_active_state :
  forall records active_state candidate,
    tombstoned (finalized_rejection_targets records) candidate ->
    exact_active active_state candidate ->
    exists summary,
      freeze_active_occurrence active_state candidate = Some summary /\
      frozen_occurrence summary = candidate.
Proof.
  intros records active_state candidate _ Hactive.
  unfold exact_active in Hactive.
  unfold freeze_active_occurrence.
  destruct (in_dec occurrence_eq_dec candidate active_state) as [Hin | Hmissing].
  - exists {| frozen_occurrence := candidate |}. split; reflexivity.
  - contradiction.
Qed.

Record located_occurrence := {
  located_value : occurrence;
  occurrence_in_finalized_closure : bool
}.

Definition freeze_finalized_active_occurrence
  (active_state : list occurrence)
  (candidate : located_occurrence) : option terminal_occurrence_summary :=
  if occurrence_in_finalized_closure candidate
  then freeze_active_occurrence active_state (located_value candidate)
  else None.

Theorem terminal_summary_uses_only_finalized_exact_active_occurrence :
  forall active_state candidate summary,
    freeze_finalized_active_occurrence active_state candidate = Some summary ->
    occurrence_in_finalized_closure candidate = true /\
    frozen_occurrence summary = located_value candidate /\
    exact_active active_state (located_value candidate).
Proof.
  intros active_state candidate summary Hfreeze.
  unfold freeze_finalized_active_occurrence in Hfreeze.
  destruct (occurrence_in_finalized_closure candidate) eqn:Hclosure.
  - split.
    + reflexivity.
    + now apply terminal_summary_freezes_only_exact_active_occurrence in Hfreeze.
  - discriminate.
Qed.

Theorem off_floor_occurrence_cannot_be_frozen :
  forall active_state candidate,
    occurrence_in_finalized_closure candidate = false ->
    freeze_finalized_active_occurrence active_state candidate = None.
Proof.
  intros active_state candidate Hofffloor.
  unfold freeze_finalized_active_occurrence.
  now rewrite Hofffloor.
Qed.

Record ranked_occurrence := {
  ranked_value : occurrence;
  ranked_height : nat;
  ranked_hash : nat
}.

Definition preferred_occurrence
  (left right : ranked_occurrence) : ranked_occurrence :=
  if Nat.ltb (ranked_height left) (ranked_height right)
  then right
  else if Nat.ltb (ranked_height right) (ranked_height left)
       then left
       else if Nat.leb (ranked_hash left) (ranked_hash right)
            then left
            else right.

Theorem preferred_occurrence_order_independent :
  forall left right,
    (ranked_height left = ranked_height right ->
     ranked_hash left = ranked_hash right ->
     left = right) ->
    preferred_occurrence left right = preferred_occurrence right left.
Proof.
  intros left right Hidentity.
  unfold preferred_occurrence.
  destruct (Nat.ltb (ranked_height left) (ranked_height right)) eqn:Hlr;
  destruct (Nat.ltb (ranked_height right) (ranked_height left)) eqn:Hrl.
  - apply Nat.ltb_lt in Hlr. apply Nat.ltb_lt in Hrl. lia.
  - reflexivity.
  - reflexivity.
  - apply Nat.ltb_ge in Hlr. apply Nat.ltb_ge in Hrl.
    assert (Hheight : ranked_height left = ranked_height right) by lia.
    destruct (Nat.leb (ranked_hash left) (ranked_hash right)) eqn:Hhash_lr;
    destruct (Nat.leb (ranked_hash right) (ranked_hash left)) eqn:Hhash_rl.
    + apply Nat.leb_le in Hhash_lr. apply Nat.leb_le in Hhash_rl.
      assert (Hhash : ranked_hash left = ranked_hash right) by lia.
      pose proof (Hidentity Hheight Hhash) as Hequal.
      now subst right.
    + reflexivity.
    + reflexivity.
    + apply Nat.leb_gt in Hhash_lr. apply Nat.leb_gt in Hhash_rl. lia.
Qed.

Print Assumptions finalized_closure_rejection_is_authoritative.
Print Assumptions main_chain_only_projection_is_incomplete.
Print Assumptions terminal_summary_freezes_only_exact_active_occurrence.
Print Assumptions exactly_inactive_occurrence_cannot_be_frozen.
Print Assumptions rejection_evidence_cannot_override_exact_active_state.
Print Assumptions terminal_summary_uses_only_finalized_exact_active_occurrence.
Print Assumptions off_floor_occurrence_cannot_be_frozen.
Print Assumptions preferred_occurrence_order_independent.
