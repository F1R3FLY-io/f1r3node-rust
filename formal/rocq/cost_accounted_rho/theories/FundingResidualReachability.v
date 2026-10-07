From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate.
Import ListNotations.

Inductive residual_reachable count (edge : nat -> nat -> bool) root : nat -> Prop :=
| residual_reachable_root : root < count -> residual_reachable count edge root root
| residual_reachable_step : forall from to,
    residual_reachable count edge root from -> to < count -> edge from to = true ->
    residual_reachable count edge root to.

Definition residual_discover count (edge : nat -> nat -> bool) current (seen : nat -> bool) node :=
  seen node || ((node <? count) && edge current node).

Definition residual_mark current (processed : nat -> bool) node :=
  if Nat.eqb node current then true else processed node.

Definition residual_exploration_invariant count edge root (seen processed : nat -> bool) :=
  root < count /\ seen root = true /\
  (forall node, node < count -> processed node = true -> seen node = true) /\
  (forall from to, from < count -> to < count -> processed from = true -> edge from to = true -> seen to = true) /\
  (forall node, node < count -> seen node = true -> residual_reachable count edge root node).

Lemma residual_discover_keeps_seen : forall count edge current seen node,
  seen node = true -> residual_discover count edge current seen node = true.
Proof. intros. unfold residual_discover. rewrite H. reflexivity. Qed.

Theorem residual_exploration_initial : forall count edge root,
  root < count ->
  residual_exploration_invariant count edge root (fun node => Nat.eqb node root) (fun _ => false).
Proof.
  intros count edge root inside. unfold residual_exploration_invariant.
  repeat split; try assumption; try apply Nat.eqb_refl; try discriminate.
  intros node _ same. apply Nat.eqb_eq in same. subst. constructor. exact inside.
Qed.

Theorem residual_exploration_step_preserves_invariant : forall count edge root seen processed current,
  residual_exploration_invariant count edge root seen processed ->
  current < count -> seen current = true ->
  residual_exploration_invariant count edge root
    (residual_discover count edge current seen) (residual_mark current processed).
Proof.
  intros count edge root seen processed current [root_inside [root_seen [included [closed sound]]]] inside current_seen.
  unfold residual_exploration_invariant. repeat split; try assumption.
  - now apply residual_discover_keeps_seen.
  - intros node bounded done. unfold residual_mark in done.
    destruct (Nat.eqb node current) eqn:same.
    + apply Nat.eqb_eq in same. subst. now apply residual_discover_keeps_seen.
    + apply residual_discover_keeps_seen. eapply included; eauto.
  - intros from to from_inside to_inside done linked. unfold residual_mark in done.
    destruct (Nat.eqb from current) eqn:same.
    + apply Nat.eqb_eq in same. subst. unfold residual_discover.
      apply orb_true_iff. right. apply andb_true_iff. split; [apply Nat.ltb_lt; assumption|assumption].
    + apply residual_discover_keeps_seen. exact (closed from to from_inside to_inside done linked).
  - intros node bounded discovered. unfold residual_discover in discovered.
    apply orb_true_iff in discovered. destruct discovered as [old|fresh].
    + eapply sound; eauto.
    + apply andb_true_iff in fresh. destruct fresh as [_ linked].
      eapply residual_reachable_step with (from := current); [apply sound; assumption|exact bounded|exact linked].
Qed.

Theorem exhausted_residual_exploration_is_exact : forall count edge root seen processed,
  residual_exploration_invariant count edge root seen processed ->
  (forall node, node < count -> seen node = true -> processed node = true) ->
  forall node, node < count -> (seen node = true <-> residual_reachable count edge root node).
Proof.
  intros count edge root seen processed [root_inside [root_seen [included [closed sound]]]] exhausted node inside.
  split; [apply sound; exact inside|]. intros reachable.
  induction reachable as [|from to reachable IH to_inside linked].
  - exact root_seen.
  - assert (from_inside : from < count).
    { inversion reachable; subst; assumption. }
    eapply closed; eauto.
Qed.

Definition residual_processed_count count (processed : nat -> bool) :=
  funding_sum count (fun node => if processed node then 1 else 0).

Lemma residual_processed_count_bound : forall count processed,
  residual_processed_count count processed <= count.
Proof.
  induction count; intros processed; unfold residual_processed_count in *; simpl; [lia|].
  specialize (IHcount processed). destruct (processed count); lia.
Qed.

Lemma marking_new_residual_vertex_increments_count : forall count processed current,
  current < count -> processed current = false ->
  residual_processed_count count (residual_mark current processed) = S (residual_processed_count count processed).
