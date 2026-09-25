From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Definition tree_nodes_bound (entries : nat) : nat :=
  match entries with 0 => 0 | S tail => 1 + tail / 5 end.

Lemma occupancy_sum : forall occupancies,
  Forall (fun count => 5 <= count) occupancies ->
  5 * length occupancies <= fold_right Nat.add 0 occupancies.
Proof.
  intros occupancies occupied. induction occupied; simpl; lia.
Qed.

Theorem checkpoint_node_count : forall root occupancies,
  1 <= root -> Forall (fun count => 5 <= count) occupancies ->
  S (length occupancies) <=
    tree_nodes_bound (root + fold_right Nat.add 0 occupancies).
Proof.
  intros root occupancies root_live occupied.
  pose proof (occupancy_sum occupancies occupied) as total.
  destruct root as [|root]; [lia|].
  change (S (length occupancies) <=
    S ((root + fold_right Nat.add 0 occupancies) / 5)).
  assert (length occupancies <= (root + fold_right Nat.add 0 occupancies) / 5).
  { apply Nat.div_le_lower_bound; lia. }
  lia.
Qed.

Definition node_layout_bound (key value pointer alignment : nat) : nat :=
  11 * (key + value) + 13 * pointer + 4 + 8 * alignment.

Theorem checkpoint_layout_padding : forall key value pointer alignment padding,
  padding <= 8 * alignment ->
  11 * key + 11 * value + 13 * pointer + 4 + padding <=
    node_layout_bound key value pointer alignment.
Proof. intros. unfold node_layout_bound. lia. Qed.

Lemma allocation_sum : forall allocations maximum,
  Forall (fun bytes => bytes <= maximum) allocations ->
  fold_right Nat.add 0 allocations <= length allocations * maximum.
Proof.
  intros allocations maximum bounded. induction bounded; simpl; nia.
Qed.

Theorem checkpoint_tree_backing : forall entries allocations maximum,
  length allocations <= tree_nodes_bound entries ->
  Forall (fun bytes => bytes <= maximum) allocations ->
  fold_right Nat.add 0 allocations <= tree_nodes_bound entries * maximum.
Proof.
  intros entries allocations maximum nodes bounded.
  pose proof (allocation_sum allocations maximum bounded). nia.
Qed.

Theorem checkpoint_payload_backing : forall nodes payload node_budget payload_budget,
  nodes <= node_budget -> payload <= payload_budget ->
  nodes + payload <= node_budget + payload_budget.
Proof. intros. lia. Qed.

Theorem checkpoint_clone_drop_work : forall clone_visits drop_visits entries nodes,
  clone_visits <= entries + nodes -> drop_visits <= entries + nodes ->
  clone_visits + drop_visits <= 2 * (entries + nodes).
Proof. intros. lia. Qed.

Theorem checkpoint_overflow_rejection : forall requested limit available,
  limit < requested -> available <= limit -> ~ requested <= available.
Proof. intros. lia. Qed.

Example entry_only_bound_is_insufficient :
  ~ node_layout_bound 8 4 8 8 <= 8 + 4.
Proof. unfold node_layout_bound. lia. Qed.

Theorem clone_field_bounds_compose : forall actual reserved,
  Forall2 Nat.le actual reserved ->
  fold_right Nat.add 0 actual <= fold_right Nat.add 0 reserved.
Proof. intros actual reserved bounded. induction bounded; simpl; lia. Qed.

Theorem clone_requires_complete_reservation : forall actual reserved used limit,
  Forall2 Nat.le actual reserved ->
  used + fold_right Nat.add 0 reserved <= limit ->
  used + fold_right Nat.add 0 actual <= limit.
Proof.
  intros actual reserved used limit bounded fits.
  pose proof (clone_field_bounds_compose _ _ bounded). lia.
Qed.

Theorem preparation_prepays_publication_or_cancellation :
  forall preparation publication cancellation prepaid,
  preparation + publication + cancellation <= prepaid ->
  preparation + publication <= prepaid /\
  preparation + cancellation <= prepaid.
Proof. intros. lia. Qed.

Theorem rejected_clone_preserves_source : forall (State : Type) (source : State)
  used requested limit,
  limit < used + requested ->
  (if Nat.leb (used + requested) limit then None else Some source) = Some source.
