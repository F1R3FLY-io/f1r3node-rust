From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import FundingDiscovery FundingParentPath FundingResidualReachability
  FundingResidualGraph FundingResidualCut FundingDeficitCertificate EligibleFundingAssignment.
Import ListNotations.

Lemma discovery_step_known : forall edge state source target node,
  discovery_known (discovery_step edge state source target) node =
  (discovery_known state node ||
    (discovery_known state source && edge source target && Nat.eqb node target)).
Proof.
  intros edge state source target node. unfold discovery_step, discovery_insert; simpl.
  destruct (Nat.eqb_spec node target) as [same|different].
  - subst node. destruct (discovery_known state source) eqn:source_known,
      (edge source target) eqn:linked, (discovery_known state target) eqn:target_known;
      simpl; rewrite ?Nat.eqb_refl, ?target_known; reflexivity.
  - apply Nat.eqb_neq in different.
    destruct (discovery_known state source) eqn:source_known, (edge source target) eqn:linked,
      (discovery_known state target) eqn:target_known, (discovery_known state node) eqn:node_known;
      simpl; rewrite ?different, ?node_known; reflexivity.
Qed.

Definition discovery_scan edge state source targets :=
  discovery_history edge state (map (fun target => (source, target)) targets).

Theorem discovery_scan_known : forall targets edge state source node,
  discovery_known state source = true ->
  discovery_known (discovery_scan edge state source targets) node =
  (discovery_known state node || (edge source node && existsb (Nat.eqb node) targets)).
Proof.
  induction targets as [|target rest IH]; intros edge state source node source_known.
  - unfold discovery_scan; simpl. now rewrite andb_false_r, orb_false_r.
  - change (discovery_known (discovery_scan edge (discovery_step edge state source target) source rest) node =
      (discovery_known state node || (edge source node && existsb (Nat.eqb node) (target :: rest)))).
    rewrite IH.
    + rewrite discovery_step_known, source_known. simpl.
      destruct (Nat.eqb_spec node target) as [same|different].
      * subst node. destruct (discovery_known state target), (edge source target), (existsb (Nat.eqb target) rest); reflexivity.
      * destruct (discovery_known state node), (edge source target), (edge source node), (existsb (Nat.eqb node) rest); reflexivity.
    + rewrite discovery_step_known, source_known. reflexivity.
Qed.

Theorem discovery_scan_preserves_parents : forall targets root edge state source,
  discovery_valid root edge state ->
  discovery_valid root edge (discovery_scan edge state source targets).
Proof.
  intros. apply arbitrary_discovery_histories_preserve_validity. exact H.
Qed.

Lemma existsb_vertex_sequence : forall count node,
  existsb (Nat.eqb node) (seq 0 count) = (node <? count).
Proof.
  intros count node. apply eq_true_iff_eq. rewrite existsb_exists, Nat.ltb_lt.
  split.
  - intros [target [inside same]]. apply Nat.eqb_eq in same. subst target. apply in_seq in inside. lia.
  - intros inside. exists node. split; [apply in_seq; lia|apply Nat.eqb_refl].
Qed.

Theorem complete_discovery_scan_matches_residual_step : forall count edge state source node,
  discovery_known state source = true ->
  discovery_known (discovery_scan edge state source (seq 0 count)) node =
  residual_discover count edge source (discovery_known state) node.
Proof.
  intros. rewrite discovery_scan_known by assumption.
  rewrite existsb_vertex_sequence. unfold residual_discover. now rewrite andb_comm.
Qed.

Theorem discovery_scan_order_independent_membership : forall left right edge state source node,
  (forall target, In target left <-> In target right) ->
  discovery_known state source = true ->
  discovery_known (discovery_scan edge state source left) node =
  discovery_known (discovery_scan edge state source right) node.
Proof.
  intros left right edge state source node same known.
  repeat rewrite discovery_scan_known by exact known.
  assert (equal : existsb (Nat.eqb node) left = existsb (Nat.eqb node) right).
  { apply eq_true_iff_eq. repeat rewrite existsb_exists.
    split; intros [target [inside equal]]; exists target; split; [now apply same|exact equal|now apply same|exact equal]. }
  now rewrite equal.
Qed.

Lemma exploration_invariant_pointwise : forall count edge root seen other processed,
  (forall node, seen node = other node) ->
  residual_exploration_invariant count edge root seen processed ->
  residual_exploration_invariant count edge root other processed.
Proof.
  intros count edge root seen other processed equal [inside [known [included [closed sound]]]].
  unfold residual_exploration_invariant. repeat split; try assumption.
  - rewrite <- equal. exact known.
  - intros node bounded done. rewrite <- equal. now apply included.
  - intros from to from_inside to_inside done linked. rewrite <- equal.
    exact (closed from to from_inside to_inside done linked).
  - intros node bounded discovered. apply sound; [exact bounded|]. now rewrite equal.
Qed.

