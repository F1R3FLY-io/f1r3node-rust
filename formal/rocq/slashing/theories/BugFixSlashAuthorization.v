From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block ValidatorLifetime PoSContract SlashDeploy.
Import ListNotations.

Set Implicit Arguments.

Record SlashEvidence : Type := mkSlashEvidence {
  se_hash : BlockHash;
  se_lifetime : ValidatorLifetimeId;
  se_activation_epoch : Epoch
}.

Definition EvidenceIdentity :=
  (Validator * (BondGeneration * Epoch))%type.

Definition evidence_lookup
  (evidence : list SlashEvidence) (hash : BlockHash)
  : option EvidenceIdentity :=
  match find (fun item => Nat.eqb (se_hash item) hash) evidence with
  | Some item =>
      Some (
        vl_validator (se_lifetime item),
        (vl_generation (se_lifetime item), se_activation_epoch item))
  | None => None
  end.

Definition authorized_slash_candidate
  (current_epoch : Epoch)
  (canonical_bonds : BondMap)
  (canonical_generations : GenerationMap)
  (deploy : SlashDeploy)
  (evidence : list SlashEvidence)
  : bool :=
  match evidence_lookup evidence (sd_target_hash deploy) with
  | Some (offender, (evidence_generation, evidence_epoch)) =>
      Nat.eqb evidence_epoch current_epoch
      && Nat.eqb (sd_target_activation_epoch deploy) current_epoch
      && Nat.eqb evidence_generation (sd_target_bond_generation deploy)
      && match gm_lookup canonical_generations offender with
         | Some canonical_generation =>
             Nat.eqb canonical_generation (sd_target_bond_generation deploy)
             && Nat.ltb 0 (bm_lookup canonical_bonds offender)
         | None => false
         end
  | None => false
  end.

Definition authorized_slash_candidate_with_ambient
  (current_epoch : Epoch)
  (ambient_bonds : BondMap)
  (ambient_generations : GenerationMap)
  (canonical_bonds : BondMap)
  (canonical_generations : GenerationMap)
  (deploy : SlashDeploy)
  (evidence : list SlashEvidence)
  : bool :=
  authorized_slash_candidate
    current_epoch canonical_bonds canonical_generations deploy evidence.

Theorem unknown_evidence_not_authorized :
  forall current_epoch canonical_bonds canonical_generations deploy evidence,
    evidence_lookup evidence (sd_target_hash deploy) = None ->
    authorized_slash_candidate
      current_epoch canonical_bonds canonical_generations deploy evidence = false.
Proof.
  intros. unfold authorized_slash_candidate. rewrite H. reflexivity.
Qed.

Theorem ambient_authority_does_not_affect_authorization :
  forall current_epoch ambient_bonds ambient_generations
         canonical_bonds canonical_generations deploy evidence,
    authorized_slash_candidate_with_ambient
      current_epoch ambient_bonds ambient_generations
      canonical_bonds canonical_generations deploy evidence =
    authorized_slash_candidate
      current_epoch canonical_bonds canonical_generations deploy evidence.
Proof.
  intros. reflexivity.
Qed.

Inductive SlashAuthorizationOrigin : Type :=
| OriginProposer
| OriginReceiver.

Definition authorized_slash_candidate_for_origin
  (origin : SlashAuthorizationOrigin)
  (current_epoch : Epoch)
  (ambient_bonds : BondMap)
  (ambient_generations : GenerationMap)
  (canonical_bonds : BondMap)
  (canonical_generations : GenerationMap)
  (deploy : SlashDeploy)
  (evidence : list SlashEvidence)
  : bool :=
  match origin with
  | OriginProposer =>
      authorized_slash_candidate
        current_epoch canonical_bonds canonical_generations deploy evidence
  | OriginReceiver =>
      authorized_slash_candidate
        current_epoch canonical_bonds canonical_generations deploy evidence
  end.