Proof.
  intros State source used requested limit rejected.
  destruct (Nat.leb (used + requested) limit) eqn:fits; [|reflexivity].
  apply Nat.leb_le in fits. lia.
Qed.

Definition copied_capacity (paid rows : nat) : nat := Nat.min paid rows.

Theorem copied_capacity_never_exceeds_backing : forall paid rows,
  copied_capacity paid rows <= paid /\ copied_capacity paid rows <= rows.
Proof. intros. split; [apply Nat.le_min_l|apply Nat.le_min_r]. Qed.

Theorem restored_growth_requires_reservation : forall paid rows additional,
  0 < additional -> copied_capacity paid rows < rows + additional.
Proof. intros. pose proof (Nat.le_min_r paid rows). unfold copied_capacity. lia. Qed.

Example copying_unused_capacity_bypasses_growth_payment :
  1 + 1 <= 4 /\ ~ 1 + 1 <= copied_capacity 4 1.
Proof. unfold copied_capacity. simpl. lia. Qed.

Inductive traversal_charge :=
| VerificationVisit (operations bytes : nat)
| WorklistAllocation (bytes : nat)
| PayloadCopy (bytes : nat).

Definition operation_charge (charge : traversal_charge) : nat :=
  match charge with VerificationVisit operations _ => operations | _ => 0 end.

Definition scan_charge (charge : traversal_charge) : nat :=
  match charge with VerificationVisit _ bytes => bytes | _ => 0 end.

Definition backing_charge (copy_payload : bool) (charge : traversal_charge) : nat :=
  match charge with
  | WorklistAllocation bytes => bytes
  | PayloadCopy bytes => if copy_payload then bytes else 0
  | VerificationVisit _ _ => 0
  end.

Definition traversal_total (project : traversal_charge -> nat)
  (trace : list traversal_charge) : nat := fold_right Nat.add 0 (map project trace).

Definition traversal_fits (copy_payload : bool) (trace : list traversal_charge)
  (operations bytes backing : nat) : Prop :=
  traversal_total operation_charge trace <= operations /\
  traversal_total scan_charge trace <= bytes /\
  traversal_total (backing_charge copy_payload) trace <= backing.

Theorem inspection_preserves_all_noncopy_reservations : forall trace,
  traversal_total (backing_charge true) trace =
    traversal_total (backing_charge false) trace +
    traversal_total (fun charge =>
      match charge with PayloadCopy bytes => bytes | _ => 0 end) trace.
Proof.
  unfold traversal_total.
  induction trace as [|charge rest IH]; [reflexivity|].
  destruct charge; cbn [map fold_right backing_charge] in *; lia.
Qed.

Theorem inspection_fits_every_successful_clone_budget :
  forall trace operations bytes backing,
  traversal_fits true trace operations bytes backing ->
  traversal_fits false trace operations bytes backing.
Proof.
  intros trace operations bytes backing [ops [scans copies]].
  pose proof (inspection_preserves_all_noncopy_reservations trace).
  unfold traversal_fits. repeat split; lia.
Qed.

Lemma traversal_prefix_total : forall project prefix suffix,
  traversal_total project prefix <= traversal_total project (prefix ++ suffix).
Proof.
  unfold traversal_total.
  intros project prefix. induction prefix; intros suffix; cbn; [lia|].
  specialize (IHprefix suffix). lia.
Qed.

Theorem inspection_prefixes_fit : forall prefix suffix operations bytes backing,
  traversal_fits false (prefix ++ suffix) operations bytes backing ->
  traversal_fits false prefix operations bytes backing.
Proof.
  intros prefix suffix operations bytes backing [ops [scans allocations]].
  pose proof (traversal_prefix_total operation_charge prefix suffix).
  pose proof (traversal_prefix_total scan_charge prefix suffix).
  pose proof (traversal_prefix_total (backing_charge false) prefix suffix).
  unfold traversal_fits. repeat split; lia.
Qed.

Example inspection_still_requires_worklist_backing :
  ~ traversal_fits false [WorklistAllocation 64; PayloadCopy 4096] 0 0 63.
Proof. unfold traversal_fits, traversal_total. cbn. lia. Qed.
