From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import FundingAdjacencyCoverage.

Record native_pair_update before after source target : Prop := {
  native_pair_count : adjacency_count after = S (S (adjacency_count before));
  native_old_owners : forall index, index < adjacency_count before ->
    adjacency_owner after index = adjacency_owner before index;
  native_forward_owner : adjacency_owner after (adjacency_count before) = source;
  native_reverse_owner : adjacency_owner after (S (adjacency_count before)) = target;
  native_old_links : forall index, index < adjacency_count before ->
    adjacency_next after index = adjacency_next before index;
  native_forward_link : adjacency_next after (adjacency_count before) = adjacency_head before source;
  native_reverse_link : adjacency_next after (S (adjacency_count before)) =
    if Nat.eqb source target then Some (adjacency_count before) else adjacency_head before target;
  native_updated_heads : forall vertex, adjacency_head after vertex =
    if Nat.eqb vertex target then Some (S (adjacency_count before))
    else if Nat.eqb vertex source then Some (adjacency_count before) else adjacency_head before vertex
}.

Definition adjacency_equivalent left right :=
  adjacency_count left = adjacency_count right /\
  (forall index, index < adjacency_count left -> adjacency_owner left index = adjacency_owner right index) /\
  (forall index, index < adjacency_count left -> adjacency_next left index = adjacency_next right index) /\
  (forall vertex, adjacency_head left vertex = adjacency_head right vertex).

Theorem equivalent_adjacency_preserves_coverage : forall left right,
  adjacency_equivalent left right -> adjacency_coverage left -> adjacency_coverage right.
Proof.
  intros left right [count_equal [owner_equal [next_equal head_equal]]] valid vertex.
  destruct (valid vertex) as [indices [chain [distinct coverage]]].
  exists indices. split.
  - rewrite <- head_equal. eapply adjacency_chain_pointwise; [exact chain|].
    intros index included. apply coverage in included. apply next_equal. tauto.
  - split; [exact distinct|]. intros index. rewrite coverage.
    split; intros [inside owner].
    + split; [lia|]. rewrite <- owner_equal by exact inside. exact owner.
    + split; [lia|]. rewrite owner_equal by lia. exact owner.
Qed.

Theorem native_pair_contract_refines_indexed_constructor : forall before after source target,
  native_pair_update before after source target ->
  adjacency_equivalent (adjacency_append_pair before source target) after.
Proof.
  intros before after source target [count_equal old_owners forward_owner reverse_owner old_links forward_link reverse_link heads].
  unfold adjacency_equivalent. split.
  - cbn [adjacency_append_pair adjacency_append adjacency_count]. symmetry. exact count_equal.
  - split.
    + intros index inside. cbn [adjacency_append_pair adjacency_append adjacency_count adjacency_owner] in *.
      destruct (Nat.eqb_spec index (S (adjacency_count before))) as [same|not_reverse].
      * subst index. symmetry. exact reverse_owner.
      * destruct (Nat.eqb_spec index (adjacency_count before)) as [same|not_forward].
        -- subst index. symmetry. exact forward_owner.
        -- symmetry. apply old_owners. lia.
    + split.
      * intros index inside. cbn [adjacency_append_pair adjacency_append adjacency_count adjacency_next adjacency_head] in *.
        destruct (Nat.eqb_spec index (S (adjacency_count before))) as [same|not_reverse].
        -- subst index. rewrite Nat.eqb_sym. symmetry. exact reverse_link.
        -- destruct (Nat.eqb_spec index (adjacency_count before)) as [same|not_forward].
           ++ subst index. symmetry. exact forward_link.
           ++ symmetry. apply old_links. lia.
      * intros vertex. cbn [adjacency_append_pair adjacency_append adjacency_head adjacency_count].
        symmetry. apply heads.
Qed.

Theorem native_pair_contract_preserves_exact_coverage : forall before after source target,
  adjacency_coverage before -> native_pair_update before after source target -> adjacency_coverage after.
Proof.
  intros before after source target valid update.
  apply equivalent_adjacency_preserves_coverage with (left := adjacency_append_pair before source target).
  - exact (native_pair_contract_refines_indexed_constructor before after source target update).
  - now apply paired_append_preserves_exact_adjacency_coverage.
Qed.

Inductive native_pair_history : adjacency_model -> adjacency_model -> Prop :=
| native_pair_history_empty : forall state, native_pair_history state state
| native_pair_history_step : forall initial current next source target,
    native_pair_history initial current -> native_pair_update current next source target ->
    native_pair_history initial next.

Theorem native_pair_histories_preserve_exact_coverage : forall initial final,
  native_pair_history initial final -> adjacency_coverage initial -> adjacency_coverage final.
Proof.
  intros initial final history.
  induction history as [state|initial current next source target history IH update]; intros valid; [exact valid|].
  exact (native_pair_contract_preserves_exact_coverage current next source target (IH valid) update).
Qed.

Theorem native_pair_history_from_empty_has_exact_coverage : forall final,
  native_pair_history adjacency_empty final -> adjacency_coverage final.
Proof.
  intros final history. eapply native_pair_histories_preserve_exact_coverage; [exact history|].
  apply empty_adjacency_has_exact_coverage.
Qed.

Print Assumptions equivalent_adjacency_preserves_coverage.
Print Assumptions native_pair_contract_refines_indexed_constructor.
Print Assumptions native_pair_contract_preserves_exact_coverage.
Print Assumptions native_pair_histories_preserve_exact_coverage.
Print Assumptions native_pair_history_from_empty_has_exact_coverage.
