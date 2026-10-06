From Stdlib Require Import Lists.List Arith.PeanoNat Bool.Bool Lia.
Import ListNotations.

Record funded_observation := {
  observed_post_root : nat;
  observed_effects : list nat;
  observed_resource_cost : nat;
  observed_fee_cost : nat
}.

Definition observation_eq_dec : forall (a b : funded_observation), {a = b} + {a <> b}.
Proof. decide equality; try apply Nat.eq_dec; apply list_eq_dec, Nat.eq_dec. Defined.

Section ExactCase.
  Variable execution : nat -> nat -> funded_observation -> Prop.
  Variable certified_scope : nat -> nat -> funded_observation -> Prop.
  Variable frozen_root captured_input : nat.
  Variable observed : funded_observation.

  Hypothesis observed_execution : execution frozen_root captured_input observed.
  Hypothesis observed_in_scope : certified_scope frozen_root captured_input observed.
  Hypothesis captured_scope_closed : forall root input outcome,
    certified_scope root input outcome ->
    root = frozen_root /\ input = captured_input /\ execution root input outcome.
  Hypothesis execution_deterministic : forall root input left right,
    execution root input left -> execution root input right -> left = right.

  Theorem state_bound_observed_case_covers_certified_scope :
    forall root input outcome,
      certified_scope root input outcome ->
      root = frozen_root /\ input = captured_input /\ outcome = observed.
  Proof.
    intros root input outcome scoped.
    destruct (captured_scope_closed _ _ _ scoped) as [root_eq [input_eq run]].
    subst root input. repeat split; auto.
    eapply execution_deterministic; eauto.
  Qed.

  Theorem state_bound_scope_is_nonempty_and_singleton :
    certified_scope frozen_root captured_input observed /\
    forall root input outcome,
      certified_scope root input outcome ->
      root = frozen_root /\ input = captured_input /\ outcome = observed.
  Proof.
    split; [exact observed_in_scope|].
    exact state_bound_observed_case_covers_certified_scope.
  Qed.
End ExactCase.

Record funded_state := {
  state_root : nat;
  state_supply : nat;
  state_effects : list nat;
  state_charges : list nat;
  state_published : list nat
}.

Record private_candidate := {
  candidate_id : nat;
  candidate_pre_root : nat;
  candidate_observation : funded_observation;
  candidate_replay_observation : funded_observation
}.

Definition candidate_charge candidate :=
  observed_resource_cost (candidate_observation candidate) +
  observed_fee_cost (candidate_observation candidate).

Definition certify_candidate state candidate :=
  Nat.eqb (candidate_pre_root candidate) (state_root state) &&
  negb (existsb (Nat.eqb (candidate_id candidate)) (state_published state)) &&
  (if observation_eq_dec (candidate_observation candidate)
       (candidate_replay_observation candidate) then true else false) &&
  Nat.leb (candidate_charge candidate) (state_supply state).

Definition publish_candidate state candidate :=
  if certify_candidate state candidate then
    {| state_root := observed_post_root (candidate_observation candidate);
       state_supply := state_supply state - candidate_charge candidate;
       state_effects := state_effects state ++
         observed_effects (candidate_observation candidate);
       state_charges := state_charges state ++ [candidate_charge candidate];
       state_published := candidate_id candidate :: state_published state |}
  else state.

Theorem rejected_private_candidate_has_no_published_effect : forall state candidate,
  certify_candidate state candidate = false -> publish_candidate state candidate = state.
Proof. intros. unfold publish_candidate. now rewrite H. Qed.

Theorem stale_private_candidate_cannot_publish : forall state candidate,
  candidate_pre_root candidate <> state_root state ->
  publish_candidate state candidate = state.
Proof.
  intros state candidate stale. apply rejected_private_candidate_has_no_published_effect.
  unfold certify_candidate.
  assert (Nat.eqb (candidate_pre_root candidate) (state_root state) = false) as no_root.
  { apply Nat.eqb_neq. exact stale. }
  now rewrite no_root.
Qed.

Theorem underfunded_private_candidate_cannot_publish : forall state candidate,
  state_supply state < candidate_charge candidate ->
  publish_candidate state candidate = state.
Proof.
  intros state candidate insufficient. apply rejected_private_candidate_has_no_published_effect.
  unfold certify_candidate.
  assert (Nat.leb (candidate_charge candidate) (state_supply state) = false) as no_funds.
  { apply Nat.leb_gt. exact insufficient. }
  now rewrite no_funds, andb_false_r.
Qed.

Theorem replay_disagreement_cannot_publish : forall state candidate,
  candidate_observation candidate <> candidate_replay_observation candidate ->
  publish_candidate state candidate = state.
