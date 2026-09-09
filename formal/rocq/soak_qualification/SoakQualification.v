From Stdlib Require Import Lists.List Arith.PeanoNat Bool.Bool Lia.
Import ListNotations.

Inductive event := Credit (nanoseconds : nat) | Idle | Invalidate.

Definition step (state : nat * bool) (input : event) : nat * bool :=
  let '(duration, valid) := state in
  match input with
  | Credit amount => (duration + amount, valid)
  | Idle => state
  | Invalidate => (duration, false)
  end.

Definition run (events : list event) (initial : nat * bool) :=
  fold_left step events initial.

Fixpoint credits (events : list event) : nat :=
  match events with
  | [] => 0
  | Credit amount :: rest => amount + credits rest
  | _ :: rest => credits rest
  end.

Definition qualifies (required : nat) (events : list event) : bool :=
  let '(duration, valid) := run events (0, true) in
  valid && (required <=? duration).

Theorem run_duration : forall events duration valid,
  fst (run events (duration, valid)) = duration + credits events.
Proof.
  induction events as [|input rest IH]; intros duration valid; simpl.
  - lia.
  - destruct input; unfold run in *; simpl in *;
      rewrite IH; simpl; lia.
Qed.

Theorem invalid_is_absorbing : forall events duration,
  snd (run events (duration, false)) = false.
Proof.
  induction events as [|input rest IH]; intros duration; simpl.
  - reflexivity.
  - destruct input; apply IH.
Qed.

Theorem qualification_requires_full_duration : forall required events,
  qualifies required events = true -> required <= credits events.
Proof.
  intros required events H.
  unfold qualifies in H.
  pose proof (run_duration events 0 true) as Hd.
  destruct (run events (0, true)) as [duration valid].
  simpl in Hd. subst duration.
  apply andb_true_iff in H as [_ H].
  apply Nat.leb_le in H. exact H.
Qed.

Theorem subthreshold_never_qualifies : forall required events,
  credits events < required -> qualifies required events = false.
Proof.
  intros required events H.
  destruct (qualifies required events) eqn:Hq; auto.
  apply qualification_requires_full_duration in Hq. lia.
Qed.

Theorem invalidation_survives_any_suffix : forall required prefix suffix,
  qualifies required (prefix ++ Invalidate :: suffix) = false.
Proof.
  intros required prefix suffix.
  unfold qualifies, run.
  rewrite fold_left_app. simpl.
  destruct (fold_left step prefix (0, true)) as [duration valid].
  cbn [step].
  pose proof (invalid_is_absorbing suffix duration) as H.
  unfold run in H.
  destruct (fold_left step suffix (duration, false)) as [total final_valid].
  simpl in H. subst final_valid. reflexivity.
Qed.

Theorem idle_does_not_create_credit : forall prefix suffix,
  credits (prefix ++ Idle :: suffix) = credits (prefix ++ suffix).
Proof.
  induction prefix as [|input rest IH]; intros suffix; simpl; auto.
  destruct input; simpl; rewrite IH; reflexivity.
Qed.

Theorem exact_threshold_qualifies : forall required,
  qualifies required [Credit required] = true.
Proof.
  intros required. unfold qualifies, run. simpl. apply Nat.leb_refl.
Qed.

Definition published (required : nat) (events : list event) (job_success : bool) :=
  job_success && qualifies required events.

Theorem failed_job_never_publishes : forall required events,
  published required events false = false.
Proof.
  reflexivity.
Qed.

Theorem publication_requires_success_and_duration : forall required events job_success,
  published required events job_success = true ->
  job_success = true /\ required <= credits events.
Proof.
  intros required events job_success H.
  apply andb_true_iff in H as [Hjob Hduration].
  split; auto. apply qualification_requires_full_duration. exact Hduration.
Qed.

Print Assumptions run_duration.
Print Assumptions invalid_is_absorbing.
Print Assumptions qualification_requires_full_duration.
Print Assumptions subthreshold_never_qualifies.
Print Assumptions invalidation_survives_any_suffix.
Print Assumptions idle_does_not_create_credit.
Print Assumptions exact_threshold_qualifies.
Print Assumptions failed_job_never_publishes.
Print Assumptions publication_requires_success_and_duration.
