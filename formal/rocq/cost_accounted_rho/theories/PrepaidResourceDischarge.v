From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia Sorting.Permutation.
From CostAccountedRho Require Import CostAccountedSyntax AuthorityResourceValuation.
Import ListNotations.

Inductive flat_authority_node :=
| FlatUnit
| FlatGround (payload : list bool)
| FlatQuote (payload : list bool)
| FlatAnd.

Fixpoint flatten_authority (authority : sig) : list flat_authority_node :=
  match authority with
  | SUnit => [FlatUnit]
  | SGround payload => [FlatGround payload]
  | SQuote payload => [FlatQuote payload]
  | SAnd lhs rhs => FlatAnd :: flatten_authority lhs ++ flatten_authority rhs
  end.

Fixpoint authority_node_count (authority : sig) : nat :=
  match authority with
  | SAnd lhs rhs => S (authority_node_count lhs + authority_node_count rhs)
  | _ => 1
  end.

Fixpoint decode_flat_authority (fuel : nat) (nodes : list flat_authority_node)
  : option (sig * list flat_authority_node) :=
  match fuel with
  | 0 => None
  | S remaining =>
      match nodes with
      | [] => None
      | FlatUnit :: tail => Some (SUnit, tail)
      | FlatGround payload :: tail => Some (SGround payload, tail)
      | FlatQuote payload :: tail => Some (SQuote payload, tail)
      | FlatAnd :: tail =>
          match decode_flat_authority remaining tail with
          | None => None
          | Some (lhs, after_left) =>
              match decode_flat_authority remaining after_left with
              | None => None
              | Some (rhs, after_right) => Some (SAnd lhs rhs, after_right)
              end
          end
      end
  end.

Theorem flatten_authority_exact_node_count : forall authority,
  length (flatten_authority authority) = authority_node_count authority.
Proof.
  induction authority; simpl; try reflexivity.
  rewrite length_app, IHauthority1, IHauthority2. reflexivity.
Qed.

Theorem flattened_authority_decodes_with_exact_tail : forall authority fuel tail,
  authority_node_count authority <= fuel ->
  decode_flat_authority fuel (flatten_authority authority ++ tail) = Some (authority, tail).
Proof.
  induction authority; intros fuel tail enough; destruct fuel; simpl in *;
    try lia; try reflexivity.
  rewrite <- app_assoc.
  rewrite IHauthority1 by lia.
  rewrite IHauthority2 by lia. reflexivity.
Qed.

Theorem flattened_authority_is_injective : forall left right,
  flatten_authority left = flatten_authority right -> left = right.
Proof.
  intros left right same.
  pose proof (flattened_authority_decodes_with_exact_tail left
    (authority_node_count left + authority_node_count right) [] ltac:(lia)) as lhs.
  pose proof (flattened_authority_decodes_with_exact_tail right
    (authority_node_count left + authority_node_count right) [] ltac:(lia)) as rhs.
  rewrite !app_nil_r in lhs, rhs. rewrite same, rhs in lhs.
  inversion lhs. reflexivity.
Qed.

Definition flat_authority_units (nodes : list flat_authority_node) : nat :=
  fold_right (fun node total =>
    match node with FlatGround _ | FlatQuote _ => S total | _ => total end) 0 nodes.

Theorem flat_authority_units_append : forall left right,
  flat_authority_units (left ++ right) = flat_authority_units left + flat_authority_units right.
Proof.
  induction left as [|node rest IH]; intros right; [reflexivity|].
  destruct node; simpl in *; unfold flat_authority_units in *; simpl in *;
    rewrite IH; lia.
Qed.

Theorem flattened_authority_preserves_valuation : forall authority,
  flat_authority_units (flatten_authority authority) = authority_units authority.
Proof.
  induction authority; simpl; try reflexivity.
  change (flat_authority_units (flatten_authority authority1 ++ flatten_authority authority2) =
    authority_units authority1 + authority_units authority2).
  rewrite flat_authority_units_append, IHauthority1, IHauthority2. reflexivity.
Qed.

Record prepaid_resource_key := {
  prepaid_location : nat;
  prepaid_class : nat;
  prepaid_terms : nat;
  prepaid_authority : sig
}.

Definition prepaid_key_eq_dec : forall (left right : prepaid_resource_key),
  {left = right} + {left <> right}.
Proof. decide equality; auto using sig_eq_dec, Nat.eq_dec. Defined.

Definition resource_count := count_occ prepaid_key_eq_dec.

