From Coq Require Import List.
From NodeObservation Require Import ObserverSession.
From NodeObservation Require Import BoundedCapture.

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

Theorem observer_cross_incarnation_distinct : forall a b left right,
  a <> b -> qualified_matches (qualify a left) (qualify b right) = false.
Proof. exact distinct_incarnations_never_match. Qed.

Theorem observer_qualified_replay_refused : forall incarnation events nonce old,
  In old (issued (run initial events)) ->
  qualified_matches (qualify incarnation old)
    (qualify incarnation (nonce, S (sequence (run initial events)))) = false.
Proof. exact qualified_replay_refused. Qed.

Theorem capture_no_interference : forall c o,
  monotone c -> well_ordered o -> observed_consistent c o ->
  forall e, In e (participants o) ->
  forall t, before_at o <= t -> t < validate_at o -> ~ commit_at c t e.
Proof. exact accepted_capture_no_interference. Qed.

Theorem capture_generation_stable : forall g, generation_monotone g ->
  forall t1 t2, t1 <= t2 -> g t1 = g t2 ->
  forall t, t1 <= t -> t < t2 -> ~ insert_at g t.
Proof. exact stable_generation_no_insert. Qed.

Theorem capture_budget_bounded : forall amounts limit used final,
  used <= limit -> charge_all limit used amounts = Some final ->
  final <= limit /\ final = used + total amounts.
Proof. exact charge_all_bounded. Qed.

Theorem capture_overflow_fails_limit : forall bound limit used amount,
  limit <= bound -> checked_add bound used amount = None -> charge limit used amount = None.
Proof. exact overflow_implies_limit_failure. Qed.

Theorem capture_prefix_roundtrip : forall payload, decode (encode payload) = Some payload.
Proof. exact decode_encode. Qed.

Theorem capture_prefix_sound : forall raw payload,
  decode raw = Some payload -> raw = encode payload.
Proof. exact decode_sound. Qed.

Theorem capture_guard_order : forall actions final,
  capture_run initial_capture actions = Some final -> guard_order final.
Proof. exact capture_reachable_guard_order. Qed.

Theorem capture_detached : forall actions final,
  capture_run initial_capture actions = Some final -> detached final.
Proof. exact capture_reachable_detached. Qed.
