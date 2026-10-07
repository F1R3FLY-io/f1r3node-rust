From Stdlib Require Import Bool.Bool Lists.List Lists.ListDec Arith.PeanoNat Lia Sorting.Permutation.
Import ListNotations.

Section RetainedCellAllocation.
Definition backing_overlap (source amount cell width : nat) :=
  Nat.min amount (cell + width - source) - Nat.min amount (cell - source).

Fixpoint partition_backing source amount start widths :=
  match widths with
  | [] => []
  | width :: rest => backing_overlap source amount start width ::
      partition_backing source amount (start + width) rest
  end.

Definition backing_sum := fold_right Nat.add 0.

Theorem backing_overlap_symmetric : forall source amount cell width,
  backing_overlap source amount cell width = backing_overlap cell width source amount.
Proof.
  intros. unfold backing_overlap.
  repeat match goal with
  | |- context [Nat.min ?a ?b] => destruct (Nat.min_spec a b) as [[? ->] | [? ->]]
  end; lia.
Qed.

Theorem backing_overlap_bounded : forall source amount cell width,
  backing_overlap source amount cell width <= amount /\
  backing_overlap source amount cell width <= width.
Proof.
  intros. split.
  - unfold backing_overlap. pose proof (Nat.le_min_l amount (cell + width - source)). lia.
  - rewrite backing_overlap_symmetric. unfold backing_overlap.
    pose proof (Nat.le_min_l width (source + amount - cell)). lia.
Qed.

Lemma backing_sub_telescope : forall first middle last,
  first <= middle -> middle <= last ->
  last - first = (middle - first) + (last - middle).
Proof. intros. lia. Qed.

Theorem backing_overlap_split : forall source amount cell left right,
  backing_overlap source amount cell (left + right) =
  backing_overlap source amount cell left + backing_overlap source amount (cell + left) right.
Proof.
  intros. unfold backing_overlap.
  replace (cell + (left + right)) with (cell + left + right) by lia.
  apply backing_sub_telescope; apply Nat.min_le_compat_l;
    apply Nat.sub_le_mono_r; lia.
Qed.

Theorem backing_partition_telescopes : forall widths source amount start,
  backing_sum (partition_backing source amount start widths) =
    backing_overlap source amount start (backing_sum widths).
Proof.
  induction widths; intros; simpl.
  - unfold backing_sum, backing_overlap. simpl. lia.
  - change (backing_overlap source amount start a +
      backing_sum (partition_backing source amount (start + a) widths) =
      backing_overlap source amount start (a + backing_sum widths)).
    rewrite IHwidths, backing_overlap_split. reflexivity.
Qed.

Theorem backing_partition_preserves_source : forall widths source amount,
  source + amount <= backing_sum widths ->
  backing_sum (partition_backing source amount 0 widths) = amount.
Proof.
  intros. rewrite backing_partition_telescopes. unfold backing_overlap.
  simpl. rewrite Nat.min_0_r. rewrite Nat.min_l by lia. lia.
Qed.

Theorem backing_partition_preserves_cell : forall sources cell width,
  cell + width <= backing_sum sources ->
  backing_sum (partition_backing cell width 0 sources) = width.
Proof. intros. now apply backing_partition_preserves_source. Qed.

Theorem backing_partition_keeps_quantity : forall widths source amount start,
  length (partition_backing source amount start widths) = length widths.
Proof. induction widths; intros; simpl; auto. Qed.

Theorem backing_zero_value_keeps_quantity : forall quantity source start,
  partition_backing source 0 start (repeat 0 quantity) = repeat 0 quantity.
Proof.
  induction quantity; intros; simpl; auto.
  unfold backing_overlap at 1. simpl. now rewrite IHquantity.
Qed.

Theorem backing_stream_step_matches_overlap : forall cell width filled remaining,
  filled <= width ->
  backing_overlap (cell + filled) remaining cell width = Nat.min remaining (width - filled).
Proof.
  intros. unfold backing_overlap.
  replace (cell - (cell + filled)) with 0 by lia. rewrite Nat.min_0_r.
  replace (cell + width - (cell + filled)) with (width - filled) by lia. lia.
Qed.

End RetainedCellAllocation.

Print Assumptions backing_overlap_symmetric.
Print Assumptions backing_overlap_bounded.
Print Assumptions backing_overlap_split.
Print Assumptions backing_partition_telescopes.
Print Assumptions backing_partition_preserves_source.
Print Assumptions backing_partition_preserves_cell.
Print Assumptions backing_partition_keeps_quantity.
Print Assumptions backing_zero_value_keeps_quantity.
Print Assumptions backing_stream_step_matches_overlap.

Section RetainedBirthFunding.
Record retained_birth := {
  retained_birth_source : nat;
  retained_birth_columns : list nat
}.

Definition retained_birth_claims births := flat_map retained_birth_columns births.

Definition check_retained_birth_funding (retained : nat -> bool) (quantity : nat -> nat)
    (valid_birth : retained_birth -> bool) columns births :=
  (if NoDup_dec Nat.eq_dec (map retained_birth_source births) then true else false) &&
  forallb valid_birth births &&
  forallb (fun column => (column <? columns) && retained column) (retained_birth_claims births) &&
  forallb (fun column => count_occ Nat.eq_dec (retained_birth_claims births) column =?
    if retained column then quantity column else 0) (seq 0 columns).

Theorem retained_birth_funding_is_exact : forall retained quantity valid columns births,
  check_retained_birth_funding retained quantity valid columns births = true <->
  NoDup (map retained_birth_source births) /\
  Forall (fun birth => valid birth = true) births /\
  Forall (fun column => column < columns /\ retained column = true) (retained_birth_claims births) /\
  forall column, column < columns ->
    count_occ Nat.eq_dec (retained_birth_claims births) column =
      if retained column then quantity column else 0.
Proof.
  intros. unfold check_retained_birth_funding.
  destruct (NoDup_dec Nat.eq_dec (map retained_birth_source births));
    rewrite !andb_true_iff, !forallb_forall, !Forall_forall.
  - setoid_rewrite andb_true_iff. setoid_rewrite Nat.ltb_lt.
    setoid_rewrite Nat.eqb_eq. setoid_rewrite in_seq. simpl. intuition.
  - intuition discriminate.
