From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia Sorting.Permutation.
From CostAccountedRho Require Import
  EligibleFundingAssignment FundingBox FundingCyclicTie FundingWitnessPrefix
  FundingOptimalDomain FundingMinimaxCertificate LexicographicMinimax
  FundingPolicyComposition.
Import ListNotations.

Record n_payer_solver_certificate
    (sources obligations : nat)
    (eligible : nat -> nat -> bool)
    (capacity demand : nat -> nat)
    (candidate : funding_flow)
    (cuts : nat -> nat -> bool)
    (spec : funding_box_spec)
    (cursor : nat) := {
  n_payer_candidate_valid :
    assignment_valid sources obligations eligible capacity demand candidate;
  n_payer_minimax_certificate :
    funding_minimax_cut_certificate sources obligations eligible capacity demand candidate cuts;
  n_payer_optimal_domain :
    exact_funding_optimal_domain sources obligations eligible capacity demand candidate spec;
  n_payer_cyclic_certificate :
    funding_box_prefix_certificate sources obligations demand spec candidate
      (cyclic_funding_order sources cursor);
  n_payer_witness_certificate :
    funding_witness_prefix_certificate sources obligations eligible
      (source_draw obligations candidate) demand candidate
      (funding_row_major_order sources obligations)
}.

Definition n_payer_contributions sources obligations (flow : funding_flow) :=
  funding_policy_contributions sources obligations flow.

Theorem n_payer_solver_conserves_and_minimizes :
  forall sources obligations eligible capacity demand candidate cuts spec cursor,
  n_payer_solver_certificate sources obligations eligible capacity demand candidate cuts spec cursor ->
  forall alternative,
    assignment_valid sources obligations eligible capacity demand alternative ->
    funding_sum sources (source_draw obligations candidate) = funding_sum obligations demand /\
    burden_le (n_payer_contributions sources obligations candidate)
      (n_payer_contributions sources obligations alternative) /\
    (burden_rank (n_payer_contributions sources obligations alternative) =
       burden_rank (n_payer_contributions sources obligations candidate) ->
     funding_lex_le
       (map (source_draw obligations alternative) (cyclic_funding_order sources cursor))
       (map (source_draw obligations candidate) (cyclic_funding_order sources cursor))) /\
    ((forall i, i < sources ->
        source_draw obligations alternative i = source_draw obligations candidate i) ->
     funding_lex_le
       (map (funding_entry_value candidate) (funding_row_major_order sources obligations))
       (map (funding_entry_value alternative) (funding_row_major_order sources obligations))).
Proof.
  intros sources obligations eligible capacity demand candidate cuts spec cursor certificate alternative feasible.
  destruct certificate as [candidate_valid minimax domain cyclic witness].
  pose proof (accepted_assignment_conserves_obligation _ _ _ _ _ _ candidate_valid) as conserved.
  pose proof (checked_funding_policy_preserves_all_three_objectives
    sources obligations eligible capacity demand candidate cuts spec cursor
    candidate_valid minimax domain cyclic witness alternative feasible) as objectives.
  repeat split.
  - exact conserved.
  - exact (proj1 objectives).
  - exact (proj1 (proj2 objectives)).
  - exact (proj2 (proj2 objectives)).
Qed.

Theorem n_payer_solver_no_overdraw :
  forall sources obligations eligible capacity demand candidate cuts spec cursor,
  n_payer_solver_certificate sources obligations eligible capacity demand candidate cuts spec cursor ->
  forall source, source < sources ->
    source_draw obligations candidate source <= capacity source.
Proof.
  intros sources obligations eligible capacity demand candidate cuts spec cursor certificate source inside.
  destruct certificate as [candidate_valid].
  exact (proj1 (proj1 candidate_valid source inside)).
Qed.

Theorem n_payer_solver_conserves_obligation :
  forall sources obligations eligible capacity demand candidate cuts spec cursor,
  n_payer_solver_certificate sources obligations eligible capacity demand candidate cuts spec cursor ->
  funding_sum sources (source_draw obligations candidate) = funding_sum obligations demand.
Proof.
  intros sources obligations eligible capacity demand candidate cuts spec cursor [candidate_valid].
  apply accepted_assignment_conserves_obligation with
    (eligible := eligible) (capacity := capacity) (demand := demand).
  exact candidate_valid.
Qed.

Record n_payer_batch_entry := {
  batch_obligations : nat;
  batch_eligible : nat -> nat -> bool;
  batch_demand : nat -> nat;
  batch_flow : funding_flow
}.

Definition batch_entry_draw entry :=
  source_draw (batch_obligations entry) (batch_flow entry).

Definition batch_entry_valid sources capacity entry :=
  assignment_valid sources (batch_obligations entry) (batch_eligible entry)
    capacity (batch_demand entry) (batch_flow entry).

Definition batch_remaining capacity entry source :=
  capacity source - batch_entry_draw entry source.

Fixpoint batch_draw entries source :=
  match entries with
  | [] => 0
  | entry :: rest => batch_entry_draw entry source + batch_draw rest source
  end.

Fixpoint batch_final_capacity capacity entries :=
  match entries with
  | [] => capacity
  | entry :: rest => batch_final_capacity (batch_remaining capacity entry) rest
  end.

Inductive n_payer_batch_valid sources : (nat -> nat) -> list n_payer_batch_entry -> Prop :=
| funding_batch_empty : forall capacity, n_payer_batch_valid sources capacity []
| funding_batch_cons : forall capacity entry rest,
    batch_entry_valid sources capacity entry ->
    n_payer_batch_valid sources (batch_remaining capacity entry) rest ->
    n_payer_batch_valid sources capacity (entry :: rest).

