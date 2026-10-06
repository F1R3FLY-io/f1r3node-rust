From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List ZArith Lia Ring.
Import ListNotations.

Definition residual_transfer limit forward reverse amount : option (nat * nat) :=
  if (amount <=? forward) && (reverse + amount <=? limit)
  then Some (forward - amount, reverse + amount)
  else None.

Theorem residual_transfer_exact : forall limit forward reverse amount next_forward next_reverse,
  residual_transfer limit forward reverse amount = Some (next_forward, next_reverse) <->
  amount <= forward /\ reverse + amount <= limit /\
  next_forward + amount = forward /\ next_reverse = reverse + amount.
Proof.
  intros. unfold residual_transfer.
  destruct ((amount <=? forward) && (reverse + amount <=? limit)) eqn:checked.
  - apply andb_true_iff in checked. destruct checked as [enough bounded].
    apply Nat.leb_le in enough. apply Nat.leb_le in bounded.
    split.
    + intros equal. inversion equal; subst. repeat split; lia.
    + intros [_ [_ [remaining returned]]]. f_equal. f_equal; lia.
  - split; [discriminate|]. intros [enough [bounded _]].
    assert ((amount <=? forward) && (reverse + amount <=? limit) = true).
    { apply andb_true_iff. split; apply Nat.leb_le; assumption. }
    congruence.
Qed.

Theorem residual_transfer_conserves_pair : forall limit forward reverse amount next_forward next_reverse,
  residual_transfer limit forward reverse amount = Some (next_forward, next_reverse) ->
  next_forward + next_reverse = forward + reverse.
Proof. intros. apply residual_transfer_exact in H. lia. Qed.

Theorem valid_pair_transfer_fails_exactly_on_overdraw : forall limit forward reverse amount,
  forward + reverse <= limit ->
  residual_transfer limit forward reverse amount = None <-> forward < amount.
Proof.
  intros limit forward reverse amount bounded. unfold residual_transfer.
  destruct ((amount <=? forward) && (reverse + amount <=? limit)) eqn:checked.
  - apply andb_true_iff in checked. destruct checked as [enough _]. apply Nat.leb_le in enough.
    split; [discriminate|lia].
  - split; [intros _|intros _; reflexivity].
    apply andb_false_iff in checked. destruct checked as [failure|failure]; apply Nat.leb_gt in failure; lia.
Qed.

Definition directed_residual_transfer limit (reversed : bool) amount (pair : nat * nat) : option (nat * nat) :=
  let '(forward, reverse) := pair in
  if reversed then
    match residual_transfer limit reverse forward amount with
    | Some (next_reverse, next_forward) => Some (next_forward, next_reverse)
    | None => None
    end
  else residual_transfer limit forward reverse amount.

Theorem directed_transfer_conserves_pair : forall limit reversed amount before after,
  directed_residual_transfer limit reversed amount before = Some after ->
  fst after + snd after = fst before + snd before.
Proof.
  intros limit reversed amount [forward reverse] [next_forward next_reverse].
  destruct reversed; simpl.
  - destruct (residual_transfer limit reverse forward amount) as [[nr nf]|] eqn:step; [|discriminate].
    intros equal. inversion equal; subst.
    apply residual_transfer_conserves_pair in step. simpl. lia.
  - intros step. apply residual_transfer_conserves_pair in step. exact step.
Qed.

Fixpoint residual_transfer_history limit (operations : list (bool * nat)) pair :=
  match operations with
  | [] => Some pair
  | (reversed, amount) :: rest =>
      match directed_residual_transfer limit reversed amount pair with
      | Some next => residual_transfer_history limit rest next
      | None => None
      end
  end.

Theorem arbitrary_residual_history_conserves_pair : forall limit operations before after,
  residual_transfer_history limit operations before = Some after ->
  fst after + snd after = fst before + snd before.
Proof.
  intros limit operations. induction operations as [|[reversed amount] rest IH]; intros before after; simpl.
  - intros equal. inversion equal. reflexivity.
  - destruct (directed_residual_transfer limit reversed amount before) as [next|] eqn:step; [|discriminate].
    intros history. apply IH in history. apply directed_transfer_conserves_pair in step. lia.