Qed.

Theorem retained_birth_funding_rejects_duplicate_sources : forall retained quantity valid columns births,
  ~ NoDup (map retained_birth_source births) ->
  check_retained_birth_funding retained quantity valid columns births = false.
Proof.
  intros. destruct (check_retained_birth_funding _ _ _ _ _) eqn:checked; auto.
  apply retained_birth_funding_is_exact in checked. tauto.
Qed.

Theorem retained_birth_funding_checks_each_physical_birth : forall retained quantity valid columns births birth,
  check_retained_birth_funding retained quantity valid columns births = true ->
  In birth births -> valid birth = true.
Proof.
  intros. apply retained_birth_funding_is_exact in H. destruct H as [_ [checked _]].
  rewrite Forall_forall in checked. now apply checked.
Qed.

Theorem retained_birth_funding_excludes_consumed_and_fee_columns : forall retained quantity valid columns births column,
  check_retained_birth_funding retained quantity valid columns births = true ->
  In column (retained_birth_claims births) -> retained column = true.
Proof.
  intros. apply retained_birth_funding_is_exact in H. destruct H as [_ [_ [checked _]]].
  rewrite Forall_forall in checked. specialize (checked column H0). tauto.
Qed.

Theorem retained_birth_funding_preserves_every_quantity : forall retained quantity valid columns births column,
  check_retained_birth_funding retained quantity valid columns births = true ->
  column < columns -> retained column = true ->
  count_occ Nat.eq_dec (retained_birth_claims births) column = quantity column.
Proof.
  intros. apply retained_birth_funding_is_exact in H. destruct H as [_ [_ [_ checked]]].
  specialize (checked column H0). now rewrite H1 in checked.
Qed.

Theorem retained_birth_funding_rejects_missing_or_extra_cells : forall retained quantity valid columns births column,
  column < columns -> retained column = true ->
  count_occ Nat.eq_dec (retained_birth_claims births) column <> quantity column ->
  check_retained_birth_funding retained quantity valid columns births = false.
Proof.
  intros. destruct (check_retained_birth_funding _ _ _ _ _) eqn:checked; auto.
  eapply retained_birth_funding_preserves_every_quantity in checked; eauto. contradiction.
Qed.

Theorem retained_birth_funding_rejects_out_of_range_column : forall retained quantity valid columns births column,
  In column (retained_birth_claims births) -> columns <= column ->
  check_retained_birth_funding retained quantity valid columns births = false.
Proof.
  intros. destruct (check_retained_birth_funding _ _ _ _ _) eqn:checked; auto.
  apply retained_birth_funding_is_exact in checked. destruct checked as [_ [_ [checked _]]].
  rewrite Forall_forall in checked. specialize (checked column H). lia.
Qed.

Theorem retained_birth_claims_preserve_permutations : forall first second,
  Permutation first second -> Permutation (retained_birth_claims first) (retained_birth_claims second).
Proof.
  intros first second order. unfold retained_birth_claims. induction order; simpl.
  - reflexivity.
  - now apply Permutation_app_head.
  - rewrite !app_assoc. apply Permutation_app_tail. apply Permutation_app_comm.
  - etransitivity; eassumption.
Qed.

Theorem retained_birth_order_preserves_every_quantity : forall first second column,
  Permutation first second ->
  count_occ Nat.eq_dec (retained_birth_claims first) column =
  count_occ Nat.eq_dec (retained_birth_claims second) column.
Proof.
  intros. apply (proj1 (Permutation_count_occ Nat.eq_dec _ _));
    now apply retained_birth_claims_preserve_permutations.
Qed.

End RetainedBirthFunding.

Print Assumptions retained_birth_funding_is_exact.
Print Assumptions retained_birth_funding_rejects_duplicate_sources.
Print Assumptions retained_birth_funding_checks_each_physical_birth.
Print Assumptions retained_birth_funding_excludes_consumed_and_fee_columns.
Print Assumptions retained_birth_funding_preserves_every_quantity.
Print Assumptions retained_birth_funding_rejects_missing_or_extra_cells.
Print Assumptions retained_birth_funding_rejects_out_of_range_column.
Print Assumptions retained_birth_claims_preserve_permutations.
Print Assumptions retained_birth_order_preserves_every_quantity.

Section RootedPhysicalReceiptBinding.
Context {cell : Type}.
Variable valid_cell : cell -> bool.

Definition check_rooted_physical_bucket
    (expected_root captured_root physical_source receipt_source occurrences cells : nat)
    (records : list (list cell)) : bool :=
  (expected_root =? captured_root) && (physical_source =? receipt_source) &&
  (0 <? occurrences) && (0 <? cells) && (length records =? occurrences) &&
  forallb (fun record => (length record =? cells) && forallb valid_cell record) records.

Theorem physical_receipt_binding_is_exact : forall expected captured source receipt occurrences cells records,
  check_rooted_physical_bucket expected captured source receipt occurrences cells records = true <->
  expected = captured /\ source = receipt /\ 0 < occurrences /\ 0 < cells /\
  length records = occurrences /\
  Forall (fun record => length record = cells /\ Forall (fun c => valid_cell c = true) record) records.
Proof.
  intros. unfold check_rooted_physical_bucket.
  repeat rewrite andb_true_iff. repeat rewrite Nat.eqb_eq. repeat rewrite Nat.ltb_lt.
  rewrite forallb_forall, Forall_forall.
  setoid_rewrite andb_true_iff. setoid_rewrite Nat.eqb_eq.
  setoid_rewrite forallb_forall. setoid_rewrite Forall_forall. tauto.
Qed.

Theorem physical_receipt_binding_rejects_mixed_roots : forall expected captured source receipt occurrences cells records,
  expected <> captured ->
  check_rooted_physical_bucket expected captured source receipt occurrences cells records = false.
Proof.
  intros. destruct (check_rooted_physical_bucket _ _ _ _ _ _ _) eqn:accepted; auto.
  apply physical_receipt_binding_is_exact in accepted. intuition congruence.
Qed.

Theorem physical_receipt_binding_rejects_incomplete_occurrences : forall expected captured source receipt occurrences cells records,
  length records <> occurrences ->
  check_rooted_physical_bucket expected captured source receipt occurrences cells records = false.
