From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block ValidatorLifetime PoSContract SlashDeploy.
Import ListNotations.

Set Implicit Arguments.

Record SlashEvidence : Type := mkSlashEvidence {
  se_hash : BlockHash;
  se_lifetime : ValidatorLifetimeId
}.

Definition evidence_lookup
  (evidence : list SlashEvidence) (h : BlockHash)
  : option (Validator * Epoch) :=
  match find (fun e => Nat.eqb (se_hash e) h) evidence with
  | Some e => Some (vl_validator (se_lifetime e), vl_epoch (se_lifetime e))
  | None => None
  end.

Definition authorized_slash_candidate
  (current_epoch : Epoch)
  (canonical_bonds : BondMap)
  (sd : SlashDeploy)
  (evidence : list SlashEvidence)
  : bool :=
  match evidence_lookup evidence (sd_target_hash sd) with
  | Some (offender, evidence_epoch) =>
      Nat.eqb evidence_epoch current_epoch
      && Nat.eqb (sd_target_epoch sd) current_epoch
      && Nat.ltb 0 (bm_lookup canonical_bonds offender)
  | None => false
  end.

Definition authorized_slash_candidate_with_ambient
  (current_epoch : Epoch)
  (ambient_bonds canonical_bonds : BondMap)
  (sd : SlashDeploy)
  (evidence : list SlashEvidence)
  : bool :=
  authorized_slash_candidate current_epoch canonical_bonds sd evidence.

Theorem unknown_evidence_not_authorized :
  forall current_epoch canonical_bonds sd evidence,
    evidence_lookup evidence (sd_target_hash sd) = None ->
    authorized_slash_candidate current_epoch canonical_bonds sd evidence = false.
Proof.
  intros. unfold authorized_slash_candidate. rewrite H. reflexivity.
Qed.

Theorem ambient_bonds_do_not_affect_authorization :
  forall current_epoch ambient_bonds canonical_bonds sd evidence,
    authorized_slash_candidate_with_ambient
      current_epoch ambient_bonds canonical_bonds sd evidence =
    authorized_slash_candidate current_epoch canonical_bonds sd evidence.
Proof.
  intros. reflexivity.
Qed.

Inductive SlashAuthorizationOrigin : Type :=
| OriginProposer
| OriginReceiver.

Definition authorized_slash_candidate_for_origin
  (origin : SlashAuthorizationOrigin)
  (current_epoch : Epoch)
  (ambient_bonds canonical_bonds : BondMap)
  (sd : SlashDeploy)
  (evidence : list SlashEvidence)
  : bool :=
  match origin with
  | OriginProposer => authorized_slash_candidate current_epoch canonical_bonds sd evidence
  | OriginReceiver => authorized_slash_candidate current_epoch canonical_bonds sd evidence
  end.

Theorem proposer_receiver_authorization_parity :
  forall current_epoch proposer_ambient receiver_ambient canonical_bonds sd evidence,
    authorized_slash_candidate_for_origin
      OriginProposer current_epoch proposer_ambient canonical_bonds sd evidence =
    authorized_slash_candidate_for_origin
      OriginReceiver current_epoch receiver_ambient canonical_bonds sd evidence.
Proof.
  intros. reflexivity.
Qed.

Definition StateRoot := nat.
Definition BondState := StateRoot -> BondMap.

Definition authorized_slash_candidate_at_root
  (origin : SlashAuthorizationOrigin)
  (current_epoch : Epoch)
  (ambient_bonds : BondMap)
  (bond_state : BondState)
  (pre_state_root : StateRoot)
  (sd : SlashDeploy)
  (evidence : list SlashEvidence)
  : bool :=
  authorized_slash_candidate_for_origin
    origin current_epoch ambient_bonds (bond_state pre_state_root) sd evidence.

Theorem same_pre_state_root_same_authorization :
  forall current_epoch proposer_ambient receiver_ambient bond_state
         proposer_root receiver_root sd evidence,
    proposer_root = receiver_root ->
    authorized_slash_candidate_at_root
      OriginProposer current_epoch proposer_ambient bond_state proposer_root sd evidence =
    authorized_slash_candidate_at_root
      OriginReceiver current_epoch receiver_ambient bond_state receiver_root sd evidence.
Proof.
  intros. subst. reflexivity.
Qed.

Theorem canonical_pre_state_authorizes_when_ambient_zero :
  forall current_epoch ambient_bonds canonical_bonds sd evidence offender,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, current_epoch) ->
    sd_target_epoch sd = current_epoch ->
    bm_lookup ambient_bonds offender = 0 ->
    bm_lookup canonical_bonds offender > 0 ->
    authorized_slash_candidate_with_ambient
      current_epoch ambient_bonds canonical_bonds sd evidence = true.
