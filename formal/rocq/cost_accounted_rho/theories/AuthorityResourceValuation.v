From Stdlib Require Import Arith.PeanoNat Lists.List Lia Sorting.Permutation.
From CostAccountedRho Require Import CostAccountedSyntax SignatureMonoid CAJoinConservation.
Import ListNotations.

Fixpoint authority_units (s : sig) : nat :=
  match s with
  | SUnit => 0
  | SGround _ | SQuote _ => 1
  | SAnd lhs rhs => authority_units lhs + authority_units rhs
  end.

Definition authority_value (unit_price : nat) (s : sig) : nat :=
  unit_price * authority_units s.

Definition presentation_value (unit_price : nat) (cells : list sig) : nat :=
  fold_right (fun cell total => authority_value unit_price cell + total) 0 cells.

Theorem authority_units_respects_signature_equivalence : forall left right,
  sig_equiv left right -> authority_units left = authority_units right.
Proof.
  intros left right equivalent. induction equivalent; simpl in *; lia.
Qed.

Theorem authority_value_respects_signature_equivalence : forall price left right,
  sig_equiv left right -> authority_value price left = authority_value price right.
Proof.
  intros price left right equivalent. unfold authority_value.
  now rewrite (authority_units_respects_signature_equivalence left right equivalent).
Qed.

Theorem compound_value_is_additive : forall price left right,
  authority_value price (SAnd left right) =
  authority_value price left + authority_value price right.
Proof. intros. unfold authority_value. simpl. nia. Qed.

Theorem presentation_value_append : forall price left right,
  presentation_value price (left ++ right) =
  presentation_value price left + presentation_value price right.
Proof.
  intros price left. induction left; intros right; simpl in *; [reflexivity|].
  unfold presentation_value in *. simpl in *. rewrite IHleft. lia.
Qed.

Theorem presentation_value_permutation : forall price left right,
  Permutation left right -> presentation_value price left = presentation_value price right.
Proof.
  intros price left right same. induction same;
    unfold presentation_value in *; simpl in *; lia.
Qed.

Theorem authority_value_matches_existing_atoms : forall price s,
  authority_value price s = presentation_value price (sig_atoms s).
Proof.
  intros price s. induction s; simpl; try (unfold authority_value, presentation_value; simpl; lia).
  rewrite presentation_value_append, <- IHs1, <- IHs2.
  apply compound_value_is_additive.
Qed.

Theorem join_value_is_presentation_independent : forall price receiver senders,
  authority_value price (combined_key receiver senders) =
  presentation_value price (sig_atoms receiver ++ concat (map sig_atoms senders)).
Proof.
  intros. rewrite authority_value_matches_existing_atoms.
  apply presentation_value_permutation. apply join_authority_conserved.
Qed.

Inductive resource_regroup : list sig -> list sig -> Prop :=
| regroup_split : forall before after left right,
    resource_regroup (before ++ SAnd left right :: after)
                    (before ++ left :: right :: after)
| regroup_join : forall before after left right,
    resource_regroup (before ++ left :: right :: after)
                    (before ++ SAnd left right :: after)
| regroup_order : forall before after,
    Permutation before after -> resource_regroup before after
| regroup_signature : forall before after left right,
    sig_equiv left right ->
    resource_regroup (before ++ left :: after) (before ++ right :: after).

Theorem resource_regroup_preserves_value : forall price before after,
  resource_regroup before after -> presentation_value price before = presentation_value price after.
Proof.
  intros price before after step. destruct step.
  - rewrite !presentation_value_append. unfold presentation_value at 2 4. simpl.
    rewrite compound_value_is_additive. lia.
  - rewrite !presentation_value_append. unfold presentation_value at 2 4. simpl.
    rewrite compound_value_is_additive. lia.
  - now apply presentation_value_permutation.
  - rewrite !presentation_value_append. unfold presentation_value at 2 4. simpl.
    now rewrite (authority_value_respects_signature_equivalence price left right H).
Qed.

Inductive resource_regroup_history : list sig -> list sig -> Prop :=
| regroup_history_refl : forall cells, resource_regroup_history cells cells
| regroup_history_next : forall start middle finish,
    resource_regroup start middle -> resource_regroup_history middle finish ->
    resource_regroup_history start finish.

Theorem arbitrary_regroup_history_preserves_value : forall price before after,
  resource_regroup_history before after ->
  presentation_value price before = presentation_value price after.
Proof.
  intros price before after history. induction history; [reflexivity|].
  rewrite (resource_regroup_preserves_value price start middle H). assumption.
Qed.

Fixpoint stack_resource_value (price : nat) (stack : token) : nat :=
  match stack with
  | TUnit => 0
  | TGate signature tail => authority_value price signature + stack_resource_value price tail
  end.

Theorem stack_concatenation_preserves_value : forall price left right,
  stack_resource_value price (tok_concat left right) =
  stack_resource_value price left + stack_resource_value price right.
Proof. intros price left. induction left; intros right; simpl; [reflexivity|]. rewrite IHleft. lia. Qed.

Theorem stack_consumption_accounts_for_exact_head : forall price signature tail,
  stack_resource_value price (TGate signature tail) =
  authority_value price signature + stack_resource_value price tail.
Proof. reflexivity. Qed.

Theorem positive_component_cannot_disappear : forall price left right,
  0 < price -> 0 < authority_units right ->
  authority_value price left < authority_value price (SAnd left right).
Proof. intros. unfold authority_value. simpl. nia. Qed.

Theorem repeated_authority_is_not_deduplicated : forall price s,
  authority_value price (SAnd s s) = 2 * authority_value price s.
Proof. intros. rewrite compound_value_is_additive. lia. Qed.

Theorem ground_and_quote_have_same_unit_value : forall price ground quoted,
  authority_value price (SGround ground) = authority_value price (SQuote quoted).
Proof. reflexivity. Qed.

Example cell_count_does_not_conserve_regrouping :
  length [SAnd (SGround []) (SQuote [])] < length [SGround []; SQuote []] /\
  presentation_value 1 [SAnd (SGround []) (SQuote [])] =
  presentation_value 1 [SGround []; SQuote []].
Proof. simpl. unfold presentation_value, authority_value. simpl. lia. Qed.

Example equal_value_does_not_establish_authority :
  authority_value 1 (SGround [true]) = authority_value 1 (SGround [false]) /\
  SGround [true] <> SGround [false].
Proof. split; [reflexivity|discriminate]. Qed.

Fixpoint valuation_samples (depth : nat) : list sig :=
  match depth with
  | 0 => [SUnit; SGround [true]; SQuote [false]]
  | S rest =>
      let smaller := valuation_samples rest in
      smaller ++ flat_map (fun left => map (SAnd left) smaller) smaller
  end.

Definition valuation_property (price : nat) (s : sig) : bool :=
  Nat.eqb (authority_value price s) (presentation_value price (sig_atoms s)) &&
  Nat.eqb (authority_value price (SAnd s SUnit)) (authority_value price s) &&
  Nat.eqb (authority_value price (SAnd s s)) (2 * authority_value price s).

Example generated_valuation_regression :
  forallb (fun price => forallb (valuation_property price) (valuation_samples 2))
          [0; 1; 2; 17; 255] = true.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions authority_value_respects_signature_equivalence.
Print Assumptions join_value_is_presentation_independent.
Print Assumptions arbitrary_regroup_history_preserves_value.
Print Assumptions positive_component_cannot_disappear.
