From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import EligibleFundingAssignment FundingDeficitCertificate FundingResidualCut FundingResidualReachability.

Inductive funding_vertex :=
| FundingSource
| FundingPayer (index : nat)
| FundingObligation (index : nat)
| FundingSink.

Definition funding_vertex_count sources obligations := sources + obligations + 2.
Definition funding_sink sources obligations := sources + obligations + 1.
Definition funding_obligation_vertex sources obligation := sources + 1 + obligation.

Definition decode_funding_vertex sources obligations node :=
  if Nat.eqb node 0 then FundingSource
  else if node <=? sources then FundingPayer (node - 1)
  else if node <? sources + 1 + obligations then FundingObligation (node - sources - 1)
  else FundingSink.

Lemma decode_funding_source : forall sources obligations,
  decode_funding_vertex sources obligations 0 = FundingSource.
Proof. reflexivity. Qed.

Lemma decode_funding_payer : forall sources obligations source,
  source < sources -> decode_funding_vertex sources obligations (S source) = FundingPayer source.
Proof.
  intros sources obligations source inside. unfold decode_funding_vertex.
  assert (S source <=? sources = true) by (apply Nat.leb_le; lia).
  simpl Nat.eqb. rewrite H. replace (S source - 1) with source by lia. reflexivity.
Qed.

Lemma decode_funding_obligation : forall sources obligations obligation,
  obligation < obligations ->
  decode_funding_vertex sources obligations (funding_obligation_vertex sources obligation) = FundingObligation obligation.
Proof.
  intros sources obligations obligation inside. unfold decode_funding_vertex, funding_obligation_vertex.
  assert (Nat.eqb (sources + 1 + obligation) 0 = false) by (apply Nat.eqb_neq; lia).
  assert (sources + 1 + obligation <=? sources = false) by (apply Nat.leb_gt; lia).
  assert (sources + 1 + obligation <? sources + 1 + obligations = true) by (apply Nat.ltb_lt; lia).
  rewrite H, H0, H1. replace (sources + 1 + obligation - sources - 1) with obligation by lia. reflexivity.
Qed.

Lemma decode_funding_sink : forall sources obligations,
  decode_funding_vertex sources obligations (funding_sink sources obligations) = FundingSink.
Proof.
  intros. unfold decode_funding_vertex, funding_sink.
  assert (Nat.eqb (sources + obligations + 1) 0 = false) by (apply Nat.eqb_neq; lia).
  assert (sources + obligations + 1 <=? sources = false) by (apply Nat.leb_gt; lia).
  assert (sources + obligations + 1 <? sources + 1 + obligations = false) by (apply Nat.ltb_ge; lia).
  now rewrite H, H0, H1.
Qed.

Definition funding_residual_graph sources obligations eligible capacity demand flow from to :=
  match decode_funding_vertex sources obligations from, decode_funding_vertex sources obligations to with
  | FundingSource, FundingPayer source =>
      source_draw obligations flow source <? Nat.min (capacity source) (funding_sum obligations demand)
  | FundingPayer source, FundingSource => 0 <? source_draw obligations flow source
  | FundingPayer source, FundingObligation obligation =>
      eligible source obligation && (flow source obligation <? funding_sum obligations demand)
  | FundingObligation obligation, FundingPayer source => 0 <? flow source obligation
  | FundingObligation obligation, FundingSink => obligation_draw sources flow obligation <? demand obligation
  | FundingSink, FundingObligation obligation => 0 <? obligation_draw sources flow obligation
  | _, _ => false
  end.

Lemma exact_residual_seen_is_closed : forall count edge seen,
  0 < count ->
  (forall node, node < count -> (seen node = true <-> residual_reachable count edge 0 node)) ->
  seen 0 = true /\
  forall from to, from < count -> to < count -> seen from = true -> edge from to = true -> seen to = true.
Proof.
  intros count edge seen inside exact_seen. split.
  - apply (proj2 (exact_seen 0 inside)). constructor. exact inside.
  - intros from to from_inside to_inside known linked.
    apply (proj2 (exact_seen to to_inside)).
    eapply residual_reachable_step with (from := from); [apply (proj1 (exact_seen from from_inside)); exact known|exact to_inside|exact linked].
Qed.

Theorem unreachable_funding_sink_establishes_cut_premises : forall sources obligations eligible capacity demand flow seen,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand ->
  (forall node, node < funding_vertex_count sources obligations ->
    (seen node = true <-> residual_reachable (funding_vertex_count sources obligations)
      (funding_residual_graph sources obligations eligible capacity demand flow) 0 node)) ->
  seen (funding_sink sources obligations) = false ->
  residual_cut_closed sources obligations eligible capacity demand flow
    (fun source => seen (S source))
    (fun obligation => seen (funding_obligation_vertex sources obligation)).
