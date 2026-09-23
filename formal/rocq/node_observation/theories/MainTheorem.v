From Coq Require Import List.
From NodeObservation Require Import ObserverSession.

Theorem observer_challenges_unique : forall events,
  NoDup (issued (run initial events)).
Proof.
  intros events. apply (proj1 (run_valid events initial initial_valid)).
Qed.

Theorem observer_replay_refused : forall events nonce old,
  In old (issued (run initial events)) ->
  matches old (nonce, S (sequence (run initial events))) = false.
Proof.
  intros events nonce old Hin.
  apply stale_request_refused; [apply run_valid; apply initial_valid | exact Hin].
Qed.

Theorem observer_counter_exhaustion_refused : forall maximum s nonce,
  maximum <= sequence s -> checked_allocate maximum s nonce = None.
Proof. exact counter_exhaustion_refused. Qed.

Theorem observer_checked_allocation_valid : forall maximum s nonce next,
  valid s -> checked_allocate maximum s nonce = Some next -> valid next.
Proof. exact checked_allocation_valid. Qed.
