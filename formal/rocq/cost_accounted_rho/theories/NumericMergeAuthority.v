From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia Sorting.Permutation.
Import ListNotations.

Definition numeric_region := (nat * nat)%type.
Definition numeric_region_eq_dec : forall x y : numeric_region, {x = y} + {x <> y}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition numeric_authority := option (list numeric_region).

Definition numeric_regions (authority : numeric_authority) : list numeric_region :=
  match authority with Some regions => regions | None => [] end.

Definition numeric_present (authority : numeric_authority) : bool :=
  match authority with Some _ => true | None => false end.

Definition numeric_inputs (authorities : list numeric_authority) : list numeric_region :=
  flat_map numeric_regions authorities.

Definition numeric_bindings_compatible (regions : list numeric_region) : bool :=
  forallb (fun left => forallb
    (fun right => negb (Nat.eqb (fst left) (fst right)) || Nat.eqb (snd left) (snd right))
    regions) regions.

Definition numeric_combine (authorities : list numeric_authority)
  : option numeric_authority :=
  let regions := numeric_inputs authorities in
  if numeric_bindings_compatible regions then
    Some (if existsb numeric_present authorities
          then Some (nodup numeric_region_eq_dec regions)
          else None)
  else None.

Lemma numeric_input_implies_presence : forall authorities region,
  In region (numeric_inputs authorities) -> existsb numeric_present authorities = true.
Proof.
  intros authorities region present. unfold numeric_inputs in present.
  apply in_flat_map in present. destruct present as [authority [source member]].
  apply existsb_exists. exists authority. split; [exact source|].
  destruct authority; [reflexivity|contradiction].
Qed.

Theorem numeric_success_preserves_exact_membership : forall authorities result region,
  numeric_combine authorities = Some result ->
  (In region (numeric_regions result) <-> In region (numeric_inputs authorities)).
Proof.
  intros authorities result region success. unfold numeric_combine in success.
  destruct (numeric_bindings_compatible (numeric_inputs authorities)) eqn:valid;
    [|discriminate].
  destruct (existsb numeric_present authorities) eqn:presence; inversion success; subst; simpl.
  - apply nodup_In.
  - split; [contradiction|]. intro member.
    apply numeric_input_implies_presence in member. congruence.
Qed.

Theorem numeric_success_preserves_presence : forall authorities result,
  numeric_combine authorities = Some result ->
  numeric_present result = existsb numeric_present authorities.
Proof.
  intros authorities result success. unfold numeric_combine in success.
  destruct (numeric_bindings_compatible (numeric_inputs authorities)); [|discriminate].
  destruct (existsb numeric_present authorities); inversion success; reflexivity.
Qed.

Theorem numeric_conflicting_binding_rejects : forall authorities id left right,
  In (id, left) (numeric_inputs authorities) ->
  In (id, right) (numeric_inputs authorities) -> left <> right ->
  numeric_combine authorities = None.
Proof.
  intros authorities id left right has_left has_right different.
  unfold numeric_combine.
  destruct (numeric_bindings_compatible (numeric_inputs authorities)) eqn:valid;
    [|reflexivity].
  unfold numeric_bindings_compatible in valid.
  rewrite forallb_forall in valid. specialize (valid (id,left) has_left).
  rewrite forallb_forall in valid. specialize (valid (id,right) has_right).
  simpl in valid. rewrite Nat.eqb_refl in valid. simpl in valid.
  apply Nat.eqb_eq in valid. contradiction.
Qed.

Theorem numeric_success_has_unique_bindings : forall authorities result,
  numeric_combine authorities = Some result -> NoDup (numeric_regions result).
Proof.
  intros authorities result success. unfold numeric_combine in success.
  destruct (numeric_bindings_compatible (numeric_inputs authorities)); [|discriminate].
  destruct (existsb numeric_present authorities); inversion success; subst; simpl.
  - apply NoDup_nodup.
  - constructor.
Qed.

Lemma numeric_nodup_length_bound : forall regions,
  length (nodup numeric_region_eq_dec regions) <= length regions.
Proof.
  induction regions as [|region rest IH]; simpl; [lia|].
  destruct (in_dec numeric_region_eq_dec region rest); simpl; lia.