Theorem proposer_receiver_authorization_parity :
  forall current_epoch proposer_ambient_bonds receiver_ambient_bonds
         proposer_ambient_generations receiver_ambient_generations
         canonical_bonds canonical_generations deploy evidence,
    authorized_slash_candidate_for_origin
      OriginProposer current_epoch proposer_ambient_bonds
      proposer_ambient_generations canonical_bonds canonical_generations
      deploy evidence =
    authorized_slash_candidate_for_origin
      OriginReceiver current_epoch receiver_ambient_bonds
      receiver_ambient_generations canonical_bonds canonical_generations
      deploy evidence.
Proof.
  intros. reflexivity.
Qed.

Definition StateRoot := nat.
Definition BondState := StateRoot -> BondMap.
Definition GenerationState := StateRoot -> GenerationMap.

Definition authorized_slash_candidate_at_root
  (origin : SlashAuthorizationOrigin)
  (current_epoch : Epoch)
  (ambient_bonds : BondMap)
  (ambient_generations : GenerationMap)
  (bond_state : BondState)
  (generation_state : GenerationState)
  (pre_state_root : StateRoot)
  (deploy : SlashDeploy)
  (evidence : list SlashEvidence)
  : bool :=
  authorized_slash_candidate_for_origin
    origin current_epoch ambient_bonds ambient_generations
    (bond_state pre_state_root) (generation_state pre_state_root)
    deploy evidence.

Theorem same_pre_state_root_same_authorization :
  forall current_epoch proposer_ambient_bonds receiver_ambient_bonds
         proposer_ambient_generations receiver_ambient_generations
         bond_state generation_state proposer_root receiver_root deploy evidence,
    proposer_root = receiver_root ->
    authorized_slash_candidate_at_root
      OriginProposer current_epoch proposer_ambient_bonds
      proposer_ambient_generations bond_state generation_state
      proposer_root deploy evidence =
    authorized_slash_candidate_at_root
      OriginReceiver current_epoch receiver_ambient_bonds
      receiver_ambient_generations bond_state generation_state
      receiver_root deploy evidence.
Proof.
  intros. subst. reflexivity.
Qed.

Theorem stale_activation_epoch_not_authorized_candidate :
  forall current_epoch canonical_bonds canonical_generations deploy evidence
         offender evidence_generation old_epoch,
    evidence_lookup evidence (sd_target_hash deploy) =
      Some (offender, (evidence_generation, old_epoch)) ->
    old_epoch <> current_epoch ->
    authorized_slash_candidate
      current_epoch canonical_bonds canonical_generations deploy evidence = false.
Proof.
  intros. unfold authorized_slash_candidate. rewrite H.
  apply Nat.eqb_neq in H0. rewrite H0. reflexivity.
Qed.

Theorem stale_generation_not_authorized_candidate :
  forall current_epoch canonical_bonds canonical_generations deploy evidence
         offender evidence_generation,
    evidence_lookup evidence (sd_target_hash deploy) =
      Some (offender, (evidence_generation, current_epoch)) ->
    evidence_generation <> sd_target_bond_generation deploy ->
    authorized_slash_candidate
      current_epoch canonical_bonds canonical_generations deploy evidence = false.
Proof.
  intros. unfold authorized_slash_candidate. rewrite H.
  rewrite Nat.eqb_refl. simpl.
  apply Nat.eqb_neq in H0. rewrite H0.
  repeat rewrite Bool.andb_false_r. reflexivity.
Qed.

Theorem canonical_generation_mismatch_not_authorized_candidate :
  forall current_epoch canonical_bonds canonical_generations deploy evidence
         offender evidence_generation canonical_generation,
    evidence_lookup evidence (sd_target_hash deploy) =
      Some (offender, (evidence_generation, current_epoch)) ->
    sd_target_activation_epoch deploy = current_epoch ->
    sd_target_bond_generation deploy = evidence_generation ->
    gm_lookup canonical_generations offender = Some canonical_generation ->
    canonical_generation <> evidence_generation ->
    authorized_slash_candidate
      current_epoch canonical_bonds canonical_generations deploy evidence = false.