Proof.
  intros. destruct (check_rooted_physical_bucket _ _ _ _ _ _ _) eqn:accepted; auto.
  apply physical_receipt_binding_is_exact in accepted. intuition congruence.
Qed.

Theorem physical_receipt_binding_checks_every_cell : forall expected captured source receipt occurrences cells records record c,
  check_rooted_physical_bucket expected captured source receipt occurrences cells records = true ->
  In record records -> In c record -> valid_cell c = true.
Proof.
  intros expected captured source receipt occurrences cells records record c accepted member present.
  apply physical_receipt_binding_is_exact in accepted.
  destruct accepted as [_ [_ [_ [_ [_ checked]]]]].
  rewrite Forall_forall in checked. specialize (checked record member).
  destruct checked as [_ checked]. rewrite Forall_forall in checked. now apply checked.
Qed.

Theorem physical_receipt_binding_rejects_truncated_record : forall expected captured source receipt occurrences cells records record,
  In record records -> length record <> cells ->
  check_rooted_physical_bucket expected captured source receipt occurrences cells records = false.
Proof.
  intros expected captured source receipt occurrences cells records record member short.
  destruct (check_rooted_physical_bucket _ _ _ _ _ _ _) eqn:accepted; auto.
  apply physical_receipt_binding_is_exact in accepted.
  destruct accepted as [_ [_ [_ [_ [_ checked]]]]].
  rewrite Forall_forall in checked. specialize (checked record member). intuition.
Qed.

Theorem physical_receipt_binding_accepts_arbitrary_finite_occurrences : forall root source occurrences cells c,
  0 < occurrences -> 0 < cells -> valid_cell c = true ->
  check_rooted_physical_bucket root root source source occurrences cells
    (repeat (repeat c cells) occurrences) = true.
Proof.
  intros. apply physical_receipt_binding_is_exact.
  repeat split; auto using repeat_length.
  apply Forall_forall. intros record member.
  apply repeat_spec in member. subst record. split; [apply repeat_length|].
  apply Forall_forall. intros x present. apply repeat_spec in present. now subst x.
Qed.

End RootedPhysicalReceiptBinding.

Print Assumptions physical_receipt_binding_is_exact.
Print Assumptions physical_receipt_binding_rejects_mixed_roots.
Print Assumptions physical_receipt_binding_rejects_incomplete_occurrences.
Print Assumptions physical_receipt_binding_checks_every_cell.
Print Assumptions physical_receipt_binding_rejects_truncated_record.
Print Assumptions physical_receipt_binding_accepts_arbitrary_finite_occurrences.

Section NativeCellBacking.
Record native_cell_backing := {
  cell_original_price : nat;
  cell_original_weight : nat;
  cell_authority_leaves : nat;
  cell_contributions : list (nat * nat)
}.

Fixpoint canonical_cell_contributions (previous : option nat) (rows : list (nat * nat)) : bool :=
  match rows with
  | [] => true
  | (key, amount) :: rest =>
      (0 <? amount) &&
      (match previous with None => true | Some prior => prior <? key end) &&
      canonical_cell_contributions (Some key) rest
  end.

Definition cell_backing_total (rows : list (nat * nat)) :=
  fold_right (fun row total => snd row + total) 0 rows.

Definition cell_acquisition_value (cell : native_cell_backing) :=
  cell_original_weight cell * cell_authority_leaves cell * cell_original_price cell.

Definition check_native_cell_backing (machine_max native_max : nat) (cell : native_cell_backing) :=
  canonical_cell_contributions None (cell_contributions cell) &&
  (cell_original_weight cell * cell_authority_leaves cell <=? machine_max) &&
  (cell_acquisition_value cell <=? machine_max) &&
  forallb (fun row => snd row <=? native_max) (cell_contributions cell) &&
  (cell_backing_total (cell_contributions cell) =? cell_acquisition_value cell).

Theorem accepted_cell_backing_is_exact : forall machine_max native_max cell,
  check_native_cell_backing machine_max native_max cell = true <->
  canonical_cell_contributions None (cell_contributions cell) = true /\
  cell_original_weight cell * cell_authority_leaves cell <= machine_max /\
  cell_acquisition_value cell <= machine_max /\
  Forall (fun row => snd row <= native_max) (cell_contributions cell) /\
  cell_backing_total (cell_contributions cell) = cell_acquisition_value cell.
Proof.
  intros. unfold check_native_cell_backing.
  repeat rewrite andb_true_iff. repeat rewrite Nat.leb_le. rewrite Nat.eqb_eq.
  rewrite forallb_forall, Forall_forall.
  setoid_rewrite Nat.leb_le. tauto.
Qed.

Theorem canonical_cell_contributions_are_positive : forall rows previous,
  canonical_cell_contributions previous rows = true ->
  Forall (fun row => 0 < snd row) rows.
Proof.
  induction rows as [|[key amount] rest IH]; intros previous accepted; constructor.
  - simpl in accepted. repeat rewrite andb_true_iff in accepted.
    apply Nat.ltb_lt. tauto.
  - simpl in accepted. repeat rewrite andb_true_iff in accepted. apply (IH (Some key)). tauto.
Qed.

Theorem canonical_cell_contributions_have_distinct_keys : forall rows previous,
  canonical_cell_contributions previous rows = true -> NoDup (map fst rows).
Proof.
  assert (greater : forall rows prior,
    canonical_cell_contributions (Some prior) rows = true ->
    Forall (fun row => prior < fst row) rows).
  { induction rows as [|[key amount] rest IH]; intros prior accepted; constructor.
    - simpl in accepted. repeat rewrite andb_true_iff in accepted.
      apply Nat.ltb_lt. tauto.
    - simpl in accepted. repeat rewrite andb_true_iff in accepted.
      destruct accepted as [[_ bound] tail]. apply Nat.ltb_lt in bound.
      specialize (IH key tail). eapply Forall_impl; [|exact IH]. simpl. intros. lia. }
  induction rows as [|[key amount] rest IH]; intros previous accepted; simpl; constructor.
  - simpl in accepted. repeat rewrite andb_true_iff in accepted.
    assert (tail : canonical_cell_contributions (Some key) rest = true) by tauto.
    specialize (greater rest key tail). rewrite Forall_forall in greater.
    intro member. apply in_map_iff in member as [row [same present]].
    specialize (greater row present). lia.
  - simpl in accepted. repeat rewrite andb_true_iff in accepted. apply (IH (Some key)). tauto.