Proof.
  induction count; intros processed current inside fresh; [lia|].
  destruct (Nat.eq_dec current count) as [same|different].
  - subst current. unfold residual_processed_count. simpl.
    unfold residual_mark at 2. rewrite Nat.eqb_refl, fresh.
    assert (prefix : funding_sum count (fun node => if residual_mark count processed node then 1 else 0) =
      funding_sum count (fun node => if processed node then 1 else 0)).
    { apply funding_sum_ext. intros node bounded. unfold residual_mark.
      assert (Nat.eqb node count = false) by (apply Nat.eqb_neq; lia). now rewrite H. }
    rewrite prefix. lia.
  - assert (bounded : current < count) by lia.
    specialize (IHcount processed current bounded fresh).
    unfold residual_processed_count in *. simpl. unfold residual_mark at 2.
    assert (Nat.eqb count current = false) by (apply Nat.eqb_neq; lia).
    rewrite H, IHcount. lia.
Qed.

Fixpoint residual_explore_vertices count edge (order : list nat) seen processed :=
  match order with
  | [] => Some (seen, processed)
  | current :: rest =>
      if ((current <? count) && seen current) && negb (processed current)
      then residual_explore_vertices count edge rest
        (residual_discover count edge current seen) (residual_mark current processed)
      else None
  end.

Theorem residual_exploration_history_preserves_invariant : forall count edge root order seen processed next_seen next_processed,
  residual_exploration_invariant count edge root seen processed ->
  residual_explore_vertices count edge order seen processed = Some (next_seen, next_processed) ->
  residual_exploration_invariant count edge root next_seen next_processed.
Proof.
  intros count edge root order. induction order as [|current rest IH]; intros seen processed next_seen next_processed valid; simpl.
  - intros same. inversion same; subst. exact valid.
  - destruct (((current <? count) && seen current) && negb (processed current)) eqn:allowed; [|discriminate].
    apply andb_true_iff in allowed. destruct allowed as [discovered _].
    apply andb_true_iff in discovered. destruct discovered as [inside known]. apply Nat.ltb_lt in inside.
    apply IH. eapply residual_exploration_step_preserves_invariant; eauto.
Qed.

Theorem residual_exploration_history_counts_distinct_vertices : forall count edge order seen processed next_seen next_processed,
  residual_explore_vertices count edge order seen processed = Some (next_seen, next_processed) ->
  residual_processed_count count next_processed = residual_processed_count count processed + length order.
Proof.
  intros count edge order. induction order as [|current rest IH]; intros seen processed next_seen next_processed; simpl.
  - intros same. inversion same; subst. lia.
  - destruct (((current <? count) && seen current) && negb (processed current)) eqn:allowed; [|discriminate].
    apply andb_true_iff in allowed. destruct allowed as [discovered fresh].
    apply andb_true_iff in discovered. destruct discovered as [inside known].
    apply Nat.ltb_lt in inside. apply negb_true_iff in fresh.
    intros history. apply IH in history.
    rewrite marking_new_residual_vertex_increments_count in history by assumption. lia.
Qed.

Theorem residual_exploration_has_at_most_vertex_count_steps : forall count edge order seen processed next_seen next_processed,
  residual_explore_vertices count edge order seen processed = Some (next_seen, next_processed) ->
  length order <= count.
Proof.
  intros. apply residual_exploration_history_counts_distinct_vertices in H.
  pose proof (residual_processed_count_bound count next_processed). lia.
Qed.

Theorem completed_residual_history_is_exact : forall count edge root order next_seen next_processed,
  root < count ->
  residual_explore_vertices count edge order (fun node => Nat.eqb node root) (fun _ => false) = Some (next_seen, next_processed) ->
  (forall node, node < count -> next_seen node = true -> next_processed node = true) ->
  forall node, node < count -> (next_seen node = true <-> residual_reachable count edge root node).
Proof.
  intros count edge root order next_seen next_processed inside history exhausted.
  apply exhausted_residual_exploration_is_exact with next_processed; [|exact exhausted].
  eapply residual_exploration_history_preserves_invariant; [apply residual_exploration_initial; exact inside|exact history].
Qed.

Definition residual_pending_vertex count (seen processed : nat -> bool) :=
  find (fun node => seen node && negb (processed node)) (seq 0 count).

Lemma residual_pending_vertex_some : forall count seen processed current,
  residual_pending_vertex count seen processed = Some current ->
  current < count /\ seen current = true /\ processed current = false.