Proof.
  intros current_epoch canonical_bonds canonical_generations deploy evidence
         offender evidence_generation canonical_generation
         Hlookup Hepoch Htarget Hcanonical Hstale.
  unfold authorized_slash_candidate. rewrite Hlookup, Hepoch, Htarget.
  repeat rewrite Nat.eqb_refl. simpl. rewrite Hcanonical.
  apply Nat.eqb_neq in Hstale. rewrite Hstale. reflexivity.
Qed.

Theorem zero_canonical_bond_not_authorized_candidate :
  forall current_epoch canonical_bonds canonical_generations deploy evidence
         offender evidence_generation evidence_epoch,
    evidence_lookup evidence (sd_target_hash deploy) =
      Some (offender, (evidence_generation, evidence_epoch)) ->
    bm_lookup canonical_bonds offender = 0 ->
    authorized_slash_candidate
      current_epoch canonical_bonds canonical_generations deploy evidence = false.
Proof.
  intros. unfold authorized_slash_candidate. rewrite H, H0.
  destruct (gm_lookup canonical_generations offender); simpl;
    repeat rewrite Bool.andb_false_r; reflexivity.
Qed.

Theorem matching_generation_current_window_positive_bond_authorized :
  forall current_epoch canonical_bonds canonical_generations deploy evidence
         offender generation,
    evidence_lookup evidence (sd_target_hash deploy) =
      Some (offender, (generation, current_epoch)) ->
    sd_target_activation_epoch deploy = current_epoch ->
    sd_target_bond_generation deploy = generation ->
    gm_lookup canonical_generations offender = Some generation ->
    bm_lookup canonical_bonds offender > 0 ->
    authorized_slash_candidate
      current_epoch canonical_bonds canonical_generations deploy evidence = true.
Proof.
  intros. unfold authorized_slash_candidate.
  rewrite H, H0, H1, H2.
  repeat rewrite Nat.eqb_refl. simpl.
  apply Nat.ltb_lt. assumption.
Qed.

Theorem canonical_pre_state_authorizes_when_ambient_differs :
  forall current_epoch ambient_bonds ambient_generations
         canonical_bonds canonical_generations deploy evidence
         offender generation,
    evidence_lookup evidence (sd_target_hash deploy) =
      Some (offender, (generation, current_epoch)) ->
    sd_target_activation_epoch deploy = current_epoch ->
    sd_target_bond_generation deploy = generation ->
    gm_lookup canonical_generations offender = Some generation ->
    bm_lookup ambient_bonds offender = 0 ->
    gm_lookup ambient_generations offender <> Some generation ->
    bm_lookup canonical_bonds offender > 0 ->
    authorized_slash_candidate_with_ambient
      current_epoch ambient_bonds ambient_generations
      canonical_bonds canonical_generations deploy evidence = true.
Proof.
  intros. unfold authorized_slash_candidate_with_ambient.
  apply matching_generation_current_window_positive_bond_authorized
    with (offender := offender) (generation := generation); assumption.
Qed.

Theorem canonical_zero_rejects_even_if_ambient_positive :
  forall current_epoch ambient_bonds ambient_generations
         canonical_bonds canonical_generations deploy evidence
         offender evidence_generation evidence_epoch,
    evidence_lookup evidence (sd_target_hash deploy) =
      Some (offender, (evidence_generation, evidence_epoch)) ->
    bm_lookup ambient_bonds offender > 0 ->
    bm_lookup canonical_bonds offender = 0 ->
    authorized_slash_candidate_with_ambient
      current_epoch ambient_bonds ambient_generations
      canonical_bonds canonical_generations deploy evidence = false.
