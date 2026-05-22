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
  (parent_bonds : BondMap)
  (sd : SlashDeploy)
  (evidence : list SlashEvidence)
  : bool :=
  match evidence_lookup evidence (sd_target_hash sd) with
  | Some (offender, evidence_epoch) =>
      Nat.eqb evidence_epoch current_epoch
      && Nat.eqb (sd_target_epoch sd) current_epoch
      && Nat.ltb 0 (bm_lookup parent_bonds offender)
  | None => false
  end.

Theorem unknown_evidence_not_authorized :
  forall current_epoch parent_bonds sd evidence,
    evidence_lookup evidence (sd_target_hash sd) = None ->
    authorized_slash_candidate current_epoch parent_bonds sd evidence = false.
Proof.
  intros. unfold authorized_slash_candidate. rewrite H. reflexivity.
Qed.

Theorem stale_evidence_not_authorized_candidate :
  forall current_epoch parent_bonds sd evidence offender old_epoch,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, old_epoch) ->
    old_epoch <> current_epoch ->
    authorized_slash_candidate current_epoch parent_bonds sd evidence = false.
Proof.
  intros. unfold authorized_slash_candidate. rewrite H.
  apply Nat.eqb_neq in H0. rewrite H0. reflexivity.
Qed.

Theorem zero_parent_bond_not_authorized_candidate :
  forall current_epoch parent_bonds sd evidence offender evidence_epoch,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, evidence_epoch) ->
    bm_lookup parent_bonds offender = 0 ->
    authorized_slash_candidate current_epoch parent_bonds sd evidence = false.
Proof.
  intros. unfold authorized_slash_candidate. rewrite H. rewrite H0.
  repeat rewrite Bool.andb_false_r. reflexivity.
Qed.

Theorem positive_parent_bond_authorizes_matching_candidate :
  forall current_epoch parent_bonds sd evidence offender,
    evidence_lookup evidence (sd_target_hash sd) = Some (offender, current_epoch) ->
    sd_target_epoch sd = current_epoch ->
    bm_lookup parent_bonds offender > 0 ->
    authorized_slash_candidate current_epoch parent_bonds sd evidence = true.
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