Proof.
  intros sources obligations eligible capacity demand flow seen partial short exact_seen sink_absent.
  destruct (exact_residual_seen_is_closed (funding_vertex_count sources obligations)
    (funding_residual_graph sources obligations eligible capacity demand flow) seen
    ltac:(unfold funding_vertex_count; lia) exact_seen) as [root_seen closed].
  unfold residual_cut_closed. repeat split.
  - intros source inside absent.
    destruct (proj1 (proj1 partial) source inside) as [capacity_bound _].
    pose proof (partial_funding_source_below_total sources obligations demand flow source short inside) as total_bound.
    assert (bounded : source_draw obligations flow source <= Nat.min (capacity source) (funding_sum obligations demand))
      by (apply Nat.min_glb; lia).
    destruct (Nat.eq_dec (source_draw obligations flow source) (Nat.min (capacity source) (funding_sum obligations demand))) as [equal|different]; [exact equal|].
    assert (reachable : seen (S source) = true).
    { apply (closed 0 (S source)); try (unfold funding_vertex_count; lia); [exact root_seen|].
      unfold funding_residual_graph. rewrite decode_funding_source, decode_funding_payer by assumption.
      apply Nat.ltb_lt. lia. }
    congruence.
  - intros source obligation source_inside obligation_inside payer_seen permitted edge_small.
    apply (closed (S source) (funding_obligation_vertex sources obligation));
      try (unfold funding_vertex_count, funding_obligation_vertex; lia); [exact payer_seen|].
    unfold funding_residual_graph. rewrite decode_funding_payer, decode_funding_obligation by assumption.
    apply andb_true_iff. split; [exact permitted|apply Nat.ltb_lt; exact edge_small].
  - intros source obligation source_inside obligation_inside obligation_seen positive.
    apply (closed (funding_obligation_vertex sources obligation) (S source));
      try (unfold funding_vertex_count, funding_obligation_vertex; lia); [exact obligation_seen|].
    unfold funding_residual_graph. rewrite decode_funding_obligation, decode_funding_payer by assumption.
    apply Nat.ltb_lt. exact positive.
  - intros obligation inside obligation_seen.
    pose proof (proj2 partial obligation inside) as bounded.
    destruct (Nat.eq_dec (obligation_draw sources flow obligation) (demand obligation)) as [equal|different]; [exact equal|].
    assert (reachable : seen (funding_sink sources obligations) = true).
    { apply (closed (funding_obligation_vertex sources obligation) (funding_sink sources obligations));
        try (unfold funding_vertex_count, funding_obligation_vertex, funding_sink; lia); [exact obligation_seen|].
      unfold funding_residual_graph. rewrite decode_funding_obligation by assumption. rewrite decode_funding_sink.
      apply Nat.ltb_lt. lia. }
    congruence.
Qed.

Theorem funding_graph_search_returns_path_or_deficit : forall sources obligations eligible capacity demand flow,
  partial_funding_valid sources obligations eligible capacity demand flow ->
  funding_sum obligations (obligation_draw sources flow) < funding_sum obligations demand ->
  exists seen processed,
    residual_complete_reference (funding_vertex_count sources obligations) (funding_vertex_count sources obligations)
      (funding_residual_graph sources obligations eligible capacity demand flow)
      (fun node => Nat.eqb node 0) (fun _ => false) = Some (seen, processed) /\
    ((seen (funding_sink sources obligations) = true /\
      residual_reachable (funding_vertex_count sources obligations)
        (funding_residual_graph sources obligations eligible capacity demand flow) 0 (funding_sink sources obligations)) \/
     (seen (funding_sink sources obligations) = false /\
      funding_deficit_check sources obligations eligible capacity demand
        (fun obligation => negb (seen (funding_obligation_vertex sources obligation))) = true /\
      ~ exists assignment, assignment_valid sources obligations eligible capacity demand assignment)).
Proof.
  intros sources obligations eligible capacity demand flow partial short.
  destruct (residual_complete_reference_computes_exact_reachability (funding_vertex_count sources obligations)
    (funding_residual_graph sources obligations eligible capacity demand flow) 0 ltac:(unfold funding_vertex_count; lia))
    as [seen [processed [result exact_seen]]].
  exists seen, processed. split; [exact result|].
  destruct (seen (funding_sink sources obligations)) eqn:sink_seen.
  - left. split; [reflexivity|]. apply (proj1 (exact_seen (funding_sink sources obligations) ltac:(unfold funding_vertex_count, funding_sink; lia))). exact sink_seen.
  - right. split; [reflexivity|].
    pose proof (unreachable_funding_sink_establishes_cut_premises sources obligations eligible capacity demand flow seen partial short exact_seen sink_seen) as cut.
    assert (deficit : funding_deficit_check sources obligations eligible capacity demand
      (fun obligation => negb (seen (funding_obligation_vertex sources obligation))) = true).
    { eapply incomplete_residual_cut_has_checked_deficit; eauto. }
    split; [exact deficit|]. eapply funding_deficit_certificate_sound. exact deficit.
Qed.

Print Assumptions decode_funding_payer.
Print Assumptions decode_funding_obligation.
Print Assumptions decode_funding_sink.
Print Assumptions unreachable_funding_sink_establishes_cut_premises.
Print Assumptions funding_graph_search_returns_path_or_deficit.
