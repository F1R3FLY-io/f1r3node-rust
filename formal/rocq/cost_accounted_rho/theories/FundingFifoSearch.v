From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import FundingDiscovery FundingSearchOutcome FundingFifoQueue FundingResidualReachability FundingParentPath.
From CostAccountedRho Require Import FundingResidualGraph FundingResidualCut FundingDeficitCertificate EligibleFundingAssignment.
Import ListNotations.

Definition fifo_processed queue cursor node := existsb (Nat.eqb node) (firstn cursor queue).

Lemma fifo_processed_member : forall queue cursor node,
  fifo_processed queue cursor node = true <-> In node (firstn cursor queue).
Proof.
  intros. unfold fifo_processed. rewrite existsb_exists.
  split.
  - intros [found [member same]]. apply Nat.eqb_eq in same. now subst found.
  - intros member. exists node. split; [exact member|apply Nat.eqb_refl].
Qed.

Lemma fifo_processed_step : forall count before cursor after next_cursor fallback node,
  fifo_queue_step count before cursor after next_cursor ->
  fifo_processed after next_cursor node =
    residual_mark (nth cursor before fallback) (fifo_processed before cursor) node.
Proof.
  intros. unfold fifo_processed, residual_mark.
  rewrite (fifo_step_processes_exactly_current count before cursor after next_cursor fallback H).
  rewrite existsb_app. simpl. rewrite orb_false_r.
  destruct (Nat.eqb node (nth cursor before fallback)), (existsb (Nat.eqb node) (firstn cursor before)); reflexivity.
Qed.

Lemma exploration_processed_pointwise : forall count edge root seen processed other,
  (forall node, processed node = other node) ->
  residual_exploration_invariant count edge root seen processed ->
  residual_exploration_invariant count edge root seen other.
Proof.
  intros count edge root seen processed other equal [inside [known [included [closed sound]]]].
  unfold residual_exploration_invariant. repeat split; try assumption.
  - intros node bounded done. apply included; [exact bounded|]. now rewrite equal.
  - intros from to from_inside to_inside done linked.
    apply (closed from to from_inside to_inside); [now rewrite equal|exact linked].
Qed.

Definition fifo_search_invariant count root edge state cursor :=
  discovery_valid root edge state /\
  Forall (fun vertex => vertex < count) (discovery_queue state) /\
  cursor <= length (discovery_queue state) /\
  residual_exploration_invariant count edge root (discovery_known state)
    (fifo_processed (discovery_queue state) cursor).

Definition fifo_neighbors_valid count edge (neighbors : nat -> list nat) :=
  forall source, source < count ->
    Forall (fun target => target < count) (neighbors source) /\
    forall target, edge source target = true -> (In target (neighbors source) <-> target < count).

Theorem fifo_search_initial : forall count root edge,
  root < count -> fifo_search_invariant count root edge (discovery_initial root) 0.
Proof.
  intros count root edge inside. unfold fifo_search_invariant.
  split; [apply discovery_initial_valid|]. split.
  - constructor; [exact inside|constructor].
  - split; [simpl; lia|].
    change (residual_exploration_invariant count edge root (fun node => Nat.eqb node root) (fun _ => false)).
    now apply residual_exploration_initial.
Qed.

Theorem fifo_search_scan_preserves_invariant : forall count root edge neighbors state cursor,
  fifo_neighbors_valid count edge neighbors -> fifo_search_invariant count root edge state cursor ->
  cursor < length (discovery_queue state) ->
  let source := nth cursor (discovery_queue state) root in
  fifo_search_invariant count root edge (discovery_scan edge state source (neighbors source)) (S cursor).
