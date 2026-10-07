From Stdlib Require Import Bool.Bool Arith.PeanoNat Lists.List.
Import ListNotations.

Inductive replay_directive := RejectIntroduction | AcceptComm | RejectComm | Store.
Inductive replay_observation := Granted | PhloDenied | OtherError.
Inductive replay_result := ReplayMismatch | ReplayDenied | ReplayCommitted | ReplayStored.

Definition replay_directive_result directive introduction candidate binding comm :=
  match directive, introduction with
  | RejectIntroduction, PhloDenied => ReplayDenied
  | AcceptComm, Granted =>
      if candidate && binding then
        match comm with Granted => ReplayCommitted | _ => ReplayMismatch end
      else ReplayMismatch
  | RejectComm, Granted =>
      if candidate then
        match comm with PhloDenied => ReplayDenied | _ => ReplayMismatch end
      else ReplayMismatch
  | Store, Granted => if candidate then ReplayMismatch else ReplayStored
  | _, _ => ReplayMismatch
  end.

Theorem replay_commit_requires_all_checks : forall d i c b o,
  replay_directive_result d i c b o = ReplayCommitted <->
  d = AcceptComm /\ i = Granted /\ c = true /\ b = true /\ o = Granted.
Proof. intros [] [] [] [] []; simpl; intuition discriminate. Qed.

Theorem replay_denial_has_exact_stage : forall d i c b o,
  replay_directive_result d i c b o = ReplayDenied <->
  (d = RejectIntroduction /\ i = PhloDenied) \/
  (d = RejectComm /\ i = Granted /\ c = true /\ o = PhloDenied).
Proof. intros [] [] [] [] []; simpl; intuition discriminate. Qed.

Theorem replay_denied_intro_needs_no_candidate : forall c b o,
  replay_directive_result RejectIntroduction PhloDenied c b o = ReplayDenied.
Proof. reflexivity. Qed.

Theorem replay_missing_candidate_is_not_denial : forall i b o,
  replay_directive_result RejectComm i false b o = ReplayMismatch.
Proof. intros [] [] []; reflexivity. Qed.

Theorem replay_unexpected_grant_is_not_denial : forall c b,
  replay_directive_result RejectComm Granted c b Granted = ReplayMismatch.
Proof. intros [] []; reflexivity. Qed.

Theorem replay_host_error_is_not_denial : forall d i c b,
  d <> RejectIntroduction -> d <> Store ->
  replay_directive_result d i c b OtherError = ReplayMismatch.
Proof. intros [] [] [] [] H H0; simpl; congruence. Qed.

Theorem replay_store_requires_no_selected_match : forall d i c b o,
  replay_directive_result d i c b o = ReplayStored <->
  d = Store /\ i = Granted /\ c = false.
Proof. intros [] [] [] [] []; simpl; intuition discriminate. Qed.

Theorem replay_accepted_comm_needs_binding : forall i c o,
  replay_directive_result AcceptComm i c false o = ReplayMismatch.
Proof. intros [] [] []; reflexivity. Qed.

Theorem replay_denial_ignores_accepted_bindings : forall i c o,
  replay_directive_result RejectComm i c true o =
  replay_directive_result RejectComm i c false o.
Proof. reflexivity. Qed.

Record replay_store := {
  tuples : list nat;
  counters : list nat;
  bindings : list nat;
  events : list nat
}.

Definition replay_publish result (before after : replay_store) :=
  match result with
  | ReplayCommitted => after
  | ReplayStored => {| tuples := tuples after; counters := counters after;
                      bindings := bindings before; events := events after |}
  | _ => before
  end.

Theorem replay_store_preserves_comm_bindings : forall before after,
  bindings (replay_publish ReplayStored before after) = bindings before.
Proof. reflexivity. Qed.

Theorem replay_denial_preserves_store : forall before after,
  replay_publish ReplayDenied before after = before.
Proof. reflexivity. Qed.

Theorem replay_mismatch_preserves_store : forall before after,
  replay_publish ReplayMismatch before after = before.
Proof. reflexivity. Qed.

Definition replay_worker_update (stores : nat -> replay_store) worker result after :=
  fun key => if Nat.eqb key worker then replay_publish result (stores key) after else stores key.

Theorem replay_independent_workers_commute : forall stores x y rx ry sx sy,
  x <> y -> forall key,
  replay_worker_update (replay_worker_update stores x rx sx) y ry sy key =
  replay_worker_update (replay_worker_update stores y ry sy) x rx sx key.
Proof.
  intros stores x y rx ry sx sy distinct key. unfold replay_worker_update.
  destruct (key =? x) eqn:hx; destruct (key =? y) eqn:hy; simpl; auto.
  apply Nat.eqb_eq in hx, hy. subst. contradiction.
Qed.

Definition replay_exact_sources (actual expected : list nat) :=
  if list_eq_dec Nat.eq_dec actual expected then true else false.

Theorem replay_exact_sources_reflects_all_fields : forall actual expected,
  replay_exact_sources actual expected = true <-> actual = expected.
Proof.
  intros. unfold replay_exact_sources. destruct (list_eq_dec Nat.eq_dec actual expected);
    split; congruence.
Qed.
