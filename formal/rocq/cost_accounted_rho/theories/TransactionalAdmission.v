From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
Import ListNotations.

Record transactional_candidate : Type := {
  tx_pre_root : nat;
  tx_post_root : nat;
  tx_cost : nat;
  tx_fee : nat;
  tx_execution_completed : bool;
  tx_physical_allocation_valid : bool
}.

Record transactional_result : Type := {
  tx_result_root : nat;
  tx_result_supply : nat;
  tx_result_evidence : list nat;
  tx_result_rejected : bool
}.

Definition tx_admitb
  (expected_root supply : nat)
  (candidate : transactional_candidate)
  : bool :=
  Nat.eqb (tx_pre_root candidate) expected_root &&
  tx_execution_completed candidate &&
  tx_physical_allocation_valid candidate &&
  Nat.leb (tx_cost candidate + tx_fee candidate) supply.

Definition authenticated_supply (pre_state_supply _candidate_mint : nat) : nat :=
  pre_state_supply.

Theorem authenticated_supply_ignores_candidate_mint :
  forall pre_state_supply first_mint second_mint,
    authenticated_supply pre_state_supply first_mint =
    authenticated_supply pre_state_supply second_mint.
Proof.
  reflexivity.
Qed.

Theorem candidate_mint_cannot_establish_authenticated_funding :
  forall pre_state_supply candidate_mint demand fee,
    demand + fee > pre_state_supply ->
    Nat.leb
      (demand + fee)
      (authenticated_supply pre_state_supply candidate_mint) = false.
Proof.
  intros pre_state_supply candidate_mint demand fee Hunderfunded.
  unfold authenticated_supply.
  now apply Nat.leb_gt.
Qed.

Definition transactional_step
  (root supply : nat)
  (evidence : list nat)
  (candidate : transactional_candidate)
  : transactional_result :=
  if tx_admitb root supply candidate then
    {|
      tx_result_root := tx_post_root candidate;
      tx_result_supply := supply - (tx_cost candidate + tx_fee candidate);
      tx_result_evidence := evidence ++ [tx_post_root candidate];
      tx_result_rejected := false
    |}
  else
    {|
      tx_result_root := root;
      tx_result_supply := supply;
      tx_result_evidence := evidence;
      tx_result_rejected := true
    |}.

Theorem rejected_candidate_preserves_root_supply_and_evidence :
  forall root supply evidence candidate,
    tx_admitb root supply candidate = false ->
    tx_result_root (transactional_step root supply evidence candidate) = root /\
    tx_result_supply (transactional_step root supply evidence candidate) = supply /\
    tx_result_evidence (transactional_step root supply evidence candidate) = evidence /\
    tx_result_rejected (transactional_step root supply evidence candidate) = true.
Proof.
  intros root supply evidence candidate Hreject.
  unfold transactional_step.
  now rewrite Hreject.
Qed.

Theorem invalid_physical_allocation_never_checkpoints :
  forall root supply evidence candidate,
    tx_physical_allocation_valid candidate = false ->
    tx_result_evidence (transactional_step root supply evidence candidate) = evidence.
Proof.
  intros root supply evidence candidate Hphysical.
  apply rejected_candidate_preserves_root_supply_and_evidence.
  unfold tx_admitb.
  now rewrite Hphysical, andb_false_r.
Qed.

Theorem admitted_candidate_is_exact_and_conservative :
  forall root supply evidence candidate,
    tx_admitb root supply candidate = true ->
    tx_result_root (transactional_step root supply evidence candidate) =
      tx_post_root candidate /\
    tx_result_evidence (transactional_step root supply evidence candidate) =
      evidence ++ [tx_post_root candidate] /\
    tx_result_supply (transactional_step root supply evidence candidate) +
      tx_cost candidate + tx_fee candidate = supply.
Proof.
  intros root supply evidence candidate Hadmit.
  unfold transactional_step.
  rewrite Hadmit.
  simpl.
  repeat split.
  unfold tx_admitb in Hadmit.
  repeat rewrite andb_true_iff in Hadmit.
  destruct Hadmit as [[[Hroot Hcomplete] Hphysical] Hfunded].
  apply Nat.leb_le in Hfunded.
  lia.
Qed.

Theorem rejected_candidate_cannot_change_later_prestate :
  forall root supply evidence rejected later,
    tx_admitb root supply rejected = false ->
    transactional_step
      (tx_result_root (transactional_step root supply evidence rejected))
      (tx_result_supply (transactional_step root supply evidence rejected))
      (tx_result_evidence (transactional_step root supply evidence rejected))
      later =
    transactional_step root supply evidence later.
Proof.
  intros root supply evidence rejected later Hreject.
  unfold transactional_step.
  now rewrite Hreject.
Qed.

Print Assumptions rejected_candidate_preserves_root_supply_and_evidence.
Print Assumptions authenticated_supply_ignores_candidate_mint.
Print Assumptions candidate_mint_cannot_establish_authenticated_funding.
Print Assumptions invalid_physical_allocation_never_checkpoints.
Print Assumptions admitted_candidate_is_exact_and_conservative.
Print Assumptions rejected_candidate_cannot_change_later_prestate.