Definition same_resources (left right : list prepaid_resource_key) : bool :=
  forallb (fun key => Nat.eqb (resource_count left key) (resource_count right key))
          (left ++ right).

Theorem same_resources_exact : forall left right,
  same_resources left right = true <-> Permutation left right.
Proof.
  intros left right. unfold same_resources.
  rewrite forallb_forall, (Permutation_count_occ prepaid_key_eq_dec).
  split.
  - intros checked key. destruct (in_dec prepaid_key_eq_dec key (left ++ right)) as [present|absent].
    + apply Nat.eqb_eq. now apply checked.
    + unfold resource_count.
      assert (not_left : ~ In key left) by (intro H; apply absent; apply in_or_app; auto).
      assert (not_right : ~ In key right) by (intro H; apply absent; apply in_or_app; auto).
      apply (count_occ_not_In prepaid_key_eq_dec) in not_left.
      apply (count_occ_not_In prepaid_key_eq_dec) in not_right. lia.
  - intros same key _. apply Nat.eqb_eq. apply same.
Qed.

Definition prepaid_discharge (available required used unused fresh : list prepaid_resource_key) : Prop :=
  Permutation available (used ++ unused) /\ Permutation required (used ++ fresh).

Definition check_prepaid_discharge available required used unused fresh : bool :=
  same_resources available (used ++ unused) && same_resources required (used ++ fresh).

Theorem prepaid_check_exact : forall available required used unused fresh,
  check_prepaid_discharge available required used unused fresh = true <->
  prepaid_discharge available required used unused fresh.
Proof.
  intros. unfold check_prepaid_discharge, prepaid_discharge.
  now rewrite andb_true_iff, !same_resources_exact.
Qed.

Theorem prepaid_discharge_counts : forall available required used unused fresh key,
  prepaid_discharge available required used unused fresh ->
  resource_count available key = resource_count used key + resource_count unused key /\
  resource_count required key = resource_count used key + resource_count fresh key.
Proof.
  intros available required used unused fresh key [supply demand].
  pose proof (proj1 (Permutation_count_occ prepaid_key_eq_dec available (used ++ unused)) supply key) as supply_count.
  pose proof (proj1 (Permutation_count_occ prepaid_key_eq_dec required (used ++ fresh)) demand key) as demand_count.
  unfold resource_count. rewrite !count_occ_app in *. auto.
Qed.

Theorem prepaid_cannot_overdraw : forall available required used unused fresh key,
  prepaid_discharge available required used unused fresh ->
  resource_count used key <= resource_count available key /\
  resource_count used key <= resource_count required key.
Proof.
  intros. pose proof (prepaid_discharge_counts _ _ _ _ _ key H). lia.
Qed.

Theorem missing_type_requires_new_acquisition : forall available required used unused fresh key,
  prepaid_discharge available required used unused fresh -> resource_count available key = 0 ->
  resource_count fresh key = resource_count required key.
Proof.
  intros. pose proof (prepaid_discharge_counts _ _ _ _ _ key H). lia.
Qed.

Theorem repeated_demand_requires_repeated_supply : forall available required used unused fresh key,
  prepaid_discharge available required used unused fresh ->
  resource_count required key - resource_count available key <= resource_count fresh key.
Proof.
  intros. pose proof (prepaid_discharge_counts _ _ _ _ _ key H). lia.
Qed.

Definition prepaid_exhausted (unused fresh : list prepaid_resource_key) : Prop :=
  forall key, resource_count unused key = 0 \/ resource_count fresh key = 0.

Definition check_prepaid_exhausted (unused fresh : list prepaid_resource_key) : bool :=
  forallb (fun key => Nat.eqb (resource_count unused key) 0 || Nat.eqb (resource_count fresh key) 0)
          (unused ++ fresh).

Theorem prepaid_exhausted_check_exact : forall unused fresh,
  check_prepaid_exhausted unused fresh = true <-> prepaid_exhausted unused fresh.
Proof.
  intros unused fresh. unfold check_prepaid_exhausted, prepaid_exhausted.
  rewrite forallb_forall. split.
  - intros checked key. destruct (in_dec prepaid_key_eq_dec key unused) as [present|absent].
    + specialize (checked key (in_or_app _ _ _ (or_introl present))).
      now rewrite orb_true_iff, !Nat.eqb_eq in checked.
    + left. now apply (count_occ_not_In prepaid_key_eq_dec).
  - intros exhausted key _. rewrite orb_true_iff, !Nat.eqb_eq. apply exhausted.
