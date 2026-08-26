From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.

Record LiveRecoveryState := {
  live_genesis : nat;
  live_known_height : nat;
  live_durable_head : nat;
  live_effects_through : nat;
  live_requested_finalization : nat;
  live_completed_finalization : nat;
  live_sequence : nat;
  live_bond_generation : nat;
  live_evidence : list nat
}.

Definition advertise_remote_tip
  (_tip : nat)
  (state : LiveRecoveryState) : LiveRecoveryState := state.

Definition admit_dependency_closure
  (tip : nat)
  (state : LiveRecoveryState) : LiveRecoveryState :=
  {| live_genesis := live_genesis state;
     live_known_height := Nat.max tip (live_known_height state);
     live_durable_head := live_durable_head state;
     live_effects_through := live_effects_through state;
     live_requested_finalization := live_requested_finalization state;
     live_completed_finalization := live_completed_finalization state;
     live_sequence := live_sequence state;
     live_bond_generation := live_bond_generation state;
     live_evidence := live_evidence state |}.

Definition request_local_finalization
  (state : LiveRecoveryState) : LiveRecoveryState :=
  {| live_genesis := live_genesis state;
     live_known_height := live_known_height state;
     live_durable_head := live_durable_head state;
     live_effects_through := live_effects_through state;
     live_requested_finalization := S (live_requested_finalization state);
     live_completed_finalization := live_completed_finalization state;
     live_sequence := live_sequence state;
     live_bond_generation := live_bond_generation state;
     live_evidence := live_evidence state |}.

Definition run_local_finalizer
  (target : nat)
  (state : LiveRecoveryState) : option LiveRecoveryState :=
  if Nat.leb (live_durable_head state) target &&
     Nat.leb target (live_known_height state) &&
     Nat.ltb (live_completed_finalization state)
             (live_requested_finalization state)
  then Some
    {| live_genesis := live_genesis state;
       live_known_height := live_known_height state;
       live_durable_head := target;
       live_effects_through := target;
       live_requested_finalization := live_requested_finalization state;
       live_completed_finalization := live_requested_finalization state;
       live_sequence := live_sequence state;
       live_bond_generation := live_bond_generation state;
       live_evidence := live_evidence state |}
  else None.

Theorem remote_tip_advertisement_cannot_mutate_local_state :
  forall tip state, advertise_remote_tip tip state = state.
Proof.
  reflexivity.
Qed.

Theorem dependency_admission_cannot_publish_finality :
  forall tip state,
    live_durable_head (admit_dependency_closure tip state) =
      live_durable_head state /\
    live_effects_through (admit_dependency_closure tip state) =
      live_effects_through state.
Proof.
  intros tip state.
  split; reflexivity.
Qed.

Theorem local_finalizer_is_monotone_and_effect_atomic :
  forall target state published,
    run_local_finalizer target state = Some published ->
    live_durable_head state <= live_durable_head published /\
    live_durable_head published = live_effects_through published /\
    live_completed_finalization published =
      live_requested_finalization state.
Proof.
  intros target state published Hrun.
  unfold run_local_finalizer in Hrun.
  destruct
    (Nat.leb (live_durable_head state) target &&
     Nat.leb target (live_known_height state) &&
     Nat.ltb (live_completed_finalization state)
             (live_requested_finalization state)) eqn:Hguard;
    try discriminate.
  inversion Hrun; subst; clear Hrun.
  repeat rewrite andb_true_iff in Hguard.
  destruct Hguard as [[Hhead _] _].
  apply Nat.leb_le in Hhead.
  repeat split; simpl; auto.
Qed.

Theorem live_recovery_preserves_validator_identity :
  forall target state published,
    run_local_finalizer target state = Some published ->
    live_sequence published = live_sequence state /\
    live_bond_generation published = live_bond_generation state /\
    live_evidence published = live_evidence state.
Proof.
  intros target state published Hrun.
  unfold run_local_finalizer in Hrun.
  destruct
    (Nat.leb (live_durable_head state) target &&
     Nat.leb target (live_known_height state) &&
     Nat.ltb (live_completed_finalization state)
             (live_requested_finalization state));
    try discriminate.
  inversion Hrun; subst.
  repeat split; reflexivity.
Qed.

Definition update_validator
  (validator : nat)
  (state : LiveRecoveryState)
  (network : nat -> LiveRecoveryState) : nat -> LiveRecoveryState :=
  fun candidate => if Nat.eq_dec candidate validator then state else network candidate.

Theorem parallel_recovery_frames_other_validators :
  forall validator other state network,
    other <> validator ->
    update_validator validator state network other = network other.
Proof.
  intros validator other state network Hneq.
  unfold update_validator.
  destruct (Nat.eq_dec other validator); congruence.
Qed.

Theorem minority_fork_recovery_correct :
  (forall tip state, advertise_remote_tip tip state = state) /\
  (forall tip state,
    live_durable_head (admit_dependency_closure tip state) =
      live_durable_head state) /\
  (forall target state published,
    run_local_finalizer target state = Some published ->
    live_durable_head state <= live_durable_head published /\
    live_durable_head published = live_effects_through published) /\
  (forall validator other state network,
    other <> validator ->
    update_validator validator state network other = network other).
Proof.
  split.
  - exact remote_tip_advertisement_cannot_mutate_local_state.
  - split.
    + intros tip state.
      exact (proj1 (dependency_admission_cannot_publish_finality tip state)).
    + split.
      * intros target state published Hrun.
        split.
        -- exact (proj1 (local_finalizer_is_monotone_and_effect_atomic
                           target state published Hrun)).
        -- exact (proj1 (proj2 (local_finalizer_is_monotone_and_effect_atomic
                                 target state published Hrun))).
      * exact parallel_recovery_frames_other_validators.
Qed.

Print Assumptions minority_fork_recovery_correct.
