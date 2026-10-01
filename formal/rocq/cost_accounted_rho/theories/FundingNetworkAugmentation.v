From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List ZArith Lia Ring.
From CostAccountedRho Require Import FundingResidualTransfer.
Import ListNotations.

Definition residual_network := nat -> nat * nat.
Definition residual_operation := (nat * bool)%type.

Definition replace_residual_pair (state : residual_network) index pair : residual_network :=
  fun other => if Nat.eqb other index then pair else state other.

Definition network_transfer limit (state : residual_network) (op : residual_operation) amount :=
  let '(index, reversed) := op in
  match directed_residual_transfer limit reversed amount (state index) with
  | Some updated_pair => Some (replace_residual_pair state index updated_pair)
  | None => None
  end.

Fixpoint network_augment limit state (operations : list residual_operation) amount :=
  match operations with
  | [] => Some state
  | op :: rest =>
      match network_transfer limit state op amount with
      | Some next => network_augment limit next rest amount
      | None => None
      end
  end.

Definition network_bounded count limit (state : residual_network) :=
  forall index, index < count -> fst (state index) + snd (state index) <= limit.

Definition operation_capacity (state : residual_network) (op : residual_operation) :=
  if snd op then snd (state (fst op)) else fst (state (fst op)).

Lemma network_transfer_preserves_pairs : forall limit state op amount next index,
  network_transfer limit state op amount = Some next ->
  fst (next index) + snd (next index) = fst (state index) + snd (state index).
Proof.
  intros limit state [selected reversed] amount next index.
  unfold network_transfer. destruct (directed_residual_transfer limit reversed amount (state selected)) as [pair|] eqn:step; [|discriminate].
  intros equal. inversion equal; subst. unfold replace_residual_pair.
  destruct (Nat.eqb index selected) eqn:same; [|reflexivity].
  apply Nat.eqb_eq in same. subst. eapply directed_transfer_conserves_pair. exact step.
Qed.

Lemma network_transfer_preserves_other_pairs : forall limit state selected reversed amount next index,
  network_transfer limit state (selected, reversed) amount = Some next ->
  index <> selected -> next index = state index.
Proof.
  intros limit state selected reversed amount next index.
  unfold network_transfer. destruct (directed_residual_transfer limit reversed amount (state selected)) as [pair|]; [|discriminate].
  intros equal different. inversion equal; subst. unfold replace_residual_pair.
  apply Nat.eqb_neq in different. now rewrite different.
Qed.

Theorem network_history_preserves_capacity : forall operations limit state amount next index,
  network_augment limit state operations amount = Some next ->
  fst (next index) + snd (next index) = fst (state index) + snd (state index).
Proof.
  induction operations as [|op rest IH]; intros limit state amount next index; simpl.
  - intros equal. inversion equal. reflexivity.
  - destruct (network_transfer limit state op amount) as [middle|] eqn:step; [|discriminate].
    intros history. rewrite (IH limit middle amount next index history).
    eapply network_transfer_preserves_pairs. exact step.
Qed.

Theorem network_history_preserves_bounds : forall count limit state operations amount next,
  network_bounded count limit state ->
  network_augment limit state operations amount = Some next ->
  network_bounded count limit next.
Proof.
  intros count limit state operations amount next bounded history index inside.
  rewrite (network_history_preserves_capacity operations limit state amount next index history).
  apply bounded. exact inside.
Qed.

Lemma bounded_directed_transfer_exists : forall limit (pair : nat * nat) (reversed : bool) amount,
  fst pair + snd pair <= limit ->
  amount <= (if reversed then snd pair else fst pair) ->
  exists next, directed_residual_transfer limit reversed amount pair = Some next.
Proof.
  intros limit [forward reverse] reversed amount bounded enough.
  destruct reversed; simpl in *.
  - assert (step : residual_transfer limit reverse forward amount = Some (reverse - amount, forward + amount)).
    { apply residual_transfer_exact. repeat split; lia. }
    unfold directed_residual_transfer. rewrite step. eauto.
  - exists (forward - amount, reverse + amount). apply residual_transfer_exact. repeat split; lia.
