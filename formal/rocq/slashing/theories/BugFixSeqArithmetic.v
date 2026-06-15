From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.

Set Implicit Arguments.

Definition checked_pred (n : nat) : option nat :=
  match n with
  | 0 => None
  | S p => Some p
  end.

Definition checked_succ_bounded (max : nat) (n : nat) : option nat :=
  if Nat.ltb n max then Some (S n) else None.

Theorem checked_pred_total_positive :
  forall n,
    n > 0 ->
    checked_pred n = Some (n - 1).
Proof.
  intros n Hpos.
  destruct n as [| p].
  - lia.
  - simpl. f_equal. lia.
Qed.

Theorem checked_pred_zero_none :
  checked_pred 0 = None.
Proof. reflexivity. Qed.

Theorem checked_succ_bounded_sound :
  forall max n m,
    checked_succ_bounded max n = Some m ->
    m = S n /\ n < max.
Proof.
  intros max n m H.
  unfold checked_succ_bounded in H.
  destruct (Nat.ltb n max) eqn:Hlt; [|discriminate].
  inversion H. subst m.
  split; [reflexivity|].
  apply Nat.ltb_lt. assumption.
Qed.

Theorem checked_succ_bounded_overflow_none :
  forall max n,
    max <= n ->
    checked_succ_bounded max n = None.
Proof.
  intros max n H.
  unfold checked_succ_bounded.
  apply Nat.ltb_ge in H.
  rewrite H. reflexivity.
Qed.
