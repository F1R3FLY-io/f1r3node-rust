(* ═══════════════════════════════════════════════════════════════════════════
   BugFixUnbondedProposer.v — Proof for Bug Fix #8 (T-9.8)

   Bug. block_creator.rs:287-332 doesn't verify that the proposer itself
   is bonded. An unbonded proposer running the proposer thread will still
   build slash deploys; the slash contract rejects them at the auth-token
   check. Wasted network work.

   Fix. Skip prepare_slashing_deploys entirely when bonds_map[proposer] = 0.

   Theorem T-9.8 (Unbonded proposer no-op). Under the fix, no slash
   deploys are emitted by an unbonded proposer.

   Companion doc: slashing-verification.md §9.8.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block SlashDeploy BlockCreator.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Post-fix prepare with proposer-bond check
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition prepare_slashing_deploys_post_fix
  (candidates : list AuthorizedCandidate)
  (bonds : BondMap)
  (proposer : Validator)
  (seqNum : nat)
  (currentEpoch : nat)
  (seed_fn : Validator -> nat -> BlockHash -> nat)
  : list SlashDeploy :=
  if Nat.eqb (bm_lookup bonds proposer) 0
  then []
  else prepare_slashing_deploys candidates bonds proposer seqNum currentEpoch seed_fn.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — T-9.8: Unbonded proposer never emits a slash deploy
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem t_9_8_unbonded_proposer_no_slash :
  forall candidates bonds proposer seqNum currentEpoch seed_fn,
    bm_lookup bonds proposer = 0 ->
    prepare_slashing_deploys_post_fix candidates bonds proposer seqNum currentEpoch seed_fn = [].
Proof.
  intros. unfold prepare_slashing_deploys_post_fix.
  rewrite H. simpl. reflexivity.
Qed.

(* Under the post-fix predicate, behavior is identical to pre-fix when the
   proposer is bonded. *)
Theorem t_9_8_post_fix_equivalent_when_bonded :
  forall candidates bonds proposer seqNum currentEpoch seed_fn,
    bm_lookup bonds proposer > 0 ->
    prepare_slashing_deploys_post_fix candidates bonds proposer seqNum currentEpoch seed_fn
    = prepare_slashing_deploys candidates bonds proposer seqNum currentEpoch seed_fn.
Proof.
  intros. unfold prepare_slashing_deploys_post_fix.
  destruct (bm_lookup bonds proposer) eqn:E.
  - lia.
  - simpl. reflexivity.
Qed.
