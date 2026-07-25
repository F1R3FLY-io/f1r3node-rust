(* ═══════════════════════════════════════════════════════════════════════════
   DAGState.v — Snapshot of the BlockDAG state relevant to slashing

   Defines the abstract DAG snapshot the detector and validator operate on:
   - the set of all known blocks
   - the invalid-block index
   - the equivocation-record store
   - the bonds-map snapshot

   This module is the bridge between the per-component types (Block,
   InvalidBlock, EquivocationRecord, BondMap) and the orchestration modules
   (EquivocationDetector, MultiParentCasper).

   Equivocation is presented as a *Boolean function* over the DAG (see §3),
   keeping decidability by construction. The propositional version in §4 is
   defined to reflect the Boolean view.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition       │ Paper Notation    │ Rust Implementation
   ──────────────────────┼───────────────────┼──────────────────────
   DAGState              │ S = (D, I, E, B)  │ CasperSnapshot + DAG store
   ds_blocks             │ D                 │ block_dag_storage entries
   ds_invalid            │ I ⊆ H             │ block.invalid flag
   ds_records            │ E ⊆ EqRec         │ equivocation_tracker_store
   ds_bonds              │ B : V → ℕ         │ on_chain_state.bonds_map
   equivocates_b         │ E?(v, s) ∈ Bool   │ check_equivocations result
   ─────────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §3.4.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block EquivocationRecord.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Core record
   ═══════════════════════════════════════════════════════════════════════════ *)

Record DAGState : Type := mkDAGState {
  ds_blocks  : list Block;
  ds_invalid : list BlockHash;
  ds_records : EqStore;
  ds_bonds   : BondMap
}.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Lookup functions
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition ds_block_by_hash (s : DAGState) (h : BlockHash) : option Block :=
  let same_hash (b : Block) : bool :=
        if hash_eq_dec (block_hash b) h then true else false
  in find same_hash (ds_blocks s).

Definition ds_is_invalid (s : DAGState) (h : BlockHash) : bool :=
  existsb (Nat.eqb h) (ds_invalid s).

Fixpoint ds_latest_seq (blocks : list Block) (v : Validator) : nat :=
  match blocks with
  | [] => 0
  | b :: rest =>
      if validator_eq_dec (block_sender b) v
      then Nat.max (block_seq b) (ds_latest_seq rest v)
      else ds_latest_seq rest v
  end.

Definition ds_latest_message_seq (s : DAGState) (v : Validator) : nat :=
  ds_latest_seq (ds_blocks s) v.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — Equivocation as a Boolean function (decidable by construction)
   ═══════════════════════════════════════════════════════════════════════════

   distinct_hashes_at gives the deduplicated list of block hashes signed by
   v at sequence number s. Equivocation occurs iff the list has length ≥ 2. *)

Fixpoint distinct_hashes_at (blocks : list Block) (v : Validator) (n : nat) : list BlockHash :=
  match blocks with
  | [] => []
  | b :: rest =>
      let acc := distinct_hashes_at rest v n in
      if andb
           (if validator_eq_dec (block_sender b) v then true else false)
           (Nat.eqb (block_seq b) n)
      then if existsb (Nat.eqb (block_hash b)) acc
           then acc
           else block_hash b :: acc
      else acc
  end.

Definition equivocates_b (s : DAGState) (v : Validator) (n : nat) : bool :=
  Nat.ltb 1 (length (distinct_hashes_at (ds_blocks s) v n)).

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — Propositional equivocation, equivalent to §3 by reflection
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition equivocates (s : DAGState) (v : Validator) (n : nat) : Prop :=
  equivocates_b s v n = true.

Lemma equivocates_dec :
  forall s v n, {equivocates s v n} + {~ equivocates s v n}.
Proof.
  intros. unfold equivocates. destruct (equivocates_b s v n).
  - left. reflexivity.
  - right. discriminate.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — Witness extraction (for soundness/completeness arguments)
   ═══════════════════════════════════════════════════════════════════════════

   If equivocates_b returns true, the distinct_hashes_at list has at least two
   elements; we can extract a pair of distinct block hashes signed by v at
   sequence number n. *)

Lemma distinct_hashes_dedup :
  forall blocks v n h,
    In h (distinct_hashes_at blocks v n) ->
    exists b, In b blocks /\ block_sender b = v /\ block_seq b = n
              /\ block_hash b = h.