Proof.
  intros current_epoch ambient_bonds canonical_bonds sd evidence offender Hlookup Htarget _ Hbond.
  unfold authorized_slash_candidate_with_ambient, authorized_slash_candidate.
  rewrite Hlookup. rewrite Htarget.
  repeat rewrite Nat.eqb_refl. simpl.
  apply Nat.ltb_lt. assumption.
Qed.

Theorem canonical_zero_rejects_even_if_ambient_positive :
  forall current_epoch ambient_bonds canonical_bonds sd evidence offender evidence_epoch,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, evidence_epoch) ->
    bm_lookup ambient_bonds offender > 0 ->
    bm_lookup canonical_bonds offender = 0 ->
    authorized_slash_candidate_with_ambient
      current_epoch ambient_bonds canonical_bonds sd evidence = false.
Proof.
  intros current_epoch ambient_bonds canonical_bonds sd evidence offender evidence_epoch Hlookup _ Hbond.
  unfold authorized_slash_candidate_with_ambient, authorized_slash_candidate.
  rewrite Hlookup. rewrite Hbond.
  repeat rewrite Bool.andb_false_r. reflexivity.
Qed.

Theorem stale_evidence_not_authorized_candidate :
  forall current_epoch canonical_bonds sd evidence offender old_epoch,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, old_epoch) ->
    old_epoch <> current_epoch ->
    authorized_slash_candidate current_epoch canonical_bonds sd evidence = false.
Proof.
  intros. unfold authorized_slash_candidate. rewrite H.
  apply Nat.eqb_neq in H0. rewrite H0. reflexivity.
Qed.

Theorem zero_canonical_bond_not_authorized_candidate :
  forall current_epoch canonical_bonds sd evidence offender evidence_epoch,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, evidence_epoch) ->
    bm_lookup canonical_bonds offender = 0 ->
    authorized_slash_candidate current_epoch canonical_bonds sd evidence = false.
Proof.
  intros. unfold authorized_slash_candidate. rewrite H. rewrite H0.
  repeat rewrite Bool.andb_false_r. reflexivity.
Qed.

Theorem positive_canonical_bond_authorizes_matching_candidate :
  forall current_epoch canonical_bonds sd evidence offender,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, current_epoch) ->
    sd_target_epoch sd = current_epoch ->
    bm_lookup canonical_bonds offender > 0 ->
    authorized_slash_candidate current_epoch canonical_bonds sd evidence = true.
Proof.
  intros. unfold authorized_slash_candidate. rewrite H. rewrite H0.
  repeat rewrite Nat.eqb_refl. simpl.
  apply Nat.ltb_lt. assumption.
Qed.

