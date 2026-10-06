From Stdlib Require Import Arith.PeanoNat Arith.Compare_dec Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment LexicographicMinimax FundingMinimaxCertificate FundingOptimalDomain FundingCyclicTie FundingWitnessPrefix.
Import ListNotations.

Record fee_completion_row := {
  fee_source_capacity : nat;
  resource_source_draw : nat;
  source_allows_fee : bool
}.

Definition remaining_fee_capacity row :=
  if source_allows_fee row then fee_source_capacity row - resource_source_draw row else 0.

Definition total_remaining_fee_capacity rows :=
  fold_right (fun row total => remaining_fee_capacity row + total) 0 rows.

Theorem one_unit_fee_completion_exact : forall rows,
  0 < total_remaining_fee_capacity rows <->
  exists row, In row rows /\ source_allows_fee row = true /\
    resource_source_draw row < fee_source_capacity row.
Proof.
  induction rows as [|head tail IH].
  - simpl. split; [lia|intros [row [absent _]]; contradiction].
  - unfold total_remaining_fee_capacity in *. simpl in *.
    split.
    + intro positive. destruct (source_allows_fee head) eqn:allowed.
      * destruct (lt_dec (resource_source_draw head) (fee_source_capacity head)) as [fits|full].
        -- exists head. auto.
        -- assert (remaining_fee_capacity head = 0) by
             (unfold remaining_fee_capacity; rewrite allowed; lia).
           assert (rest : 0 < fold_right (fun row total => remaining_fee_capacity row + total) 0 tail) by lia.
           apply IH in rest. destruct rest as [row [member terms]]. exists row. auto.
      * assert (remaining_fee_capacity head = 0) by
          (unfold remaining_fee_capacity; rewrite allowed; reflexivity).
        assert (rest : 0 < fold_right (fun row total => remaining_fee_capacity row + total) 0 tail) by lia.
        apply IH in rest. destruct rest as [row [member terms]]. exists row. auto.
    + intros [row [[same|member] [allowed fits]]].
      * subst. unfold remaining_fee_capacity at 1. rewrite allowed. lia.
      * assert (rest : 0 < fold_right (fun row total => remaining_fee_capacity row + total) 0 tail).
        { apply IH. exists row. auto. } lia.
Qed.

Example resource_only_selection_can_exhaust_the_only_fee_source :
  total_remaining_fee_capacity
    [{| fee_source_capacity := 1; resource_source_draw := 1; source_allows_fee := true |};
     {| fee_source_capacity := 1; resource_source_draw := 0; source_allows_fee := false |}] = 0 /\
  total_remaining_fee_capacity
    [{| fee_source_capacity := 1; resource_source_draw := 0; source_allows_fee := true |};
     {| fee_source_capacity := 1; resource_source_draw := 1; source_allows_fee := false |}] = 1.
Proof. split; reflexivity. Qed.

Print Assumptions one_unit_fee_completion_exact.
Print Assumptions resource_only_selection_can_exhaust_the_only_fee_source.

Definition fee_branch_capacity (capacity : nat -> nat) payer source :=
  if Nat.eq_dec source payer then capacity source - 1 else capacity source.

Theorem unit_fee_branch_projection_exact : forall count capacity draw allowed,
  ((forall i, i < count -> draw i <= capacity i) /\
   exists payer, payer < count /\ allowed payer = true /\ draw payer < capacity payer) <->
  exists payer, payer < count /\ allowed payer = true /\ 0 < capacity payer /\
    forall i, i < count -> draw i <= fee_branch_capacity capacity payer i.
Proof.
  intros count capacity draw allowed. split.
  - intros [bounded [payer [inside [permitted spare]]]].
    exists payer. repeat split; try assumption; try lia.
    intros i valid. unfold fee_branch_capacity.
    destruct (Nat.eq_dec i payer); subst; [lia|apply bounded; assumption].
  - intros [payer [inside [permitted [positive bounded]]]]. split.
    + intros i valid. specialize (bounded i valid). unfold fee_branch_capacity in bounded.
      destruct (Nat.eq_dec i payer); lia.
    + exists payer. repeat split; try assumption.
      specialize (bounded payer inside). unfold fee_branch_capacity in bounded.
      destruct (Nat.eq_dec payer payer); [lia|contradiction].
Qed.