Qed.

Theorem distinct_pair_augmentation_succeeds : forall operations count limit state amount,
  NoDup (map fst operations) ->
  network_bounded count limit state ->
  (forall op, In op operations -> fst op < count /\ amount <= operation_capacity state op) ->
  exists next, network_augment limit state operations amount = Some next.
Proof.
  induction operations as [|[selected reversed] rest IH]; intros count limit state amount distinct bounded permitted.
  - exists state. reflexivity.
  - inversion distinct as [|? ? absent rest_distinct]; subst.
    destruct (permitted (selected, reversed) (or_introl eq_refl)) as [inside enough].
    destruct (bounded_directed_transfer_exists limit (state selected) reversed amount (bounded selected inside) enough) as [pair step].
    assert (transfer : network_transfer limit state (selected, reversed) amount =
      Some (replace_residual_pair state selected pair)) by (unfold network_transfer; now rewrite step).
    destruct (IH count limit (replace_residual_pair state selected pair) amount rest_distinct) as [next history].
    + intros index valid. rewrite (network_transfer_preserves_pairs limit state (selected, reversed) amount _ index transfer).
      apply bounded. exact valid.
    + intros [index direction] member. destruct (permitted (index, direction) (or_intror member)) as [valid enough_rest].
      split; [exact valid|]. unfold operation_capacity in *. simpl in *.
      assert (different : index <> selected).
      { intros equal. apply absent. apply in_map_iff. exists (index, direction). split; [exact equal|exact member]. }
      rewrite (network_transfer_preserves_other_pairs limit state selected reversed amount _ index transfer different).
      exact enough_rest.
    + exists next. change (match network_transfer limit state (selected, reversed) amount with
        | Some middle => network_augment limit middle rest amount | None => None end = Some next).
      now rewrite transfer.
Qed.

Fixpoint network_sum count (value : nat -> Z) : Z :=
  match count with 0 => 0%Z | S n => (network_sum n value + value n)%Z end.

Lemma network_sum_ext : forall count left right,
  (forall index, index < count -> left index = right index) ->
  network_sum count left = network_sum count right.
Proof.
  induction count; intros left right equal; simpl; [reflexivity|].
  rewrite (IHcount left right) by (intros; apply equal; lia).
  rewrite equal by lia. reflexivity.
Qed.

Lemma network_sum_add : forall count left right,
  network_sum count (fun index => (left index + right index)%Z) =
  (network_sum count left + network_sum count right)%Z.
Proof. induction count; intros; simpl; [ring|rewrite IHcount; ring]. Qed.

Lemma network_sum_zero : forall count, network_sum count (fun _ => 0%Z) = 0%Z.
Proof. induction count; simpl; lia. Qed.

Lemma network_sum_single : forall count selected value,
  selected < count ->
  network_sum count (fun index => if Nat.eqb index selected then value else 0%Z) = value.
Proof.
  induction count as [|count IH]; intros selected value inside; [lia|].
  simpl. destruct (Nat.eq_dec count selected) as [same|different].
  - subst selected. rewrite Nat.eqb_refl.
    assert (zero : network_sum count (fun index => if Nat.eqb index count then value else 0%Z) = 0%Z).
    { transitivity (network_sum count (fun _ => 0%Z)); [|apply network_sum_zero].
      apply network_sum_ext. intros index bound.
      assert (Nat.eqb index count = false) by (apply Nat.eqb_neq; lia). now rewrite H. }
    rewrite zero. ring.
  - assert (Nat.eqb count selected = false) by (apply Nat.eqb_neq; exact different).
    rewrite H, IH by lia. ring.
Qed.

Definition network_divergence count from to (state : residual_network) node : Z :=
  network_sum count (fun index =>
    (Z.of_nat (snd (state index)) * (vertex_indicator node (from index) - vertex_indicator node (to index)))%Z).