Proof.
  intros count root edge neighbors state cursor neighbors_valid [valid [bounded [limited exploration]]] pending source.
  assert (source_inside : source < count).
  { rewrite Forall_forall in bounded. apply bounded. apply nth_In. exact pending. }
  assert (source_known : discovery_known state source = true).
  { apply (proj1 (proj2 (proj2 valid))). apply nth_In. exact pending. }
  destruct (neighbors_valid source source_inside) as [targets_bounded coverage].
  pose proof (discovery_scan_gives_fifo_step count root edge state cursor (neighbors source)
    valid bounded pending targets_bounded) as step.
  destruct (proj2 (proj2 (proj2 step))) as [next_distinct [next_bounded next_limited]].
  unfold fifo_search_invariant. split; [now apply discovery_scan_preserves_parents|].
  split; [exact next_bounded|]. split; [exact next_limited|].
  eapply exploration_processed_pointwise.
  - intros node. symmetry. exact (fifo_processed_step count (discovery_queue state) cursor
      (discovery_queue (discovery_scan edge state source (neighbors source))) (S cursor) root node step).
  - exact (sparse_discovery_scan_preserves_exploration (neighbors source) count root edge state
      (fifo_processed (discovery_queue state) cursor) source exploration source_inside source_known coverage).
Qed.

Fixpoint fifo_search_reference fuel root sink edge neighbors state cursor :=
  if discovery_known state sink then Some (state, cursor)
  else if cursor <? length (discovery_queue state) then
    match fuel with
    | 0 => None
    | S remaining =>
        let source := nth cursor (discovery_queue state) root in
        fifo_search_reference remaining root sink edge neighbors
          (discovery_scan edge state source (neighbors source)) (S cursor)
    end
  else Some (state, cursor).

Theorem fifo_search_completes_with_vertex_fuel : forall fuel count root sink edge neighbors state cursor,
  fifo_neighbors_valid count edge neighbors -> fifo_search_invariant count root edge state cursor ->
  count <= cursor + fuel ->
  exists next next_cursor,
    fifo_search_reference fuel root sink edge neighbors state cursor = Some (next, next_cursor) /\
    fifo_search_invariant count root edge next next_cursor /\
    (discovery_known next sink = true \/ next_cursor = length (discovery_queue next)).
Proof.
  induction fuel as [|fuel IH]; intros count root sink edge neighbors state cursor neighbors_valid valid enough; simpl.
  - destruct (discovery_known state sink) eqn:found.
    + exists state, cursor. split; [reflexivity|]. split; [exact valid|]. now left.
    + destruct (cursor <? length (discovery_queue state)) eqn:pending.
      * apply Nat.ltb_lt in pending. destruct valid as [discovery [bounded [limited exploration]]].
        pose proof (unique_bounded_queue_length count (discovery_queue state) (proj1 discovery) bounded). lia.
      * apply Nat.ltb_ge in pending. exists state, cursor. split; [reflexivity|]. split; [exact valid|].
        right. destruct valid as [_ [_ [limited _]]]. lia.
  - destruct (discovery_known state sink) eqn:found.
    + exists state, cursor. split; [reflexivity|]. split; [exact valid|]. now left.
    + destruct (cursor <? length (discovery_queue state)) eqn:pending.
      * apply Nat.ltb_lt in pending. apply (IH count root sink edge neighbors); [exact neighbors_valid| |lia].
        now apply fifo_search_scan_preserves_invariant.
      * apply Nat.ltb_ge in pending. exists state, cursor. split; [reflexivity|]. split; [exact valid|].
        right. destruct valid as [_ [_ [limited _]]]. lia.
Qed.

Theorem completed_fifo_without_sink_has_exact_reachability : forall count root sink edge state cursor,
  fifo_search_invariant count root edge state cursor ->
  (discovery_known state sink = true \/ cursor = length (discovery_queue state)) ->
  discovery_known state sink = false ->
  forall node, node < count ->
    (discovery_known state node = true <-> residual_reachable count edge root node).
Proof.
  intros count root sink edge state cursor [valid [bounded [limited exploration]]] exit absent.
  destruct exit as [found|exhausted]; [congruence|].
  apply exhausted_residual_exploration_is_exact with (processed := fifo_processed (discovery_queue state) cursor); [exact exploration|].
  intros node inside known. apply fifo_processed_member. rewrite exhausted, firstn_all.
  now apply (proj1 (proj2 (proj2 valid))).
Qed.