Qed.

Theorem arbitrary_residual_history_preserves_machine_bound : forall limit operations before after,
  fst before + snd before <= limit ->
  residual_transfer_history limit operations before = Some after ->
  fst after <= limit /\ snd after <= limit.
Proof.
  intros. apply arbitrary_residual_history_conserves_pair in H0. lia.
Qed.

Theorem reverse_transfer_restores_pair : forall limit forward reverse amount,
  forward + reverse <= limit -> amount <= forward ->
  residual_transfer_history limit [(false, amount); (true, amount)] (forward, reverse) = Some (forward, reverse).
Proof.
  intros limit forward reverse amount bounded enough.
  assert (first : residual_transfer limit forward reverse amount = Some (forward - amount, reverse + amount)).
  { apply residual_transfer_exact. repeat split; lia. }
  assert (second : residual_transfer limit (reverse + amount) (forward - amount) amount = Some (reverse, forward)).
  { apply residual_transfer_exact. repeat split; lia. }
  cbn [residual_transfer_history directed_residual_transfer]. rewrite first.
  cbn [residual_transfer_history directed_residual_transfer]. rewrite second. reflexivity.
Qed.

Definition vertex_indicator (node vertex : nat) : Z := if Nat.eqb node vertex then 1%Z else 0%Z.

Definition residual_edge_boundary amount node from to : Z :=
  (Z.of_nat amount * (vertex_indicator node from - vertex_indicator node to))%Z.

Fixpoint residual_path_end current (tail : list nat) :=
  match tail with [] => current | next :: rest => residual_path_end next rest end.

Fixpoint residual_path_boundary amount node current (tail : list nat) : Z :=
  match tail with
  | [] => 0%Z
  | next :: rest => (residual_edge_boundary amount node current next + residual_path_boundary amount node next rest)%Z
  end.

Theorem arbitrary_path_boundary_telescopes : forall tail amount node start,
  residual_path_boundary amount node start tail =
  (Z.of_nat amount * (vertex_indicator node start - vertex_indicator node (residual_path_end start tail)))%Z.
Proof.
  induction tail as [|next rest IH]; intros; simpl; [ring|].
  rewrite IH. unfold residual_edge_boundary. ring.
Qed.

Theorem path_internal_vertices_conserve_flow : forall tail amount node start,
  node <> start -> node <> residual_path_end start tail ->
  residual_path_boundary amount node start tail = 0%Z.
Proof.
  intros. rewrite arbitrary_path_boundary_telescopes. unfold vertex_indicator.
  apply Nat.eqb_neq in H. apply Nat.eqb_neq in H0. rewrite H, H0. ring.
Qed.

Theorem path_source_gains_augmentation : forall tail amount start,
  start <> residual_path_end start tail ->
  residual_path_boundary amount start start tail = Z.of_nat amount.
Proof.
  intros. rewrite arbitrary_path_boundary_telescopes. unfold vertex_indicator.
  rewrite Nat.eqb_refl. apply Nat.eqb_neq in H. rewrite H. ring.
Qed.

Theorem path_sink_loses_augmentation : forall tail amount start,
  start <> residual_path_end start tail ->
  residual_path_boundary amount (residual_path_end start tail) start tail = (- Z.of_nat amount)%Z.
Proof.
  intros. rewrite arbitrary_path_boundary_telescopes. unfold vertex_indicator.
  rewrite Nat.eqb_refl. assert (Nat.eqb (residual_path_end start tail) start = false) by (apply Nat.eqb_neq; congruence).
  rewrite H0. ring.
Qed.

Print Assumptions residual_transfer_exact.
Print Assumptions valid_pair_transfer_fails_exactly_on_overdraw.
Print Assumptions arbitrary_residual_history_conserves_pair.
Print Assumptions arbitrary_residual_history_preserves_machine_bound.
Print Assumptions reverse_transfer_restores_pair.
Print Assumptions arbitrary_path_boundary_telescopes.
Print Assumptions path_internal_vertices_conserve_flow.
Print Assumptions path_source_gains_augmentation.
Print Assumptions path_sink_loses_augmentation.