Definition operation_from (from to : nat -> nat) (op : residual_operation) :=
  if snd op then to (fst op) else from (fst op).

Definition operation_to (from to : nat -> nat) (op : residual_operation) :=
  if snd op then from (fst op) else to (fst op).

Lemma directed_transfer_changes_flow : forall limit reversed amount before after,
  directed_residual_transfer limit reversed amount before = Some after ->
  Z.of_nat (snd after) =
    (Z.of_nat (snd before) + if reversed then - Z.of_nat amount else Z.of_nat amount)%Z.
Proof.
  intros limit reversed amount [forward reverse] [next_forward next_reverse].
  destruct reversed; simpl.
  - destruct (residual_transfer limit reverse forward amount) as [[nr nf]|] eqn:step; [|discriminate].
    intros equal. inversion equal; subst. apply residual_transfer_exact in step. simpl. lia.
  - intros step. apply residual_transfer_exact in step. simpl. lia.
Qed.

Theorem network_transfer_changes_divergence : forall count limit from to state op amount next node,
  fst op < count -> network_transfer limit state op amount = Some next ->
  network_divergence count from to next node =
    (network_divergence count from to state node +
      residual_edge_boundary amount node (operation_from from to op) (operation_to from to op))%Z.
Proof.
  intros count limit from to state [selected reversed] amount next node inside.
  unfold network_transfer. destruct (directed_residual_transfer limit reversed amount (state selected)) as [pair|] eqn:step; [|discriminate].
  intros equal. inversion equal; subst next.
  pose proof (directed_transfer_changes_flow limit reversed amount (state selected) pair step) as flow.
  unfold network_divergence.
  transitivity (network_sum count (fun index =>
    (Z.of_nat (snd (state index)) * (vertex_indicator node (from index) - vertex_indicator node (to index)) +
      if Nat.eqb index selected then
        residual_edge_boundary amount node (operation_from from to (selected, reversed)) (operation_to from to (selected, reversed))
      else 0)%Z)).
  - apply network_sum_ext. intros index valid. unfold replace_residual_pair.
    destruct (Nat.eqb index selected) eqn:same.
    + apply Nat.eqb_eq in same. subst index. rewrite flow.
      unfold residual_edge_boundary, operation_from, operation_to. simpl.
      destruct reversed; ring.
    + ring.
  - rewrite network_sum_add, network_sum_single by exact inside. reflexivity.
Qed.

Fixpoint operation_boundary from to amount node (operations : list residual_operation) : Z :=
  match operations with
  | [] => 0%Z
  | op :: rest => (residual_edge_boundary amount node (operation_from from to op) (operation_to from to op) +
      operation_boundary from to amount node rest)%Z
  end.

Theorem network_history_changes_divergence : forall operations count limit from to state amount next node,
  (forall op, In op operations -> fst op < count) ->
  network_augment limit state operations amount = Some next ->
  network_divergence count from to next node =
    (network_divergence count from to state node + operation_boundary from to amount node operations)%Z.
Proof.
  induction operations as [|op rest IH]; intros count limit from to state amount next node inside; simpl.
  - intros equal. inversion equal; subst. ring.
  - destruct (network_transfer limit state op amount) as [middle|] eqn:step; [|discriminate].
    intros history. rewrite (IH count limit from to middle amount next node) by (auto; intros; apply inside; now right).
    rewrite (network_transfer_changes_divergence count limit from to state op amount middle node) by (auto; apply inside; now left).
    ring.
Qed.

Fixpoint linked_operations from to start (operations : list residual_operation) finish : Prop :=
  match operations with
  | [] => start = finish
  | op :: rest => operation_from from to op = start /\ linked_operations from to (operation_to from to op) rest finish
  end.

Theorem linked_operation_boundary : forall operations from to start finish amount node,
  linked_operations from to start operations finish ->
  operation_boundary from to amount node operations = residual_edge_boundary amount node start finish.