Proof.
  induction blocks as [| b rest IH]; intros v n h Hin; simpl in Hin.
  - inversion Hin.
  - destruct (validator_eq_dec (block_sender b) v) as [Hs | Hs].
    + simpl in Hin.
      destruct (Nat.eqb (block_seq b) n) eqn:En.
      * apply Nat.eqb_eq in En.
        destruct (existsb (Nat.eqb (block_hash b))
                          (distinct_hashes_at rest v n)) eqn:Ex.
        -- (* h was already in the recursive list. *)
           specialize (IH _ _ _ Hin).
           destruct IH as [b' [Hin' [Hs' [Hn' Hh']]]].
           exists b'. repeat split; try assumption. right. assumption.
        -- (* h is either b's hash or in the recursive list. *)
           simpl in Hin. destruct Hin as [E | Hin'].
           ++ exists b. repeat split; try assumption.
              left. reflexivity.
           ++ specialize (IH _ _ _ Hin').
              destruct IH as [b' [Hin'' [Hs' [Hn' Hh']]]].
              exists b'. repeat split; try assumption. right. assumption.
      * specialize (IH _ _ _ Hin).
        destruct IH as [b' [Hin' [Hs' [Hn' Hh']]]].
        exists b'. repeat split; try assumption. right. assumption.
    + specialize (IH _ _ _ Hin).
      destruct IH as [b' [Hin' [Hs' [Hn' Hh']]]].
      exists b'. repeat split; try assumption. right. assumption.
Qed.

Lemma distinct_hashes_nodup :
  forall blocks v n, NoDup (distinct_hashes_at blocks v n).
Proof.
  induction blocks as [| b rest IH]; intros v n; simpl.
  - constructor.
  - destruct (validator_eq_dec (block_sender b) v) as [_ | _]; [|apply IH].
    simpl. destruct (Nat.eqb (block_seq b) n); [|apply IH].
    destruct (existsb (Nat.eqb (block_hash b))
                      (distinct_hashes_at rest v n)) eqn:Ex.
    + apply IH.
    + constructor; [|apply IH].
      intro Hin.
      assert (Hex : existsb (Nat.eqb (block_hash b))
                            (distinct_hashes_at rest v n) = true).
      { apply existsb_exists. exists (block_hash b).
        split; [assumption | apply Nat.eqb_refl]. }
      rewrite Hex in Ex. discriminate.
Qed.

Lemma equivocates_witnesses :
  forall s v n,
    equivocates s v n ->
    exists h1 h2 b1 b2,
      h1 <> h2
      /\ In b1 (ds_blocks s) /\ In b2 (ds_blocks s)
      /\ block_sender b1 = v /\ block_sender b2 = v
      /\ block_seq b1 = n    /\ block_seq b2 = n
      /\ block_hash b1 = h1  /\ block_hash b2 = h2.
Proof.
  intros s v n Heq.
  unfold equivocates, equivocates_b in Heq.
  apply Nat.ltb_lt in Heq.
  remember (distinct_hashes_at (ds_blocks s) v n) as L eqn:HL.
  destruct L as [| h1 [| h2 rest]]; simpl in Heq; try lia.
  assert (Hnd : NoDup (h1 :: h2 :: rest)) by (rewrite HL; apply distinct_hashes_nodup).
  assert (Hnotin : ~ In h1 (h2 :: rest)).
  { intro Hin. inversion Hnd; subst. contradiction. }
  assert (H1in : In h1 (h1 :: h2 :: rest)) by (left; reflexivity).
  assert (H2in : In h2 (h1 :: h2 :: rest)) by (right; left; reflexivity).
  rewrite HL in H1in, H2in.
  destruct (distinct_hashes_dedup _ _ _ _ H1in) as [b1 [Hin1 [Hs1 [Hn1 Hh1]]]].
  destruct (distinct_hashes_dedup _ _ _ _ H2in) as [b2 [Hin2 [Hs2 [Hn2 Hh2]]]].
  exists h1, h2, b1, b2.
  repeat split; try assumption.
  intro Heq12. apply Hnotin. rewrite Heq12. left. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — Adding blocks preserves prior detected equivocations
   ═══════════════════════════════════════════════════════════════════════════ *)

Lemma equivocates_monotone_extend :
  forall blocks_old blocks_new v n,
    incl blocks_old blocks_new ->
    incl (distinct_hashes_at blocks_old v n)
         (distinct_hashes_at blocks_new v n).
Proof.
  intros bo bn v n Hsub h Hin.
  apply distinct_hashes_dedup in Hin.
  destruct Hin as [b [Hb [Hs [Hn Hh]]]].
  apply Hsub in Hb.
  (* Now show that h ∈ distinct_hashes_at bn v n using b. *)
  clear Hsub bo.
  induction bn as [| b' rest IH]; simpl in Hb |- *.
  - inversion Hb.
  - destruct (validator_eq_dec (block_sender b') v) as [Eq1 | Ne1].
    + simpl. destruct (Nat.eqb (block_seq b') n) eqn:En.
      * destruct (existsb (Nat.eqb (block_hash b'))
                          (distinct_hashes_at rest v n)) eqn:Ex.
        -- destruct Hb as [E | Hin].
           ++ subst b'. simpl in Eq1, En.
              apply Nat.eqb_eq in En.
              (* h = block_hash b = block_hash b'. Already in the list. *)
              apply existsb_exists in Ex.
              destruct Ex as [h' [Hin' Hh']].
              apply Nat.eqb_eq in Hh'. subst h'.
              rewrite Hh in *. assumption.
           ++ apply IH. assumption.
        -- destruct Hb as [E | Hin].
           ++ subst b'.
              left. rewrite Hh. reflexivity.
           ++ right. apply IH. assumption.
      * destruct Hb as [E | Hin].
        -- subst b'. apply Nat.eqb_neq in En. congruence.
        -- apply IH. assumption.
    + destruct Hb as [E | Hin].
      * subst b'. congruence.
      * apply IH. assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §7 — Latest-message recovery (used by JustificationRegression checks)
   ═══════════════════════════════════════════════════════════════════════════ *)

Lemma ds_latest_seq_lower_bound :
  forall blocks v b,
    In b blocks ->
    block_sender b = v ->
    block_seq b <= ds_latest_seq blocks v.
Proof.
  induction blocks as [| b' rest IH]; intros v b Hin Hs; simpl in Hin |- *.
  - inversion Hin.
  - destruct (validator_eq_dec (block_sender b') v) as [Eq | Ne].
    + destruct Hin as [E | Hin'].
      * subst b'. apply Nat.le_max_l.
      * specialize (IH _ _ Hin' Hs).
        apply Nat.max_le_iff. right. assumption.
    + destruct Hin as [E | Hin'].
      * subst b'. congruence.
      * apply IH; assumption.
Qed.