Theorem sparse_discovery_scan_matches_residual_step : forall targets count edge state source node,
  discovery_known state source = true ->
  (forall target, edge source target = true -> (In target targets <-> target < count)) ->
  discovery_known (discovery_scan edge state source targets) node =
  residual_discover count edge source (discovery_known state) node.
Proof.
  intros targets count edge state source node known coverage.
  rewrite discovery_scan_known by exact known. unfold residual_discover.
  destruct (edge source node) eqn:linked; simpl.
  - assert (equal : existsb (Nat.eqb node) targets = (node <? count)).
    { apply eq_true_iff_eq. rewrite existsb_exists, Nat.ltb_lt.
      split.
      - intros [target [inside same]]. apply Nat.eqb_eq in same. subst target. now apply coverage.
      - intros inside. exists node. split; [now apply coverage|apply Nat.eqb_refl]. }
    now rewrite equal, andb_true_r.
  - now rewrite andb_false_r.
Qed.

Theorem sparse_discovery_scan_preserves_exploration : forall targets count root edge state processed source,
  residual_exploration_invariant count edge root (discovery_known state) processed ->
  source < count -> discovery_known state source = true ->
  (forall target, edge source target = true -> (In target targets <-> target < count)) ->
  residual_exploration_invariant count edge root
    (discovery_known (discovery_scan edge state source targets)) (residual_mark source processed).
Proof.
  intros targets count root edge state processed source valid inside known coverage.
  eapply exploration_invariant_pointwise.
  - intros node. symmetry.
    exact (sparse_discovery_scan_matches_residual_step targets count edge state source node known coverage).
  - now apply residual_exploration_step_preserves_invariant.
Qed.

Theorem complete_discovery_scan_preserves_exploration : forall count root edge state processed source,
  residual_exploration_invariant count edge root (discovery_known state) processed ->
  source < count -> discovery_known state source = true ->
  residual_exploration_invariant count edge root
    (discovery_known (discovery_scan edge state source (seq 0 count))) (residual_mark source processed).
Proof.
  intros count root edge state processed source valid inside known.
  eapply exploration_invariant_pointwise.
  - intros node. symmetry. now apply complete_discovery_scan_matches_residual_step.
  - now apply residual_exploration_step_preserves_invariant.
Qed.

Theorem discovery_search_returns_simple_path_or_checked_deficit : forall sources obligations eligible capacity demand flow state processed,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand ->
  discovery_valid 0 (funding_residual_graph sources obligations eligible capacity demand flow) state ->
  residual_exploration_invariant (funding_vertex_count sources obligations)
    (funding_residual_graph sources obligations eligible capacity demand flow) 0 (discovery_known state) processed ->
  (discovery_known state (funding_sink sources obligations) = false ->
    forall node, node < funding_vertex_count sources obligations -> discovery_known state node = true -> processed node = true) ->
  ((discovery_known state (funding_sink sources obligations) = true /\
    exists path,
      extract_ranked_parent_path (length (discovery_queue state)) 0 (discovery_parent state)
        (funding_sink sources obligations) = Some path /\
      ranked_parent_path_certificate 0 (discovery_known state) (discovery_rank state)
        (funding_residual_graph sources obligations eligible capacity demand flow) (funding_sink sources obligations) path) \/
   (discovery_known state (funding_sink sources obligations) = false /\
    funding_deficit_check sources obligations eligible capacity demand
      (fun obligation => negb (discovery_known state (funding_obligation_vertex sources obligation))) = true /\
    ~ exists assignment, assignment_valid sources obligations eligible capacity demand assignment)).
Proof.
  intros sources obligations eligible capacity demand flow state processed partial short valid exploration exhausted.
  destruct (discovery_known state (funding_sink sources obligations)) eqn:found.
  - left. split; [reflexivity|]. destruct valid as [_ [_ [_ [ranks parents]]]].
    destruct (ranks _ found) as [bounded positioned].
    eapply strictly_ranked_parents_extract_a_simple_path; [exact parents|exact found|lia].
  - right. split; [reflexivity|].
    pose proof (exhausted_residual_exploration_is_exact _ _ _ _ _ exploration (exhausted eq_refl)) as exact_seen.
    pose proof (unreachable_funding_sink_establishes_cut_premises sources obligations eligible capacity demand flow
      (discovery_known state) partial short exact_seen found) as cut.
    assert (deficit : funding_deficit_check sources obligations eligible capacity demand
      (fun obligation => negb (discovery_known state (funding_obligation_vertex sources obligation))) = true).
    { eapply incomplete_residual_cut_has_checked_deficit; eauto. }
    split; [exact deficit|]. eapply funding_deficit_certificate_sound. exact deficit.
Qed.

Print Assumptions discovery_scan_known.
Print Assumptions discovery_scan_preserves_parents.
Print Assumptions complete_discovery_scan_matches_residual_step.
Print Assumptions discovery_scan_order_independent_membership.
Print Assumptions sparse_discovery_scan_matches_residual_step.
Print Assumptions sparse_discovery_scan_preserves_exploration.
Print Assumptions complete_discovery_scan_preserves_exploration.
Print Assumptions discovery_search_returns_simple_path_or_checked_deficit.
