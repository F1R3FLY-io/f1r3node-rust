From Coq Require Import List Arith Lia Bool.
Import ListNotations.

Record budget := {
  used : nat;
  failed : bool
}.

Definition charge (limit : nat) (b : budget) (amount : nat) : budget :=
  if failed b then b
  else if used b + amount <=? limit
  then {| used := used b + amount; failed := false |}
  else {| used := used b; failed := true |}.

Definition charge_all (limit : nat) (b : budget) (amounts : list nat) : budget :=
  fold_left (charge limit) amounts b.

Lemma charge_bounded : forall limit b amount,
  used b <= limit -> used (charge limit b amount) <= limit.
Proof.
  intros limit b amount Hb. unfold charge.
  destruct (failed b); [exact Hb |].
  destruct (used b + amount <=? limit) eqn:H; simpl.
  - apply Nat.leb_le in H. exact H.
  - exact Hb.
Qed.

Theorem shared_budget_bounded : forall limit amounts b,
  used b <= limit -> used (charge_all limit b amounts) <= limit.
Proof.
  intros limit amounts. induction amounts as [| a rest IH]; intros b Hb; simpl.
  - exact Hb.
  - apply IH. apply charge_bounded. exact Hb.
Qed.

Lemma charge_failed : forall limit b amount,
  failed b = true -> charge limit b amount = b.
Proof.
  intros limit b amount Hf. unfold charge. rewrite Hf. reflexivity.
Qed.

Theorem budget_failure_sticky : forall limit amounts b,
  failed b = true -> charge_all limit b amounts = b.
Proof.
  intros limit amounts. induction amounts as [| a rest IH]; intros b Hf; simpl.
  - reflexivity.
  - rewrite (charge_failed limit b a Hf). apply IH. exact Hf.
Qed.

Theorem budget_failure_keeps_usage : forall limit b amount,
  failed b = false -> failed (charge limit b amount) = true ->
  used (charge limit b amount) = used b /\ limit < used b + amount.
Proof.
  intros limit b amount Hok Hfail. unfold charge in *. rewrite Hok in *.
  destruct (used b + amount <=? limit) eqn:H; simpl in *.
  - discriminate Hfail.
  - apply Nat.leb_gt in H. split; [reflexivity | exact H].
Qed.

Theorem paths_share_one_budget : forall limit b xs ys,
  charge_all limit b (xs ++ ys) = charge_all limit (charge_all limit b xs) ys.
Proof.
  intros limit b xs ys. unfold charge_all. apply fold_left_app.
Qed.

Definition checked_add (bound used amount : nat) : option nat :=
  if used + amount <=? bound then Some (used + amount) else None.

Theorem checked_overflow_is_limit_failure : forall bound limit b amount,
  limit <= bound -> failed b = false ->
  checked_add bound (used b) amount = None -> failed (charge limit b amount) = true.
Proof.
  intros bound limit b amount Hle Hok Hover. unfold checked_add in Hover. unfold charge.
  rewrite Hok.
  destruct (used b + amount <=? bound) eqn:Hb; [discriminate |].
  apply Nat.leb_gt in Hb.
  destruct (used b + amount <=? limit) eqn:Hl; simpl.
  - apply Nat.leb_le in Hl. lia.
  - reflexivity.
Qed.

Definition compare {A R : Type} (digest : A -> nat) (decide : A -> A -> R) (x y : A) : option R :=
  if digest x =? digest y then Some (decide x y) else None.

Theorem comparison_requires_equal_digest : forall (A R : Type) (digest : A -> nat)
  (decide : A -> A -> R) (x y : A) (r : R),
  compare digest decide x y = Some r -> digest x = digest y.
Proof.
  intros A R digest decide x y r H. unfold compare in H.
  destruct (digest x =? digest y) eqn:Heq; [| discriminate].
  apply Nat.eqb_eq in Heq. exact Heq.
Qed.