Qed.

Theorem numeric_output_size_bound : forall authorities result,
  numeric_combine authorities = Some result ->
  length (numeric_regions result) <= length (numeric_inputs authorities).
Proof.
  intros authorities result success. unfold numeric_combine in success.
  destruct (numeric_bindings_compatible (numeric_inputs authorities)); [|discriminate].
  destruct (existsb numeric_present authorities); inversion success; subst; simpl.
  - apply numeric_nodup_length_bound.
  - lia.
Qed.

Theorem numeric_permutations_preserve_membership : forall left right lout rout,
  Permutation left right -> numeric_combine left = Some lout ->
  numeric_combine right = Some rout ->
  forall region, In region (numeric_regions lout) <-> In region (numeric_regions rout).
Proof.
  intros left right lout rout order lok rok region.
  rewrite (numeric_success_preserves_exact_membership _ _ _ lok).
  rewrite (numeric_success_preserves_exact_membership _ _ _ rok).
  unfold numeric_inputs. repeat rewrite in_flat_map.
  split; intros [authority [source member]]; exists authority; split; try assumption.
  - eapply Permutation_in; eauto.
  - eapply Permutation_in; [apply Permutation_sym; exact order|exact source].
Qed.

Theorem numeric_repeated_contributors_preserve_membership : forall authorities original repeated,
  numeric_combine authorities = Some original ->
  numeric_combine (authorities ++ authorities) = Some repeated ->
  forall region, In region (numeric_regions original) <-> In region (numeric_regions repeated).
Proof.
  intros authorities original repeated once twice region.
  rewrite (numeric_success_preserves_exact_membership _ _ _ once).
  rewrite (numeric_success_preserves_exact_membership _ _ _ twice).
  unfold numeric_inputs. rewrite flat_map_app, in_app_iff. tauto.
Qed.

Theorem numeric_consumed_authority_does_not_return : forall authorities result region,
  numeric_combine authorities = Some result ->
  (forall authority, In authority authorities -> ~ In region (numeric_regions authority)) ->
  ~ In region (numeric_regions result).
Proof.
  intros authorities result region success absent present.
  apply (numeric_success_preserves_exact_membership _ _ _ success) in present.
  unfold numeric_inputs in present. apply in_flat_map in present.
  destruct present as [authority [source member]]. exact (absent authority source member).
Qed.

Theorem numeric_distinct_occurrences_remain_distinct : forall authorities result a b signature,
  numeric_combine authorities = Some result -> a <> b ->
  In (a, signature) (numeric_inputs authorities) ->
  In (b, signature) (numeric_inputs authorities) ->
  In (a, signature) (numeric_regions result) /\
  In (b, signature) (numeric_regions result) /\ (a, signature) <> (b, signature).
Proof.
  intros authorities result a b signature success different left right.
  split; [apply (numeric_success_preserves_exact_membership _ _ _ success); exact left|].
  split; [apply (numeric_success_preserves_exact_membership _ _ _ success); exact right|].
  congruence.
Qed.

Example numeric_explicit_empty_is_not_absence :
  numeric_combine [None] = Some None /\
  numeric_combine [Some []] = Some (Some []).
Proof. split; reflexivity. Qed.

Example numeric_unit_regions_are_preserved :
  numeric_combine [Some [(1,0)]; Some [(2,0)]] = Some (Some [(1,0); (2,0)]).
Proof. reflexivity. Qed.

Fixpoint numeric_region_weight (weight : numeric_region -> nat)
  (regions : list numeric_region) : nat :=
  match regions with
  | [] => 0
  | region :: rest => weight region + numeric_region_weight weight rest
  end.

Lemma numeric_weight_permutation : forall weight left right,
  Permutation left right ->
  numeric_region_weight weight left = numeric_region_weight weight right.
Proof.
  intros weight left right order. induction order; simpl; lia.
Qed.

Theorem numeric_success_is_retained_permutation : forall authorities result,
  numeric_combine authorities = Some result ->
  Permutation (numeric_regions result)
    (nodup numeric_region_eq_dec (numeric_inputs authorities)).
Proof.
  intros authorities result success. apply NoDup_Permutation.
  - eapply numeric_success_has_unique_bindings; exact success.
  - apply NoDup_nodup.
  - intro region. rewrite nodup_In.
    apply numeric_success_preserves_exact_membership. exact success.