Theorem authorized_execution_zeros_offender :
  forall ps sd evidence offender current_epoch,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, current_epoch) ->
    sd_target_epoch sd = current_epoch ->
    let (ps', _) := execute_slash_deploy ps sd current_epoch (evidence_lookup evidence) in
    bm_lookup (ps_allBonds ps') offender = 0.
Proof.
  intros.
  apply execute_zeros_target_bond; assumption.
Qed.

Theorem unauthorized_unknown_execution_noop :
  forall ps sd evidence current_epoch,
    evidence_lookup evidence (sd_target_hash sd) = None ->
    execute_slash_deploy ps sd current_epoch (evidence_lookup evidence) = (ps, false).
Proof.
  intros. apply execute_unknown_evidence_noop. assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — The full §9.8 seven-rule receive gate (T-9.13)
   ═══════════════════════════════════════════════════════════════════════════

   `authorized_slash_candidate` above models the CORE per-deploy predicate
   (the three conjuncts current-epoch ∧ evidence-epoch ∧ positive-bond; rules
   3+4 are folded into `evidence_lookup = Some`, since evidence exists only for
   invalid blocks). The Rust receive gate
   `validate_received_slash_deploys` (slashing_authorization.rs:342-508)
   enforces FOUR more things around that core:

     Rule 1  the deploy issuer equals the block sender          (:416-432)
     Rule 7  no two slashes in one block share (offender,epoch) (:497-504)

   This section adds those, giving a faithful 7-rule model:
     received_slash_deploy_authorized = (Rule 1) ∧ authorized_slash_candidate
     validate_block_slash_deploys     = ∀ deploys authorized ∧ NoDup keys. *)

(* Rule 1: issuer must equal the block sender. *)
Definition issuer_matches_sender (sd : SlashDeploy) (block_sender : Validator) : bool :=
  if validator_eq_dec (sd_issuer sd) block_sender then true else false.

Definition received_slash_deploy_authorized
  (block_sender : Validator)
  (current_epoch : Epoch)
  (canonical_bonds : BondMap)
  (sd : SlashDeploy)
  (evidence : list SlashEvidence)
  : bool :=
  issuer_matches_sender sd block_sender
  && authorized_slash_candidate current_epoch canonical_bonds sd evidence.

(* Rule 7 machinery: the (offender, target_epoch) uniqueness key. *)
Definition slash_target_key
  (evidence : list SlashEvidence) (sd : SlashDeploy) : option (Validator * Epoch) :=
  match evidence_lookup evidence (sd_target_hash sd) with
  | Some (offender, _) => Some (offender, sd_target_epoch sd)
  | None => None
  end.

Definition slash_key_eqb (k1 k2 : Validator * Epoch) : bool :=
  (if validator_eq_dec (fst k1) (fst k2) then true else false)
  && Nat.eqb (snd k1) (snd k2).

Fixpoint key_mem (k : Validator * Epoch) (ks : list (Validator * Epoch)) : bool :=
  match ks with
  | [] => false
  | k' :: rest => slash_key_eqb k k' || key_mem k rest
  end.

(* Block-level receive gate: every slash deploy authorized AND no repeated
   (offender, target_epoch) key. `seen` accumulates the keys already admitted,
   mirroring the Rust `seen: BTreeMap<(Validator,Epoch), _>`. *)
Fixpoint validate_rec
  (block_sender : Validator) (current_epoch : Epoch) (canonical_bonds : BondMap)
  (evidence : list SlashEvidence) (seen : list (Validator * Epoch))
  (deploys : list SlashDeploy) : bool :=
  match deploys with
  | [] => true
  | sd :: rest =>
      if received_slash_deploy_authorized block_sender current_epoch canonical_bonds sd evidence
      then match slash_target_key evidence sd with
           | Some k =>
               if key_mem k seen
               then false
               else validate_rec block_sender current_epoch canonical_bonds
                                 evidence (k :: seen) rest
           | None => false
           end
      else false
  end.

Definition validate_block_slash_deploys
  (block_sender : Validator) (current_epoch : Epoch) (canonical_bonds : BondMap)
  (evidence : list SlashEvidence) (deploys : list SlashDeploy) : bool :=
  validate_rec block_sender current_epoch canonical_bonds evidence [] deploys.

(* Rule 1 — issuer ≠ sender is rejected (the per-deploy predicate is false). *)
Theorem issuer_mismatch_not_authorized :
  forall block_sender current_epoch canonical_bonds sd evidence,
    sd_issuer sd <> block_sender ->
    received_slash_deploy_authorized block_sender current_epoch canonical_bonds sd evidence = false.
Proof.
  intros block_sender current_epoch canonical_bonds sd evidence Hne.
  unfold received_slash_deploy_authorized, issuer_matches_sender.
  destruct (validator_eq_dec (sd_issuer sd) block_sender) as [Heq | _].
  - contradiction.
  - reflexivity.
Qed.

(* When the issuer matches, the per-deploy verdict is exactly the core
   `authorized_slash_candidate` (so the existing T-9.13 core theorems apply). *)
Theorem issuer_match_authorized_iff_candidate :
  forall block_sender current_epoch canonical_bonds sd evidence,
    sd_issuer sd = block_sender ->
    received_slash_deploy_authorized block_sender current_epoch canonical_bonds sd evidence
    = authorized_slash_candidate current_epoch canonical_bonds sd evidence.
Proof.
  intros block_sender current_epoch canonical_bonds sd evidence Heq.
  unfold received_slash_deploy_authorized, issuer_matches_sender.
  destruct (validator_eq_dec (sd_issuer sd) block_sender) as [_ | Hne].
  - reflexivity.
  - contradiction.
Qed.

Lemma slash_key_eqb_refl : forall k, slash_key_eqb k k = true.
Proof.
  intros [kv ke]. unfold slash_key_eqb. simpl.
  destruct (validator_eq_dec kv kv) as [_ | C]; [| contradiction].
  rewrite Nat.eqb_refl. reflexivity.
Qed.

(* Rule 7 — two slash deploys sharing an (offender, target_epoch) key are
   rejected at the block level, even when both pass the per-deploy gate. *)
Theorem duplicate_target_rejected :
  forall block_sender current_epoch canonical_bonds evidence sd1 sd2 rest k,
    received_slash_deploy_authorized block_sender current_epoch canonical_bonds sd1 evidence = true ->
    received_slash_deploy_authorized block_sender current_epoch canonical_bonds sd2 evidence = true ->
    slash_target_key evidence sd1 = Some k ->
    slash_target_key evidence sd2 = Some k ->
    validate_block_slash_deploys block_sender current_epoch canonical_bonds evidence
      (sd1 :: sd2 :: rest) = false.
Proof.
  intros block_sender current_epoch canonical_bonds evidence sd1 sd2 rest k
         Hauth1 Hauth2 Hk1 Hk2.
  unfold validate_block_slash_deploys. cbn [validate_rec key_mem].
  rewrite Hauth1, Hk1, Hauth2, Hk2. cbn [key_mem].
  rewrite slash_key_eqb_refl. reflexivity.
Qed.

(* A single authorized deploy passes the block-level gate. *)
Theorem single_authorized_deploy_validates :
  forall block_sender current_epoch canonical_bonds evidence sd k,
    received_slash_deploy_authorized block_sender current_epoch canonical_bonds sd evidence = true ->
    slash_target_key evidence sd = Some k ->
    validate_block_slash_deploys block_sender current_epoch canonical_bonds evidence [sd] = true.
Proof.
  intros block_sender current_epoch canonical_bonds evidence sd k Hauth Hk.
  unfold validate_block_slash_deploys. cbn [validate_rec key_mem].
  rewrite Hauth, Hk. reflexivity.
Qed.

(* Two authorized deploys with DISTINCT keys both pass. *)
Theorem two_distinct_authorized_deploys_validate :
  forall block_sender current_epoch canonical_bonds evidence sd1 sd2 k1 k2,
    received_slash_deploy_authorized block_sender current_epoch canonical_bonds sd1 evidence = true ->
    received_slash_deploy_authorized block_sender current_epoch canonical_bonds sd2 evidence = true ->
    slash_target_key evidence sd1 = Some k1 ->
    slash_target_key evidence sd2 = Some k2 ->
    slash_key_eqb k2 k1 = false ->
    validate_block_slash_deploys block_sender current_epoch canonical_bonds evidence
      [sd1; sd2] = true.
Proof.
  intros block_sender current_epoch canonical_bonds evidence sd1 sd2 k1 k2
         Hauth1 Hauth2 Hk1 Hk2 Hdiff.
  unfold validate_block_slash_deploys. cbn [validate_rec key_mem].
  rewrite Hauth1, Hk1, Hauth2, Hk2. cbn [key_mem].
  rewrite Hdiff. reflexivity.
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
  (sd : SlashDeploy) : SlashDependencyDisposition :=
  if in_dec hash_eq_dec (sd_target_hash sd) available
  then SlashDependencyReady
  else if in_dec hash_eq_dec
         (sd_target_hash sd) (slash_evidence_dependencies deploys)
       then SlashDependencyWaiting
       else SlashDependencyRejectedForLocalAbsence.

Theorem every_slash_target_is_a_dependency :
  forall deploys sd,
    In sd deploys ->
    In (sd_target_hash sd) (slash_evidence_dependencies deploys).
Proof.
  intros deploys sd Hin.
  unfold slash_evidence_dependencies.
  apply nodup_In.
  apply in_map.
  exact Hin.
Qed.

Theorem unavailable_declared_slash_waits_for_evidence :
  forall available deploys sd,
    In sd deploys ->
    ~ In (sd_target_hash sd) available ->
    receive_slash_dependency available deploys sd = SlashDependencyWaiting.
Proof.
  intros available deploys sd Hin Hmissing.
  unfold receive_slash_dependency.
  destruct (in_dec hash_eq_dec (sd_target_hash sd) available) as [Havailable | _].
  - contradiction.
  - destruct (in_dec hash_eq_dec
      (sd_target_hash sd) (slash_evidence_dependencies deploys)) as [_ | Habsent].
    + reflexivity.
    + exfalso. apply Habsent. apply every_slash_target_is_a_dependency. exact Hin.
Qed.

Theorem unavailable_declared_slash_not_rejected_as_unauthorized :
  forall available deploys sd,
    In sd deploys ->
    ~ In (sd_target_hash sd) available ->
    receive_slash_dependency available deploys sd <>
      SlashDependencyRejectedForLocalAbsence.
Proof.
  intros available deploys sd Hin Hmissing.
  rewrite unavailable_declared_slash_waits_for_evidence by assumption.
  discriminate.
Qed.

Definition receive_slash_dependency_with_tracker
  (available tracker_witnesses : list BlockHash)
  (deploys : list SlashDeploy)
  (sd : SlashDeploy) : SlashDependencyDisposition :=
  receive_slash_dependency available deploys sd.

Theorem tracker_witness_does_not_satisfy_slash_evidence_dependency :
  forall available tracker_witnesses deploys sd,
    In sd deploys ->
    In (sd_target_hash sd) tracker_witnesses ->
    ~ In (sd_target_hash sd) available ->
    receive_slash_dependency_with_tracker
      available tracker_witnesses deploys sd = SlashDependencyWaiting.
Proof.
  intros available tracker_witnesses deploys sd Hin _ Hmissing.
  unfold receive_slash_dependency_with_tracker.
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
  destruct (in_dec hash_eq_dec hash dag) as [Hin | _].
  - contradiction.
  - destruct (in_dec hash_eq_dec hash buffered) as [Hin | _].
    + contradiction.
    + reflexivity.
Qed.