Qed.

Definition check_prepaid_funding available required used unused fresh : bool :=
  check_prepaid_discharge available required used unused fresh && check_prepaid_exhausted unused fresh.

Theorem accepted_funding_has_exact_typed_residual : forall available required used unused fresh key,
  check_prepaid_funding available required used unused fresh = true ->
  resource_count fresh key = resource_count required key - resource_count available key.
Proof.
  intros available required used unused fresh key checked.
  unfold check_prepaid_funding in checked.
  rewrite andb_true_iff, prepaid_check_exact, prepaid_exhausted_check_exact in checked.
  destruct checked as [conserved exhausted].
  pose proof (prepaid_discharge_counts _ _ _ _ _ key conserved).
  destruct (exhausted key); lia.
Qed.

Definition acquisition_value (price : prepaid_resource_key -> nat) (resources : list prepaid_resource_key) : nat :=
  fold_right (fun resource total => price resource + total) 0 resources.

Theorem acquisition_value_append : forall price left right,
  acquisition_value price (left ++ right) = acquisition_value price left + acquisition_value price right.
Proof. intros price left. induction left; intros right; simpl; [reflexivity|]. rewrite IHleft. lia. Qed.

Theorem acquisition_value_permutation : forall price left right,
  Permutation left right -> acquisition_value price left = acquisition_value price right.
Proof.
  intros price left right same. induction same;
    unfold acquisition_value in *; simpl in *; lia.
Qed.

Theorem prepaid_value_conservation : forall price available required used unused fresh,
  prepaid_discharge available required used unused fresh ->
  acquisition_value price available + acquisition_value price fresh =
  acquisition_value price required + acquisition_value price unused.
Proof.
  intros price available required used unused fresh [supply demand].
  rewrite (acquisition_value_permutation price _ _ supply),
          (acquisition_value_permutation price _ _ demand), !acquisition_value_append. lia.
Qed.

Theorem fully_prepaid_has_no_new_acquisition : forall current_price required unused,
  check_prepaid_discharge (required ++ unused) required required unused [] = true /\
  acquisition_value current_price [] = 0.
Proof.
  intros. split; [apply prepaid_check_exact|reflexivity].
  unfold prepaid_discharge. rewrite app_nil_r. auto.
Qed.

Inductive prepaid_discharge_history : list prepaid_resource_key -> list prepaid_resource_key ->
                                     list prepaid_resource_key -> Prop :=
| prepaid_history_refl : forall available, prepaid_discharge_history available [] available
| prepaid_history_step : forall available required used unused fresh later final,
    prepaid_discharge available required used unused fresh ->
    prepaid_discharge_history unused later final ->
    prepaid_discharge_history available (used ++ later) final.

Theorem arbitrary_discharge_history_conserves_inventory : forall available used unused,
  prepaid_discharge_history available used unused -> Permutation available (used ++ unused).
Proof.
  intros available used unused history. induction history; [reflexivity|].
  destruct H as [supply _]. eapply Permutation_trans; [exact supply|].
  rewrite <- app_assoc. apply Permutation_app_head. exact IHhistory.
Qed.

Theorem disjoint_discharge_composes : forall a1 r1 u1 n1 f1 a2 r2 u2 n2 f2,
  prepaid_discharge a1 r1 u1 n1 f1 -> prepaid_discharge a2 r2 u2 n2 f2 ->
  prepaid_discharge (a1 ++ a2) (r1 ++ r2) (u1 ++ u2) (n1 ++ n2) (f1 ++ f2).
Proof.
  intros a1 r1 u1 n1 f1 a2 r2 u2 n2 f2 [s1 d1] [s2 d2].
  split.
  - eapply Permutation_trans; [apply Permutation_app; eassumption|].
    rewrite <- !app_assoc. apply Permutation_app_head.
    rewrite !app_assoc. apply Permutation_app_tail. apply Permutation_app_comm.
  - eapply Permutation_trans; [apply Permutation_app; eassumption|].
    rewrite <- !app_assoc. apply Permutation_app_head.
    rewrite !app_assoc. apply Permutation_app_tail. apply Permutation_app_comm.
Qed.

Definition prepaid_sample (location class terms : nat) (authority : sig) : prepaid_resource_key :=
  {| prepaid_location := location; prepaid_class := class; prepaid_terms := terms;
     prepaid_authority := authority |}.

Definition prepaid_a := prepaid_sample 0 0 0 (SGround [true]).
Definition prepaid_b := prepaid_sample 0 0 0 (SGround [false]).

