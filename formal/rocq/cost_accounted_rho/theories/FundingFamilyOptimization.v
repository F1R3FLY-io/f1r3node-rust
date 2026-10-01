From Stdlib Require Import Lists.List Arith.PeanoNat Arith.Compare_dec Lia NArith.
From CostAccountedRho Require Import LexicographicMinimax.
Import ListNotations.

Definition funding_schedule_advance (rows : nat -> list nat) selected rest row :=
  if Nat.eq_dec row selected then rest else rows row.

Fixpoint funding_schedule_emit schedule (rows : nat -> list nat) :=
  match schedule with
  | [] => []
  | row :: remaining =>
      match rows row with
      | [] => funding_schedule_emit remaining rows
      | head :: tail => head :: funding_schedule_emit remaining (funding_schedule_advance rows row tail)
      end
  end.

Theorem funding_schedule_preserves_local_minima : forall schedule left right,
  (forall row, length (left row) = length (right row)) ->
  (forall row, funding_lex_le (left row) (right row)) ->
  funding_lex_le (funding_schedule_emit schedule left) (funding_schedule_emit schedule right).
Proof.
  induction schedule as [|row remaining IH]; intros left right sizes minima; [simpl; auto|].
  cbn [funding_schedule_emit].
  specialize (sizes row) as size. specialize (minima row) as minimum.
  destruct (left row) as [|x xs] eqn:l; destruct (right row) as [|y ys] eqn:r;
    simpl in size; try discriminate.
  - now apply IH.
  - destruct minimum as [strict|[same rest]].
    + simpl. now left.
    + simpl. right. split; [exact same|]. apply IH.
      * intro i. unfold funding_schedule_advance. destruct (Nat.eq_dec i row); [congruence|apply sizes].
      * intro i. unfold funding_schedule_advance. destruct (Nat.eq_dec i row); [exact rest|apply minima].
Qed.

Section FamilyOptimization.
  Context {Plan Box : Type}.
  Variable valid : Plan -> Prop.
  Variable contains : Box -> Plan -> Prop.
  Variable preferred : Plan -> Plan -> Prop.
  Hypothesis preferred_transitive : forall a b c, preferred a b -> preferred b c -> preferred a c.

  Definition family_relaxed_minimum box relaxed :=
    forall candidate, contains box candidate -> preferred relaxed candidate.

  Inductive family_optimality_certificate (selected : Plan) : Box -> Prop :=
  | family_optimal_empty : forall box,
      (forall candidate, ~ contains box candidate) -> family_optimality_certificate selected box
  | family_optimal_bound : forall box relaxed,
      family_relaxed_minimum box relaxed -> preferred selected relaxed ->
      family_optimality_certificate selected box
  | family_optimal_partition : forall box left right,
      (forall candidate, contains box candidate -> contains left candidate \/ contains right candidate) ->
      family_optimality_certificate selected left -> family_optimality_certificate selected right ->
      family_optimality_certificate selected box.

  Theorem family_optimality_certificate_sound : forall selected box,
    family_optimality_certificate selected box ->
    forall candidate, contains box candidate -> preferred selected candidate.
  Proof.
    intros selected box certificate. induction certificate; intros candidate member.
    - exfalso. exact (H candidate member).
    - eapply preferred_transitive; [exact H0|apply H; exact member].
    - destruct (H candidate member); [apply IHcertificate1|apply IHcertificate2]; assumption.
  Qed.

  Theorem family_optimality_certificate_global : forall selected root,
    valid selected -> (forall candidate, valid candidate -> contains root candidate) ->
    family_optimality_certificate selected root ->
    valid selected /\ forall candidate, valid candidate -> preferred selected candidate.
  Proof.
    intros selected root sound covers certificate. split; [exact sound|].
    intros candidate feasible. eapply family_optimality_certificate_sound; [exact certificate|now apply covers].
  Qed.

  Theorem family_improved_incumbent_preserves_certificates : forall old selected box,
    preferred selected old -> family_optimality_certificate old box ->
    family_optimality_certificate selected box.
  Proof.
    intros old selected box improves certificate. induction certificate.
    - now apply family_optimal_empty.
    - eapply family_optimal_bound; [exact H|]. eapply preferred_transitive; eauto.
    - eapply family_optimal_partition; eauto.
  Qed.

  Definition family_frontier_covers root boxes selected :=
    forall candidate, contains root candidate ->
      preferred selected candidate \/ exists box, In box boxes /\ contains box candidate.

  Theorem family_completed_frontier_is_optimal : forall root selected,
    valid selected -> family_frontier_covers root [] selected ->
    valid selected /\ forall candidate, contains root candidate -> preferred selected candidate.
  Proof.
    intros root selected sound covered. split; [exact sound|].
    intros candidate member. destruct (covered candidate member) as [optimal|[box [absent _]]];
      [exact optimal|contradiction].
  Qed.

  Theorem family_frontier_improvement : forall root boxes old selected,
    preferred selected old -> family_frontier_covers root boxes old -> family_frontier_covers root boxes selected.
  Proof.
    intros root boxes old selected improves covered candidate member.
    destruct (covered candidate member) as [bounded|pending]; [left; eapply preferred_transitive; eauto|now right].
  Qed.
