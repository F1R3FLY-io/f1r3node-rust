From Stdlib Require Import Arith Bool Lia List.
Import ListNotations.
Set Implicit Arguments.

Module DepthProbeLifecycle.

Record State := state {
  reaped : bool;
  eof : bool;
  failed : bool;
  timed_out : bool;
  cleanup_started : option nat
}.

Definition initial := state false false false false None.

Definition can_read s := negb (eof s || failed s).

Definition first_error {E : Type} (prior next : option E) :=
  match prior with Some error => Some error | None => next end.

Theorem first_error_preserved : forall E (error : E) next,
  first_error (Some error) next = Some error.
Proof. reflexivity. Qed.

Theorem failed_capture_disables_reads : forall s,
  failed s = true -> can_read s = false.
Proof. intros; unfold can_read; rewrite H, orb_true_r; reflexivity. Qed.

Theorem eof_disables_reads : forall s,
  eof s = true -> can_read s = false.
Proof. intros; unfold can_read; rewrite H; reflexivity. Qed.

Definition running s :=
  match cleanup_started s with None => true | Some _ => false end.

Definition observe deadline now exited closed error s :=
  let r := reaped s || exited in
  let e := eof s || closed in
  let f := failed s || error in
  let t := timed_out s || (negb r && running s && (deadline <=? now)) in
  let c := match cleanup_started s with
           | Some start => Some start
           | None => if r || f || t then Some now else None
           end in
  state r e f t c.

Inductive Decision := Poll | Classify (timeout : bool) | HarnessError.

Definition decide grace now s :=
  if reaped s && (eof s || failed s) then
    if failed s then HarnessError else Classify (timed_out s)
  else match cleanup_started s with
       | Some start => if grace <=? now - start then HarnessError else Poll
       | None => Poll
       end.

Theorem initial_polls : forall grace now, decide grace now initial = Poll.
Proof. reflexivity. Qed.

Theorem reaped_is_sticky : forall d n x c e s,
  reaped s = true -> reaped (observe d n x c e s) = true.
Proof. intros; cbn; rewrite H; reflexivity. Qed.

Theorem eof_is_sticky : forall d n x c e s,
  eof s = true -> eof (observe d n x c e s) = true.
Proof. intros; cbn; rewrite H; reflexivity. Qed.

Theorem failure_is_sticky : forall d n x c e s,
  failed s = true -> failed (observe d n x c e s) = true.
Proof. intros; cbn; rewrite H; reflexivity. Qed.

Theorem timeout_is_sticky : forall d n x c e s,
  timed_out s = true -> timed_out (observe d n x c e s) = true.
Proof. intros; cbn; rewrite H; reflexivity. Qed.

Theorem cleanup_is_never_renewed : forall d n x c e s start,
  cleanup_started s = Some start ->
  cleanup_started (observe d n x c e s) = Some start.
Proof. intros; cbn; rewrite H; reflexivity. Qed.

Theorem exit_starts_cleanup : forall d n c e s,
  cleanup_started s = None ->
  cleanup_started (observe d n true c e s) = Some n.
Proof. intros; cbn; rewrite H, orb_true_r; reflexivity. Qed.

Theorem failure_starts_cleanup : forall d n x c s,
  cleanup_started s = None ->
  cleanup_started (observe d n x c true s) = Some n.
Proof. intros; cbn; rewrite H, orb_true_r, orb_true_r; reflexivity. Qed.

Theorem live_deadline_latches_timeout : forall d n c s,
  reaped s = false -> cleanup_started s = None -> d <= n ->
  timed_out (observe d n false c false s) = true.
Proof.
  intros; cbn; unfold running; rewrite H, H0; cbn.
  rewrite (proj2 (Nat.leb_le d n) H1), orb_true_r; reflexivity.
Qed.

Theorem live_deadline_starts_cleanup : forall d n c s,
  reaped s = false -> cleanup_started s = None -> d <= n ->
  cleanup_started (observe d n false c false s) = Some n.
Proof.
  intros; unfold observe, running; rewrite H, H0; cbn.
  rewrite (proj2 (Nat.leb_le d n) H1), !orb_true_r; reflexivity.
Qed.

Theorem observed_exit_precedes_timeout : forall d n c e s,
  timed_out s = false -> timed_out (observe d n true c e s) = false.
Proof. intros; cbn; rewrite H, orb_true_r; reflexivity. Qed.

Theorem classify_requires_complete_capture : forall grace n s timeout,
  decide grace n s = Classify timeout ->
  reaped s = true /\ eof s = true /\ failed s = false /\ timed_out s = timeout.
Proof.
  intros grace n [r e f t c] timeout H; unfold decide in H; cbn in *.
  destruct r, e, f; cbn in H; try (inversion H; auto).
  all: destruct c as [start|]; try discriminate;
    destruct (grace <=? n - start); discriminate.
Qed.

Theorem complete_capture_classifies : forall grace n s,
  reaped s = true -> eof s = true -> failed s = false ->
  decide grace n s = Classify (timed_out s).
Proof. intros; unfold decide; rewrite H, H0, H1; reflexivity. Qed.

Theorem reaped_error_needs_no_eof : forall grace n s,
  reaped s = true -> failed s = true -> decide grace n s = HarnessError.