Qed.

Theorem numeric_success_preserves_weighted_obligations : forall authorities result weight,
  numeric_combine authorities = Some result ->
  numeric_region_weight weight (numeric_regions result) =
  numeric_region_weight weight (nodup numeric_region_eq_dec (numeric_inputs authorities)).
Proof.
  intros authorities result weight success.
  apply numeric_weight_permutation.
  apply numeric_success_is_retained_permutation. exact success.
Qed.

Lemma numeric_nodup_weight_bound : forall regions weight,
  numeric_region_weight weight (nodup numeric_region_eq_dec regions) <=
  numeric_region_weight weight regions.
Proof.
  induction regions as [|region rest IH]; intros weight; simpl; [lia|].
  specialize (IH weight). destruct (in_dec numeric_region_eq_dec region rest); simpl; lia.
Qed.

Theorem numeric_output_weight_bound : forall authorities result weight,
  numeric_combine authorities = Some result ->
  numeric_region_weight weight (numeric_regions result) <=
  numeric_region_weight weight (numeric_inputs authorities).
Proof.
  intros authorities result weight success.
  rewrite (numeric_success_preserves_weighted_obligations _ _ _ success).
  apply numeric_nodup_weight_bound.
Qed.

Definition numeric_wallet_units (wallet : nat) (region : numeric_region) : nat :=
  if Nat.eqb wallet 0 then 0 else if Nat.eqb (snd region) wallet then 1 else 0.

Theorem numeric_wallet_compute_and_byte_obligations : forall authorities result wallet bytes,
  numeric_combine authorities = Some result ->
  (1 + bytes) * numeric_region_weight (numeric_wallet_units wallet) (numeric_regions result) =
  (1 + bytes) * numeric_region_weight (numeric_wallet_units wallet)
    (nodup numeric_region_eq_dec (numeric_inputs authorities)).
Proof.
  intros authorities result wallet bytes success.
  rewrite (numeric_success_preserves_weighted_obligations _ _ _ success). reflexivity.
Qed.

Theorem numeric_compatible_regrouping_preserves_regions :
  forall left right left_result right_result flat_result grouped_result,
  numeric_combine left = Some left_result ->
  numeric_combine right = Some right_result ->
  numeric_combine (left ++ right) = Some flat_result ->
  numeric_combine [left_result; right_result] = Some grouped_result ->
  Permutation (numeric_regions flat_result) (numeric_regions grouped_result).
Proof.
  intros left right left_result right_result flat_result grouped_result
    left_ok right_ok flat_ok grouped_ok.
  apply NoDup_Permutation.
  - eapply numeric_success_has_unique_bindings; exact flat_ok.
  - eapply numeric_success_has_unique_bindings; exact grouped_ok.
  - intro region.
    rewrite (numeric_success_preserves_exact_membership _ _ _ flat_ok).
    rewrite (numeric_success_preserves_exact_membership _ _ _ grouped_ok).
    unfold numeric_inputs. rewrite flat_map_app. simpl.
    repeat rewrite in_app_iff. simpl.
    rewrite (numeric_success_preserves_exact_membership _ _ _ left_ok).
    rewrite (numeric_success_preserves_exact_membership _ _ _ right_ok).
    tauto.
Qed.

Print Assumptions numeric_success_is_retained_permutation.
Print Assumptions numeric_success_preserves_weighted_obligations.
Print Assumptions numeric_output_weight_bound.
Print Assumptions numeric_wallet_compute_and_byte_obligations.
Print Assumptions numeric_compatible_regrouping_preserves_regions.
Print Assumptions numeric_success_preserves_exact_membership.
Print Assumptions numeric_success_preserves_presence.
Print Assumptions numeric_conflicting_binding_rejects.
Print Assumptions numeric_success_has_unique_bindings.
Print Assumptions numeric_output_size_bound.
Print Assumptions numeric_permutations_preserve_membership.
Print Assumptions numeric_repeated_contributors_preserve_membership.
Print Assumptions numeric_consumed_authority_does_not_return.
Print Assumptions numeric_distinct_occurrences_remain_distinct.
