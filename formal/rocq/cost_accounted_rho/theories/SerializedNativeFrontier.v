From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
From CostAccountedRho Require Import StateBoundCandidateFunding.
Import ListNotations.

Record native_frontier_event := {
  frontier_operation : nat;
  frontier_pre_root : nat;
  frontier_captured_input : nat;
  frontier_post_root : nat;
  frontier_host_charge : nat;
  frontier_phlo_charge : nat;
  frontier_external_effects : list nat
}.

Fixpoint final_frontier_root
  (step_root : nat -> nat -> nat -> nat)
  (root captured_input : nat) (operations : list nat) : nat :=
  match operations with
  | [] => root
  | operation :: rest =>
      final_frontier_root step_root
        (step_root root captured_input operation) captured_input rest
  end.

Fixpoint trace_post_root (root : nat) (trace : list native_frontier_event) : nat :=
  match trace with
  | [] => root
  | event :: rest => trace_post_root (frontier_post_root event) rest
  end.

Definition trace_observation root fee trace : funded_observation :=
  {| observed_post_root := trace_post_root root trace;
     observed_effects := map frontier_operation trace;
     observed_resource_cost := fold_right Nat.add 0 (map frontier_phlo_charge trace);
     observed_fee_cost := fee |}.

Section SerializedFrontier.
  Variable step_root : nat -> nat -> nat -> nat.
  Variable host_charge : nat -> nat.
  Variable phlo_charge : nat -> nat.
  Variable frontiers : list (list nat).
  Variable host_limit frozen_root captured_input fee_rev : nat.

  Definition complete_order := concat frontiers.

  Inductive refines_complete_trace : nat -> nat -> list nat ->
    list native_frontier_event -> Prop :=
  | refines_empty : forall root input,
      refines_complete_trace root input [] []
  | refines_next : forall root input operation rest event trace,
      frontier_operation event = operation ->
      frontier_pre_root event = root ->
      frontier_captured_input event = input ->
      frontier_post_root event = step_root root input operation ->
      frontier_host_charge event = host_charge operation ->
      frontier_phlo_charge event = phlo_charge operation ->
      frontier_external_effects event = [] ->
      refines_complete_trace
        (step_root root input operation) input rest trace ->
      refines_complete_trace root input (operation :: rest) (event :: trace).

  Definition serial_observation root input operations : funded_observation :=
    {| observed_post_root := final_frontier_root step_root root input operations;
       observed_effects := operations;
       observed_resource_cost := fold_right Nat.add 0 (map phlo_charge operations);
       observed_fee_cost := fee_rev |}.

  Lemma complete_trace_refines_serial_observation : forall root input operations trace,
    refines_complete_trace root input operations trace ->
    trace_observation root fee_rev trace = serial_observation root input operations /\
    fold_right Nat.add 0 (map frontier_host_charge trace) =
      fold_right Nat.add 0 (map host_charge operations) /\
    Forall (fun event => frontier_external_effects event = []) trace.
  Proof.
    intros root input operations trace refined.
    induction refined.
    - simpl. split; [reflexivity|]. split; [reflexivity|constructor].
    - destruct IHrefined as [same [same_host pure]].
      unfold trace_observation, serial_observation in *.
      simpl in *.
      rewrite H, H2, H4.
      inversion same; subst.
      rewrite H3, same_host.
      split; [reflexivity|]. split; [reflexivity|constructor; assumption].
  Qed.

  Definition accepted_serial_execution root input observation : Prop :=
    exists trace,
      refines_complete_trace root input complete_order trace /\
      fold_right Nat.add 0 (map frontier_host_charge trace) <= host_limit /\
      observation = trace_observation root fee_rev trace.

  Lemma host_work_prefix_bounded : forall amounts count limit,
    fold_right Nat.add 0 amounts <= limit ->
    fold_right Nat.add 0 (firstn count amounts) <= limit.
  Proof.
    intros amounts count limit bounded.
    assert (sum_app : forall left right,
      fold_right Nat.add 0 (left ++ right) =
      fold_right Nat.add 0 left + fold_right Nat.add 0 right).
    { intros left right. induction left; simpl; lia. }
    rewrite <- (firstn_skipn count amounts) in bounded at 1.
    rewrite sum_app in bounded. lia.
  Qed.

  Theorem accepted_serial_execution_has_no_host_budget_prefix_failure :
    forall root input observation,
      accepted_serial_execution root input observation ->
      exists trace,
        refines_complete_trace root input complete_order trace /\
        observation = trace_observation root fee_rev trace /\
        forall count,
          fold_right Nat.add 0
            (firstn count (map frontier_host_charge trace)) <= host_limit.
  Proof.
    intros root input observation [trace [refined [bounded same]]].
    exists trace. repeat split; auto.
    intros count. apply host_work_prefix_bounded. exact bounded.
  Qed.

  Lemma accepted_serial_execution_has_complete_bounded_observation :
    forall root input observation,
      accepted_serial_execution root input observation ->
      observation = serial_observation root input complete_order /\
      fold_right Nat.add 0 (map host_charge complete_order) <= host_limit.
  Proof.
    intros root input observation [trace [refined [bounded same]]].
    pose proof (complete_trace_refines_serial_observation
      root input complete_order trace refined) as [exact [host_exact _]].
    subst observation. split; [exact exact|].
    rewrite <- host_exact. exact bounded.
  Qed.

  Lemma accepted_serial_execution_is_deterministic :
    forall root input left right,
      accepted_serial_execution root input left ->
      accepted_serial_execution root input right ->
      left = right.
  Proof.
    intros root input left right left_run right_run.
    destruct (accepted_serial_execution_has_complete_bounded_observation
      _ _ _ left_run) as [left_exact _].
    destruct (accepted_serial_execution_has_complete_bounded_observation
      _ _ _ right_run) as [right_exact _].
    congruence.
  Qed.

  Definition frozen_certified_scope root input observation : Prop :=
    root = frozen_root /\ input = captured_input /\
    accepted_serial_execution root input observation.

  Theorem serialized_native_scope_is_singleton : forall observed,
    accepted_serial_execution frozen_root captured_input observed ->
    frozen_certified_scope frozen_root captured_input observed /\
    forall root input outcome,
      frozen_certified_scope root input outcome ->
      root = frozen_root /\ input = captured_input /\ outcome = observed.
  Proof.
    intros observed accepted. split.
    - repeat split; auto.
    - intros root input outcome [same_root [same_input run]].
      subst root input. repeat split; auto.
      eapply accepted_serial_execution_is_deterministic; eauto.
  Qed.
End SerializedFrontier.

Print Assumptions complete_trace_refines_serial_observation.
Print Assumptions accepted_serial_execution_has_complete_bounded_observation.
Print Assumptions accepted_serial_execution_has_no_host_budget_prefix_failure.
Print Assumptions accepted_serial_execution_is_deterministic.
Print Assumptions serialized_native_scope_is_singleton.