Qed.

Theorem cell_backing_rejects_inconsistent_funding : forall machine_max native_max cell,
  cell_backing_total (cell_contributions cell) <> cell_acquisition_value cell ->
  check_native_cell_backing machine_max native_max cell = false.
Proof.
  intros. destruct (check_native_cell_backing machine_max native_max cell) eqn:checked; auto.
  apply accepted_cell_backing_is_exact in checked. tauto.
Qed.

Theorem cell_backing_rejects_native_overflow : forall machine_max native_max cell,
  (exists row, In row (cell_contributions cell) /\ native_max < snd row) ->
  check_native_cell_backing machine_max native_max cell = false.
Proof.
  intros machine_max native_max cell [row [member overflow]].
  destruct (check_native_cell_backing machine_max native_max cell) eqn:checked; auto.
  apply accepted_cell_backing_is_exact in checked as [_ [_ [_ [bounded _]]]].
  rewrite Forall_forall in bounded. specialize (bounded row member). lia.
Qed.

Theorem accepted_cell_funding_is_not_wallet_count_pricing : forall machine_max native_max left right,
  check_native_cell_backing machine_max native_max left = true ->
  check_native_cell_backing machine_max native_max right = true ->
  cell_original_price left = cell_original_price right ->
  cell_original_weight left = cell_original_weight right ->
  cell_authority_leaves left = cell_authority_leaves right ->
  cell_backing_total (cell_contributions left) = cell_backing_total (cell_contributions right).
Proof.
  intros machine_max native_max left right accepted_left accepted_right price weight leaves.
  apply accepted_cell_backing_is_exact in accepted_left as [_ [_ [_ [_ ->]]]].
  apply accepted_cell_backing_is_exact in accepted_right as [_ [_ [_ [_ ->]]]].
  unfold cell_acquisition_value. now rewrite price, weight, leaves.
Qed.

Theorem receipt_value_survives_consumption_partition : forall before used remaining,
  Permutation before (used ++ remaining) ->
  fold_right (fun cell sum => cell_acquisition_value cell + sum) 0 before =
  fold_right (fun cell sum => cell_acquisition_value cell + sum) 0 used +
  fold_right (fun cell sum => cell_acquisition_value cell + sum) 0 remaining.
Proof.
  intros before used remaining same.
  assert (permutes : forall a b, Permutation a b ->
    fold_right (fun cell sum => cell_acquisition_value cell + sum) 0 a =
    fold_right (fun cell sum => cell_acquisition_value cell + sum) 0 b).
  { intros a b perm. induction perm; cbn [fold_right]; lia. }
  rewrite (permutes _ _ same). clear same. induction used; cbn [fold_right app]; lia.
Qed.
End NativeCellBacking.

Print Assumptions accepted_cell_backing_is_exact.
Print Assumptions canonical_cell_contributions_are_positive.
Print Assumptions canonical_cell_contributions_have_distinct_keys.
Print Assumptions cell_backing_rejects_inconsistent_funding.
Print Assumptions cell_backing_rejects_native_overflow.
Print Assumptions accepted_cell_funding_is_not_wallet_count_pricing.
Print Assumptions receipt_value_survives_consumption_partition.

Section RootedReceiptSnapshot.
Context {root key value : Type}.
Context (root_eq : forall left right : root, {left = right} + {left <> right}).
Context (key_eq : forall left right : key, {left = right} + {left <> right}).

Record rooted_receipt_snapshot := {
  captured_receipt_root : root;
  captured_receipt_entries : list (key * option value)
}.

Definition capture_receipts (history : root -> key -> option value) selected keys :=
  {| captured_receipt_root := selected;
     captured_receipt_entries := map (fun k => (k, history selected k)) keys |}.

Definition capture_registered_receipts (registered : root -> bool) history selected keys :=
  if registered selected then Some (capture_receipts history selected keys) else None.

Theorem registered_receipt_capture_requires_root_evidence : forall registered history selected keys snapshot,
  capture_registered_receipts registered history selected keys = Some snapshot <->
  registered selected = true /\ snapshot = capture_receipts history selected keys.
Proof.
  intros. unfold capture_registered_receipts.
  destruct (registered selected); split; intros; intuition congruence.
Qed.

Theorem unregistered_root_cannot_produce_even_an_empty_snapshot : forall registered history selected keys,
  registered selected = false -> capture_registered_receipts registered history selected keys = None.
Proof. intros. unfold capture_registered_receipts. now rewrite H. Qed.

Fixpoint lookup_captured_receipt (queried : key) (entries : list (key * option value))
    : option (option value) :=
  match entries with
  | [] => None
  | (stored, data) :: rest =>
      if key_eq queried stored then Some data else lookup_captured_receipt queried rest
  end.

Definition read_captured_receipt expected snapshot queried :=
  if root_eq expected (captured_receipt_root snapshot)
  then lookup_captured_receipt queried (captured_receipt_entries snapshot)
  else None.

Theorem captured_receipt_lookup_binds_the_selected_root : forall history selected keys queried,
  In queried keys ->
  read_captured_receipt selected (capture_receipts history selected keys) queried =
    Some (history selected queried).
Proof.
  intros history selected keys queried member. unfold read_captured_receipt, capture_receipts; simpl.
  destruct (root_eq selected selected); try contradiction.
  induction keys as [|head rest IH]; simpl in *; try contradiction.
  destruct (key_eq queried head) as [same|different].
  - now subst.
  - apply IH. intuition congruence.
Qed.

Theorem registered_capture_retains_exact_rooted_values : forall registered history selected keys snapshot queried,
  capture_registered_receipts registered history selected keys = Some snapshot ->
  In queried keys ->
  read_captured_receipt selected snapshot queried = Some (history selected queried).
Proof.
  intros. apply registered_receipt_capture_requires_root_evidence in H as [_ ->].
  now apply captured_receipt_lookup_binds_the_selected_root.
Qed.

Theorem unchecked_total_history_can_fabricate_absence : forall registered selected queried,
  registered selected = false ->
  read_captured_receipt selected (capture_receipts (fun _ _ => None) selected [queried]) queried = Some None /\
  capture_registered_receipts registered (fun _ _ => None) selected [queried] = None.
