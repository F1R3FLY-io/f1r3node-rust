From Stdlib Require Import Arith.PeanoNat Arith.Wf_nat Lists.List Lia.
Import ListNotations.

Section NodeLocalTemporalLifting.

Context {Node : Type}.

Variable node_eq_dec : forall left right : Node, {left = right} + {left <> right}.
Variable active_nodes : list Node.
Variable terminal_phase : nat.
Variable trace : nat -> Node -> nat.
Variable scheduled : nat -> option Node.

Hypothesis active_nodes_nodup : NoDup active_nodes.
Hypothesis phase_bounded :
  forall time node, trace time node <= terminal_phase.
Hypothesis trace_step :
  forall time node,
    trace (S time) node =
    match scheduled time with
    | None => trace time node
    | Some selected =>
        if node_eq_dec node selected
        then if trace time node <? terminal_phase
             then S (trace time node)
             else trace time node
        else trace time node
    end.

Definition enabled (node : Node) (time : nat) : Prop :=
  trace time node < terminal_phase.

Definition globally_weakly_fair : Prop :=
  forall node time,
    In node active_nodes ->
    enabled node time ->
    exists decision_time,
      time <= decision_time /\
      (~ enabled node decision_time \/ scheduled decision_time = Some node).

Hypothesis global_weak_fairness : globally_weakly_fair.

Inductive local_phase_step : nat -> nat -> Prop :=
| local_phase_stutter :
    forall phase, local_phase_step phase phase
| local_phase_advance :
    forall phase,
      phase < terminal_phase ->
      local_phase_step phase (S phase).

Lemma projected_trace_is_local :
  forall node time,
    local_phase_step (trace time node) (trace (S time) node).
Proof.
  intros node time.
  rewrite trace_step.
  destruct (scheduled time) as [selected|].
  - destruct (node_eq_dec node selected) as [same|different].
    + destruct (trace time node <? terminal_phase) eqn:lt.
      * apply local_phase_advance.
        apply Nat.ltb_lt.
        exact lt.
      * apply local_phase_stutter.
    + apply local_phase_stutter.
  - apply local_phase_stutter.
Qed.

Lemma other_node_steps_are_stutters :
  forall time selected node,
    scheduled time = Some selected ->
    node <> selected ->
    trace (S time) node = trace time node.
Proof.
  intros time selected node chosen different.
  rewrite trace_step, chosen.
  destruct (node_eq_dec node selected) as [same|not_same].
  - contradiction.
  - reflexivity.
Qed.

Lemma selected_enabled_node_advances :
  forall time node,
    scheduled time = Some node ->
    enabled node time ->
    trace (S time) node = S (trace time node).
Proof.
  intros time node chosen active.
  rewrite trace_step, chosen.
  destruct (node_eq_dec node node) as [_|impossible].
  - destruct (trace time node <? terminal_phase) eqn:comparison.
    + reflexivity.
    + apply Nat.ltb_ge in comparison.
      unfold enabled in active.
      lia.
  - contradiction.
Qed.

Lemma one_step_monotone :
  forall time node,
    trace time node <= trace (S time) node.
Proof.
  intros time node.
  rewrite trace_step.
  destruct (scheduled time) as [selected|].
  - destruct (node_eq_dec node selected).
    + destruct (trace time node <? terminal_phase); lia.
    + lia.
  - lia.
Qed.

Lemma trace_monotone :
  forall node earlier later,
    earlier <= later ->
    trace earlier node <= trace later node.
Proof.
  intros node earlier later order.
  induction order.
  - apply Nat.le_refl.
  - eapply Nat.le_trans.
    + exact IHorder.
    + apply one_step_monotone.
Qed.

Lemma terminal_phase_is_stable :
  forall node time later,
    time <= later ->
    trace time node = terminal_phase ->
    trace later node = terminal_phase.
Proof.
  intros node time later order terminal.
  pose proof (trace_monotone node time later order) as monotone.
  pose proof (phase_bounded later node) as bounded.
  lia.
Qed.

Lemma global_fairness_projects_to_each_active_node :
  forall node time,
    In node active_nodes ->
    enabled node time ->
    exists decision_time,
      time <= decision_time /\
      (~ enabled node decision_time \/ scheduled decision_time = Some node).
Proof.
  intros node time present active.
  apply global_weak_fairness; assumption.
