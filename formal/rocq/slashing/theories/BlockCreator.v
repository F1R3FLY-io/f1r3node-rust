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
From Slashing Require Import Validator ValidatorLifetime Block SlashDeploy.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Inputs to prepare_slashing_deploys
   ═══════════════════════════════════════════════════════════════════════════

   The algorithm takes:
   - A list of authorized candidate quadruples
     (validator, invalidBlockHash, targetActivationEpoch, bondGeneration)
     derived from the
     DAG invalid-block evidence index. The Rust helper canonicalizes multiple
     same-window invalid hashes for one validator generation by minimum hash;
     this Rocq module takes that canonical candidate list as input.
   - The bond map.
   - The proposer's identity and the next sequence number (for seed gen).
   - A seed-generation function over proposer, sequence number, and invalid
     block hash. *)

Definition AuthorizedCandidate :=
  (Validator * BlockHash * Epoch * BondGeneration)%type.

Definition candidate_key
  (candidate : AuthorizedCandidate) : Validator * BondGeneration :=
  match candidate with
  | (validator, _, _, targetGeneration) =>
      (validator, targetGeneration)
  end.

Definition candidate_authorized
  (bonds : BondMap)
  (generations : GenerationMap)
  (currentEpoch : Epoch)
  (candidate : AuthorizedCandidate) : bool :=
  match candidate with
  | (validator, _, targetEpoch, targetGeneration) =>
      Nat.eqb targetEpoch currentEpoch
      && match gm_lookup generations validator with
         | Some canonicalGeneration =>
             Nat.eqb targetGeneration canonicalGeneration
             && Nat.ltb 0 (bm_lookup bonds validator)
         | None => false
         end
  end.

Definition selected_slash_candidates
  (candidates : list AuthorizedCandidate)
  (bonds : BondMap)
  (generations : GenerationMap)
  (currentEpoch : Epoch)
  : list AuthorizedCandidate :=
  filter (candidate_authorized bonds generations currentEpoch) candidates.

Definition prepare_slashing_deploys
  (candidates : list AuthorizedCandidate)
  (bonds : BondMap)
  (generations : GenerationMap)
  (proposer : Validator)
  (seqNum : nat)
  (currentEpoch : Epoch)
  (seed_fn : Validator -> nat -> BlockHash -> nat)
  : list SlashDeploy :=
  map
       (fun p =>
          match p with
          | (_, h, targetEpoch, targetGeneration) =>
              (* In honest construction the proposer IS the issuer: the block
                 sender signs the slash deploy it mints, so sd_issuer = proposer
                 and the §9.8 receive gate's issuer==sender rule holds. *)
              mkSlashDeploy h proposer targetEpoch targetGeneration
                (seed_fn proposer seqNum h) proposer
          end)
       (selected_slash_candidates candidates bonds generations currentEpoch).

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Properties
   ═══════════════════════════════════════════════════════════════════════════ *)

(* Every emitted slash deploy targets an authorized invalid-block candidate. *)
Theorem deploy_target_in_candidates :
  forall candidates bonds generations proposer seqNum currentEpoch seed_fn sd,
    In sd (prepare_slashing_deploys
      candidates bonds generations proposer seqNum currentEpoch seed_fn) ->
    exists v,
      In (v, sd_target_hash sd, sd_target_activation_epoch sd,
        sd_target_bond_generation sd) candidates.
