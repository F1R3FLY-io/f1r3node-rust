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
  (invalidBlocks_lookup : BlockHash -> option Validator)
  : PoSState * bool :=
  match invalidBlocks_lookup (sd_target_hash sd) with
  | Some offender => slash ps offender
  | None          => slash ps (sd_proposer sd)
                       (* degraded path: if no invalid-block evidence,
                          slash the deployer (matches PoS.rhox semantics) *)
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — Successful execution invariants
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem execute_zeros_target_bond :
  forall ps sd lookup offender,
    lookup (sd_target_hash sd) = Some offender ->
    let (ps', _) := execute_slash_deploy ps sd lookup in
    bm_lookup (ps_allBonds ps') offender = 0.
Proof.
  intros ps sd lookup offender Hl.
  unfold execute_slash_deploy. rewrite Hl.
  apply (slash_zeros_bond ps offender).
Qed.

Theorem execute_other_unchanged :
  forall ps sd lookup offender v',
    lookup (sd_target_hash sd) = Some offender ->
    offender <> v' ->
    let (ps', _) := execute_slash_deploy ps sd lookup in
    bm_lookup (ps_allBonds ps') v' = bm_lookup (ps_allBonds ps) v'.
Proof.
  intros ps sd lookup offender v' Hl Hne.
  unfold execute_slash_deploy. rewrite Hl.
  apply slash_other_unchanged. assumption.
Qed.