Example incompatible_authority_is_rejected_despite_equal_value :
  authority_value 1 (prepaid_authority prepaid_a) = authority_value 1 (prepaid_authority prepaid_b) /\
  check_prepaid_discharge [prepaid_a] [prepaid_b] [prepaid_a] [] [] = false.
Proof. vm_compute. auto. Qed.

Example incompatible_location_class_or_terms_is_rejected :
  forallb (fun other => negb (check_prepaid_discharge [prepaid_a] [other] [prepaid_a] [] []))
    [prepaid_sample 1 0 0 (SGround [true]); prepaid_sample 0 1 0 (SGround [true]);
     prepaid_sample 0 0 1 (SGround [true])] = true.
Proof. vm_compute. reflexivity. Qed.

Example one_resource_cannot_discharge_two_occurrences :
  check_prepaid_discharge [prepaid_a] [prepaid_a; prepaid_a] [prepaid_a; prepaid_a] [] [] = false /\
  check_prepaid_discharge [prepaid_a] [prepaid_a; prepaid_a] [prepaid_a] [] [prepaid_a] = true.
Proof. vm_compute. auto. Qed.

Example price_increase_does_not_reprice_a_prepaid_right :
  acquisition_value (fun _ => 1) [prepaid_a] = 1 /\
  check_prepaid_discharge [prepaid_a] [prepaid_a] [prepaid_a] [] [] = true /\
  acquisition_value (fun _ => 2) [] = 0.
Proof. vm_compute. auto. Qed.

Example skipping_compatible_prepaid_resources_is_rejected :
  check_prepaid_discharge [prepaid_a] [prepaid_a] [] [prepaid_a] [prepaid_a] = true /\
  check_prepaid_funding [prepaid_a] [prepaid_a] [] [prepaid_a] [prepaid_a] = false.
Proof. vm_compute. auto. Qed.

Example generated_discharge_regression :
  forallb (fun used_count => forallb (fun unused_count => forallb (fun fresh_count =>
    let used := repeat prepaid_a used_count in
    let unused := repeat prepaid_b unused_count in
    let fresh := repeat prepaid_a fresh_count in
    check_prepaid_funding (used ++ unused) (used ++ fresh) used unused fresh)
    (seq 0 9)) (seq 0 9)) (seq 0 9) = true.
Proof. vm_compute. reflexivity. Qed.

Example generated_discharge_rejection_oracle :
  forallb (fun supply => forallb (fun demand => forallb (fun used =>
    forallb (fun unused => forallb (fun fresh =>
      Bool.eqb
        (check_prepaid_funding (repeat prepaid_a supply) (repeat prepaid_a demand)
          (repeat prepaid_a used) (repeat prepaid_a unused) (repeat prepaid_a fresh))
        (Nat.eqb supply (used + unused) && Nat.eqb demand (used + fresh) &&
          (Nat.eqb unused 0 || Nat.eqb fresh 0)))
      (seq 0 5)) (seq 0 5)) (seq 0 5)) (seq 0 5)) (seq 0 5) = true.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions flatten_authority_exact_node_count.
Print Assumptions flattened_authority_decodes_with_exact_tail.
Print Assumptions flattened_authority_is_injective.
Print Assumptions flat_authority_units_append.
Print Assumptions flattened_authority_preserves_valuation.
Print Assumptions prepaid_check_exact.
Print Assumptions prepaid_cannot_overdraw.
Print Assumptions missing_type_requires_new_acquisition.
Print Assumptions repeated_demand_requires_repeated_supply.
Print Assumptions prepaid_exhausted_check_exact.
Print Assumptions accepted_funding_has_exact_typed_residual.
Print Assumptions prepaid_value_conservation.
Print Assumptions fully_prepaid_has_no_new_acquisition.
Print Assumptions arbitrary_discharge_history_conserves_inventory.
Print Assumptions disjoint_discharge_composes.

Definition counted_resources := list (prepaid_resource_key * nat).

Fixpoint expand_resources (rows : counted_resources) : list prepaid_resource_key :=
  match rows with
  | [] => []
  | (key, quantity) :: rest => repeat key quantity ++ expand_resources rest
  end.

Fixpoint counted_quantity (rows : counted_resources) (key : prepaid_resource_key) : nat :=
  match rows with
  | [] => 0
  | (candidate, quantity) :: rest =>
      (if prepaid_key_eq_dec candidate key then quantity else 0) + counted_quantity rest key
  end.