Theorem minimum_over_fee_branches_is_global :
  forall (A : Type) (domain : nat -> A -> Prop) (better : A -> A -> Prop)
    (winners : nat -> option A) selected,
  (forall a b c, better a b -> better b c -> better a c) ->
  (forall payer candidate, winners payer = Some candidate -> domain payer candidate) ->
  (forall payer candidate, domain payer candidate ->
    exists winner, winners payer = Some winner /\ better winner candidate) ->
  (exists payer, winners payer = Some selected) ->
  (forall payer winner, winners payer = Some winner -> better selected winner) ->
  (exists payer, domain payer selected) /\
  forall candidate, (exists payer, domain payer candidate) -> better selected candidate.
Proof.
  intros A domain better winners selected transitive sound complete member least.
  split.
  - destruct member as [payer selected_here]. exists payer. eapply sound; eauto.
  - intros candidate [payer valid]. destruct (complete payer candidate valid) as [winner [found optimal]].
    eapply transitive; [eapply least; exact found|exact optimal].
Qed.

Definition eligible_fee_capacity rows :=
  fold_right (fun row total => (if source_allows_fee row then fee_source_capacity row else 0) + total) 0 rows.

Definition total_resource_draw rows :=
  fold_right (fun row total => resource_source_draw row + total) 0 rows.

Theorem no_fee_completion_bounds_eligible_capacity : forall rows,
  total_remaining_fee_capacity rows = 0 ->
  eligible_fee_capacity rows <= total_resource_draw rows.
Proof.
  induction rows as [|head tail IH]; [simpl; lia|].
  unfold total_remaining_fee_capacity, eligible_fee_capacity, total_resource_draw in *.
  simpl in *. intro none.
  assert (rest : fold_right (fun row total => remaining_fee_capacity row + total) 0 tail = 0) by lia.
  specialize (IH rest). unfold remaining_fee_capacity in none.
  destruct (source_allows_fee head); lia.
Qed.

Theorem fee_capacity_above_resource_total_preserves_completion : forall rows,
  total_resource_draw rows < eligible_fee_capacity rows ->
  exists row, In row rows /\ source_allows_fee row = true /\
    resource_source_draw row < fee_source_capacity row.
Proof.
  intros rows spare. apply one_unit_fee_completion_exact.
  destruct (Nat.eq_dec (total_remaining_fee_capacity rows) 0) as [none|positive]; [|lia].
  pose proof (no_fee_completion_bounds_eligible_capacity rows none). lia.
Qed.

Print Assumptions unit_fee_branch_projection_exact.
Print Assumptions minimum_over_fee_branches_is_global.
Print Assumptions fee_capacity_above_resource_total_preserves_completion.

Definition other_source_capacity rows :=
  fold_right (fun row total => (if source_allows_fee row then 0 else fee_source_capacity row) + total) 0 rows.

Definition replace_resource_draw row draw :=
  {| fee_source_capacity := fee_source_capacity row;
     resource_source_draw := draw;
     source_allows_fee := source_allows_fee row |}.

Fixpoint fill_fee_saturated rows extra :=
  match rows with
  | [] => []
  | row :: tail =>
      if source_allows_fee row then
        replace_resource_draw row (fee_source_capacity row) :: fill_fee_saturated tail extra
      else
        let used := Nat.min extra (fee_source_capacity row) in
        replace_resource_draw row used :: fill_fee_saturated tail (extra - used)
  end.

Lemma fill_fee_saturated_total : forall rows extra,
  extra <= other_source_capacity rows ->
  total_resource_draw (fill_fee_saturated rows extra) = eligible_fee_capacity rows + extra.
Proof.
  induction rows as [|head tail IH]; intros extra bounded; [simpl in *; lia|].
  change (extra <= (if source_allows_fee head then 0 else fee_source_capacity head) + other_source_capacity tail) in bounded.
  cbn [fill_fee_saturated].
  destruct (source_allows_fee head) eqn:allowed; simpl in bounded.
  - change (fee_source_capacity head + total_resource_draw (fill_fee_saturated tail extra) =
      eligible_fee_capacity (head :: tail) + extra).
    rewrite IH by exact bounded.
    change (fee_source_capacity head + (eligible_fee_capacity tail + extra) =
      (if source_allows_fee head then fee_source_capacity head else 0) + eligible_fee_capacity tail + extra).
    rewrite allowed. lia.
  - assert (rest : extra - Nat.min extra (fee_source_capacity head) <= other_source_capacity tail).
    { destruct (le_dec extra (fee_source_capacity head)).
      - rewrite Nat.min_l by assumption. lia.
      - rewrite Nat.min_r by lia. lia. }
    change (Nat.min extra (fee_source_capacity head) +
      total_resource_draw (fill_fee_saturated tail (extra - Nat.min extra (fee_source_capacity head))) =
      eligible_fee_capacity (head :: tail) + extra).
    rewrite IH by exact rest.
    change (Nat.min extra (fee_source_capacity head) +
      (eligible_fee_capacity tail + (extra - Nat.min extra (fee_source_capacity head))) =
      (if source_allows_fee head then fee_source_capacity head else 0) + eligible_fee_capacity tail + extra).
    rewrite allowed.
    pose proof (Nat.le_min_l extra (fee_source_capacity head)). lia.
