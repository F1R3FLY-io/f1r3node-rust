From Stdlib Require Import Lists.List Bool.Bool.
From FinalizedFloor Require Import RecoveryPumpControl.
Import ListNotations.

Definition merge_demand (left right : demand) : demand :=
  match left, right with
  | Stopped, _ | _, Stopped => Stopped
  | Idle, other | other, Idle => other
  | Work a, Work b => Work (a || b)
  end.

Definition handoff (local shared : demand) : demand * demand :=
  (merge_demand local (fst (take_demand shared)), snd (take_demand shared)).

Theorem merge_idle_left : forall state, merge_demand Idle state = state.
Proof. intros [|[]|]; reflexivity. Qed.

Theorem merge_idle_right : forall state, merge_demand state Idle = state.
Proof. intros [|[]|]; reflexivity. Qed.

Theorem merge_commutes : forall left right,
  merge_demand left right = merge_demand right left.
Proof. intros [|[]|] [|[]|]; reflexivity. Qed.

Theorem merge_associates : forall a b c,
  merge_demand (merge_demand a b) c = merge_demand a (merge_demand b c).
Proof. intros [|[]|] [|[]|] [|[]|]; reflexivity. Qed.

Theorem merge_idempotent : forall state, merge_demand state state = state.
Proof. intros [|[]|]; reflexivity. Qed.

Theorem merge_stop_is_absorbing : forall state,
  merge_demand state Stopped = Stopped /\ merge_demand Stopped state = Stopped.
Proof. intros [|[]|]; split; reflexivity. Qed.

Theorem handoff_preserves_total_demand : forall local shared,
  merge_demand (fst (handoff local shared)) (snd (handoff local shared)) =
  merge_demand local shared.
Proof. intros [|[]|] [|[]|]; reflexivity. Qed.

Theorem handoff_releases_shared_demand : forall local shared,
  snd (handoff local shared) = Idle \/ snd (handoff local shared) = Stopped.
Proof. intros [|[]|] [|[]|]; simpl; auto. Qed.

Theorem handoff_retains_prior_proposal : forall incoming,
  fst (handoff (Work true) incoming) =
  match incoming with Stopped => Stopped | _ => Work true end.
Proof. intros [|[]|]; reflexivity. Qed.

Theorem arbitrary_handoffs_preserve_merge : forall incoming local,
  fold_left (fun state value => fst (handoff state value)) incoming local =
  fold_left merge_demand incoming local.
Proof.
  induction incoming as [|value rest IH]; intros local; simpl; auto.
  rewrite IH. destruct value; reflexivity.
Qed.

Print Assumptions merge_idle_left.
Print Assumptions merge_idle_right.
Print Assumptions merge_commutes.
Print Assumptions merge_associates.
Print Assumptions merge_idempotent.
Print Assumptions merge_stop_is_absorbing.
Print Assumptions handoff_preserves_total_demand.
Print Assumptions handoff_releases_shared_demand.
Print Assumptions handoff_retains_prior_proposal.
Print Assumptions arbitrary_handoffs_preserve_merge.