Qed.

Lemma active_node_eventually_reaches_terminal_phase :
  forall node time,
    In node active_nodes ->
    exists terminal_time,
      time <= terminal_time /\
      trace terminal_time node = terminal_phase.
Proof.
  assert (progress :
    forall deficit node time,
      In node active_nodes ->
      terminal_phase - trace time node = deficit ->
      exists terminal_time,
        time <= terminal_time /\
        trace terminal_time node = terminal_phase).
  {
    intros deficit.
    induction deficit as [deficit induction] using lt_wf_ind.
    intros node time present equation.
    destruct (Nat.eq_dec (trace time node) terminal_phase) as [terminal|not_terminal].
    - exists time.
      split; [lia|exact terminal].
    - assert (active : enabled node time).
      {
        unfold enabled.
        pose proof (phase_bounded time node).
        lia.
      }
      destruct (global_fairness_projects_to_each_active_node node time present active)
        as [decision_time [ordered [disabled|chosen]]].
      + exists decision_time.
        split; [exact ordered|].
        unfold enabled in disabled.
        pose proof (phase_bounded decision_time node).
        lia.
      + destruct (trace decision_time node <? terminal_phase) eqn:comparison.
        * apply Nat.ltb_lt in comparison.
          pose proof
            (selected_enabled_node_advances
              decision_time node chosen comparison) as advanced.
          assert (smaller :
            terminal_phase - trace (S decision_time) node < deficit).
          {
            pose proof (trace_monotone node time decision_time ordered) as monotone.
            lia.
          }
          destruct
            (induction
              (terminal_phase - trace (S decision_time) node)
              smaller
              node
              (S decision_time)
              present
              eq_refl)
            as [terminal_time [after terminal]].
          exists terminal_time.
          split; [lia|exact terminal].
        * apply Nat.ltb_ge in comparison.
          exists decision_time.
          split; [exact ordered|].
          pose proof (phase_bounded decision_time node).
          lia.
  }
  intros node time present.
  eapply progress.
  - exact present.
  - reflexivity.
Qed.

Lemma finite_active_nodes_reach_one_common_terminal_suffix :
  exists start_time,
    forall later node,
      start_time <= later ->
      In node active_nodes ->
      trace later node = terminal_phase.
Proof.
  assert (finite_prefix :
    forall nodes,
      (forall node, In node nodes -> In node active_nodes) ->
      exists time,
        forall node, In node nodes -> trace time node = terminal_phase).
  {
    intros nodes.
    induction nodes as [|node tail induction].
    - intros subset.
      exists 0.
      intros candidate impossible.
      contradiction.
    - intros subset.
      assert (node_active : In node active_nodes).
      {
        apply subset.
        left.
        reflexivity.
      }
      assert (tail_subset : forall candidate, In candidate tail -> In candidate active_nodes).
      {
        intros candidate present.
        apply subset.
        right.
        exact present.
      }
      destruct (active_node_eventually_reaches_terminal_phase node 0 node_active)
        as [node_time [_ node_terminal]].
      destruct (induction tail_subset) as [tail_time tail_terminal].
      exists (Nat.max node_time tail_time).
      intros candidate [same|in_tail].
      + subst candidate.
        eapply terminal_phase_is_stable.
        * apply Nat.le_max_l.
        * exact node_terminal.
      + eapply terminal_phase_is_stable.
        * apply Nat.le_max_r.
        * apply tail_terminal.
          exact in_tail.
  }
  destruct (finite_prefix active_nodes (fun node present => present))
    as [start_time all_terminal].
  exists start_time.
  intros later node ordered present.
  eapply terminal_phase_is_stable.
  - exact ordered.
  - apply all_terminal.
    exact present.
Qed.

Theorem node_local_temporal_product_lifting_correct :
  NoDup active_nodes /\
  (forall node time,
    local_phase_step (trace time node) (trace (S time) node)) /\
  (exists start_time,
    forall later node,
      start_time <= later ->
      In node active_nodes ->
      trace later node = terminal_phase).
Proof.
  split.
  - exact active_nodes_nodup.
  - split.
    + exact projected_trace_is_local.
    + exact finite_active_nodes_reach_one_common_terminal_suffix.
Qed.

End NodeLocalTemporalLifting.

Print Assumptions node_local_temporal_product_lifting_correct.