Proof.
  induction operations as [|op rest IH]; intros from to start finish amount node linked; simpl in *.
  - subst finish. unfold residual_edge_boundary. ring.
  - destruct linked as [begins tail]. rewrite (IH from to _ finish amount node tail), begins.
    unfold residual_edge_boundary. ring.
Qed.

Lemma operation_boundary_append : forall left right from to amount node,
  operation_boundary from to amount node (left ++ right) =
    (operation_boundary from to amount node left + operation_boundary from to amount node right)%Z.
Proof. induction left; intros; simpl; [ring|rewrite IHleft; ring]. Qed.

Theorem reverse_operations_preserve_boundary : forall operations from to amount node,
  operation_boundary from to amount node (rev operations) = operation_boundary from to amount node operations.
Proof.
  induction operations; intros; simpl; [reflexivity|].
  rewrite operation_boundary_append, IHoperations. simpl. ring.
Qed.

Theorem reverse_path_augmentation_conserves_internal_flow : forall operations count limit from to state amount next start finish node,
  linked_operations from to start operations finish ->
  (forall op, In op operations -> fst op < count) ->
  network_augment limit state (rev operations) amount = Some next ->
  node <> start -> node <> finish ->
  network_divergence count from to next node = network_divergence count from to state node.
Proof.
  intros operations count limit from to state amount next start finish node linked inside history not_start not_finish.
  rewrite (network_history_changes_divergence (rev operations) count limit from to state amount next node).
  - rewrite reverse_operations_preserve_boundary, (linked_operation_boundary operations from to start finish amount node linked).
    unfold residual_edge_boundary, vertex_indicator. apply Nat.eqb_neq in not_start. apply Nat.eqb_neq in not_finish.
    rewrite not_start, not_finish. ring.
  - intros op member. apply inside. now apply in_rev in member.
  - exact history.
Qed.

Theorem complete_reverse_path_augmentation : forall operations count limit from to state amount start finish,
  NoDup (map fst operations) ->
  network_bounded count limit state ->
  (forall op, In op operations -> fst op < count /\ amount <= operation_capacity state op) ->
  linked_operations from to start operations finish ->
  exists next,
    network_augment limit state (rev operations) amount = Some next /\
    network_bounded count limit next /\
    (forall index, fst (next index) + snd (next index) = fst (state index) + snd (state index)) /\
    (forall node, network_divergence count from to next node =
      (network_divergence count from to state node + residual_edge_boundary amount node start finish)%Z).
Proof.
  intros operations count limit from to state amount start finish distinct bounded permitted linked.
  assert (reverse_distinct : NoDup (map fst (rev operations))).
  { rewrite map_rev. apply NoDup_rev. exact distinct. }
  assert (reverse_permitted : forall op, In op (rev operations) -> fst op < count /\ amount <= operation_capacity state op).
  { intros op member. apply permitted. now apply in_rev in member. }
  destruct (distinct_pair_augmentation_succeeds (rev operations) count limit state amount reverse_distinct bounded reverse_permitted) as [next history].
  exists next. split; [exact history|]. split.
  - eapply network_history_preserves_bounds; eauto.
  - split.
    + intros index. eapply network_history_preserves_capacity. exact history.
    + intros node. rewrite (network_history_changes_divergence (rev operations) count limit from to state amount next node).
      * rewrite reverse_operations_preserve_boundary, (linked_operation_boundary operations from to start finish amount node linked). reflexivity.
      * intros op member. apply (proj1 (reverse_permitted op member)).
      * exact history.
Qed.

Print Assumptions complete_reverse_path_augmentation.
Print Assumptions network_history_preserves_capacity.
Print Assumptions network_history_preserves_bounds.
Print Assumptions distinct_pair_augmentation_succeeds.
Print Assumptions network_transfer_changes_divergence.
Print Assumptions network_history_changes_divergence.
Print Assumptions linked_operation_boundary.
Print Assumptions reverse_operations_preserve_boundary.
Print Assumptions reverse_path_augmentation_conserves_internal_flow.
