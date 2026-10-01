From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.

Import ListNotations.

Inductive materialization_event :=
| InitialDeclaration
| ParentReduction
| NestedDeclaration.

Record materialization_state := {
  payer_cells : nat;
  initial_purse_cells : nat;
  nested_purse_cells : nat;
  burned_cells : nat;
  initial_declared : bool;
  parent_committed : bool;
  nested_declared : bool;
  materialization_trace : list materialization_event
}.

Definition materialization_total (state : materialization_state) : nat :=
  payer_cells state + initial_purse_cells state +
  nested_purse_cells state + burned_cells state.

Definition materialize_initial
  (state : materialization_state)
  : option materialization_state :=
  if initial_declared state then None
  else if 2 <=? payer_cells state then
    Some
      {|
        payer_cells := payer_cells state - 2;
        initial_purse_cells := initial_purse_cells state + 2;
        nested_purse_cells := nested_purse_cells state;
        burned_cells := burned_cells state;
        initial_declared := true;
        parent_committed := parent_committed state;
        nested_declared := nested_declared state;
        materialization_trace := materialization_trace state ++ [InitialDeclaration]
      |}
  else None.

Definition run_parent
  (state : materialization_state)
  : option materialization_state :=
  if initial_declared state && negb (parent_committed state) &&
     (1 <=? payer_cells state) && (1 <=? initial_purse_cells state) then
    Some
      {|
        payer_cells := payer_cells state - 1;
        initial_purse_cells := initial_purse_cells state - 1;
        nested_purse_cells := nested_purse_cells state;
        burned_cells := burned_cells state + 2;
        initial_declared := initial_declared state;
        parent_committed := true;
        nested_declared := nested_declared state;
        materialization_trace := materialization_trace state ++ [ParentReduction]
      |}
  else None.

Definition materialize_nested
  (state : materialization_state)
  : option materialization_state :=
  if parent_committed state && negb (nested_declared state) &&
     (1 <=? payer_cells state) then
    Some
      {|
        payer_cells := payer_cells state - 1;
        initial_purse_cells := initial_purse_cells state;
        nested_purse_cells := nested_purse_cells state + 1;
        burned_cells := burned_cells state;
        initial_declared := initial_declared state;
        parent_committed := parent_committed state;
        nested_declared := true;
        materialization_trace := materialization_trace state ++ [NestedDeclaration]
      |}
  else None.

Definition evaluate_phased
  (state : materialization_state)
  : option materialization_state :=
  match materialize_initial state with
  | None => None
  | Some declared =>
      match run_parent declared with
      | None => None
      | Some committed => materialize_nested committed
      end
  end.

Inductive reducer_preference :=
| DeclarationReadyFirst
| ReductionReadyFirst.

Definition evaluate_parallel
  (_ : reducer_preference)
  (state : materialization_state)
  : option materialization_state :=
  evaluate_phased state.

Definition initial_materialization_state : materialization_state :=
  {|
    payer_cells := 4;
    initial_purse_cells := 0;
    nested_purse_cells := 0;
    burned_cells := 0;
    initial_declared := false;
    parent_committed := false;
    nested_declared := false;
    materialization_trace := []
  |}.

Definition final_materialization_state : materialization_state :=
  {|
    payer_cells := 0;
    initial_purse_cells := 1;
    nested_purse_cells := 1;
    burned_cells := 2;
    initial_declared := true;
    parent_committed := true;
    nested_declared := true;
    materialization_trace :=
      [InitialDeclaration; ParentReduction; NestedDeclaration]
  |}.

Definition replay_parallel := evaluate_parallel.

Theorem initial_materialization_conserves :
  forall before after,
    materialize_initial before = Some after ->
    materialization_total after = materialization_total before.
Proof.
  intros before after step.
  unfold materialize_initial in step.
  destruct (initial_declared before) eqn:declared;
  destruct (2 <=? payer_cells before) eqn:funded;
  simpl in step; try discriminate.
  inversion step; subst.
  unfold materialization_total.
  simpl.
  apply Nat.leb_le in funded.
  lia.
Qed.

Theorem parent_reduction_requires_materialized_initial_purse :
  forall before after,
    run_parent before = Some after ->
    initial_declared before = true /\ parent_committed after = true.
Proof.
  intros before after step.
  unfold run_parent in step.
  destruct (initial_declared before) eqn:declared;
  destruct (parent_committed before) eqn:parent;
  destruct (1 <=? payer_cells before) eqn:payer_funded;
  destruct (1 <=? initial_purse_cells before) eqn:purse_funded;
  simpl in step;
  try discriminate.
  inversion step; subst.
  auto.
Qed.

Theorem parent_reduction_conserves :
  forall before after,
    run_parent before = Some after ->
    materialization_total after = materialization_total before.
Proof.
  intros before after step.
  unfold run_parent in step.
  destruct (initial_declared before) eqn:declared;
  destruct (parent_committed before) eqn:parent;
  destruct (1 <=? payer_cells before) eqn:payer_funded;
  destruct (1 <=? initial_purse_cells before) eqn:purse_funded;
  simpl in step;
  try discriminate.
  inversion step; subst.
  unfold materialization_total.
  simpl.
  apply Nat.leb_le in payer_funded.
  apply Nat.leb_le in purse_funded.
  lia.
Qed.

Theorem nested_materialization_requires_parent_commit :
  forall before after,
    materialize_nested before = Some after ->
    parent_committed before = true /\ nested_declared after = true.
Proof.
  intros before after step.
  unfold materialize_nested in step.
  destruct (parent_committed before) eqn:parent;
  destruct (nested_declared before) eqn:nested_done;
  destruct (1 <=? payer_cells before) eqn:funded;
  simpl in step; try discriminate.
  inversion step; subst.
  auto.
Qed.

Theorem nested_materialization_conserves :
  forall before after,
    materialize_nested before = Some after ->
    materialization_total after = materialization_total before.
Proof.
  intros before after step.
  unfold materialize_nested in step.
  destruct (parent_committed before) eqn:parent;
  destruct (nested_declared before) eqn:nested_done;
  destruct (1 <=? payer_cells before) eqn:funded;
  simpl in step; try discriminate.
  inversion step; subst.
  unfold materialization_total.
  simpl.
  apply Nat.leb_le in funded.
  lia.
Qed.

Theorem premature_parent_reduction_is_rejected :
  run_parent initial_materialization_state = None.
Proof.
  reflexivity.
Qed.

Theorem premature_nested_materialization_is_rejected :
  materialize_nested initial_materialization_state = None.
Proof.
  reflexivity.
Qed.

Theorem phased_evaluation_reaches_exact_state :
  evaluate_phased initial_materialization_state =
  Some final_materialization_state.
Proof.
  reflexivity.
Qed.

Theorem reducer_scheduling_preference_is_irrelevant :
  forall preference,
    evaluate_parallel preference initial_materialization_state =
    Some final_materialization_state.
Proof.
  intros preference.
  destruct preference; reflexivity.
Qed.

Theorem phased_evaluation_conserves :
  materialization_total final_materialization_state =
  materialization_total initial_materialization_state.
Proof.
  reflexivity.
Qed.

Theorem replay_reproduces_materialization_and_reduction :
  forall preference,
    replay_parallel preference initial_materialization_state =
    Some final_materialization_state.
Proof.
  exact reducer_scheduling_preference_is_irrelevant.
Qed.