Proof. intros; unfold decide; rewrite H, H0, orb_true_r; reflexivity. Qed.

Theorem eof_alone_cannot_classify : forall grace n s timeout,
  reaped s = false -> decide grace n s <> Classify timeout.
Proof.
  intros grace n s timeout H Hbad.
  apply classify_requires_complete_capture in Hbad; intuition congruence.
Qed.

Theorem expired_cleanup_cannot_poll : forall grace n s start,
  cleanup_started s = Some start -> start + grace <= n -> decide grace n s <> Poll.
Proof.
  intros; unfold decide; rewrite H.
  destruct (reaped s && (eof s || failed s)); [destruct (failed s); discriminate|].
  assert (grace <=? n - start = true) by (apply Nat.leb_le; lia).
  rewrite H1; discriminate.
Qed.

Theorem failed_capture_never_classifies : forall grace n s timeout,
  failed s = true -> decide grace n s <> Classify timeout.
Proof.
  intros grace n s timeout H Hbad.
  apply classify_requires_complete_capture in Hbad; intuition congruence.
Qed.

Theorem inherited_writer_expiry_is_error : forall grace n s start,
  eof s = false -> failed s = false -> cleanup_started s = Some start ->
  start + grace <= n -> decide grace n s = HarnessError.
Proof.
  intros; unfold decide; rewrite H, H0, H1, andb_false_r; cbn.
  assert (grace <=? n - start = true) by (apply Nat.leb_le; lia).
  rewrite H3; reflexivity.
Qed.

Definition retain {A : Type} limit (bytes : list A) :=
  skipn (length bytes - limit) bytes.

Theorem retained_length : forall A limit (bytes : list A),
  length (retain limit bytes) = Nat.min limit (length bytes).
Proof. intros; unfold retain; rewrite length_skipn; lia. Qed.

Theorem retained_suffix : forall A limit (bytes : list A),
  exists prefix, bytes = prefix ++ retain limit bytes.
Proof.
  intros; exists (firstn (length bytes - limit) bytes).
  symmetry; apply firstn_skipn.
Qed.

Theorem short_capture_is_unchanged : forall A limit (bytes : list A),
  length bytes <= limit -> retain limit bytes = bytes.
Proof. intros; unfold retain; replace (length bytes - limit) with 0 by lia; reflexivity. Qed.

Theorem retain_is_idempotent : forall A limit (bytes : list A),
  retain limit (retain limit bytes) = retain limit bytes.
Proof. intros; apply short_capture_is_unchanged; rewrite retained_length; lia. Qed.

Theorem retention_is_chunk_independent : forall A limit (prefix suffix : list A),
  retain limit (retain limit prefix ++ suffix) = retain limit (prefix ++ suffix).
Proof.
  intros; unfold retain; rewrite !length_app, !length_skipn.
  rewrite !skipn_app, skipn_skipn.
  f_equal; f_equal; rewrite ?length_skipn; lia.
Qed.

Inductive ReadEvent := Bytes | Interrupted | Pending | Closed | ReadError.

Fixpoint attempts quota events :=
  match quota, events with
  | S rest, (Bytes | Interrupted) :: tail => S (attempts rest tail)
  | S _, _ :: _ => 1
  | _, _ => 0
  end.

Theorem attempts_bounded : forall quota events, attempts quota events <= quota.
Proof.
  induction quota; intros [|event events]; cbn; try lia.
  destruct event; cbn; specialize (IHquota events); lia.
Qed.

Theorem interruptions_consume_quota : forall quota events,
  attempts (S quota) (Interrupted :: events) = S (attempts quota events).
Proof. reflexivity. Qed.

Theorem pending_stops_quantum : forall quota events,
  attempts (S quota) (Pending :: events) = 1.
Proof. reflexivity. Qed.

Print Assumptions initial_polls.
Print Assumptions first_error_preserved.
Print Assumptions failed_capture_disables_reads.
Print Assumptions eof_disables_reads.
Print Assumptions reaped_is_sticky.
Print Assumptions eof_is_sticky.
Print Assumptions failure_is_sticky.
Print Assumptions timeout_is_sticky.
Print Assumptions cleanup_is_never_renewed.
Print Assumptions exit_starts_cleanup.
Print Assumptions failure_starts_cleanup.
Print Assumptions live_deadline_latches_timeout.
Print Assumptions live_deadline_starts_cleanup.
Print Assumptions observed_exit_precedes_timeout.
Print Assumptions classify_requires_complete_capture.
Print Assumptions complete_capture_classifies.
Print Assumptions reaped_error_needs_no_eof.
Print Assumptions eof_alone_cannot_classify.
Print Assumptions expired_cleanup_cannot_poll.
Print Assumptions failed_capture_never_classifies.
Print Assumptions inherited_writer_expiry_is_error.
Print Assumptions retained_length.
Print Assumptions retained_suffix.
Print Assumptions short_capture_is_unchanged.
Print Assumptions retain_is_idempotent.
Print Assumptions retention_is_chunk_independent.
Print Assumptions attempts_bounded.
Print Assumptions interruptions_consume_quota.
Print Assumptions pending_stops_quantum.

End DepthProbeLifecycle.
