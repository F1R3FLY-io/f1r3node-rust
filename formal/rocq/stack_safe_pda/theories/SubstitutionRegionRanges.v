From Stdlib Require Import List Arith Bool Lia.

Import ListNotations.
Set Implicit Arguments.

Module SubstitutionRegionRanges.

Record region := Region {
  owner : nat;
  first : nat;
  last : nat
}.

Definition valid (bound : nat) (r : region) : Prop :=
  first r <= last r /\ last r <= bound.

Definition local_valid (r : region) (lo hi : nat) : Prop :=
  lo <= hi /\ hi <= last r - first r.

Definition child (r : region) (lo hi : nat) : region :=
  Region (owner r) (first r + lo) (first r + hi).

Definition subregion (limit : nat) (r : region) (lo hi : nat)
  : option region :=
  if (lo <=? hi) && (hi <=? last r - first r) &&
     (first r + lo <=? limit) && (first r + hi <=? limit)
  then Some (child r lo hi)
  else None.

Theorem subregion_success_exact :
  forall limit r lo hi out,
    subregion limit r lo hi = Some out ->
    out = child r lo hi /\ local_valid r lo hi /\ last out <= limit.
Proof.
  intros limit r lo hi out H.
  unfold subregion in H.
  destruct (_ && _) eqn:E; inversion H; subst; clear H.
  repeat rewrite andb_true_iff in E.
  repeat rewrite Nat.leb_le in E.
  unfold local_valid, child; simpl; intuition.
Qed.

Theorem subregion_complete :
  forall limit bound r lo hi,
    valid bound r -> bound <= limit -> local_valid r lo hi ->
    subregion limit r lo hi = Some (child r lo hi).
Proof.
  intros limit bound r lo hi V B L.
  unfold valid in V; unfold local_valid in L.
  unfold subregion.
  assert (G : (lo <=? hi) && (hi <=? last r - first r) &&
      (first r + lo <=? limit) && (first r + hi <=? limit) = true).
  { repeat rewrite andb_true_iff; repeat rewrite Nat.leb_le; intuition lia. }
  now rewrite G.
Qed.

Theorem successful_child_is_valid :
  forall limit bound r lo hi out,
    valid bound r ->
    subregion limit r lo hi = Some out ->
    valid bound out.
Proof.
  intros limit bound r lo hi out V H.
  apply subregion_success_exact in H.
  destruct H as [-> [L _]].
  unfold valid, local_valid, child in *; simpl in *; intuition lia.
Qed.

Theorem successful_child_retains_owner :
  forall limit r lo hi out,
    subregion limit r lo hi = Some out -> owner out = owner r.
Proof.
  intros limit r lo hi out H.
  apply subregion_success_exact in H.
  destruct H as [-> _]; reflexivity.
Qed.

Theorem reversed_range_refused :
  forall limit r lo hi,
    hi < lo -> subregion limit r lo hi = None.
Proof.
  intros limit r lo hi H; unfold subregion.
  assert (E : (lo <=? hi) = false) by (apply Nat.leb_gt; lia).
  now rewrite E.
Qed.

Theorem beyond_parent_refused :
  forall limit r lo hi,
    last r - first r < hi -> subregion limit r lo hi = None.
Proof.
  intros limit r lo hi H; unfold subregion.
  assert (E : (hi <=? last r - first r) = false) by (apply Nat.leb_gt; lia).
  rewrite E; repeat rewrite andb_false_r; reflexivity.
Qed.

Theorem absolute_end_overflow_refused :
  forall limit r lo hi,
    limit < first r + hi -> subregion limit r lo hi = None.
Proof.
  intros limit r lo hi H; unfold subregion.
  assert (E : (first r + hi <=? limit) = false) by (apply Nat.leb_gt; lia).
  now rewrite E, andb_false_r.
Qed.

Theorem empty_at_end_is_valid :
  forall limit bound r,
    valid bound r -> bound <= limit ->
    subregion limit r (last r - first r) (last r - first r) =
      Some (Region (owner r) (last r) (last r)).
Proof.
  intros limit bound [o a b] V B.
  rewrite (@subregion_complete limit bound).
  - unfold child; simpl; unfold valid in V; simpl in V.
    replace (a + (b - a)) with b by lia; reflexivity.
  - exact V.
  - exact B.
  - unfold local_valid; simpl; lia.
Qed.

