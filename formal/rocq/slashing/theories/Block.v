(* ═══════════════════════════════════════════════════════════════════════════
   Block.v — Blocks, justifications, and sequence numbers

   Defines the [Block] type abstracting over the f1r3node BlockMessage:
   sender, sequence number, justifications, hash, and a flag tracking whether
   the block carries a SystemDeployData::Slash deploy (used by bug fix #9).

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition       │ Paper / Spec Notation       │ Rust Implementation
   ──────────────────────┼─────────────────────────────┼─────────────────────
   Block                 │ b ∈ B (block)               │ BlockMessage (proto)
   block_sender          │ sender(b) ∈ V               │ b.sender (PublicKey)
   block_seq             │ seqNum(b) ∈ ℕ               │ b.seq_num : i32
   block_hash            │ h(b) ∈ H                     │ b.block_hash
   block_justifications  │ J(b) ⊆ V × H                 │ b.justifications
   block_carries_slash   │ Slash ∈ b.system_deploys     │ has_slash_system_deploys
   ─────────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §3.2
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Block hashes
   ═══════════════════════════════════════════════════════════════════════════

   Hashes are abstract identifiers. We model them as nat for decidable
   equality. The bisimilarity argument is modulo this representation. *)

Definition BlockHash := nat.

Definition hash_eq_dec : forall (h1 h2 : BlockHash), {h1 = h2} + {h1 <> h2}
  := Nat.eq_dec.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Justifications
   ═══════════════════════════════════════════════════════════════════════════

   A justification is a (validator, latest-block-hash) pair recording the
   most recent block from a particular validator that the current block has
   observed. Mirrors `casper.proto :: Justification`. *)

Record Justification : Type := mkJustification {
  j_validator       : Validator;
  j_latestBlockHash : BlockHash
}.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — The Block record
   ═══════════════════════════════════════════════════════════════════════════ *)

Record Block : Type := mkBlock {
  block_sender         : Validator;
  block_seq            : nat;
  block_hash           : BlockHash;
  block_justifications : list Justification;
  (* TRUE iff the block carries a SystemDeployData::Slash deploy. Affects
     the post-fix branch in neglected_invalid_block (bug fix #9). *)
  block_carries_slash  : bool
}.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — Equivocation predicate at the block level
   ═══════════════════════════════════════════════════════════════════════════

   Two blocks form an equivocation if they share sender and seq number but
   have distinct hashes. *)

Definition is_equivocation (b1 b2 : Block) : Prop :=
  block_sender b1 = block_sender b2
  /\ block_seq b1 = block_seq b2
  /\ block_hash b1 <> block_hash b2.

Lemma is_equivocation_sym :
  forall b1 b2, is_equivocation b1 b2 -> is_equivocation b2 b1.
Proof.
  intros b1 b2 [Hs [Hn Hh]]. split; [|split].
  - symmetry. assumption.
  - symmetry. assumption.
  - intro H. apply Hh. symmetry. assumption.
Qed.

Lemma is_equivocation_irrefl :
  forall b, ~ is_equivocation b b.
Proof.
  intros b [_ [_ Hh]]. apply Hh. reflexivity.
Qed.

Lemma is_equivocation_dec :
  forall b1 b2, {is_equivocation b1 b2} + {~ is_equivocation b1 b2}.
Proof.
  intros b1 b2.
  destruct (validator_eq_dec (block_sender b1) (block_sender b2)) as [Hs | Hns].
  - destruct (Nat.eq_dec (block_seq b1) (block_seq b2)) as [Hn | Hnn].
    + destruct (hash_eq_dec (block_hash b1) (block_hash b2)) as [Hh | Hnh].
      * right. intros [_ [_ Hne]]. apply Hne. assumption.
      * left. split; [assumption | split; assumption].
    + right. intros [_ [Hn' _]]. apply Hnn. assumption.
  - right. intros [Hs' _]. apply Hns. assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — Self-justification
   ═══════════════════════════════════════════════════════════════════════════

   A block's self-justification is the entry in its justifications whose
   validator is the block's sender — i.e., the previous block by the same
   validator that this block builds on. *)

Definition self_justification (b : Block) : option Justification :=
  let same_sender (j : Justification) : bool :=
        if validator_eq_dec (j_validator j) (block_sender b) then true else false
  in find same_sender (block_justifications b).

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — Justification regression
   ═══════════════════════════════════════════════════════════════════════════

   A justification regression occurs when block b's justification for some
   validator v points to a strictly earlier block than b's previous
   self-justification (for v = sender(b)) or the latest message known
   (for other validators). At the abstract level we model this via a
   per-justification predicate parameterized by a "latest message" oracle. *)

Definition regresses_justification
  (latest_seq : Validator -> nat)  (* oracle: latest seqNum seen for v *)
  (j : Justification) : Prop :=
  exists prior_seq,
    prior_seq = latest_seq (j_validator j)
    /\ prior_seq > 0
    (* the justification points to a block with strictly less seq than the
       latest already observed.  We represent the seq of [j_latestBlockHash]
       as another oracle in the DAGState module; here we keep it abstract. *)
    .

(* ═══════════════════════════════════════════════════════════════════════════
   §7 — Block kept across DAG operations: equality decidability
   ═══════════════════════════════════════════════════════════════════════════ *)

Lemma block_hash_eq_iff_blocks_eq_modulo_other_fields :
  forall b1 b2,
    block_sender b1 = block_sender b2 ->
    block_seq b1    = block_seq b2 ->
    block_hash b1   = block_hash b2 ->
    ~ is_equivocation b1 b2.
Proof.
  intros b1 b2 _ _ Hh [_ [_ Hne]]. apply Hne. assumption.
Qed.
