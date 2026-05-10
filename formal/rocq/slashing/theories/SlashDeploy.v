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
   sd_seed                 │ generate_slash_deploy_random_seed(self, seqNum)  │
   ─────────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §3.7.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Slashing Require Import Validator Block InvalidBlock PoSContract.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — SlashDeploy record
   ═══════════════════════════════════════════════════════════════════════════ *)

Record SlashDeploy : Type := mkSlashDeploy {
  sd_target_hash  : BlockHash;       (* hash of the offending invalid block *)
  sd_proposer     : Validator;       (* public key of the deployer *)
  sd_target_epoch : nat;             (* validator lifetime/epoch being targeted *)
  sd_seed         : nat              (* deterministic seed (splitByte(1)) *)
}.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Execution semantics
   ═══════════════════════════════════════════════════════════════════════════

   A SlashDeploy executes against a PoSState. The target validator is
   resolved via the invalidBlocks lookup function (an oracle in this
   abstraction; concretely, the on-chain `getInvalidBlocks` channel).
   The execution then defers to the PoS slash transition. *)

Definition execute_slash_deploy
  (ps : PoSState)
  (sd : SlashDeploy)
  (current_epoch : nat)
  (invalidBlocks_lookup : BlockHash -> option (Validator * nat))
  : PoSState * bool :=
  match invalidBlocks_lookup (sd_target_hash sd) with
  | Some (offender, evidence_epoch) =>
      if Nat.eq_dec evidence_epoch current_epoch then
        if Nat.eq_dec (sd_target_epoch sd) current_epoch then slash ps offender else (ps, false)
      else (ps, false)
  | None => (ps, false)
  end.

Definition execute_authenticated_slash_deploy
  (ps : PoSState)
  (sd : SlashDeploy)
  (current_epoch : nat)
  (invalidBlocks_lookup : BlockHash -> option (Validator * nat))
  (auth_ok : bool)
  : PoSState * bool :=
  if auth_ok
  then execute_slash_deploy ps sd current_epoch invalidBlocks_lookup
  else (ps, false).

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — Successful execution invariants
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem execute_zeros_target_bond :
  forall ps sd lookup offender current_epoch,
    lookup (sd_target_hash sd) = Some (offender, current_epoch) ->
    sd_target_epoch sd = current_epoch ->
    let (ps', _) := execute_slash_deploy ps sd current_epoch lookup in
    bm_lookup (ps_allBonds ps') offender = 0.
Proof.
  intros ps sd lookup offender current_epoch Hl He.
  unfold execute_slash_deploy. rewrite Hl.
  destruct (Nat.eq_dec current_epoch current_epoch) as [_ | Hneq]; [|contradiction].
  rewrite He.
  destruct (Nat.eq_dec current_epoch current_epoch) as [_ | Hneq]; [|contradiction].
  apply (slash_zeros_bond ps offender).
Qed.

Theorem execute_other_unchanged :
  forall ps sd lookup offender v' current_epoch,
    lookup (sd_target_hash sd) = Some (offender, current_epoch) ->
    sd_target_epoch sd = current_epoch ->
    offender <> v' ->
    let (ps', _) := execute_slash_deploy ps sd current_epoch lookup in
    bm_lookup (ps_allBonds ps') v' = bm_lookup (ps_allBonds ps) v'.
Proof.
  intros ps sd lookup offender v' current_epoch Hl He Hne.
  unfold execute_slash_deploy. rewrite Hl.
  destruct (Nat.eq_dec current_epoch current_epoch) as [_ | Hneq]; [|contradiction].
  rewrite He.
  destruct (Nat.eq_dec current_epoch current_epoch) as [_ | Hneq]; [|contradiction].
  apply slash_other_unchanged. assumption.
Qed.

Theorem execute_unknown_evidence_noop :
  forall ps sd lookup current_epoch,
    lookup (sd_target_hash sd) = None ->
    execute_slash_deploy ps sd current_epoch lookup = (ps, false).
Proof.
  intros. unfold execute_slash_deploy. rewrite H. reflexivity.
Qed.

Theorem execute_stale_evidence_noop :
  forall ps sd lookup offender evidence_epoch current_epoch,
    lookup (sd_target_hash sd) = Some (offender, evidence_epoch) ->
    evidence_epoch <> current_epoch ->
    execute_slash_deploy ps sd current_epoch lookup = (ps, false).
Proof.
  intros. unfold execute_slash_deploy. rewrite H.
  destruct (Nat.eq_dec evidence_epoch current_epoch) as [Heq | _].
  - contradiction.
  - reflexivity.
Qed.

Theorem execute_invalid_auth_token_noop :
  forall ps sd lookup current_epoch,
    execute_authenticated_slash_deploy ps sd current_epoch lookup false = (ps, false).
Proof.
  intros. reflexivity.
Qed.

Theorem execute_valid_auth_token_equiv :
  forall ps sd lookup current_epoch,
    execute_authenticated_slash_deploy ps sd current_epoch lookup true =
    execute_slash_deploy ps sd current_epoch lookup.
Proof.
  intros. reflexivity.
Qed.
