(* ═══════════════════════════════════════════════════════════════════════════
   BlockCreator.v — prepare_slashing_deploys algorithm

   Models the prepare_slashing_deploys method at
     casper/src/rust/blocks/proposer/block_creator.rs:287-332
   which enumerates authorized current-epoch invalid-block evidence and
   produces one SlashDeploy per offender.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition          │ Rust Implementation                          │
   ─────────────────────────┼──────────────────────────────────────────────┤
   prepare_slashing_deploys │ BlockCreator.prepare_slashing_deploys         │
   authorized_candidates    │ authorized_slash_candidates(snapshot)          │
   current_epoch_filter     │ target_activation_epoch == current_epoch       │
   bonded_filter            │ bonds_map[v] > 0                              │
   ─────────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §3.6.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block SlashDeploy.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Inputs to prepare_slashing_deploys
   ═══════════════════════════════════════════════════════════════════════════

   The algorithm takes:
   - A list of authorized candidate triples
     (validator, invalidBlockHash, targetActivationEpoch) derived from the
     DAG invalid-block evidence index. The Rust helper canonicalizes multiple
     same-epoch invalid hashes for one offender by choosing the minimum hash;
     this Rocq module takes that canonical candidate list as input.
   - The bond map.
   - The proposer's identity and the next sequence number (for seed gen).
   - A seed-generation function (deterministic). *)

Definition AuthorizedCandidate := (Validator * BlockHash * nat)%type.

Definition prepare_slashing_deploys
  (candidates : list AuthorizedCandidate)
  (bonds : BondMap)
  (proposer : Validator)
  (seqNum : nat)
  (currentEpoch : nat)
  (seed_fn : Validator -> nat -> nat)
  : list SlashDeploy :=
  let authorized := filter
                  (fun p =>
                     match p with
                     | (v, _, targetEpoch) =>
                         Nat.eqb targetEpoch currentEpoch
                         && Nat.ltb 0 (bm_lookup bonds v)
                     end)
                  candidates
  in map
       (fun p =>
          match p with
          | (_, h, targetEpoch) =>
              mkSlashDeploy h proposer targetEpoch (seed_fn proposer seqNum)
          end)
       authorized.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Properties
   ═══════════════════════════════════════════════════════════════════════════ *)

(* Every emitted slash deploy targets an authorized invalid-block candidate. *)
Theorem deploy_target_in_candidates :
  forall candidates bonds proposer seqNum currentEpoch seed_fn sd,
    In sd (prepare_slashing_deploys candidates bonds proposer seqNum currentEpoch seed_fn) ->
    exists v,
      In (v, sd_target_hash sd, sd_target_epoch sd) candidates.
Proof.
  intros candidates bonds proposer seqNum currentEpoch seed_fn sd Hin.
  unfold prepare_slashing_deploys in Hin.
  apply in_map_iff in Hin.
  destruct Hin as [[[v h] targetEpoch] [Hsd Hin']].
  apply filter_In in Hin'.
  destruct Hin' as [Hin_candidates _].
  exists v.
  rewrite <- Hsd. simpl. assumption.
Qed.

(* Every offender named by an emitted slash deploy is bonded. *)
Theorem deploy_offender_bonded :
  forall candidates bonds proposer seqNum currentEpoch seed_fn sd,
    In sd (prepare_slashing_deploys candidates bonds proposer seqNum currentEpoch seed_fn) ->
    exists v,
      In (v, sd_target_hash sd, sd_target_epoch sd) candidates /\
      bm_lookup bonds v > 0.
Proof.
  intros candidates bonds proposer seqNum currentEpoch seed_fn sd Hin.
  unfold prepare_slashing_deploys in Hin.
  apply in_map_iff in Hin.
  destruct Hin as [[[v h] targetEpoch] [Hsd Hin']].
  apply filter_In in Hin'.
  destruct Hin' as [Hin_candidates Hauth].
  apply andb_prop in Hauth as [_ Hbonded].
  exists v. split.
  - rewrite <- Hsd. simpl. assumption.
  - apply Nat.ltb_lt in Hbonded. assumption.
Qed.

(* Empty input gives empty output. *)
Theorem prepare_empty :
  forall bonds proposer seqNum currentEpoch seed_fn,
    prepare_slashing_deploys [] bonds proposer seqNum currentEpoch seed_fn = [].
Proof.
  intros. reflexivity.
Qed.

Theorem deploy_epoch_matches_target :
  forall candidates bonds proposer seqNum currentEpoch seed_fn sd,
    In sd (prepare_slashing_deploys candidates bonds proposer seqNum currentEpoch seed_fn) ->
    sd_target_epoch sd = currentEpoch.
Proof.
  intros candidates bonds proposer seqNum currentEpoch seed_fn sd Hin.
  unfold prepare_slashing_deploys in Hin.
  apply in_map_iff in Hin.
  destruct Hin as [[[v h] targetEpoch] [Hsd Hin']].
  apply filter_In in Hin'.
  destruct Hin' as [_ Hauth].
  apply andb_prop in Hauth as [Hepoch _].
  apply Nat.eqb_eq in Hepoch.
  rewrite <- Hsd. simpl. assumption.
Qed.