Proof.
  intros. unfold authorized_slash_candidate_with_ambient.
  apply zero_canonical_bond_not_authorized_candidate
    with (offender := offender)
         (evidence_generation := evidence_generation)
         (evidence_epoch := evidence_epoch); assumption.
Qed.

Theorem authorized_execution_zeros_offender :
  forall state deploy evidence offender generation current_epoch
         canonical_generations,
    evidence_lookup evidence (sd_target_hash deploy) =
      Some (offender, (generation, current_epoch)) ->
    sd_target_activation_epoch deploy = current_epoch ->
    sd_target_bond_generation deploy = generation ->
    gm_lookup canonical_generations offender = Some generation ->
    let (state_after, _) := execute_slash_deploy
      state deploy current_epoch canonical_generations
      (evidence_lookup evidence) in
    bm_lookup (ps_allBonds (psc_pos state_after)) offender = 0.
Proof.
  intros. apply execute_zeros_target_bond with (evidence_generation := generation);
    assumption.
Qed.

Theorem unauthorized_unknown_execution_noop :
  forall state deploy evidence current_epoch canonical_generations,
    evidence_lookup evidence (sd_target_hash deploy) = None ->
    execute_slash_deploy state deploy current_epoch canonical_generations
      (evidence_lookup evidence) = (state, false).
Proof.
  intros. apply execute_unknown_evidence_noop. assumption.
Qed.

Definition issuer_matches_sender
  (deploy : SlashDeploy) (block_sender : Validator) : bool :=
  if validator_eq_dec (sd_issuer deploy) block_sender then true else false.

Definition received_slash_deploy_authorized
  (block_sender : Validator)
  (current_epoch : Epoch)
  (canonical_bonds : BondMap)
  (canonical_generations : GenerationMap)
  (deploy : SlashDeploy)
  (evidence : list SlashEvidence)
  : bool :=
  issuer_matches_sender deploy block_sender
  && authorized_slash_candidate
       current_epoch canonical_bonds canonical_generations deploy evidence.

Definition slash_target_key
  (evidence : list SlashEvidence) (deploy : SlashDeploy)
  : option (Validator * BondGeneration) :=
  match evidence_lookup evidence (sd_target_hash deploy) with
  | Some (offender, _) =>
      Some (offender, sd_target_bond_generation deploy)
  | None => None
  end.

Definition slash_key_eqb
  (first second : Validator * BondGeneration) : bool :=
  (if validator_eq_dec (fst first) (fst second) then true else false)
  && Nat.eqb (snd first) (snd second).

Fixpoint key_mem
  (key : Validator * BondGeneration)
  (keys : list (Validator * BondGeneration)) : bool :=
  match keys with
  | [] => false
  | current :: rest => slash_key_eqb key current || key_mem key rest
  end.

Fixpoint validate_rec
  (block_sender : Validator)
  (current_epoch : Epoch)
  (canonical_bonds : BondMap)
  (canonical_generations : GenerationMap)
  (evidence : list SlashEvidence)
  (seen : list (Validator * BondGeneration))
  (deploys : list SlashDeploy) : bool :=
  match deploys with
  | [] => true
  | deploy :: rest =>
      if received_slash_deploy_authorized block_sender current_epoch
           canonical_bonds canonical_generations deploy evidence
      then match slash_target_key evidence deploy with
           | Some key =>
               if key_mem key seen
               then false
               else validate_rec block_sender current_epoch canonical_bonds
                      canonical_generations evidence (key :: seen) rest
           | None => false
           end
      else false
  end.

Definition validate_block_slash_deploys
  (block_sender : Validator)
  (current_epoch : Epoch)
  (canonical_bonds : BondMap)
  (canonical_generations : GenerationMap)
  (evidence : list SlashEvidence)
  (deploys : list SlashDeploy) : bool :=
  validate_rec block_sender current_epoch canonical_bonds
    canonical_generations evidence [] deploys.