Proof.
  intros. split.
  - apply captured_receipt_lookup_binds_the_selected_root. simpl. auto.
  - now apply unregistered_root_cannot_produce_even_an_empty_snapshot.
Qed.

Theorem unrequested_receipt_is_not_a_captured_absence : forall history selected keys queried,
  ~ In queried keys ->
  read_captured_receipt selected (capture_receipts history selected keys) queried = None.
Proof.
  intros history selected keys queried absent. unfold read_captured_receipt, capture_receipts; simpl.
  destruct (root_eq selected selected); try contradiction.
  induction keys as [|head rest IH]; simpl in *; auto.
  destruct (key_eq queried head); [subst; tauto|apply IH; tauto].
Qed.

Theorem requested_absence_remains_explicit : forall history selected keys queried,
  In queried keys -> history selected queried = None ->
  read_captured_receipt selected (capture_receipts history selected keys) queried = Some None.
Proof. intros. rewrite captured_receipt_lookup_binds_the_selected_root by assumption. now rewrite H0. Qed.

Theorem captured_receipt_cannot_be_read_as_another_root : forall expected snapshot queried,
  expected <> captured_receipt_root snapshot -> read_captured_receipt expected snapshot queried = None.
Proof. intros. unfold read_captured_receipt. destruct (root_eq _ _); congruence. Qed.

Theorem independent_snapshot_reads_preserve_original_roots : forall history left right keys queried,
  In queried keys ->
  read_captured_receipt left (capture_receipts history left keys) queried = Some (history left queried) /\
  read_captured_receipt right (capture_receipts history right keys) queried = Some (history right queried).
Proof. intros. split; now apply captured_receipt_lookup_binds_the_selected_root. Qed.

Theorem same_root_history_extension_preserves_captured_values : forall before after selected keys,
  (forall queried, In queried keys -> before selected queried = after selected queried) ->
  capture_receipts before selected keys = capture_receipts after selected keys.
Proof.
  intros. unfold capture_receipts. f_equal. apply map_ext_in. intros. f_equal. now apply H.
Qed.
End RootedReceiptSnapshot.

Print Assumptions captured_receipt_lookup_binds_the_selected_root.
Print Assumptions registered_receipt_capture_requires_root_evidence.
Print Assumptions unregistered_root_cannot_produce_even_an_empty_snapshot.
Print Assumptions registered_capture_retains_exact_rooted_values.
Print Assumptions unchecked_total_history_can_fabricate_absence.
Print Assumptions unrequested_receipt_is_not_a_captured_absence.
Print Assumptions requested_absence_remains_explicit.
Print Assumptions captured_receipt_cannot_be_read_as_another_root.
Print Assumptions independent_snapshot_reads_preserve_original_roots.
Print Assumptions same_root_history_extension_preserves_captured_values.

Section ReceiptOccurrences.
Context {receipt : Type}.

Fixpoint remove_receipt_occurrence (index : nat) (receipts : list receipt)
    : option (receipt * list receipt) :=
  match index, receipts with
  | _, [] => None
  | 0, head :: tail => Some (head, tail)
  | S next, head :: tail =>
      match remove_receipt_occurrence next tail with
      | None => None
      | Some (selected, remaining) => Some (selected, head :: remaining)
      end
  end.

Theorem removed_receipt_has_exact_position : forall index receipts selected remaining,
  remove_receipt_occurrence index receipts = Some (selected, remaining) ->
  exists prefix suffix, length prefix = index /\
    receipts = prefix ++ selected :: suffix /\ remaining = prefix ++ suffix.
Proof.
  induction index as [|index IH]; intros receipts selected remaining accepted;
    destruct receipts as [|head tail]; simpl in accepted; try discriminate.
  - inversion accepted; subst. exists [], remaining. simpl. auto.
  - destruct (remove_receipt_occurrence index tail) as [[picked rest]|] eqn:step; try discriminate.
    inversion accepted; subst. specialize (IH _ _ _ step) as [prefix [suffix [size [original residual]]]].
    exists (head :: prefix), suffix. simpl. repeat split; congruence.
Qed.

Theorem receipt_removal_preserves_all_other_occurrences : forall index receipts selected remaining,
  remove_receipt_occurrence index receipts = Some (selected, remaining) ->
  Permutation receipts (selected :: remaining).
Proof.
  intros. apply removed_receipt_has_exact_position in H as [prefix [suffix [_ [-> ->]]]].
  apply Permutation_sym. apply Permutation_middle.
Qed.

Theorem receipt_removal_decreases_multiplicity_once : forall index receipts selected remaining,
  remove_receipt_occurrence index receipts = Some (selected, remaining) ->
  length receipts = S (length remaining).
Proof.
  intros. apply receipt_removal_preserves_all_other_occurrences in H.
  now apply Permutation_length in H.
Qed.

Theorem out_of_range_receipt_selection_fails : forall index receipts,
  length receipts <= index -> remove_receipt_occurrence index receipts = None.
Proof.
  induction index as [|index IH]; intros receipts outside; destruct receipts; simpl in *;
    try reflexivity; try lia. rewrite IH by lia. reflexivity.
Qed.

Theorem receipt_removal_preserves_physical_count_binding : forall index receipts selected remaining count,
  length receipts = S count ->
  remove_receipt_occurrence index receipts = Some (selected, remaining) ->
  length remaining = count.
Proof.
  intros. pose proof (receipt_removal_decreases_multiplicity_once _ _ _ _ H0). lia.
Qed.

Inductive receipt_consumption_history : list receipt -> list nat -> list receipt -> list receipt -> Prop :=
| ReceiptHistoryDone : forall receipts, receipt_consumption_history receipts [] [] receipts
| ReceiptHistoryStep : forall before index selected middle indices used after,
    remove_receipt_occurrence index before = Some (selected, middle) ->
    receipt_consumption_history middle indices used after ->
    receipt_consumption_history before (index :: indices) (selected :: used) after.

Theorem arbitrary_receipt_consumption_preserves_provenance : forall before indices used after,
  receipt_consumption_history before indices used after ->
  Permutation before (used ++ after).