Theorem nested_child_composes :
  forall r lo hi inner_lo inner_hi,
    child (child r lo hi) inner_lo inner_hi =
    child r (lo + inner_lo) (lo + inner_hi).
Proof.
  intros; unfold child; simpl.
  now rewrite !Nat.add_assoc.
Qed.

Theorem nested_local_bounds_compose :
  forall bound r lo hi inner_lo inner_hi,
    valid bound r ->
    local_valid r lo hi ->
    local_valid (child r lo hi) inner_lo inner_hi ->
    local_valid r (lo + inner_lo) (lo + inner_hi).
Proof.
  intros bound r lo hi inner_lo inner_hi V A B.
  unfold valid, local_valid, child in *; simpl in *; intuition lia.
Qed.

Definition register_cut (cuts : list region) (cut : region) :=
  (length cuts, cuts ++ [cut]).

Theorem registered_cut_has_exact_ordinal :
  forall cuts cut,
    nth_error (snd (register_cut cuts cut)) (fst (register_cut cuts cut)) = Some cut.
Proof.
  intros cuts cut; unfold register_cut; simpl.
  rewrite nth_error_app2 by lia.
  now rewrite Nat.sub_diag.
Qed.

Theorem registration_preserves_previous_ordinals :
  forall cuts cut ordinal,
    ordinal < length cuts ->
    nth_error (snd (register_cut cuts cut)) ordinal = nth_error cuts ordinal.
Proof.
  intros; unfold register_cut; simpl.
  now rewrite nth_error_app1.
Qed.

Definition selected_cuts (cuts : list region) (start count : nat) :=
  firstn count (skipn start cuts).

Theorem value_group_has_exact_order :
  forall prefix values suffix,
    selected_cuts (prefix ++ values ++ suffix) (length prefix) (length values) = values.
Proof.
  intros prefix values suffix; unfold selected_cuts.
  rewrite skipn_app, skipn_all, Nat.sub_diag; simpl.
  rewrite firstn_app, firstn_all, Nat.sub_diag, app_nil_r; reflexivity.
Qed.

Theorem selected_group_preserves_owner :
  forall cuts start count o,
    Forall (fun r => owner r = o) cuts ->
    Forall (fun r => owner r = o) (selected_cuts cuts start count).
Proof.
  intros cuts start count o H.
  apply Forall_forall; intros r I.
  apply Forall_forall with (x := r) in H; auto.
  unfold selected_cuts in I.
  assert (A : In r (firstn count (skipn start cuts) ++
                    skipn count (skipn start cuts))).
  { apply in_or_app; left; exact I. }
  rewrite firstn_skipn in A.
  assert (B : In r (firstn start cuts ++ skipn start cuts)).
  { apply in_or_app; right; exact A. }
  now rewrite firstn_skipn in B.
Qed.

Example wrong_owner_is_observable :
  owner (child (Region 7 3 9) 1 2) <> owner (Region 8 4 5).
Proof. discriminate. Qed.

Example absolute_offset_mutant_is_observable :
  child (Region 7 3 9) 1 2 <> Region 7 1 2.
Proof. discriminate. Qed.

Example dropped_empty_cut_is_observable :
  snd (register_cut [] (Region 7 9 9)) <> [].
Proof. discriminate. Qed.

Example swapped_value_ordinals_are_observable :
  selected_cuts [Region 7 1 2; Region 7 3 4] 0 2 <>
  [Region 7 3 4; Region 7 1 2].
Proof. discriminate. Qed.

Print Assumptions subregion_success_exact.
Print Assumptions subregion_complete.
Print Assumptions successful_child_is_valid.
Print Assumptions successful_child_retains_owner.
Print Assumptions reversed_range_refused.
Print Assumptions beyond_parent_refused.
Print Assumptions absolute_end_overflow_refused.
Print Assumptions empty_at_end_is_valid.
Print Assumptions nested_child_composes.
Print Assumptions nested_local_bounds_compose.
Print Assumptions registered_cut_has_exact_ordinal.
Print Assumptions registration_preserves_previous_ordinals.
Print Assumptions value_group_has_exact_order.
Print Assumptions selected_group_preserves_owner.
Print Assumptions wrong_owner_is_observable.
Print Assumptions absolute_offset_mutant_is_observable.
Print Assumptions dropped_empty_cut_is_observable.
Print Assumptions swapped_value_ordinals_are_observable.

End SubstitutionRegionRanges.
