From Stdlib Require Import Arith.PeanoNat Lists.List Lia.

Import ListNotations.

Definition monetary_sum (values : list nat) : nat :=
  fold_right Nat.add 0 values.

Definition base_debits (capacities : list nat) (level : nat) : list nat :=
  map (fun capacity => Nat.min capacity level) capacities.

Fixpoint eligible_count (capacities : list nat) (level : nat) : nat :=
  match capacities with
  | [] => 0
  | capacity :: rest =>
      (if level <? capacity then 1 else 0) + eligible_count rest level
  end.

Fixpoint residual_debits (capacities : list nat) (level residual : nat) : list nat :=
  match capacities with
  | [] => []
  | capacity :: rest =>
      match residual with
      | 0 => Nat.min capacity level :: residual_debits rest level 0
      | S remaining =>
          if level <? capacity then
            S (Nat.min capacity level) :: residual_debits rest level remaining
          else Nat.min capacity level :: residual_debits rest level (S remaining)
      end
  end.

Theorem residual_debits_preserve_payer_count :
  forall capacities level residual,
    length (residual_debits capacities level residual) = length capacities.
Proof.
  induction capacities as [|capacity rest IH]; intros level residual; simpl.
  - reflexivity.
  - destruct residual; simpl; [now rewrite IH|].
    destruct (level <? capacity); simpl; now rewrite IH.
Qed.

Theorem residual_debits_conserve :
  forall capacities level residual,
    monetary_sum (residual_debits capacities level residual) =
      monetary_sum (base_debits capacities level) +
      Nat.min residual (eligible_count capacities level).
Proof.
  induction capacities as [|capacity rest IH]; intros level residual.
  - simpl. now rewrite Nat.min_0_r.
  - destruct residual as [|remaining].
    + simpl. rewrite IH. simpl. lia.
    + simpl. destruct (level <? capacity) eqn:eligible; simpl;
        rewrite IH; simpl; lia.
Qed.

Theorem residual_debits_never_overdraw :
  forall capacities level residual,
    Forall2 Nat.le (residual_debits capacities level residual) capacities.
Proof.
  induction capacities as [|capacity rest IH]; intros level residual; simpl.
  - constructor.
  - destruct residual as [|remaining].
    + constructor; [apply Nat.le_min_l|apply IH].
    + destruct (level <? capacity) eqn:eligible.
      * constructor; [|apply IH].
        apply Nat.ltb_lt in eligible.
        rewrite Nat.min_r by lia. lia.
      * constructor; [apply Nat.le_min_l|apply IH].
Qed.

Theorem residual_debits_have_one_unit_spread :
  forall capacities level residual,
    Forall2
      (fun debit capacity =>
         Nat.min capacity level <= debit /\
         debit <= Nat.min capacity level + 1 /\
         (debit < capacity -> level <= debit))
      (residual_debits capacities level residual) capacities.
Proof.
  induction capacities as [|capacity rest IH]; intros level residual; simpl.
  - constructor.
  - destruct residual as [|remaining].
    + constructor; [|apply IH].
      destruct (Nat.le_ge_cases capacity level) as [small|large].
      * rewrite Nat.min_l by lia. lia.
      * rewrite Nat.min_r by lia. lia.
    + destruct (level <? capacity) eqn:eligible; constructor; try apply IH.
      * apply Nat.ltb_lt in eligible. rewrite Nat.min_r by lia. lia.
      * apply Nat.ltb_ge in eligible. rewrite Nat.min_l by lia. lia.
Qed.

Theorem funded_residual_plan_pays_exact_obligation :
  forall capacities level residual obligation,
    residual <= eligible_count capacities level ->
    obligation = monetary_sum (base_debits capacities level) + residual ->
    monetary_sum (residual_debits capacities level residual) = obligation /\
    Forall2 Nat.le (residual_debits capacities level residual) capacities.
Proof.
  intros capacities level residual obligation enough definition.
  split.
  - rewrite residual_debits_conserve, Nat.min_l by assumption. exact (eq_sym definition).
  - apply residual_debits_never_overdraw.
Qed.

Theorem water_level_successor_counts_eligible :
  forall capacities level,
    monetary_sum (base_debits capacities (S level)) =
      monetary_sum (base_debits capacities level) + eligible_count capacities level.
Proof.
  induction capacities as [|capacity rest IH]; intros level; simpl.
  - reflexivity.
  - change (Nat.min capacity (S level) + monetary_sum (base_debits rest (S level)) =
      Nat.min capacity level + monetary_sum (base_debits rest level) +
      ((if level <? capacity then 1 else 0) + eligible_count rest level)).
    rewrite IH. destruct (level <? capacity) eqn:eligible.
    + apply Nat.ltb_lt in eligible. rewrite !Nat.min_r by lia. lia.
    + apply Nat.ltb_ge in eligible. rewrite !Nat.min_l by lia. lia.
Qed.

Theorem strict_water_boundary_funds_residual :
  forall capacities level obligation,
    monetary_sum (base_debits capacities level) <= obligation ->
    obligation < monetary_sum (base_debits capacities (S level)) ->
    obligation - monetary_sum (base_debits capacities level) < eligible_count capacities level.
Proof.
  intros capacities level obligation lower upper.
  rewrite water_level_successor_counts_eligible in upper. lia.
Qed.

Lemma monetary_bounds_transitive : forall actual reserved capacities,
  Forall2 Nat.le actual reserved -> Forall2 Nat.le reserved capacities ->
  Forall2 Nat.le actual capacities.
Proof.
  intros actual reserved capacities H. revert capacities.
  induction H; intros capacities outer; inversion outer; subst; constructor; eauto; lia.
Qed.

Theorem reservation_bounded_settlement_preserves_original_caps :
  forall capacities reserve_level reserve_residual actual_level actual_residual,
  Forall2 Nat.le
    (residual_debits (residual_debits capacities reserve_level reserve_residual)
      actual_level actual_residual)
    capacities.
Proof.
  intros. eapply monetary_bounds_transitive; apply residual_debits_never_overdraw.
Qed.

Fixpoint monetary_refunds (reserved actual : list nat) : list nat :=
  match reserved, actual with
  | held :: rest, debit :: tail => (held - debit) :: monetary_refunds rest tail
  | _, _ => []
  end.

Theorem reserved_debit_refund_exact : forall actual reserved,
  Forall2 Nat.le actual reserved ->
  monetary_sum (monetary_refunds reserved actual) + monetary_sum actual =
  monetary_sum reserved.
Proof.
  intros actual reserved bounded. induction bounded; simpl; lia.
Qed.

Theorem allocation_from_reserved_caps_has_exact_refunds :
  forall reserved level residual,
  monetary_sum (monetary_refunds reserved (residual_debits reserved level residual)) +
  monetary_sum (residual_debits reserved level residual) = monetary_sum reserved.
Proof.
  intros. apply reserved_debit_refund_exact. apply residual_debits_never_overdraw.
Qed.

Print Assumptions residual_debits_preserve_payer_count.
Print Assumptions residual_debits_conserve.
Print Assumptions residual_debits_never_overdraw.
Print Assumptions residual_debits_have_one_unit_spread.
Print Assumptions funded_residual_plan_pays_exact_obligation.
Print Assumptions water_level_successor_counts_eligible.
Print Assumptions strict_water_boundary_funds_residual.
