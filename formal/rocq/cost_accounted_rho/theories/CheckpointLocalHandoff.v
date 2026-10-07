From Stdlib Require Import Lists.List.
Import ListNotations.

Section LocalHandoff.
Context {Local Root Reader Trace : Type}.
Variable finish : Local -> Reader -> Local.
Variable trace : Local -> Trace.

Definition checkpoint_handoff (before : Local) (durable : Root) (prepared : option Reader) :=
  match prepared with
  | None => (before, durable, None)
  | Some reader => (finish before reader, durable, Some (trace before))
  end.

Theorem failed_handoff_preserves_complete_local_state : forall before durable,
  checkpoint_handoff before durable None = (before, durable, None).
Proof. reflexivity. Qed.

Theorem failed_handoff_preserves_every_local_projection : forall (Observation : Type)
    (observe : Local -> Observation) before durable after published output,
  checkpoint_handoff before durable None = (after, published, output) ->
  observe after = observe before.
Proof. intros. inversion H. reflexivity. Qed.

Theorem successful_handoff_returns_original_trace : forall before durable reader after published output,
  checkpoint_handoff before durable (Some reader) = (after, published, output) ->
  after = finish before reader /\ published = durable /\ output = Some (trace before).
Proof. intros. inversion H. auto. Qed.

Fixpoint failed_prefix (before : Local) (roots : list Root) : Local :=
  match roots with
  | [] => before
  | root :: rest =>
      let '(after, _, _) := checkpoint_handoff before root None in failed_prefix after rest
  end.

Theorem arbitrary_failed_prefix_preserves_local_state : forall roots before,
  failed_prefix before roots = before.
Proof. induction roots; intros; simpl; auto. Qed.

Theorem retry_after_any_failed_prefix_keeps_trace : forall roots before durable reader,
  checkpoint_handoff (failed_prefix before roots) durable (Some reader) =
  (finish before reader, durable, Some (trace before)).
Proof. intros. rewrite arbitrary_failed_prefix_preserves_local_state. reflexivity. Qed.

Definition commit_left (reader : Reader) (states : Local * Local) :=
  (finish (fst states) reader, snd states).
Definition commit_right (reader : Reader) (states : Local * Local) :=
  (fst states, finish (snd states) reader).

Theorem independent_local_handoffs_commute : forall left right states,
  commit_left left (commit_right right states) = commit_right right (commit_left left states).
Proof. intros left right [first second]. reflexivity. Qed.
End LocalHandoff.

Example early_trace_drain_breaks_retry :
  let finish := fun (_ : list nat) (_ : unit) => [] in
  let trace := fun values : list nat => values in
  checkpoint_handoff finish trace [] 1 (Some tt) <>
    checkpoint_handoff finish trace [7; 11] 1 (Some tt).
Proof. discriminate. Qed.