Qed.

Lemma fill_fee_saturated_bounded : forall rows extra,
  Forall (fun row => resource_source_draw row <= fee_source_capacity row) (fill_fee_saturated rows extra).
Proof.
  induction rows as [|head tail IH]; intro extra; [constructor|].
  cbn [fill_fee_saturated]. destruct (source_allows_fee head); constructor.
  - change (fee_source_capacity head <= fee_source_capacity head). lia.
  - apply IH.
  - change (Nat.min extra (fee_source_capacity head) <= fee_source_capacity head). apply Nat.le_min_r.
  - apply IH.
Qed.

Lemma fill_fee_saturated_no_completion : forall rows extra,
  total_remaining_fee_capacity (fill_fee_saturated rows extra) = 0.
Proof.
  induction rows as [|head tail IH]; intro extra; [reflexivity|].
  cbn [fill_fee_saturated]. destruct (source_allows_fee head) eqn:allowed.
  - change ((if source_allows_fee head then fee_source_capacity head - fee_source_capacity head else 0) +
      total_remaining_fee_capacity (fill_fee_saturated tail extra) = 0).
    rewrite allowed, IH. lia.
  - change ((if source_allows_fee head then fee_source_capacity head - Nat.min extra (fee_source_capacity head) else 0) +
      total_remaining_fee_capacity (fill_fee_saturated tail (extra - Nat.min extra (fee_source_capacity head))) = 0).
    rewrite allowed, IH. lia.
Qed.

Lemma fill_fee_saturated_preserves_sources : forall rows extra,
  map (fun row => (fee_source_capacity row, source_allows_fee row)) (fill_fee_saturated rows extra) =
  map (fun row => (fee_source_capacity row, source_allows_fee row)) rows.
Proof.
  induction rows as [|head tail IH]; intro extra; [reflexivity|].
  cbn [fill_fee_saturated]. destruct (source_allows_fee head);
    cbn [map replace_resource_draw]; rewrite IH; reflexivity.
Qed.

Theorem fee_restriction_has_capped_domain_counterexample : forall rows total,
  eligible_fee_capacity rows <= total ->
  total <= eligible_fee_capacity rows + other_source_capacity rows ->
  exists counterexample,
    map (fun row => (fee_source_capacity row, source_allows_fee row)) counterexample =
      map (fun row => (fee_source_capacity row, source_allows_fee row)) rows /\
    Forall (fun row => resource_source_draw row <= fee_source_capacity row) counterexample /\
    total_resource_draw counterexample = total /\
    total_remaining_fee_capacity counterexample = 0.
Proof.
  intros rows total lower upper.
  exists (fill_fee_saturated rows (total - eligible_fee_capacity rows)).
  split; [apply fill_fee_saturated_preserves_sources|].
  split; [apply fill_fee_saturated_bounded|].
  split; [rewrite fill_fee_saturated_total by lia; lia|apply fill_fee_saturated_no_completion].
Qed.

Print Assumptions fee_restriction_has_capped_domain_counterexample.

Definition two_case_exposure (left right : list nat) :=
  fold_right (fun pair total => Nat.max (fst pair) (snd pair) + total) 0 (combine left right).

Example independent_case_ties_can_exceed_family_exposure :
  burden_rank [0; 1; 1; 0; 0] = burden_rank [0; 0; 1; 1; 0] /\
  burden_rank [1; 0; 1; 1; 0] = burden_rank [0; 1; 1; 1; 0] /\
  funding_lex_le [0; 0; 1; 1; 0] [0; 1; 1; 0; 0] /\
  funding_lex_le [0; 1; 1; 1; 0] [1; 0; 1; 1; 0] /\
  two_case_exposure [0; 1; 1; 0; 1] [1; 0; 1; 1; 1] = 5 /\
  two_case_exposure [0; 0; 1; 1; 1] [1; 0; 1; 1; 1] = 4 /\
  two_case_exposure [0; 1; 1; 0; 1] [0; 1; 1; 1; 1] = 4.
Proof. vm_compute. repeat split; auto. Qed.

Print Assumptions independent_case_ties_can_exceed_family_exposure.

Definition funding_policy_contributions sources obligations flow :=
  map (source_draw obligations flow) (seq 0 sources).

