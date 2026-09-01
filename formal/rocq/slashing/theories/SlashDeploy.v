(* ═══════════════════════════════════════════════════════════════════════════
   SlashDeploy.v — System deploy invoking the PoS slash transition

   Models the SlashDeploy system deploy (`SystemDeployEnum::Slash`) which
   is the bridge from the orchestration layer (BlockCreator) to the
   on-chain effect (PoSContract).

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition         │ Rust Implementation                              │
   ────────────────────────┼──────────────────────────────────────────────────┤
   SlashDeploy             │ SystemDeployEnum::Slash                          │
   sd_target               │ invalid_block_hash → looked up in invalidBlocks   │
   sd_proposer             │ validator_identity.public_key                    │
   sd_issuer               │ SystemDeployData::Slash.issuer_public_key         │
   sd_seed                 │ generate_slash_deploy_random_seed(self, seqNum, invalid_block_hash) │
   ─────────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §3.7.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Slashing Require Import
  Validator ValidatorLifetime BondGenerationLifecycle Block InvalidBlock PoSContract.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — SlashDeploy record
   ═══════════════════════════════════════════════════════════════════════════ *)

Record SlashDeploy : Type := mkSlashDeploy {
  sd_target_hash : BlockHash;
  sd_proposer : Validator;
  sd_target_activation_epoch : Epoch;
  sd_target_bond_generation : BondGeneration;
  sd_seed : nat;
  sd_issuer : Validator
}.

Definition sd_target_epoch := sd_target_activation_epoch.

Definition slash_seed_input
  (proposer : Validator) (seqNum : nat) (target_hash : BlockHash)
  : Validator * nat * BlockHash :=
  (proposer, seqNum, target_hash).

Theorem slash_seed_input_hash_injective :
  forall proposer seqNum h1 h2,
    slash_seed_input proposer seqNum h1 =
    slash_seed_input proposer seqNum h2 ->
    h1 = h2.
Proof.
  intros proposer seqNum h1 h2 H.
  inversion H. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Execution semantics
   ═══════════════════════════════════════════════════════════════════════════

   A SlashDeploy executes against a PoSStateC. The target validator is
   resolved via the invalidBlocks lookup function (an oracle in this
   abstraction; concretely, the on-chain `getInvalidBlocks` channel).
   The execution then defers to the PoS slash transition. *)

Definition execute_slash_deploy
  (ps : PoSStateC)
  (sd : SlashDeploy)
  (current_epoch : Epoch)
  (canonical_generations : GenerationMap)
  (invalidBlocks_lookup :
    BlockHash -> option (Validator * (BondGeneration * Epoch)))
  : PoSStateC * bool :=
  match invalidBlocks_lookup (sd_target_hash sd) with
  | Some (offender, (evidence_generation, evidence_epoch)) =>
      if Nat.eq_dec evidence_epoch current_epoch then
        if Nat.eq_dec (sd_target_activation_epoch sd) current_epoch then
          if Nat.eq_dec evidence_generation (sd_target_bond_generation sd) then
            match gm_lookup canonical_generations offender with
            | Some canonical_generation =>
                if Nat.eq_dec canonical_generation (sd_target_bond_generation sd)
                then slashC ps offender
                else (ps, false)
            | None => (ps, false)
            end
          else (ps, false)
        else (ps, false)
      else (ps, false)
  | None => (ps, false)
  end.