Theorem initial_fifo_search_returns_path_or_closed_frontier : forall count root sink edge neighbors,
  root < count -> fifo_neighbors_valid count edge neighbors ->
  exists state cursor,
    fifo_search_reference count root sink edge neighbors (discovery_initial root) 0 = Some (state, cursor) /\
    ((discovery_known state sink = true /\ exists path,
        extract_ranked_parent_path (length (discovery_queue state)) root (discovery_parent state) sink = Some path /\
        ranked_parent_path_certificate root (discovery_known state) (discovery_rank state) edge sink path) \/
     (discovery_known state sink = false /\ forall node, node < count ->
        (discovery_known state node = true <-> residual_reachable count edge root node))).
Proof.
  intros count root sink edge neighbors inside neighbors_valid.
  destruct (fifo_search_completes_with_vertex_fuel count count root sink edge neighbors (discovery_initial root) 0
    neighbors_valid (fifo_search_initial count root edge inside) ltac:(lia)) as [state [cursor [result [valid exit]]]].
  exists state, cursor. split; [exact result|].
  destruct (discovery_known state sink) eqn:found.
  - left. split; [reflexivity|]. destruct valid as [[_ [_ [_ [ranks parents]]]] _].
    destruct (ranks sink found) as [bounded positioned].
    eapply strictly_ranked_parents_extract_a_simple_path; [exact parents|exact found|lia].
  - right. split; [reflexivity|].
    apply (completed_fifo_without_sink_has_exact_reachability count root sink edge state cursor valid).
    + rewrite found. exact exit.
    + exact found.
Qed.

Theorem initial_funding_fifo_returns_path_or_checked_deficit : forall sources obligations eligible capacity demand flow neighbors,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand ->
  fifo_neighbors_valid (funding_vertex_count sources obligations)
    (funding_residual_graph sources obligations eligible capacity demand flow) neighbors ->
  exists state cursor,
    fifo_search_reference (funding_vertex_count sources obligations) 0 (funding_sink sources obligations)
      (funding_residual_graph sources obligations eligible capacity demand flow) neighbors
      (discovery_initial 0) 0 = Some (state, cursor) /\
    ((discovery_known state (funding_sink sources obligations) = true /\ exists path,
        extract_ranked_parent_path (length (discovery_queue state)) 0 (discovery_parent state)
          (funding_sink sources obligations) = Some path /\
        ranked_parent_path_certificate 0 (discovery_known state) (discovery_rank state)
          (funding_residual_graph sources obligations eligible capacity demand flow) (funding_sink sources obligations) path) \/
     (discovery_known state (funding_sink sources obligations) = false /\
      funding_deficit_check sources obligations eligible capacity demand
        (fun obligation => negb (discovery_known state (funding_obligation_vertex sources obligation))) = true /\
      ~ exists assignment, assignment_valid sources obligations eligible capacity demand assignment)).
Proof.
  intros sources obligations eligible capacity demand flow neighbors partial short neighbors_valid.
  destruct (initial_fifo_search_returns_path_or_closed_frontier
    (funding_vertex_count sources obligations) 0 (funding_sink sources obligations)
    (funding_residual_graph sources obligations eligible capacity demand flow) neighbors
    ltac:(unfold funding_vertex_count; lia) neighbors_valid) as [state [cursor [result outcome]]].
  exists state, cursor. split; [exact result|].
  destruct outcome as [path|[absent exact_seen]]; [now left|].
  right. split; [exact absent|].
  pose proof (unreachable_funding_sink_establishes_cut_premises sources obligations eligible capacity demand flow
    (discovery_known state) partial short exact_seen absent) as cut.
  assert (deficit : funding_deficit_check sources obligations eligible capacity demand
    (fun obligation => negb (discovery_known state (funding_obligation_vertex sources obligation))) = true).
  { eapply incomplete_residual_cut_has_checked_deficit; eauto. }
  split; [exact deficit|]. eapply funding_deficit_certificate_sound. exact deficit.
Qed.

Print Assumptions initial_funding_fifo_returns_path_or_checked_deficit.
Print Assumptions initial_fifo_search_returns_path_or_closed_frontier.
Print Assumptions fifo_processed_step.
Print Assumptions fifo_search_initial.
Print Assumptions fifo_search_scan_preserves_invariant.
Print Assumptions fifo_search_completes_with_vertex_fuel.
Print Assumptions completed_fifo_without_sink_has_exact_reachability.
