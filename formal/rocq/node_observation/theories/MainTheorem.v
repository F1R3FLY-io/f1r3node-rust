From Coq Require Import List.
Import ListNotations.
From NodeObservation Require Import ObserverSession.
From NodeObservation Require Import BoundedCapture InterfaceSafety CaptureIntegrity.

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

Theorem observer_path_admission : forall uid parents leaf,
  safe_path uid parents leaf = true ->
  Forall (fun c => directory c = true /\ (owner c = 0 \/ owner c = uid) /\
    (writable c = false \/ (owner c = 0 /\ sticky c = true))) (parents ++ [leaf]) /\
  owner leaf = uid /\ permissions leaf = 448.
Proof. exact InterfaceSafety.path_admission_sound. Qed.

Theorem observer_peer_admission : forall uid pid start actual_uid actual_pid actual_start,
  peer_admitted uid pid start actual_uid actual_pid actual_start = true ->
  actual_uid = uid /\ actual_pid = Some pid /\ actual_start = start.
Proof. exact InterfaceSafety.peer_admission_sound. Qed.

Theorem observer_cleanup_preserves_replacement : forall dev ino e,
  socket e = false \/ device e <> dev \/ inode e <> ino ->
  cleanup dev ino (Some e) = Some e.
Proof. exact InterfaceSafety.cleanup_preserves_replacement. Qed.

Theorem observer_shutdown_before_exit : forall actions cleaned,
  shutdown_run (Running, false) actions = Some (Exited, cleaned) -> cleaned = true.
Proof. exact InterfaceSafety.shutdown_before_exit. Qed.

Theorem observer_write_before_deadline : forall deadline now,
  write_admitted deadline now = true -> now < deadline.
Proof. exact InterfaceSafety.write_before_deadline. Qed.

Theorem capture_lock_deadline : forall deadline attempts,
  forallb (fun a => lock_admitted deadline (fst a) (snd a)) attempts = true ->
  forall now, In (now, true) attempts -> now < deadline.
Proof. exact InterfaceSafety.lock_sequence_uses_one_deadline. Qed.

Theorem capture_complete_metadata : forall held requested metadata body,
  rows_complete held requested metadata body = true ->
  forall h, In h held -> metadata h = true.
Proof. exact CaptureIntegrity.complete_metadata. Qed.

Theorem capture_complete_requested_bodies : forall held requested metadata body,
  rows_complete held requested metadata body = true ->
  forall h, In h held -> In h requested -> body h = true.
Proof. exact CaptureIntegrity.complete_requested_bodies. Qed.

Theorem capture_canonical_record_injective : forall left right,
  canonical_record left = canonical_record right -> left = right.
Proof. exact CaptureIntegrity.canonical_record_injective. Qed.

Theorem capture_scratch_preserves_production : forall rows h next address,
  addresses_fresh next rows -> address < next -> populate h rows address = h address.
Proof. exact CaptureIntegrity.fresh_scratch_preserves_production. Qed.

Theorem capture_scratch_preserves_sibling : forall h first second value,
  first <> second -> put h first value second = h second.
Proof. exact CaptureIntegrity.scratch_writes_preserve_sibling. Qed.