End FamilyOptimization.

Definition family_source_bill_bound capacity resource_total unit_fee :=
  Nat.min capacity (resource_total + unit_fee).

Theorem family_source_bill_clipping_exact : forall draw capacity resource_total unit_fee,
  draw <= family_source_bill_bound capacity resource_total unit_fee <->
  draw <= capacity /\ draw <= resource_total + unit_fee.
Proof.
  intros. unfold family_source_bill_bound. lia.
Qed.

Theorem family_source_bill_clipping_representable : forall maximum capacity resource_total unit_fee,
  capacity <= maximum ->
  family_source_bill_bound capacity resource_total unit_fee <= maximum.
Proof.
  intros. unfold family_source_bill_bound. lia.
Qed.

Theorem family_source_bill_clipping_preserves_large_bills : forall maximum capacity resource_total unit_fee,
  capacity <= maximum -> maximum < resource_total + unit_fee ->
  family_source_bill_bound capacity resource_total unit_fee = capacity.
Proof.
  intros. unfold family_source_bill_bound. apply Nat.min_l. lia.
Qed.

Definition family_narrow_amount maximum amount :=
  if le_dec amount maximum then Some amount else None.

Theorem family_clip_before_narrowing_succeeds : forall maximum capacity resource_total unit_fee,
  capacity <= maximum ->
  family_narrow_amount maximum (family_source_bill_bound capacity resource_total unit_fee) =
  Some (family_source_bill_bound capacity resource_total unit_fee).
Proof.
  intros maximum capacity resource_total unit_fee bounded. unfold family_narrow_amount.
  destruct (le_dec _ _); [reflexivity|]. exfalso. apply n.
  now apply family_source_bill_clipping_representable.
Qed.

Theorem family_fee_bill_wide_accumulator_sufficient : forall maximum wide_maximum resource_total unit_fee,
  resource_total <= maximum -> unit_fee <= 1 -> maximum + 1 <= wide_maximum ->
  resource_total + unit_fee <= wide_maximum.
Proof. intros. lia. Qed.

Theorem family_maximum_resource_with_separate_fee_counterexample : forall maximum,
  1 <= maximum ->
  maximum + 0 <= maximum /\ 0 + 1 <= maximum /\
  maximum < (maximum + 0) + (0 + 1) /\
  family_narrow_amount maximum (maximum + 1) = None /\
  family_narrow_amount maximum (family_source_bill_bound maximum maximum 1) = Some maximum.
Proof.
  intros maximum positive. split; [lia|]. split; [lia|]. split; [lia|]. split.
  - unfold family_narrow_amount. destruct (le_dec (maximum + 1) maximum); [lia|reflexivity].
  - assert (clipped : family_source_bill_bound maximum maximum 1 = maximum).
    { unfold family_source_bill_bound. apply Nat.min_l. lia. }
    rewrite clipped. unfold family_narrow_amount. destruct (le_dec maximum maximum); [reflexivity|lia].
Qed.

Example family_u64_maximum_plus_fee_fits_u128 :
  let source_maximum := (2 ^ 64 - 1)%N in
  let wide_maximum := (2 ^ 128 - 1)%N in
  (source_maximum <? source_maximum + 1)%N = true /\
  (source_maximum + 1 <=? wide_maximum)%N = true /\
  N.min source_maximum (source_maximum + 1) = source_maximum.
Proof. vm_compute. repeat split; reflexivity. Qed.

Print Assumptions family_optimality_certificate_global.
Print Assumptions family_improved_incumbent_preserves_certificates.
Print Assumptions family_completed_frontier_is_optimal.
Print Assumptions funding_schedule_preserves_local_minima.
Print Assumptions family_source_bill_clipping_exact.
Print Assumptions family_clip_before_narrowing_succeeds.
Print Assumptions family_source_bill_clipping_preserves_large_bills.
Print Assumptions family_fee_bill_wide_accumulator_sufficient.
Print Assumptions family_maximum_resource_with_separate_fee_counterexample.
Print Assumptions family_u64_maximum_plus_fee_fits_u128.