Theorem issuer_mismatch_not_authorized :
  forall block_sender current_epoch canonical_bonds canonical_generations
         deploy evidence,
    sd_issuer deploy <> block_sender ->
    received_slash_deploy_authorized block_sender current_epoch
      canonical_bonds canonical_generations deploy evidence = false.
Proof.
  intros. unfold received_slash_deploy_authorized, issuer_matches_sender.
  destruct (validator_eq_dec (sd_issuer deploy) block_sender);
    [contradiction | reflexivity].
Qed.

Theorem issuer_match_authorized_iff_candidate :
  forall block_sender current_epoch canonical_bonds canonical_generations
         deploy evidence,
    sd_issuer deploy = block_sender ->
    received_slash_deploy_authorized block_sender current_epoch
      canonical_bonds canonical_generations deploy evidence =
    authorized_slash_candidate current_epoch canonical_bonds
      canonical_generations deploy evidence.
Proof.
  intros. unfold received_slash_deploy_authorized, issuer_matches_sender.
  destruct (validator_eq_dec (sd_issuer deploy) block_sender);
    [reflexivity | contradiction].
Qed.

Lemma slash_key_eqb_refl : forall key, slash_key_eqb key key = true.
Proof.
  intros [validator generation]. unfold slash_key_eqb. simpl.
  destruct (validator_eq_dec validator validator);
    [rewrite Nat.eqb_refl; reflexivity | contradiction].
Qed.

Theorem duplicate_target_rejected :
  forall block_sender current_epoch canonical_bonds canonical_generations
         evidence first second rest key,
    received_slash_deploy_authorized block_sender current_epoch
      canonical_bonds canonical_generations first evidence = true ->
    received_slash_deploy_authorized block_sender current_epoch
      canonical_bonds canonical_generations second evidence = true ->
    slash_target_key evidence first = Some key ->
    slash_target_key evidence second = Some key ->
    validate_block_slash_deploys block_sender current_epoch canonical_bonds
      canonical_generations evidence (first :: second :: rest) = false.
Proof.
  intros. unfold validate_block_slash_deploys.
  cbn [validate_rec key_mem]. rewrite H, H1, H0, H2.
  cbn [key_mem]. rewrite slash_key_eqb_refl. reflexivity.
Qed.

Theorem single_authorized_deploy_validates :
  forall block_sender current_epoch canonical_bonds canonical_generations
         evidence deploy key,
    received_slash_deploy_authorized block_sender current_epoch
      canonical_bonds canonical_generations deploy evidence = true ->
    slash_target_key evidence deploy = Some key ->
    validate_block_slash_deploys block_sender current_epoch canonical_bonds
      canonical_generations evidence [deploy] = true.
Proof.
  intros. unfold validate_block_slash_deploys.
  cbn [validate_rec key_mem]. rewrite H, H0. reflexivity.
Qed.

Theorem two_distinct_authorized_deploys_validate :
  forall block_sender current_epoch canonical_bonds canonical_generations
         evidence first second first_key second_key,
    received_slash_deploy_authorized block_sender current_epoch
      canonical_bonds canonical_generations first evidence = true ->
    received_slash_deploy_authorized block_sender current_epoch
      canonical_bonds canonical_generations second evidence = true ->
    slash_target_key evidence first = Some first_key ->
    slash_target_key evidence second = Some second_key ->
    slash_key_eqb second_key first_key = false ->
    validate_block_slash_deploys block_sender current_epoch canonical_bonds
      canonical_generations evidence [first; second] = true.
Proof.
  intros. unfold validate_block_slash_deploys.
  cbn [validate_rec key_mem]. rewrite H, H1, H0, H2.
  cbn [key_mem]. rewrite H3. reflexivity.
Qed.

Inductive SlashDependencyDisposition : Type :=
| SlashDependencyReady
| SlashDependencyWaiting
| SlashDependencyRejectedForLocalAbsence.

Definition slash_evidence_dependencies
  (deploys : list SlashDeploy) : list BlockHash :=
  nodup hash_eq_dec (map sd_target_hash deploys).

