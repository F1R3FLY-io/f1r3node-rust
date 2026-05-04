(* ═══════════════════════════════════════════════════════════════════════════
   BlockCreator.v — prepare_slashing_deploys algorithm

   Models the prepare_slashing_deploys method at
     casper/src/rust/blocks/proposer/block_creator.rs:287-332
   which enumerates bonded validators with invalid latest messages and
   produces one SlashDeploy per offender.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition          │ Rust Implementation                          │
   ─────────────────────────┼──────────────────────────────────────────────┤
   prepare_slashing_deploys │ BlockCreator.prepare_slashing_deploys         │
   invalid_latest_messages  │ s.dag.invalid_latest_messages                 │
   ilm_from_bonded          │ filter bonds_map[v] > 0                       │
   ─────────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §3.6.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block SlashDeploy.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Inputs to prepare_slashing_deploys
   ═══════════════════════════════════════════════════════════════════════════

   The algorithm takes:
   - A list of (validator, invalidBlockHash) pairs (the invalid latest
     messages from the DAG).
   - The bond map.
   - The proposer's identity and the next sequence number (for seed gen).
   - A seed-generation function (deterministic). *)

Definition prepare_slashing_deploys
  (ilm : list (Validator * BlockHash))
  (bonds : BondMap)
  (proposer : Validator)
  (seqNum : nat)
  (seed_fn : Validator -> nat -> nat)
  : list SlashDeploy :=
  let bonded := filter
                  (fun p =>
                     match p with
                     | (v, _) => Nat.ltb 0 (bm_lookup bonds v)
                     end)
                  ilm
  in map
       (fun p =>
          match p with
          | (_, h) => mkSlashDeploy h proposer (seed_fn proposer seqNum)
          end)
       bonded.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Properties
   ═══════════════════════════════════════════════════════════════════════════ *)

(* Every emitted slash deploy targets an invalid-block hash present in ilm. *)
Theorem deploy_target_in_ilm :
  forall ilm bonds proposer seqNum seed_fn sd,
    In sd (prepare_slashing_deploys ilm bonds proposer seqNum seed_fn) ->
    exists v, In (v, sd_target_hash sd) ilm.
Proof.
  intros ilm bonds proposer seqNum seed_fn sd Hin.
  unfold prepare_slashing_deploys in Hin.
  apply in_map_iff in Hin.
  destruct Hin as [[v h] [Hsd Hin']].
  apply filter_In in Hin'.
  destruct Hin' as [Hin_ilm _].
  exists v.
  rewrite <- Hsd. simpl. assumption.
Qed.

(* Every offender named by an emitted slash deploy is bonded. *)
Theorem deploy_offender_bonded :
  forall ilm bonds proposer seqNum seed_fn sd,
    In sd (prepare_slashing_deploys ilm bonds proposer seqNum seed_fn) ->
    exists v, In (v, sd_target_hash sd) ilm /\ bm_lookup bonds v > 0.
Proof.
  intros ilm bonds proposer seqNum seed_fn sd Hin.
  unfold prepare_slashing_deploys in Hin.
  apply in_map_iff in Hin.
  destruct Hin as [[v h] [Hsd Hin']].
  apply filter_In in Hin'.
  destruct Hin' as [Hin_ilm Hbonded].
  exists v. split.
  - rewrite <- Hsd. simpl. assumption.
  - apply Nat.ltb_lt in Hbonded. assumption.
Qed.

(* Empty input gives empty output. *)
Theorem prepare_empty :
  forall bonds proposer seqNum seed_fn,
    prepare_slashing_deploys [] bonds proposer seqNum seed_fn = [].
Proof.
  intros. reflexivity.
Qed.