Proof.
  intros state candidate mismatch. apply rejected_private_candidate_has_no_published_effect.
  unfold certify_candidate.
  destruct (observation_eq_dec _ _) as [same|different]; [contradiction|].
  simpl. now rewrite andb_false_r.
Qed.

Theorem certified_candidate_has_exact_replay_and_funding : forall state candidate,
  certify_candidate state candidate = true ->
  candidate_pre_root candidate = state_root state /\
  candidate_observation candidate = candidate_replay_observation candidate /\
  candidate_charge candidate <= state_supply state /\
  ~ In (candidate_id candidate) (state_published state).
Proof.
  intros state candidate certified. unfold certify_candidate in certified.
  repeat rewrite andb_true_iff in certified.
  destruct certified as [[[root fresh] replay] funded].
  apply Nat.eqb_eq in root. apply Nat.leb_le in funded.
  destruct (observation_eq_dec _ _) as [equal|different]; [|discriminate].
  apply negb_true_iff in fresh.
  repeat split; auto.
  intro member.
  assert (existsb (Nat.eqb (candidate_id candidate)) (state_published state) = true) as found.
  { apply existsb_exists. exists (candidate_id candidate).
    split; [exact member|apply Nat.eqb_refl]. }
  congruence.
Qed.

Theorem certified_state_bound_candidate_refines_complete_scope :
  forall (execution certified_scope : nat -> nat -> funded_observation -> Prop)
    (frozen_root captured_input : nat) (observed : funded_observation)
    (state : funded_state) (candidate : private_candidate),
    execution frozen_root captured_input observed ->
    certified_scope frozen_root captured_input observed ->
    (forall root input outcome, certified_scope root input outcome ->
       root = frozen_root /\ input = captured_input /\ execution root input outcome) ->
    (forall root input left right, execution root input left ->
       execution root input right -> left = right) ->
    candidate_pre_root candidate = frozen_root ->
    candidate_observation candidate = observed ->
    certify_candidate state candidate = true ->
    state_root state = frozen_root /\
    candidate_charge candidate <= state_supply state /\
    forall root input outcome, certified_scope root input outcome ->
      root = frozen_root /\ input = captured_input /\
      outcome = candidate_replay_observation candidate.
Proof.
  intros execution scope root input observed state candidate run nonempty closed deterministic
    candidate_root candidate_observed certified.
  pose proof (certified_candidate_has_exact_replay_and_funding _ _ certified)
    as [fresh [same [funded _]]].
  split; [congruence|]. split; [exact funded|].
  intros other_root other_input outcome scoped.
  pose proof (state_bound_scope_is_nonempty_and_singleton execution scope root input
    observed run nonempty closed deterministic) as [_ covers].
  specialize (covers _ _ _ scoped).
  destruct covers as [same_root [same_input same_outcome]].
  repeat split; auto. congruence.
Qed.

Theorem accepted_candidate_charges_once_and_matches_replay : forall state candidate,
  certify_candidate state candidate = true ->
  state_supply (publish_candidate state candidate) + candidate_charge candidate =
    state_supply state /\
  state_charges (publish_candidate state candidate) =
    state_charges state ++ [candidate_charge candidate] /\
  state_effects (publish_candidate state candidate) =
    state_effects state ++ observed_effects (candidate_replay_observation candidate).
Proof.
  intros state candidate certified.
  pose proof (certified_candidate_has_exact_replay_and_funding _ _ certified)
    as [_ [same [funded _]]].
  unfold publish_candidate. rewrite certified. simpl.
  rewrite <- same. repeat split; auto. lia.
Qed.

Theorem duplicate_delivery_does_not_charge_again : forall state candidate,
  certify_candidate state candidate = true ->
  publish_candidate (publish_candidate state candidate) candidate =
    publish_candidate state candidate.
Proof.
  intros state candidate certified.
  apply rejected_private_candidate_has_no_published_effect.
  unfold publish_candidate at 1. rewrite certified.
  unfold certify_candidate. simpl.
  rewrite Nat.eqb_refl. now rewrite andb_false_r.
Qed.

Print Assumptions state_bound_observed_case_covers_certified_scope.
Print Assumptions state_bound_scope_is_nonempty_and_singleton.
Print Assumptions rejected_private_candidate_has_no_published_effect.
Print Assumptions stale_private_candidate_cannot_publish.
Print Assumptions underfunded_private_candidate_cannot_publish.
Print Assumptions replay_disagreement_cannot_publish.
Print Assumptions accepted_candidate_charges_once_and_matches_replay.
Print Assumptions duplicate_delivery_does_not_charge_again.
Print Assumptions certified_state_bound_candidate_refines_complete_scope.