Proof.
  intros before indices used after history. induction history.
  - apply Permutation_refl.
  - eapply Permutation_trans.
    + eapply receipt_removal_preserves_all_other_occurrences; eauto.
    + simpl. now apply perm_skip.
Qed.

Theorem snapshot_positions_cannot_be_persistent_receipt_keys : forall first second : receipt,
  remove_receipt_occurrence 0 [first; second] = Some (first, [second]) /\
  nth_error [first; second] 1 = Some second /\ nth_error [second] 0 = Some second.
Proof. intros. repeat split; reflexivity. Qed.
End ReceiptOccurrences.

Section OrderedCellProvenance.
Context {cell : Type}.

Definition consume_cell_prefix (count : nat) (cells : list cell) :=
  if (0 <? count) && (count <=? length cells)
  then Some (firstn count cells, skipn count cells) else None.

Theorem cell_prefix_consumption_keeps_exact_order : forall count cells used remaining,
  consume_cell_prefix count cells = Some (used, remaining) ->
  cells = used ++ remaining /\ length used = count /\
  remaining = skipn count cells /\ 0 < count.
Proof.
  intros count cells used remaining accepted. unfold consume_cell_prefix in accepted.
  destruct ((0 <? count) && (count <=? length cells)) eqn:fits; try discriminate.
  apply andb_true_iff in fits as [positive bounded].
  apply Nat.ltb_lt in positive. apply Nat.leb_le in bounded.
  inversion accepted; subst. repeat split; auto.
  - symmetry. apply firstn_skipn.
  - rewrite length_firstn. apply Nat.min_l. assumption.
Qed.

Theorem invalid_cell_prefix_cannot_change_provenance : forall count cells,
  count = 0 \/ length cells < count -> consume_cell_prefix count cells = None.
Proof.
  intros count cells [zero|excess]; unfold consume_cell_prefix.
  - subst count. reflexivity.
  - apply Nat.leb_gt in excess. rewrite excess, andb_false_r. reflexivity.
Qed.

Theorem cell_prefix_preserves_every_surviving_record : forall count cells used remaining index,
  consume_cell_prefix count cells = Some (used, remaining) ->
  nth_error remaining index = nth_error cells (count + index).
Proof.
  intros count cells used remaining index accepted.
  apply cell_prefix_consumption_keeps_exact_order in accepted as [same [size [tail positive]]].
  rewrite same. rewrite nth_error_app2 by lia.
  replace (count + index - length used) with index by lia. reflexivity.
Qed.

Theorem cell_prefix_matches_physical_tail_length : forall count cells physical used remaining,
  length cells = physical -> consume_cell_prefix count cells = Some (used, remaining) ->
  length remaining = physical - count.
Proof.
  intros count cells physical used remaining bound accepted.
  apply cell_prefix_consumption_keeps_exact_order in accepted as [same [size _]].
  rewrite same, length_app in bound. lia.
Qed.

Theorem consecutive_cell_consumption_preserves_original_provenance :
  forall first second cells used1 middle used2 remaining,
  consume_cell_prefix first cells = Some (used1, middle) ->
  consume_cell_prefix second middle = Some (used2, remaining) ->
  cells = (used1 ++ used2) ++ remaining.
Proof.
  intros first second cells used1 middle used2 remaining step1 step2.
  apply cell_prefix_consumption_keeps_exact_order in step1 as [-> _].
  apply cell_prefix_consumption_keeps_exact_order in step2 as [-> _].
  apply app_assoc.
Qed.
End OrderedCellProvenance.

Print Assumptions cell_prefix_consumption_keeps_exact_order.
Print Assumptions invalid_cell_prefix_cannot_change_provenance.
Print Assumptions cell_prefix_preserves_every_surviving_record.
Print Assumptions cell_prefix_matches_physical_tail_length.
Print Assumptions consecutive_cell_consumption_preserves_original_provenance.

Print Assumptions removed_receipt_has_exact_position.
Print Assumptions receipt_removal_preserves_all_other_occurrences.
Print Assumptions receipt_removal_decreases_multiplicity_once.
Print Assumptions out_of_range_receipt_selection_fails.
Print Assumptions receipt_removal_preserves_physical_count_binding.
Print Assumptions arbitrary_receipt_consumption_preserves_provenance.
Print Assumptions snapshot_positions_cannot_be_persistent_receipt_keys.

Section StackCaptureBinding.
Context {payload : Type}.
Context (payload_eq : forall x y : payload, {x = y} + {x <> y}).

Record stack_capture := {
  captured_instance : nat;
  captured_source : nat;
  captured_channel : nat;
  captured_index : nat;
  captured_persistent : bool;
  captured_payload : payload
}.

Definition stack_capture_eq : forall x y : stack_capture, {x = y} + {x <> y}.
Proof. decide equality; first [apply payload_eq | apply Bool.bool_dec | apply Nat.eq_dec]. Defined.

Definition stack_position capture := (captured_channel capture, captured_index capture).

Definition checked_stack_capture
    (inventory : (nat * nat) -> option stack_capture) (capture : stack_capture) :=
  match inventory (stack_position capture) with
  | None => false
  | Some live => if stack_capture_eq live capture then true else false
  end.

Theorem stack_capture_acceptance_binds_complete_live_record : forall inventory capture,
  checked_stack_capture inventory capture = true <->
  inventory (stack_position capture) = Some capture.
Proof.
  intros. unfold checked_stack_capture.
  destruct (inventory (stack_position capture)) as [live|]; [|intuition discriminate].
  destruct (stack_capture_eq live capture); subst; intuition congruence.
Qed.

Theorem stack_capture_acceptance_preserves_source_and_persistence : forall inventory capture live,
  inventory (stack_position capture) = Some live ->
  checked_stack_capture inventory capture = true ->
  captured_instance capture = captured_instance live /\
  captured_source capture = captured_source live /\
  captured_persistent capture = captured_persistent live /\
  captured_payload capture = captured_payload live.
Proof.
  intros inventory capture live stored accepted.
  apply stack_capture_acceptance_binds_complete_live_record in accepted.
  rewrite stored in accepted. inversion accepted. auto.
Qed.

Theorem altered_stack_capture_is_rejected : forall inventory capture live,
  inventory (stack_position capture) = Some live -> capture <> live ->
  checked_stack_capture inventory capture = false.