Definition exact_funding_optimal_domain sources obligations eligible capacity demand candidate spec :=
  forall alternative,
    funding_spec_valid sources obligations demand spec alternative <->
    assignment_valid sources obligations eligible capacity demand alternative /\
    burden_rank (funding_policy_contributions sources obligations alternative) =
      burden_rank (funding_policy_contributions sources obligations candidate).

Theorem checked_funding_policy_preserves_all_three_objectives :
  forall sources obligations eligible capacity demand candidate cuts spec cursor,
  assignment_valid sources obligations eligible capacity demand candidate ->
  funding_minimax_cut_certificate sources obligations eligible capacity demand candidate cuts ->
  exact_funding_optimal_domain sources obligations eligible capacity demand candidate spec ->
  funding_box_prefix_certificate sources obligations demand spec candidate (cyclic_funding_order sources cursor) ->
  funding_witness_prefix_certificate sources obligations eligible (source_draw obligations candidate) demand candidate
    (funding_row_major_order sources obligations) ->
  forall alternative, assignment_valid sources obligations eligible capacity demand alternative ->
    burden_le (funding_policy_contributions sources obligations candidate)
      (funding_policy_contributions sources obligations alternative) /\
    (burden_rank (funding_policy_contributions sources obligations alternative) =
      burden_rank (funding_policy_contributions sources obligations candidate) ->
      funding_lex_le (map (source_draw obligations alternative) (cyclic_funding_order sources cursor))
        (map (source_draw obligations candidate) (cyclic_funding_order sources cursor))) /\
    ((forall i, i < sources -> source_draw obligations alternative i = source_draw obligations candidate i) ->
      funding_lex_le (map (funding_entry_value candidate) (funding_row_major_order sources obligations))
        (map (funding_entry_value alternative) (funding_row_major_order sources obligations))).
Proof.
  intros sources obligations eligible capacity demand candidate cuts spec cursor valid rank domain cyclic witness alternative feasible.
  split.
  - exact (checked_excess_cuts_prove_global_funding_minimax _ _ _ _ _ _ _ valid rank alternative feasible).
  - split.
    + intro equal_rank. apply (checked_box_prefix_is_lexicographically_greatest _ sources obligations demand spec candidate).
      * apply domain. split; [exact valid|reflexivity].
      * exact cyclic.
      * apply domain. auto.
    + intro same. apply (checked_witness_prefix_is_lexicographically_least _ sources obligations eligible (source_draw obligations candidate) demand candidate).
      * eapply accepted_assignment_respects_reserved_sources; [exact valid|auto].
      * exact (accepted_assignment_conserves_obligation _ _ _ _ _ _ valid).
      * exact witness.
      * eapply accepted_assignment_respects_reserved_sources; [exact feasible|]. intros i inside. rewrite same by exact inside. lia.
Qed.

Definition funding_policy_next_position count cursor (unrestricted : bool) unrestricted_next total :=
  if total =? 0 then None else
  Some (if unrestricted then unrestricted_next else if S cursor =? count then 0 else S cursor).

Theorem zero_funding_policy_has_no_position_update : forall count cursor unrestricted unrestricted_next,
  funding_policy_next_position count cursor unrestricted unrestricted_next 0 = None.
Proof. reflexivity. Qed.

Theorem positive_unrestricted_policy_preserves_existing_next : forall count cursor unrestricted_next total,
  0 < total -> funding_policy_next_position count cursor true unrestricted_next total = Some unrestricted_next.
Proof.
  intros. unfold funding_policy_next_position. destruct (total =? 0) eqn:zero; [apply Nat.eqb_eq in zero; lia|reflexivity].
Qed.

Theorem positive_restricted_policy_advances_one_position : forall count cursor unrestricted_next total,
  0 < total ->
  funding_policy_next_position count cursor false unrestricted_next total =
    Some (if S cursor =? count then 0 else S cursor).
Proof.
  intros. unfold funding_policy_next_position. destruct (total =? 0) eqn:zero; [apply Nat.eqb_eq in zero; lia|reflexivity].
Qed.

Theorem funding_policy_next_position_is_bounded : forall count cursor unrestricted unrestricted_next total next,
  cursor < count -> unrestricted_next < count ->
  funding_policy_next_position count cursor unrestricted unrestricted_next total = Some next -> next < count.
Proof.
  intros count cursor unrestricted unrestricted_next total next inside prior checked.
  unfold funding_policy_next_position in checked. destruct (total =? 0); [discriminate|].
  destruct unrestricted; [inversion checked; subst; assumption|].
  destruct (S cursor =? count) eqn:wrap; inversion checked; subst.
  - lia.
  - apply Nat.eqb_neq in wrap. lia.
Qed.