Lemma batch_entry_rebound : forall sources capacity replacement entry,
  batch_entry_valid sources capacity entry ->
  (forall source, source < sources -> batch_entry_draw entry source <= replacement source) ->
  batch_entry_valid sources replacement entry.
Proof.
  intros sources capacity replacement entry valid bounded.
  unfold batch_entry_valid in *. eapply accepted_assignment_respects_reserved_sources; eauto.
Qed.

Lemma batch_draw_covers_member : forall entries entry source,
  In entry entries -> batch_entry_draw entry source <= batch_draw entries source.
Proof.
  induction entries as [|head rest IH]; intros entry source included; cbn in *.
  - contradiction.
  - destruct included as [same|included].
    + subst. lia.
    + specialize (IH entry source included). lia.
Qed.

Theorem n_payer_batch_exact : forall sources capacity entries,
  n_payer_batch_valid sources capacity entries <->
  Forall (batch_entry_valid sources capacity) entries /\
  (forall source, source < sources -> batch_draw entries source <= capacity source).
Proof.
  intros sources capacity entries. split.
  - intros valid. induction valid as [capacity|capacity entry rest head tail IH].
    + split; [constructor|intros; cbn; lia].
    + destruct IH as [remaining_valid remaining_bound]. split.
      * constructor; [exact head|]. eapply Forall_impl; [|exact remaining_valid].
        intros item valid. eapply batch_entry_rebound; [exact valid|].
        intros source inside. destruct valid as [rows _].
        specialize (rows source inside). unfold source_valid in rows.
        unfold batch_remaining in *. unfold batch_entry_draw. lia.
      * intros source inside. specialize (remaining_bound source inside).
        destruct head as [rows _]. specialize (rows source inside).
        unfold source_valid in rows. cbn [batch_draw]. unfold batch_remaining in *.
        unfold batch_entry_draw in *. lia.
  - revert capacity. induction entries as [|entry rest IH]; intros capacity [valid bounded].
    + constructor.
    + inversion valid as [|? ? head tail]; subst. constructor; [exact head|].
      apply IH. split.
      * rewrite Forall_forall in tail |- *. intros item included.
        eapply batch_entry_rebound; [apply tail; exact included|]. intros source inside.
        pose proof (batch_draw_covers_member rest item source included) as covered.
        specialize (bounded source inside). cbn [batch_draw] in bounded.
        unfold batch_remaining. lia.
      * intros source inside. specialize (bounded source inside).
        cbn [batch_draw] in bounded. unfold batch_remaining. lia.
Qed.

Theorem n_payer_batch_conserves_each_source : forall sources capacity entries,
  n_payer_batch_valid sources capacity entries ->
  forall source, source < sources ->
  batch_final_capacity capacity entries source + batch_draw entries source = capacity source.
Proof.
  intros sources capacity entries valid.
  induction valid as [capacity|capacity entry rest head tail IH]; intros source inside.
  - cbn. lia.
  - specialize (IH source inside). cbn [batch_final_capacity batch_draw].
    destruct head as [rows _]. specialize (rows source inside).
    unfold source_valid in rows. unfold batch_remaining, batch_entry_draw in *. lia.
Qed.

Lemma batch_draw_permutation : forall first second,
  Permutation first second -> forall source, batch_draw first source = batch_draw second source.
Proof.
  intros first second same. induction same; intros source; cbn [batch_draw]; try reflexivity.
  - rewrite IHsame. reflexivity.
  - lia.
  - rewrite IHsame1, IHsame2. reflexivity.
Qed.

Theorem n_payer_solver_batch_serializable : forall sources capacity first second,
  n_payer_batch_valid sources capacity first ->
  Permutation first second ->
  n_payer_batch_valid sources capacity second /\
  (forall source, source < sources ->
    batch_final_capacity capacity first source = batch_final_capacity capacity second source).
Proof.
  intros sources capacity first second valid same.
  assert (reordered : n_payer_batch_valid sources capacity second).
  { apply n_payer_batch_exact. apply n_payer_batch_exact in valid.
    destruct valid as [individual total]. split.
    - eapply Permutation_Forall; eauto.
    - intros source inside. rewrite <- (batch_draw_permutation first second same source).
      apply total. exact inside. }
  split; [exact reordered|]. intros source inside.
  pose proof (n_payer_batch_conserves_each_source _ _ _ valid source inside).
  pose proof (n_payer_batch_conserves_each_source _ _ _ reordered source inside).
  rewrite (batch_draw_permutation first second same source) in H. lia.
Qed.

Definition batch_shared_unit : n_payer_batch_entry :=
  {| batch_obligations := 1; batch_eligible := fun _ _ => true;
     batch_demand := fun _ => 1; batch_flow := fun _ _ => 1 |}.

Example individually_valid_plans_can_overdraw_a_shared_source :
  batch_entry_valid 1 (fun _ => 1) batch_shared_unit /\
  ~ n_payer_batch_valid 1 (fun _ => 1) [batch_shared_unit; batch_shared_unit].
Proof.
  split.
  - unfold batch_entry_valid. apply assignment_check_exact. reflexivity.
  - intros valid. apply n_payer_batch_exact in valid. destruct valid as [_ total].
    specialize (total 0 (Nat.lt_0_succ 0)). cbv in total. lia.
Qed.

Print Assumptions n_payer_solver_conserves_and_minimizes.
Print Assumptions n_payer_solver_no_overdraw.
Print Assumptions n_payer_solver_conserves_obligation.
Print Assumptions n_payer_batch_exact.
Print Assumptions n_payer_batch_conserves_each_source.
Print Assumptions n_payer_solver_batch_serializable.
Print Assumptions individually_valid_plans_can_overdraw_a_shared_source.