Definition receive_slash_dependency
  (available : list BlockHash)
  (deploys : list SlashDeploy)
  (deploy : SlashDeploy) : SlashDependencyDisposition :=
  if in_dec hash_eq_dec (sd_target_hash deploy) available
  then SlashDependencyReady
  else if in_dec hash_eq_dec
         (sd_target_hash deploy) (slash_evidence_dependencies deploys)
       then SlashDependencyWaiting
       else SlashDependencyRejectedForLocalAbsence.

Theorem every_slash_target_is_a_dependency :
  forall deploys deploy,
    In deploy deploys ->
    In (sd_target_hash deploy) (slash_evidence_dependencies deploys).
Proof.
  intros. unfold slash_evidence_dependencies.
  apply nodup_In. apply in_map. assumption.
Qed.

Theorem unavailable_declared_slash_waits_for_evidence :
  forall available deploys deploy,
    In deploy deploys ->
    ~ In (sd_target_hash deploy) available ->
    receive_slash_dependency available deploys deploy = SlashDependencyWaiting.
Proof.
  intros. unfold receive_slash_dependency.
  destruct (in_dec hash_eq_dec (sd_target_hash deploy) available)
    as [Havailable | Hnot_available];
    [contradiction |].
  destruct (in_dec hash_eq_dec
    (sd_target_hash deploy) (slash_evidence_dependencies deploys))
    as [Hdependency | Hnot_dependency];
    [reflexivity |].
  exfalso. apply Hnot_dependency.
  apply every_slash_target_is_a_dependency. assumption.
Qed.

Theorem unavailable_declared_slash_not_rejected_as_unauthorized :
  forall available deploys deploy,
    In deploy deploys ->
    ~ In (sd_target_hash deploy) available ->
    receive_slash_dependency available deploys deploy <>
      SlashDependencyRejectedForLocalAbsence.
Proof.
  intros. rewrite unavailable_declared_slash_waits_for_evidence by assumption.
  discriminate.
Qed.

Definition receive_slash_dependency_with_tracker
  (available tracker_witnesses : list BlockHash)
  (deploys : list SlashDeploy)
  (deploy : SlashDeploy) : SlashDependencyDisposition :=
  receive_slash_dependency available deploys deploy.

Theorem tracker_witness_does_not_satisfy_slash_evidence_dependency :
  forall available tracker_witnesses deploys deploy,
    In deploy deploys ->
    In (sd_target_hash deploy) tracker_witnesses ->
    ~ In (sd_target_hash deploy) available ->
    receive_slash_dependency_with_tracker
      available tracker_witnesses deploys deploy = SlashDependencyWaiting.
Proof.
  intros. unfold receive_slash_dependency_with_tracker.
  apply unavailable_declared_slash_waits_for_evidence; assumption.
Qed.

Definition block_is_processed
  (dag buffered : list BlockHash)
  (hash : BlockHash) : bool :=
  if in_dec hash_eq_dec hash dag
  then true
  else if in_dec hash_eq_dec hash buffered then true else false.

Definition block_is_processed_with_tracker
  (dag buffered tracker_witnesses : list BlockHash)
  (hash : BlockHash) : bool :=
  block_is_processed dag buffered hash.

Theorem tracker_witness_does_not_mark_block_processed :
  forall dag buffered tracker_witnesses hash,
    In hash tracker_witnesses ->
    ~ In hash dag ->
    ~ In hash buffered ->
    block_is_processed_with_tracker
      dag buffered tracker_witnesses hash = false.
Proof.
  intros dag buffered tracker_witnesses hash _ Hdag Hbuffered.
  unfold block_is_processed_with_tracker, block_is_processed.
  destruct (in_dec hash_eq_dec hash dag); [contradiction |].
  destruct (in_dec hash_eq_dec hash buffered); [contradiction | reflexivity].
Qed.