Proof.
  intros inventory capture live stored different.
  destruct (checked_stack_capture inventory capture) eqn:accepted; auto.
  apply stack_capture_acceptance_binds_complete_live_record in accepted.
  rewrite stored in accepted. congruence.
Qed.

Theorem accepted_distinct_captures_cannot_alias_one_position : forall inventory left right,
  checked_stack_capture inventory left = true ->
  checked_stack_capture inventory right = true ->
  captured_instance left <> captured_instance right ->
  stack_position left <> stack_position right.
Proof.
  intros inventory left right first second distinct same.
  apply stack_capture_acceptance_binds_complete_live_record in first, second.
  rewrite same, second in first. congruence.
Qed.

Theorem complete_stack_preflight_checks_every_selected_capture : forall inventory captures,
  forallb (checked_stack_capture inventory) captures = true <->
  forall capture, In capture captures -> inventory (stack_position capture) = Some capture.
Proof.
  intros. rewrite forallb_forall. split; intros ready capture present;
    specialize (ready capture present);
    now apply stack_capture_acceptance_binds_complete_live_record.
Qed.

Definition stack_capture_at_index (capture : stack_capture) (index : nat) : stack_capture :=
  {| captured_instance := captured_instance capture;
     captured_source := captured_source capture;
     captured_channel := captured_channel capture;
     captured_index := index;
     captured_persistent := captured_persistent capture;
     captured_payload := captured_payload capture |}.

Definition rebind_stack_capture (capture live : stack_capture) : option stack_capture :=
  let moved := stack_capture_at_index capture (captured_index live) in
  if stack_capture_eq moved live then Some live else None.

Theorem rebound_capture_changes_only_index : forall capture live result,
  rebind_stack_capture capture live = Some result ->
  result = live /\ live = stack_capture_at_index capture (captured_index live).
Proof.
  intros capture live result accepted. unfold rebind_stack_capture in accepted.
  destruct (stack_capture_eq (stack_capture_at_index capture (captured_index live)) live);
    inversion accepted; subst; auto.
Qed.

Theorem unchanged_stack_at_new_index_can_rebind : forall capture index,
  rebind_stack_capture capture (stack_capture_at_index capture index) =
  Some (stack_capture_at_index capture index).
Proof.
  intros. unfold rebind_stack_capture.
  change ((if stack_capture_eq (stack_capture_at_index capture index)
    (stack_capture_at_index capture index) then Some (stack_capture_at_index capture index)
    else None) = Some (stack_capture_at_index capture index)).
  destruct (stack_capture_eq (stack_capture_at_index capture index)
    (stack_capture_at_index capture index)); congruence.
Qed.

Theorem rebound_capture_preserves_strict_live_preflight : forall inventory capture live result,
  inventory (stack_position live) = Some live ->
  rebind_stack_capture capture live = Some result ->
  checked_stack_capture inventory result = true.
Proof.
  intros inventory capture live result stored accepted.
  apply rebound_capture_changes_only_index in accepted as [same _]. subst result.
  now apply stack_capture_acceptance_binds_complete_live_record.
Qed.

Theorem changed_source_cannot_rebind : forall capture live,
  captured_source capture <> captured_source live ->
  rebind_stack_capture capture live = None.
Proof.
  intros capture live different. unfold rebind_stack_capture.
  destruct (stack_capture_eq (stack_capture_at_index capture (captured_index live)) live) as [same|]; auto.
  apply (f_equal captured_source) in same. simpl in same. contradiction.
Qed.
End StackCaptureBinding.

Print Assumptions stack_capture_eq.
Print Assumptions stack_capture_acceptance_binds_complete_live_record.
Print Assumptions stack_capture_acceptance_preserves_source_and_persistence.
Print Assumptions altered_stack_capture_is_rejected.
Print Assumptions accepted_distinct_captures_cannot_alias_one_position.
Print Assumptions complete_stack_preflight_checks_every_selected_capture.
Print Assumptions rebound_capture_changes_only_index.
Print Assumptions unchanged_stack_at_new_index_can_rebind.
Print Assumptions rebound_capture_preserves_strict_live_preflight.
Print Assumptions changed_source_cannot_rebind.

Section PrepaidReceiptStorage.
Context {key value : Type}.
Context (key_eq : forall x y : key, {x = y} + {x <> y}).
Context (value_eq : forall x y : value, {x = y} + {x <> y}).

Definition receipt_store := key -> option value.

Definition receipt_value_eq : forall x y : option value, {x = y} + {x <> y}.
Proof. decide equality. Defined.

Record receipt_change := {
  receipt_key : key;
  receipt_expected : option value;
  receipt_replacement : option value
}.

Definition write_receipt (store : receipt_store) selected replacement : receipt_store :=
  fun queried => if key_eq queried selected then replacement else store queried.

Definition apply_receipt (store : receipt_store) change : option receipt_store :=
  if receipt_value_eq (store (receipt_key change)) (receipt_expected change)
  then Some (write_receipt store (receipt_key change) (receipt_replacement change))
  else None.

Fixpoint prepare_receipts store changes :=
  match changes with
  | [] => Some store
  | change :: rest =>
      match apply_receipt store change with
      | None => None
      | Some next => prepare_receipts next rest
      end
  end.

Definition publish_receipts store changes :=
  match prepare_receipts store changes with
  | Some next => (next, true)
  | None => (store, false)
  end.

Definition preflight_receipts store changes :=
  Forall (fun change => store (receipt_key change) = receipt_expected change) changes.

Theorem receipt_write_reads_exact_replacement : forall store selected replacement,
  write_receipt store selected replacement selected = replacement.
Proof. intros. unfold write_receipt. destruct (key_eq selected selected); congruence. Qed.

Theorem receipt_write_preserves_other_keys : forall store selected replacement queried,
  queried <> selected -> write_receipt store selected replacement queried = store queried.
Proof. intros. unfold write_receipt. destruct (key_eq queried selected); congruence. Qed.

Theorem receipt_acceptance_requires_exact_prior_value : forall store change next,
  apply_receipt store change = Some next ->
  store (receipt_key change) = receipt_expected change /\
  next (receipt_key change) = receipt_replacement change.
