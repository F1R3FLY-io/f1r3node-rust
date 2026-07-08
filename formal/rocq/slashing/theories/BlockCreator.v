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
   - A seed-generation function over proposer, sequence number, and invalid
     block hash. *)

Definition AuthorizedCandidate := (Validator * BlockHash * nat)%type.

Definition prepare_slashing_deploys
  (candidates : list AuthorizedCandidate)
  (bonds : BondMap)
  (proposer : Validator)
  (seqNum : nat)
  (currentEpoch : nat)
  (seed_fn : Validator -> nat -> BlockHash -> nat)
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
              mkSlashDeploy h proposer targetEpoch (seed_fn proposer seqNum h)
          end)
       authorized.

Definition RejectedSlash := (BlockHash * Validator)%type.

Definition rejected_slash_hash (rs : RejectedSlash) : BlockHash := fst rs.

Fixpoint hash_member (h : BlockHash) (hs : list BlockHash) : bool :=
  match hs with
  | [] => false
  | h' :: rest => Nat.eqb h h' || hash_member h rest
  end.

Definition recoverable_rejected_slash_hashes
  (rejected : list RejectedSlash) (own_invalid_hashes : list BlockHash)
  : list BlockHash :=
  nodup hash_eq_dec
    (filter
       (fun h => negb (hash_member h own_invalid_hashes))
       (map rejected_slash_hash rejected)).

Definition recoverable_current_rejected_slash_hashes
  (rejected : list RejectedSlash)
  (own_invalid_hashes current_evidence_hashes : list BlockHash)
  : list BlockHash :=
  filter
    (fun h => hash_member h current_evidence_hashes)
    (recoverable_rejected_slash_hashes rejected own_invalid_hashes).

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

Theorem deploy_seed_uses_invalid_block_hash :
  forall candidates bonds proposer seqNum currentEpoch seed_fn sd,
    In sd (prepare_slashing_deploys candidates bonds proposer seqNum currentEpoch seed_fn) ->
    sd_seed sd = seed_fn proposer seqNum (sd_target_hash sd).
Proof.
  intros candidates bonds proposer seqNum currentEpoch seed_fn sd Hin.
  unfold prepare_slashing_deploys in Hin.
  apply in_map_iff in Hin.
  destruct Hin as [[[v h] targetEpoch] [Hsd Hin']].
  rewrite <- Hsd. reflexivity.
Qed.

Lemma hash_member_true_iff :
  forall h hs,
    hash_member h hs = true <-> In h hs.
Proof.
  intros h hs. induction hs as [| x xs IH]; simpl.
  - split; intros H; [discriminate | contradiction].
  - rewrite Bool.orb_true_iff. rewrite Nat.eqb_eq. rewrite IH.
    split.
    + intros [Heq | Hin]; [left; symmetry; assumption | right; assumption].
    + intros [Heq | Hin]; [left; symmetry; assumption | right; assumption].
Qed.

Lemma hash_member_false_iff :
  forall h hs,
    hash_member h hs = false <-> ~ In h hs.
Proof.
  intros h hs. rewrite <- Bool.not_true_iff_false.
  rewrite hash_member_true_iff. reflexivity.
Qed.

Theorem recoverable_rejected_slash_hashes_nodup :
  forall rejected own_invalid_hashes,
    NoDup (recoverable_rejected_slash_hashes rejected own_invalid_hashes).
Proof.
  intros. unfold recoverable_rejected_slash_hashes. apply NoDup_nodup.
Qed.

Theorem own_detected_hash_not_recovered :
  forall rejected own_invalid_hashes h,
    In h own_invalid_hashes ->
    ~ In h (recoverable_rejected_slash_hashes rejected own_invalid_hashes).
Proof.
  intros rejected own_invalid_hashes h Hown Hin.
  unfold recoverable_rejected_slash_hashes in Hin.
  apply nodup_In in Hin.
  apply filter_In in Hin.
  destruct Hin as [_ Hfilter].
  rewrite Bool.negb_true_iff in Hfilter.
  apply hash_member_false_iff in Hfilter.
  contradiction.
Qed.

Theorem uncovered_rejected_hash_recovered :
  forall rejected own_invalid_hashes rs,
    In rs rejected ->
    ~ In (rejected_slash_hash rs) own_invalid_hashes ->
    In (rejected_slash_hash rs)
       (recoverable_rejected_slash_hashes rejected own_invalid_hashes).
Proof.
  intros rejected own_invalid_hashes rs Hrej Hnotown.
  unfold recoverable_rejected_slash_hashes.
  apply nodup_In.
  apply filter_In. split.
  - apply in_map. assumption.
  - rewrite Bool.negb_true_iff. apply hash_member_false_iff. assumption.
Qed.

Theorem recoverable_rejected_slash_requires_current_evidence :
  forall rejected own_invalid_hashes current_evidence_hashes h,
    In h (recoverable_current_rejected_slash_hashes
            rejected own_invalid_hashes current_evidence_hashes) ->
    In h current_evidence_hashes.
Proof.
  intros rejected own_invalid_hashes current_evidence_hashes h Hin.
  unfold recoverable_current_rejected_slash_hashes in Hin.
  apply filter_In in Hin.
  destruct Hin as [_ Hcurrent].
  apply hash_member_true_iff. assumption.
Qed.

Theorem current_uncovered_rejected_hash_recovered :
  forall rejected own_invalid_hashes current_evidence_hashes rs,
    In rs rejected ->
    ~ In (rejected_slash_hash rs) own_invalid_hashes ->
    In (rejected_slash_hash rs) current_evidence_hashes ->
    In (rejected_slash_hash rs)
       (recoverable_current_rejected_slash_hashes
          rejected own_invalid_hashes current_evidence_hashes).
Proof.
  intros rejected own_invalid_hashes current_evidence_hashes rs Hrej Hnotown Hcurrent.
  unfold recoverable_current_rejected_slash_hashes.
  apply filter_In. split.
  - apply uncovered_rejected_hash_recovered; assumption.
  - apply hash_member_true_iff. assumption.
Qed.