Proof.
  intros count seen processed current found. unfold residual_pending_vertex in found.
  apply find_some in found. destruct found as [inside pending].
  apply in_seq in inside. apply andb_true_iff in pending. destruct pending as [known fresh].
  apply negb_true_iff in fresh. repeat split; try assumption; lia.
Qed.

Lemma residual_pending_vertex_none : forall count seen processed,
  residual_pending_vertex count seen processed = None ->
  forall node, node < count -> seen node = true -> processed node = true.
Proof.
  intros count seen processed absent node inside known.
  unfold residual_pending_vertex in absent.
  pose proof (find_none _ _ absent node ltac:(apply in_seq; lia)) as not_pending.
  cbn in not_pending.
  rewrite known in not_pending. simpl in not_pending.
  apply negb_false_iff in not_pending. exact not_pending.
Qed.

Fixpoint residual_complete_reference fuel count edge seen processed :=
  match residual_pending_vertex count seen processed with
  | None => Some (seen, processed)
  | Some current =>
      match fuel with
      | 0 => None
      | S rest => residual_complete_reference rest count edge
          (residual_discover count edge current seen) (residual_mark current processed)
      end
  end.

Theorem residual_complete_reference_has_sufficient_fuel : forall fuel count edge seen processed,
  count <= residual_processed_count count processed + fuel ->
  exists next_seen next_processed,
    residual_complete_reference fuel count edge seen processed = Some (next_seen, next_processed).
Proof.
  induction fuel as [|fuel IH]; intros count edge seen processed enough; simpl.
  - destruct (residual_pending_vertex count seen processed) as [current|] eqn:pending.
    + apply residual_pending_vertex_some in pending. destruct pending as [inside [_ fresh]].
      pose proof (marking_new_residual_vertex_increments_count count processed current inside fresh).
      pose proof (residual_processed_count_bound count (residual_mark current processed)). lia.
    + eauto.
  - destruct (residual_pending_vertex count seen processed) as [current|] eqn:pending; [|eauto].
    apply residual_pending_vertex_some in pending. destruct pending as [inside [_ fresh]].
    apply IH. rewrite marking_new_residual_vertex_increments_count by assumption. lia.
Qed.

Theorem residual_complete_reference_preserves_and_exhausts : forall fuel count edge root seen processed next_seen next_processed,
  residual_exploration_invariant count edge root seen processed ->
  residual_complete_reference fuel count edge seen processed = Some (next_seen, next_processed) ->
  residual_exploration_invariant count edge root next_seen next_processed /\
  (forall node, node < count -> next_seen node = true -> next_processed node = true).
Proof.
  induction fuel as [|fuel IH]; intros count edge root seen processed next_seen next_processed valid; simpl.
  - destruct (residual_pending_vertex count seen processed) as [current|] eqn:pending; [discriminate|].
    intros same. inversion same; subst. split; [exact valid|now apply residual_pending_vertex_none].
  - destruct (residual_pending_vertex count seen processed) as [current|] eqn:pending.
    + apply residual_pending_vertex_some in pending. destruct pending as [inside [known _]].
      apply IH. eapply residual_exploration_step_preserves_invariant; eauto.
    + intros same. inversion same; subst. split; [exact valid|now apply residual_pending_vertex_none].
Qed.

Theorem residual_complete_reference_computes_exact_reachability : forall count edge root,
  root < count -> exists seen processed,
  residual_complete_reference count count edge (fun node => Nat.eqb node root) (fun _ => false) = Some (seen, processed) /\
  forall node, node < count -> (seen node = true <-> residual_reachable count edge root node).
Proof.
  intros count edge root inside.
  destruct (residual_complete_reference_has_sufficient_fuel count count edge (fun node => Nat.eqb node root) (fun _ => false) ltac:(lia))
    as [seen [processed result]].
  exists seen, processed. split; [exact result|].
  pose proof (residual_complete_reference_preserves_and_exhausts count count edge root _ _ seen processed
    (residual_exploration_initial count edge root inside) result) as [valid exhausted].
  eapply exhausted_residual_exploration_is_exact; eauto.
Qed.

Print Assumptions residual_complete_reference_has_sufficient_fuel.
Print Assumptions residual_complete_reference_preserves_and_exhausts.
Print Assumptions residual_complete_reference_computes_exact_reachability.
Print Assumptions residual_exploration_initial.
Print Assumptions residual_exploration_step_preserves_invariant.
Print Assumptions exhausted_residual_exploration_is_exact.
Print Assumptions residual_exploration_history_preserves_invariant.
Print Assumptions residual_exploration_history_counts_distinct_vertices.
Print Assumptions residual_exploration_has_at_most_vertex_count_steps.
Print Assumptions completed_residual_history_is_exact.