Proof.
  intros store change next accepted. unfold apply_receipt in accepted.
  destruct (receipt_value_eq _ _) as [same|different]; try discriminate.
  inversion accepted; subst. split; auto. apply receipt_write_reads_exact_replacement.
Qed.

Theorem stale_receipt_replacement_is_rejected : forall store change,
  store (receipt_key change) <> receipt_expected change -> apply_receipt store change = None.
Proof. intros. unfold apply_receipt. destruct (receipt_value_eq _ _); congruence. Qed.

Theorem changed_receipt_cannot_repeat_the_old_transition : forall store change next,
  apply_receipt store change = Some next ->
  receipt_replacement change <> receipt_expected change -> apply_receipt next change = None.
Proof.
  intros store change next accepted different.
  apply receipt_acceptance_requires_exact_prior_value in accepted as [_ replaced].
  apply stale_receipt_replacement_is_rejected. now rewrite replaced.
Qed.

Theorem disjoint_receipt_writes_commute : forall store left right lv rv queried,
  left <> right ->
  write_receipt (write_receipt store left lv) right rv queried =
  write_receipt (write_receipt store right rv) left lv queried.
Proof.
  intros. unfold write_receipt.
  destruct (key_eq queried right), (key_eq queried left); congruence.
Qed.

Theorem disjoint_receipt_write_preserves_comparison : forall store selected replacement change,
  receipt_key change <> selected ->
  write_receipt store selected replacement (receipt_key change) = store (receipt_key change).
Proof. intros. now apply receipt_write_preserves_other_keys. Qed.

Theorem receipt_preflight_ignores_unmentioned_write : forall changes store selected replacement,
  ~ In selected (map receipt_key changes) ->
  (preflight_receipts store changes <->
   preflight_receipts (write_receipt store selected replacement) changes).
Proof.
  intros changes store selected replacement absent.
  unfold preflight_receipts. repeat rewrite Forall_forall.
  assert (unchanged : forall change, In change changes ->
    write_receipt store selected replacement (receipt_key change) = store (receipt_key change)).
  { intros change member. apply receipt_write_preserves_other_keys.
    intro same. apply absent. rewrite <- same. now apply in_map. }
  split; intros ready change member.
  - rewrite unchanged by assumption. now apply ready.
  - specialize (ready change member). rewrite unchanged in ready by assumption. exact ready.
Qed.

Theorem distinct_receipt_preflight_matches_sequential_acceptance : forall changes store,
  NoDup (map receipt_key changes) ->
  (preflight_receipts store changes <-> exists next, prepare_receipts store changes = Some next).
Proof.
  induction changes as [|change rest IH]; intros store distinct.
  - split; intros; [exists store; reflexivity|constructor].
  - inversion distinct as [|selected keys absent tail_distinct]; subst.
    unfold preflight_receipts at 1. rewrite Forall_cons_iff.
    change ((store (receipt_key change) = receipt_expected change /\ preflight_receipts store rest) <->
      exists next, prepare_receipts store (change :: rest) = Some next).
    simpl. unfold apply_receipt at 1.
    destruct (receipt_value_eq (store (receipt_key change)) (receipt_expected change)) as [same|different].
    + rewrite <- (IH (write_receipt store (receipt_key change) (receipt_replacement change)) tail_distinct).
      rewrite <- receipt_preflight_ignores_unmentioned_write by exact absent. tauto.
    + split.
      * intros [contradiction _]. contradiction.
      * intros [next impossible]. discriminate.
Qed.

Theorem receipt_batch_composes : forall prefix store suffix,
  prepare_receipts store (prefix ++ suffix) =
  match prepare_receipts store prefix with
  | None => None
  | Some middle => prepare_receipts middle suffix
  end.
Proof.
  induction prefix as [|change rest IH]; intros store suffix; simpl; auto.
  destruct (apply_receipt store change); auto.
Qed.

Theorem receipt_batch_preserves_unmentioned_keys : forall changes store next queried,
  prepare_receipts store changes = Some next ->
  ~ In queried (map receipt_key changes) -> next queried = store queried.
Proof.
  induction changes as [|change rest IH]; intros store next queried accepted absent.
  - simpl in accepted. inversion accepted. reflexivity.
  - simpl in accepted, absent.
    destruct (apply_receipt store change) as [middle|] eqn:first; try discriminate.
    rewrite (IH middle next queried accepted) by tauto.
    unfold apply_receipt in first. destruct (receipt_value_eq _ _); try discriminate.
    inversion first; subst. apply receipt_write_preserves_other_keys. intuition.
Qed.

Theorem failed_receipt_batch_publishes_no_prefix : forall prefix suffix store middle,
  prepare_receipts store prefix = Some middle -> prepare_receipts middle suffix = None ->
  publish_receipts store (prefix ++ suffix) = (store, false).
Proof.
  intros prefix suffix store middle prepared failed.
  unfold publish_receipts. rewrite receipt_batch_composes, prepared, failed. reflexivity.
Qed.

Theorem receipt_publication_preserves_unmentioned_keys : forall changes store queried,
  ~ In queried (map receipt_key changes) ->
  fst (publish_receipts store changes) queried = store queried.
Proof.
  intros changes store queried absent. unfold publish_receipts.
  destruct (prepare_receipts store changes) eqn:prepared; simpl; auto.
  eapply receipt_batch_preserves_unmentioned_keys; eauto.
Qed.
End PrepaidReceiptStorage.

Print Assumptions receipt_write_reads_exact_replacement.
Print Assumptions receipt_write_preserves_other_keys.
Print Assumptions receipt_acceptance_requires_exact_prior_value.
Print Assumptions stale_receipt_replacement_is_rejected.
Print Assumptions changed_receipt_cannot_repeat_the_old_transition.
Print Assumptions disjoint_receipt_writes_commute.
Print Assumptions disjoint_receipt_write_preserves_comparison.
Print Assumptions receipt_preflight_ignores_unmentioned_write.
Print Assumptions distinct_receipt_preflight_matches_sequential_acceptance.
Print Assumptions receipt_batch_composes.
Print Assumptions receipt_batch_preserves_unmentioned_keys.
Print Assumptions failed_receipt_batch_publishes_no_prefix.
Print Assumptions receipt_publication_preserves_unmentioned_keys.
