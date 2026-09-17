From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import FundingDiscovery FundingNetworkAugmentation FundingPathEncoding.
Import ListNotations.

Record parent_edge_state := {
  edge_discovery : discovery_state;
  discovered_operation : nat -> residual_operation
}.

Definition parent_edges_valid count root edge from to network state :=
  discovery_valid root edge (edge_discovery state) /\
  forall node, discovery_known (edge_discovery state) node = true -> node <> root ->
    fst (discovered_operation state node) < count /\
    operation_to from to (discovered_operation state node) = node /\
    operation_from from to (discovered_operation state node) = discovery_parent (edge_discovery state) node /\
    0 < operation_capacity network (discovered_operation state node).

Definition parent_edges_initial root := {|
  edge_discovery := discovery_initial root;
  discovered_operation := fun _ => (0, false)
|}.

Definition parent_edge_step from to network state op :=
  let source := operation_from from to op in
  let target := operation_to from to op in
  if discovery_known (edge_discovery state) source && (0 <? operation_capacity network op) &&
    negb (discovery_known (edge_discovery state) target)
  then {| edge_discovery := discovery_insert (edge_discovery state) source target;
          discovered_operation := fun node => if Nat.eqb node target then op else discovered_operation state node |}
  else state.

Fixpoint parent_edge_history from to network state operations :=
  match operations with
  | [] => state
  | op :: rest => parent_edge_history from to network (parent_edge_step from to network state op) rest
  end.

Theorem parent_edges_initial_valid : forall count root edge from to network,
  parent_edges_valid count root edge from to network (parent_edges_initial root).
Proof.
  intros. split; [apply discovery_initial_valid|].
  intros node known different. simpl in known. apply Nat.eqb_eq in known. contradiction.
Qed.

Theorem parent_edge_step_preserves_validity : forall count root edge from to network state op,
  parent_edges_valid count root edge from to network state ->
  fst op < count ->
  (0 < operation_capacity network op -> edge (operation_from from to op) (operation_to from to op) = true) ->
  parent_edges_valid count root edge from to network (parent_edge_step from to network state op).
Proof.
  intros count root edge from to network state op [valid parents] inside linked.
  unfold parent_edge_step.
  destruct (discovery_known (edge_discovery state) (operation_from from to op) &&
    (0 <? operation_capacity network op) && negb (discovery_known (edge_discovery state) (operation_to from to op))) eqn:enabled.
  - apply andb_true_iff in enabled. destruct enabled as [first absent].
    apply andb_true_iff in first. destruct first as [known positive].
    apply Nat.ltb_lt in positive. apply negb_true_iff in absent.
    split.
    + apply discovery_insert_preserves_validity; auto.
    + intros node present nonroot. cbn [edge_discovery discovery_insert discovered_operation] in *.
      cbn [discovery_insert discovery_known discovery_parent] in *.
      destruct (Nat.eqb node (operation_to from to op)) eqn:same.
      * apply Nat.eqb_eq in same. subst node. repeat split; try assumption; reflexivity.
      * exact (parents node present nonroot).
  - split; assumption.
Qed.

Theorem parent_edge_histories_preserve_validity : forall operations count root edge from to network state,
  parent_edges_valid count root edge from to network state ->
  (forall op, In op operations -> fst op < count /\
    (0 < operation_capacity network op -> edge (operation_from from to op) (operation_to from to op) = true)) ->
  parent_edges_valid count root edge from to network (parent_edge_history from to network state operations).
Proof.
  induction operations as [|op rest IH]; intros count root edge from to network state valid permitted; simpl; [exact valid|].
  apply IH.
  - destruct (permitted op (or_introl eq_refl)) as [inside linked].
    now apply parent_edge_step_preserves_validity.
  - intros next included. apply permitted. now right.
Qed.

Lemma parent_edge_operations_are_permitted : forall fuel count root edge from to network state node operations,
  parent_edges_valid count root edge from to network state ->
  discovery_known (edge_discovery state) node = true ->
  extract_parent_operations fuel root from to (discovered_operation state) node = Some operations ->
  forall op, In op operations -> fst op < count /\ 0 < operation_capacity network op.
