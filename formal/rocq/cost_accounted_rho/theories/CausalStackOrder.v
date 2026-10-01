From Stdlib Require Import Lists.List.

Import ListNotations.

Section CausalStackOrder.

Context {cell : Type}.

Inductive consumes_in_order : list cell -> list cell -> list cell -> Prop :=
| consumes_in_order_nil :
    forall stack,
      consumes_in_order [] stack stack
| consumes_in_order_cons :
    forall head events stack tail,
      consumes_in_order events stack tail ->
      consumes_in_order (head :: events) (head :: stack) tail.

Theorem causal_consumption_is_deterministic :
  forall events stack left right,
    consumes_in_order events stack left ->
    consumes_in_order events stack right ->
    left = right.
Proof.
  intros events stack left right Hleft.
  revert right.
  induction Hleft; intros right Hright.
  - inversion Hright.
    reflexivity.
  - inversion Hright; subst.
    now apply IHHleft.
Qed.

Theorem every_causal_event_pops_one_head :
  forall events stack tail,
    consumes_in_order events stack tail ->
    length stack = length events + length tail.
Proof.
  intros events stack tail Hconsume.
  induction Hconsume.
  - reflexivity.
  - simpl.
    now rewrite IHHconsume.
Qed.

Theorem causal_pair_consumes_matching_stack :
  forall first second tail,
    consumes_in_order [first; second] (first :: second :: tail) tail.
Proof.
  intros first second tail.
  repeat constructor.
Qed.

Theorem reversed_pair_cannot_consume_causal_stack :
  forall first second tail result,
    first <> second ->
    ~ consumes_in_order [second; first] (first :: second :: tail) result.
Proof.
  intros first second tail result Hneq Hconsume.
  inversion Hconsume; subst.
  congruence.
Qed.

Theorem certified_causal_replay_matches_settlement :
  forall certificate stack settled replayed,
    consumes_in_order certificate stack settled ->
    consumes_in_order certificate stack replayed ->
    settled = replayed.
Proof.
  exact causal_consumption_is_deterministic.
Qed.

End CausalStackOrder.