Definition execute_authenticated_slash_deploy
  (ps : PoSStateC)
  (sd : SlashDeploy)
  (current_epoch : Epoch)
  (canonical_generations : GenerationMap)
  (invalidBlocks_lookup :
    BlockHash -> option (Validator * (BondGeneration * Epoch)))
  (auth_ok : bool)
  : PoSStateC * bool :=
  if auth_ok
  then execute_slash_deploy
         ps sd current_epoch canonical_generations invalidBlocks_lookup
  else (ps, false).

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — Successful execution invariants
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem execute_zeros_target_bond :
  forall ps sd lookup offender evidence_generation current_epoch generations,
    lookup (sd_target_hash sd) =
      Some (offender, (evidence_generation, current_epoch)) ->
    sd_target_activation_epoch sd = current_epoch ->
    sd_target_bond_generation sd = evidence_generation ->
    gm_lookup generations offender = Some evidence_generation ->
    let (ps', _) :=
      execute_slash_deploy ps sd current_epoch generations lookup in
    bm_lookup (ps_allBonds (psc_pos ps')) offender = 0.
Proof.
  intros ps sd lookup offender evidence_generation current_epoch generations
         Hl He Hg Hcanonical.
  unfold execute_slash_deploy. rewrite Hl.
  destruct (Nat.eq_dec current_epoch current_epoch) as [_ | Hneq]; [|contradiction].
  rewrite He.
  destruct (Nat.eq_dec current_epoch current_epoch) as [_ | Hneq]; [|contradiction].
  rewrite Hg.
  destruct (Nat.eq_dec evidence_generation evidence_generation) as [_ | Hneq];
    [|contradiction].
  rewrite Hcanonical.
  destruct (Nat.eq_dec evidence_generation evidence_generation) as [_ | Hneq];
    [|contradiction].
  apply (slashC_zeros_bond ps offender).
Qed.

Theorem execute_other_unchanged :
  forall ps sd lookup offender v' evidence_generation current_epoch generations,
    lookup (sd_target_hash sd) =
      Some (offender, (evidence_generation, current_epoch)) ->
    sd_target_activation_epoch sd = current_epoch ->
    sd_target_bond_generation sd = evidence_generation ->
    gm_lookup generations offender = Some evidence_generation ->
    offender <> v' ->
    let (ps', _) :=
      execute_slash_deploy ps sd current_epoch generations lookup in
    bm_lookup (ps_allBonds (psc_pos ps')) v' =
      bm_lookup (ps_allBonds (psc_pos ps)) v'.
Proof.
  intros ps sd lookup offender v' evidence_generation current_epoch generations
         Hl He Hg Hcanonical Hne.
  unfold execute_slash_deploy. rewrite Hl.
  destruct (Nat.eq_dec current_epoch current_epoch) as [_ | Hneq]; [|contradiction].
  rewrite He.
  destruct (Nat.eq_dec current_epoch current_epoch) as [_ | Hneq]; [|contradiction].
  rewrite Hg.
  destruct (Nat.eq_dec evidence_generation evidence_generation) as [_ | Hneq];
    [|contradiction].
  rewrite Hcanonical.
  destruct (Nat.eq_dec evidence_generation evidence_generation) as [_ | Hneq];
    [|contradiction].
  pose proof (@slashC_other_unchanged ps offender v' Hne) as Hother.
  destruct (slashC ps offender). simpl in Hother |- *.
  exact (proj1 Hother).
Qed.

Theorem execute_unknown_evidence_noop :
  forall ps sd lookup current_epoch generations,
    lookup (sd_target_hash sd) = None ->
    execute_slash_deploy ps sd current_epoch generations lookup = (ps, false).
Proof.
  intros. unfold execute_slash_deploy. rewrite H. reflexivity.
Qed.

Theorem execute_stale_activation_epoch_noop :
  forall ps sd lookup offender evidence_generation evidence_epoch
         current_epoch generations,
    lookup (sd_target_hash sd) =
      Some (offender, (evidence_generation, evidence_epoch)) ->
    evidence_epoch <> current_epoch ->
    execute_slash_deploy ps sd current_epoch generations lookup = (ps, false).
Proof.
  intros. unfold execute_slash_deploy. rewrite H.
  destruct (Nat.eq_dec evidence_epoch current_epoch) as [Heq | _].
  - contradiction.
  - reflexivity.
Qed.

Theorem execute_stale_generation_noop :
  forall ps sd lookup offender evidence_generation current_epoch generations,
    lookup (sd_target_hash sd) =
      Some (offender, (evidence_generation, current_epoch)) ->
    evidence_generation <> sd_target_bond_generation sd ->
    execute_slash_deploy ps sd current_epoch generations lookup = (ps, false).
Proof.
  intros ps sd lookup offender evidence_generation current_epoch generations
         Hlookup Hstale.
  unfold execute_slash_deploy. rewrite Hlookup.
  destruct (Nat.eq_dec current_epoch current_epoch) as [_ | Hbad]; [|contradiction].
  destruct (Nat.eq_dec (sd_target_activation_epoch sd) current_epoch) as [_ | _];
    [|reflexivity].
  destruct (Nat.eq_dec evidence_generation (sd_target_bond_generation sd));
    [contradiction | reflexivity].
Qed.

Theorem execute_canonical_generation_mismatch_noop :
  forall ps sd lookup offender evidence_generation current_epoch generations
         canonical_generation,
    lookup (sd_target_hash sd) =
      Some (offender, (evidence_generation, current_epoch)) ->
    sd_target_activation_epoch sd = current_epoch ->
    sd_target_bond_generation sd = evidence_generation ->
    gm_lookup generations offender = Some canonical_generation ->
    canonical_generation <> evidence_generation ->
    execute_slash_deploy ps sd current_epoch generations lookup = (ps, false).
Proof.
  intros ps sd lookup offender evidence_generation current_epoch generations
         canonical_generation Hlookup Hepoch Htarget Hcanonical Hstale.
  unfold execute_slash_deploy. rewrite Hlookup.
  destruct (Nat.eq_dec current_epoch current_epoch) as [_ | Hbad]; [|contradiction].
  rewrite Hepoch.
  destruct (Nat.eq_dec current_epoch current_epoch) as [_ | Hbad]; [|contradiction].
  rewrite Htarget.
  destruct (Nat.eq_dec evidence_generation evidence_generation) as [_ | Hbad];
    [|contradiction].
  rewrite Hcanonical.
  destruct (Nat.eq_dec canonical_generation evidence_generation);
    [contradiction | reflexivity].
Qed.

Theorem execute_invalid_auth_token_noop :
  forall ps sd lookup current_epoch generations,
    execute_authenticated_slash_deploy
      ps sd current_epoch generations lookup false = (ps, false).
Proof.
  intros. reflexivity.
Qed.

Theorem execute_valid_auth_token_equiv :
  forall ps sd lookup current_epoch generations,
    execute_authenticated_slash_deploy
      ps sd current_epoch generations lookup true =
    execute_slash_deploy ps sd current_epoch generations lookup.
Proof.
  intros. reflexivity.
Qed.

Definition generation_scoped_slash_effect
  (ps : PoSStateC)
  (lifecycle : ValidatorBondLifecycle)
  (offender : Validator)
  (target_generation : BondGeneration)
  : (PoSStateC * ValidatorBondLifecycle) * bool :=
  match slash_lifecycle lifecycle target_generation with
  | (lifecycle_after, true) =>
      let (ps_after, accepted) := slashC ps offender in
      ((ps_after, lifecycle_after), accepted)
  | (_, false) => ((ps, lifecycle), false)
  end.

Theorem generation_scoped_slash_stale_noop :
  forall ps lifecycle offender current_generation target_generation,
    lifecycle_generation lifecycle = Some current_generation ->
    target_generation <> current_generation ->
    generation_scoped_slash_effect
      ps lifecycle offender target_generation = ((ps, lifecycle), false).
Proof.
  intros ps lifecycle offender current_generation target_generation Hcurrent Hstale.
  unfold generation_scoped_slash_effect.
  rewrite (@stale_generation_slash_is_noninterfering
    lifecycle current_generation target_generation Hcurrent Hstale).
  reflexivity.
Qed.

Theorem generation_scoped_slash_locked_refines_pos :
  forall ps offender generation successful_bonds phase,
    phase = BondedPhase \/
    phase = PendingWithdrawPhase \/
    phase = WithdrawingPhase ->
    generation_scoped_slash_effect
      ps
      (mkValidatorBondLifecycle (Some generation) successful_bonds phase)
      offender generation =
    let (ps_after, accepted) := slashC ps offender in
    ((ps_after,
      mkValidatorBondLifecycle
        (Some generation) successful_bonds QuarantinedPhase),
     accepted).
Proof.
  intros ps offender generation successful_bonds phase Hphase.
  unfold generation_scoped_slash_effect.
  rewrite (@current_generation_locked_slash_quarantines
    generation successful_bonds phase Hphase).
  destruct (slashC ps offender). reflexivity.
Qed.

Theorem generation_scoped_slash_locked_zeros_and_quarantines :
  forall ps offender generation successful_bonds phase,
    phase = BondedPhase \/
    phase = PendingWithdrawPhase \/
    phase = WithdrawingPhase ->
    bm_lookup (ps_allBonds (psc_pos ps)) offender > 0 ->
    let result := generation_scoped_slash_effect
      ps
      (mkValidatorBondLifecycle (Some generation) successful_bonds phase)
      offender generation in
    let ps_after := fst (fst result) in
    let lifecycle_after := snd (fst result) in
    bm_lookup (ps_allBonds (psc_pos ps_after)) offender = 0 /\
    qs_lookup (psc_quarantined ps_after) offender =
      Some (bm_lookup (ps_allBonds (psc_pos ps)) offender) /\
    halted_mem (psc_mintingHalted ps_after) offender = true /\
    ps_coopVault (psc_pos ps_after) = ps_coopVault (psc_pos ps) /\
    lifecycle_phase lifecycle_after = QuarantinedPhase /\
    lifecycle_generation lifecycle_after = Some generation.
Proof.
  intros ps offender generation successful_bonds phase Hphase Hbond.
  rewrite (@generation_scoped_slash_locked_refines_pos
    ps offender generation successful_bonds phase Hphase).
  destruct (slashC ps offender) as [ps_after accepted] eqn:Hslash.
  simpl.
  pose proof (slashC_zeros_bond ps offender) as Hzero.
  pose proof (slash_quarantines_stake ps offender) as Hquarantine.
  pose proof (slashC_halts ps offender) as Hhalt.
  rewrite Hslash in Hzero, Hquarantine, Hhalt.
  simpl in Hzero, Hquarantine, Hhalt.
  destruct (Hquarantine Hbond) as [Hstake Hcoop].
  repeat split.
  - exact Hzero.
  - exact Hstake.
  - apply Hhalt. exact Hbond.
  - exact Hcoop.
Qed.