Proof.
  induction fuel as [|fuel IH]; intros count root edge from to network state node operations valid known extracted op included;
    cbn [extract_parent_operations] in extracted;
    destruct (Nat.eqb node root) eqn:is_root.
  - inversion extracted; subst. contradiction.
  - discriminate.
  - inversion extracted; subst. contradiction.
  - destruct (extract_parent_operations fuel root from to (discovered_operation state)
      (operation_from from to (discovered_operation state node))) as [prefix|] eqn:earlier; [|discriminate].
    inversion extracted; subst operations. apply in_app_iff in included.
    pose proof valid as [discovery parents]. apply Nat.eqb_neq in is_root.
    destruct (parents node known is_root) as [inside [target [source positive]]].
    destruct included as [previous|current].
    + eapply IH; [exact valid| |exact earlier|exact previous].
      rewrite source. exact (proj1 (proj2 (proj2 (proj2 (proj2 discovery))) node known is_root)).
    + destruct current as [same|absent]; [subst op; auto|contradiction].
Qed.

Theorem recorded_parent_edges_extract_valid_operations : forall count root edge from to network state node,
  parent_edges_valid count root edge from to network state ->
  discovery_known (edge_discovery state) node = true ->
  exists operations,
    extract_parent_operations (length (discovery_queue (edge_discovery state))) root from to
      (discovered_operation state) node = Some operations /\
    linked_operations from to root operations node /\
    NoDup (operation_vertices from to root operations) /\
    NoDup (map fst operations) /\
    length operations < length (discovery_queue (edge_discovery state)) /\
    forall op, In op operations -> fst op < count /\ 0 < operation_capacity network op.
Proof.
  intros count root edge from to network state node valid known.
  pose proof valid as [discovery parents].
  destruct discovery as [distinct [root_known [members [ranks ranked]]]].
  assert (parent_ranks : forall current, discovery_known (edge_discovery state) current = true -> current <> root ->
    operation_to from to (discovered_operation state current) = current /\
    discovery_known (edge_discovery state) (operation_from from to (discovered_operation state current)) = true /\
    discovery_rank (edge_discovery state) (operation_from from to (discovered_operation state current)) <
      discovery_rank (edge_discovery state) current).
  { intros current present nonroot. destruct (parents current present nonroot) as [_ [target [source _]]].
    rewrite source. destruct (ranked current present nonroot) as [previous [smaller _]]. auto. }
  destruct (ranks node known) as [bounded positioned].
  destruct (ranked_parents_extract_linked_simple_operations (length (discovery_queue (edge_discovery state))) root from to
    (discovered_operation state) (discovery_known (edge_discovery state)) (discovery_rank (edge_discovery state)) node
    parent_ranks known ltac:(lia)) as [operations [extracted [linked [simple [path_ranks length_bound]]]]].
  exists operations. split; [exact extracted|]. split; [exact linked|]. split; [exact simple|].
  split; [eapply simple_linked_path_has_distinct_pairs; eassumption|]. split; [lia|].
  exact (parent_edge_operations_are_permitted (length (discovery_queue (edge_discovery state)))
    count root edge from to network state node operations valid known extracted).
Qed.

Theorem initial_parent_edge_histories_extract_valid_operations : forall operations count root edge from to network node,
  (forall op, In op operations -> fst op < count /\
    (0 < operation_capacity network op -> edge (operation_from from to op) (operation_to from to op) = true)) ->
  let state := parent_edge_history from to network (parent_edges_initial root) operations in
  discovery_known (edge_discovery state) node = true ->
  exists path,
    extract_parent_operations (length (discovery_queue (edge_discovery state))) root from to
      (discovered_operation state) node = Some path /\
    linked_operations from to root path node /\
    NoDup (operation_vertices from to root path) /\
    NoDup (map fst path) /\
    length path < length (discovery_queue (edge_discovery state)) /\
    forall op, In op path -> fst op < count /\ 0 < operation_capacity network op.
Proof.
  intros operations count root edge from to network node permitted state known.
  apply (recorded_parent_edges_extract_valid_operations count root edge from to network state node); [|exact known].
  apply parent_edge_histories_preserve_validity; [apply parent_edges_initial_valid|exact permitted].
Qed.

Print Assumptions initial_parent_edge_histories_extract_valid_operations.
Print Assumptions parent_edges_initial_valid.
Print Assumptions parent_edge_step_preserves_validity.
Print Assumptions parent_edge_histories_preserve_validity.
Print Assumptions parent_edge_operations_are_permitted.
Print Assumptions recorded_parent_edges_extract_valid_operations.