Lemma repeated_resource_count : forall candidate quantity key,
  resource_count (repeat candidate quantity) key =
  if prepaid_key_eq_dec candidate key then quantity else 0.
Proof.
  intros candidate quantity key. induction quantity as [|quantity IH];
    unfold resource_count in *; simpl; destruct (prepaid_key_eq_dec candidate key);
    simpl in *; congruence.
Qed.

Theorem counted_quantity_matches_expansion : forall rows key,
  counted_quantity rows key = resource_count (expand_resources rows) key.
Proof.
  intros rows key. induction rows as [|[candidate quantity] rest IH]; [reflexivity|].
  cbn [counted_quantity expand_resources]. unfold resource_count at 1.
  rewrite count_occ_app. fold (resource_count (repeat candidate quantity) key).
  rewrite repeated_resource_count, IH. reflexivity.
Qed.

Theorem counted_equality_is_expanded_permutation : forall left right,
  (forall key, counted_quantity left key = counted_quantity right key) <->
  Permutation (expand_resources left) (expand_resources right).
Proof.
  intros left right. rewrite (Permutation_count_occ prepaid_key_eq_dec).
  split; intros equal key; specialize (equal key);
    rewrite !counted_quantity_matches_expansion in *; exact equal.
Qed.

Definition counted_discharge available required used unused fresh : Prop :=
  forall key,
    counted_quantity available key = counted_quantity used key + counted_quantity unused key /\
    counted_quantity required key = counted_quantity used key + counted_quantity fresh key.

Theorem counted_discharge_matches_expansion : forall available required used unused fresh,
  counted_discharge available required used unused fresh <->
  prepaid_discharge (expand_resources available) (expand_resources required)
    (expand_resources used) (expand_resources unused) (expand_resources fresh).
Proof.
  intros available required used unused fresh. split.
  - intros counts. split; apply (proj2 (Permutation_count_occ prepaid_key_eq_dec _ _));
      intro key; rewrite count_occ_app; specialize (counts key);
      unfold counted_discharge in counts;
      rewrite !counted_quantity_matches_expansion in counts; tauto.
  - intros valid key. rewrite !counted_quantity_matches_expansion.
    now apply prepaid_discharge_counts.
Qed.

Theorem counted_exhaustion_matches_expansion : forall unused fresh,
  (forall key, counted_quantity unused key = 0 \/ counted_quantity fresh key = 0) <->
  prepaid_exhausted (expand_resources unused) (expand_resources fresh).
Proof.
  intros unused fresh. unfold prepaid_exhausted.
  split; intros exhausted key; specialize (exhausted key);
    rewrite !counted_quantity_matches_expansion in *; exact exhausted.
Qed.

Fixpoint counted_value (price : prepaid_resource_key -> nat) (rows : counted_resources) : nat :=
  match rows with
  | [] => 0
  | (key, quantity) :: rest => quantity * price key + counted_value price rest
  end.

Lemma repeated_acquisition_value : forall price key quantity,
  acquisition_value price (repeat key quantity) = quantity * price key.
Proof.
  intros price key quantity. induction quantity as [|quantity IH];
    unfold acquisition_value in *; simpl in *; lia.
Qed.

Theorem counted_value_matches_expansion : forall price rows,
  counted_value price rows = acquisition_value price (expand_resources rows).
Proof.
  intros price rows. induction rows as [|[key quantity] rest IH]; [reflexivity|].
  cbn [counted_value expand_resources].
  rewrite acquisition_value_append, repeated_acquisition_value, IH. reflexivity.
Qed.

Theorem counted_split_preserves_quantity : forall key first second rest sought,
  counted_quantity ((key, first + second) :: rest) sought =
  counted_quantity ((key, first) :: (key, second) :: rest) sought.
Proof.
  intros. cbn [counted_quantity]. destruct (prepaid_key_eq_dec key sought); lia.
Qed.

Theorem counted_split_preserves_value : forall price key first second rest,
  counted_value price ((key, first + second) :: rest) =
  counted_value price ((key, first) :: (key, second) :: rest).
Proof. intros. cbn [counted_value]. nia. Qed.

Print Assumptions counted_quantity_matches_expansion.
Print Assumptions counted_equality_is_expanded_permutation.
Print Assumptions counted_discharge_matches_expansion.
Print Assumptions counted_exhaustion_matches_expansion.
Print Assumptions counted_value_matches_expansion.
Print Assumptions counted_split_preserves_quantity.
Print Assumptions counted_split_preserves_value.