Proof.
  intros candidates bonds generations proposer seqNum currentEpoch seed_fn sd Hin.
  unfold prepare_slashing_deploys in Hin.
  apply in_map_iff in Hin.
  destruct Hin as [[[[v h] targetEpoch] targetGeneration] [Hsd Hin']].
  apply filter_In in Hin'.
  destruct Hin' as [Hin_candidates _].
  exists v.
  rewrite <- Hsd. simpl. assumption.
Qed.

(* Every offender named by an emitted slash deploy is bonded. *)
Theorem deploy_offender_bonded :
  forall candidates bonds generations proposer seqNum currentEpoch seed_fn sd,
    In sd (prepare_slashing_deploys
      candidates bonds generations proposer seqNum currentEpoch seed_fn) ->
    exists v,
      In (v, sd_target_hash sd, sd_target_activation_epoch sd,
        sd_target_bond_generation sd) candidates /\
      bm_lookup bonds v > 0.
Proof.
  intros candidates bonds generations proposer seqNum currentEpoch seed_fn sd Hin.
  unfold prepare_slashing_deploys in Hin.
  apply in_map_iff in Hin.
  destruct Hin as [[[[v h] targetEpoch] targetGeneration] [Hsd Hin']].
  apply filter_In in Hin'.
  destruct Hin' as [Hin_candidates Hauth].
  apply andb_prop in Hauth as [_ Hauthority].
  destruct (gm_lookup generations v) as [canonicalGeneration |];
    [|discriminate].
  apply andb_prop in Hauthority as [_ Hbonded].
  exists v. split.
  - rewrite <- Hsd. simpl. assumption.
  - apply Nat.ltb_lt in Hbonded. assumption.
Qed.

(* Empty input gives empty output. *)
Theorem prepare_empty :
  forall bonds generations proposer seqNum currentEpoch seed_fn,
    prepare_slashing_deploys
      [] bonds generations proposer seqNum currentEpoch seed_fn = [].
Proof.
  intros. reflexivity.
Qed.

Theorem deploy_activation_epoch_is_current :
  forall candidates bonds generations proposer seqNum currentEpoch seed_fn sd,
    In sd (prepare_slashing_deploys
      candidates bonds generations proposer seqNum currentEpoch seed_fn) ->
    sd_target_activation_epoch sd = currentEpoch.
Proof.
  intros candidates bonds generations proposer seqNum currentEpoch seed_fn sd Hin.
  unfold prepare_slashing_deploys in Hin.
  apply in_map_iff in Hin.
  destruct Hin as [[[[v h] targetEpoch] targetGeneration] [Hsd Hin']].
  apply filter_In in Hin'.
  destruct Hin' as [_ Hauth].
  apply andb_prop in Hauth as [Hepoch _].
  apply Nat.eqb_eq in Hepoch.
  rewrite <- Hsd. simpl. assumption.
Qed.

Theorem deploy_generation_matches_canonical_authority :
  forall candidates bonds generations proposer seqNum currentEpoch seed_fn sd,
    In sd (prepare_slashing_deploys
      candidates bonds generations proposer seqNum currentEpoch seed_fn) ->
    exists offender,
      gm_lookup generations offender =
        Some (sd_target_bond_generation sd).
Proof.
  intros candidates bonds generations proposer seqNum currentEpoch seed_fn sd Hin.
  unfold prepare_slashing_deploys in Hin.
  apply in_map_iff in Hin.
  destruct Hin as [[[[offender hash] targetEpoch] targetGeneration]
    [Hsd Hin']].
  apply filter_In in Hin' as [_ Hauth].
  apply andb_prop in Hauth as [_ Hgeneration].
  destruct (gm_lookup generations offender) as [canonicalGeneration |]
    eqn:Hlookup; [|discriminate].
  apply andb_prop in Hgeneration as [Heq _].
  apply Nat.eqb_eq in Heq. subst canonicalGeneration.
  exists offender. rewrite <- Hsd. simpl. assumption.
Qed.

Theorem deploy_seed_uses_invalid_block_hash :
  forall candidates bonds generations proposer seqNum currentEpoch seed_fn sd,
    In sd (prepare_slashing_deploys
      candidates bonds generations proposer seqNum currentEpoch seed_fn) ->
    sd_seed sd = seed_fn proposer seqNum (sd_target_hash sd).
Proof.
  intros candidates bonds generations proposer seqNum currentEpoch seed_fn sd Hin.
  unfold prepare_slashing_deploys in Hin.
  apply in_map_iff in Hin.
  destruct Hin as [[[[v h] targetEpoch] targetGeneration] [Hsd Hin']].
  rewrite <- Hsd. reflexivity.
Qed.

Theorem authorized_candidate_selected :
  forall candidates bonds generations currentEpoch candidate,
    In candidate candidates ->
    candidate_authorized bonds generations currentEpoch candidate = true ->
    In candidate
      (selected_slash_candidates candidates bonds generations currentEpoch).
Proof.
  intros candidates bonds generations currentEpoch candidate Hin Hauthorized.
  apply filter_In. split; assumption.
Qed.

Theorem merge_rejected_hint_subsumed_by_authorized_scan :
  forall rejectedHints candidates bonds generations currentEpoch candidate,
    In candidate rejectedHints ->
    In candidate candidates ->
    candidate_authorized bonds generations currentEpoch candidate = true ->
    In candidate
      (selected_slash_candidates candidates bonds generations currentEpoch).
Proof.
  intros rejectedHints candidates bonds generations currentEpoch candidate
         _ Hin Hauthorized.
  apply authorized_candidate_selected; assumption.
Qed.

Theorem zero_bond_candidate_not_selected :
  forall candidates bonds generations currentEpoch validator hash
         targetEpoch targetGeneration,
    bm_lookup bonds validator = 0 ->
    ~ In (validator, hash, targetEpoch, targetGeneration)
        (selected_slash_candidates
          candidates bonds generations currentEpoch).
Proof.
  intros candidates bonds generations currentEpoch validator hash
         targetEpoch targetGeneration Hzero Hin.
  unfold selected_slash_candidates in Hin.
  apply filter_In in Hin as [_ Hauthorized].
  unfold candidate_authorized in Hauthorized. simpl in Hauthorized.
  apply andb_prop in Hauthorized as [_ Hauthority].
  destruct (gm_lookup generations validator); [|discriminate].
  apply andb_prop in Hauthority as [_ Hpositive].
  apply Nat.ltb_lt in Hpositive.
  rewrite Hzero in Hpositive. lia.
Qed.

Lemma mapped_keys_of_filter_nodup :
  forall candidates predicate,
    NoDup (map candidate_key candidates) ->
    NoDup (map candidate_key (filter predicate candidates)).
Proof.
  intros candidates predicate Hnodup.
  induction candidates as [| candidate rest IH]; simpl.
  - constructor.
  - inversion Hnodup as [| key keys Hnotin Hrest]; subst.
    destruct (predicate candidate) eqn:Hpredicate; simpl.
    + constructor.
      * intro Hin.
        apply Hnotin.
        apply in_map_iff in Hin.
        destruct Hin as [other [Hkey Hother]].
        apply in_map_iff.
        exists other. split; [assumption |].
        apply filter_In in Hother as [Hother _]. assumption.
      * apply IH. assumption.
    + apply IH. assumption.
Qed.

Theorem selected_target_keys_nodup :
  forall candidates bonds generations currentEpoch,
    NoDup (map candidate_key candidates) ->
    NoDup
      (map candidate_key
        (selected_slash_candidates
          candidates bonds generations currentEpoch)).
Proof.
  intros candidates bonds generations currentEpoch Hcanonical.
  unfold selected_slash_candidates.
  apply mapped_keys_of_filter_nodup. assumption.
Qed.
