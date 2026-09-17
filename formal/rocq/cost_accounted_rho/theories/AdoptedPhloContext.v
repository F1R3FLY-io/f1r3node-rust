From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import SignedPhloControls.

Definition captured_chain_context_matches
    (adopted : phlo_environment) (minimum : nat)
    (schedule : phlo_schedule) (captured_minimum : nat) :=
  (captured_minimum =? minimum) &&
  (phlo_protocol_version schedule =? active_phlo_version adopted) &&
  (phlo_shard schedule =? native_phlo_shard adopted).

Theorem captured_chain_context_exact : forall adopted minimum schedule captured_minimum,
  captured_chain_context_matches adopted minimum schedule captured_minimum = true <->
  captured_minimum = minimum /\
  phlo_protocol_version schedule = active_phlo_version adopted /\
  phlo_shard schedule = native_phlo_shard adopted.
Proof.
  intros. unfold captured_chain_context_matches.
  repeat rewrite andb_true_iff. repeat rewrite Nat.eqb_eq. tauto.
Qed.

Theorem altered_captured_chain_context_rejected : forall adopted minimum schedule captured_minimum,
  (captured_minimum <> minimum \/
   phlo_protocol_version schedule <> active_phlo_version adopted \/
   phlo_shard schedule <> native_phlo_shard adopted) ->
  captured_chain_context_matches adopted minimum schedule captured_minimum = false.
Proof.
  intros adopted minimum schedule captured_minimum altered.
  destruct (captured_chain_context_matches adopted minimum schedule captured_minimum) eqn:checked; auto.
  apply captured_chain_context_exact in checked. tauto.
Qed.

Theorem adopted_chain_offer_respects_minimum_and_owners :
  forall adopted minimum captured_environment captured_minimum machine_max offer terms schedule bound,
  captured_chain_context_matches adopted minimum schedule captured_minimum = true ->
  admit_offered_phlo_controls captured_environment captured_minimum machine_max
    offer terms schedule bound = true ->
  minimum <= offered_phlo_price offer /\
  Forall (fun ceiling => offered_phlo_price offer <= ceiling) (signed_required_ceilings terms).
Proof.
  intros adopted minimum captured_environment captured_minimum machine_max offer terms schedule bound matched admitted.
  apply captured_chain_context_exact in matched. destruct matched as [same _]. subst captured_minimum.
  now apply offered_price_respects_chain_and_all_owners in admitted.
Qed.

Theorem common_family_context_checks_every_case :
  forall (Case : Type) (cases : list Case) first (schedule_of : Case -> phlo_schedule)
    (minimum_of : Case -> nat) adopted minimum,
  Forall (fun case => schedule_of case = schedule_of first /\ minimum_of case = minimum_of first) cases ->
  captured_chain_context_matches adopted minimum (schedule_of first) (minimum_of first) = true ->
  Forall (fun case => captured_chain_context_matches adopted minimum (schedule_of case) (minimum_of case) = true) cases.
Proof.
  intros Case cases first schedule_of minimum_of adopted minimum common checked.
  induction common; constructor; auto. destruct H as [schedule_same minimum_same].
  now rewrite schedule_same, minimum_same.
Qed.

Print Assumptions captured_chain_context_exact.
Print Assumptions altered_captured_chain_context_rejected.
Print Assumptions adopted_chain_offer_respects_minimum_and_owners.
Print Assumptions common_family_context_checks_every_case.

Definition genesis_bound_context_matches adopted adopted_minimum genesis_minimum schedule captured_minimum :=
  (adopted_minimum =? genesis_minimum) &&
  captured_chain_context_matches adopted adopted_minimum schedule captured_minimum.

Theorem genesis_bound_context_exact : forall adopted adopted_minimum genesis_minimum schedule captured_minimum,
  genesis_bound_context_matches adopted adopted_minimum genesis_minimum schedule captured_minimum = true <->
  adopted_minimum = genesis_minimum /\ captured_minimum = genesis_minimum /\
  phlo_protocol_version schedule = active_phlo_version adopted /\
  phlo_shard schedule = native_phlo_shard adopted.
Proof.
  intros. unfold genesis_bound_context_matches.
  rewrite andb_true_iff, Nat.eqb_eq, captured_chain_context_exact.
  split; intros [same rest]; subst; tauto.
Qed.

Theorem genesis_bound_offer_respects_authoritative_minimum :
  forall adopted adopted_minimum genesis_minimum captured_environment captured_minimum machine_max offer terms schedule bound,
  genesis_bound_context_matches adopted adopted_minimum genesis_minimum schedule captured_minimum = true ->
  admit_offered_phlo_controls captured_environment captured_minimum machine_max offer terms schedule bound = true ->
  genesis_minimum <= offered_phlo_price offer /\
  Forall (fun ceiling => offered_phlo_price offer <= ceiling) (signed_required_ceilings terms).
Proof.
  intros adopted adopted_minimum genesis_minimum captured_environment captured_minimum machine_max offer terms schedule bound matched admitted.
  apply genesis_bound_context_exact in matched. destruct matched as [_ [same _]]. subst captured_minimum.
  now apply offered_price_respects_chain_and_all_owners in admitted.
Qed.

Theorem mixed_genesis_minimum_rejected : forall adopted adopted_minimum genesis_minimum schedule captured_minimum,
  adopted_minimum <> genesis_minimum ->
  genesis_bound_context_matches adopted adopted_minimum genesis_minimum schedule captured_minimum = false.
Proof.
  intros. unfold genesis_bound_context_matches.
  apply Nat.eqb_neq in H. rewrite H. reflexivity.
Qed.

Print Assumptions genesis_bound_context_exact.
Print Assumptions genesis_bound_offer_respects_authoritative_minimum.
Print Assumptions mixed_genesis_minimum_rejected.
