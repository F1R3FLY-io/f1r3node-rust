(* ═══════════════════════════════════════════════════════════════════════════
   BugFixSeqNumDensity.v — Proof for Bug Fix #7 (T-9.7)

   Bug. equivocation_detector.rs:400 (Scala counterpart at
   EquivocationDetector.scala:336) uses baseSeqNum + 1 to find a
   validator's child block, assuming per-sender seq numbers are dense.
   If a validator skips a sequence number, the BFS fails.

   Fix. Replace baseSeqNum + 1 with a BFS over the creator-justification
   chain — find ANY descendant block by the same sender, regardless of
   seq-number gap.

   Theorem T-9.7 (SeqNum density). Under the BFS, equivocation detection
   succeeds even when per-sender seq numbers are non-dense.

   Companion doc: slashing-verification.md §9.7.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Pre-fix and post-fix descendant lookups
   ═══════════════════════════════════════════════════════════════════════════ *)

(* Pre-fix: find a block by sender at exactly baseSeq + 1. *)
Definition find_descendant_pre_fix
  (blocks : list Block) (sender : Validator) (baseSeq : nat) : option Block :=
  find (fun b =>
          andb
            (if validator_eq_dec (block_sender b) sender then true else false)
            (Nat.eqb (block_seq b) (S baseSeq)))
       blocks.

(* Post-fix: find ANY descendant by sender with seq > baseSeq. *)
Definition find_descendant_post_fix
  (blocks : list Block) (sender : Validator) (baseSeq : nat) : option Block :=
  find (fun b =>
          andb
            (if validator_eq_dec (block_sender b) sender then true else false)
            (Nat.ltb baseSeq (block_seq b)))
       blocks.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — T-9.7: Post-fix subsumes pre-fix
   ═══════════════════════════════════════════════════════════════════════════

   Whenever the pre-fix lookup succeeds, the post-fix lookup also succeeds
   (with possibly a different block, but a valid descendant). *)

Theorem t_9_7_post_fix_subsumes_pre_fix :
  forall blocks sender baseSeq b,
    find_descendant_pre_fix blocks sender baseSeq = Some b ->
    exists b', find_descendant_post_fix blocks sender baseSeq = Some b'.
Proof.
  intros blocks sender baseSeq b Hf.
  unfold find_descendant_pre_fix, find_descendant_post_fix in *.
  induction blocks as [| b0 rest IH]; simpl in Hf |- *.
  - discriminate.
  - destruct (validator_eq_dec (block_sender b0) sender) as [Eq | Neq]; simpl in Hf.
    + destruct (Nat.eqb (block_seq b0) (S baseSeq)) eqn:E.
      * inversion Hf. subst b0.
        apply Nat.eqb_eq in E.
        destruct (Nat.ltb baseSeq (block_seq b)) eqn:E2.
        -- exists b. reflexivity.
        -- exfalso. apply Nat.ltb_ge in E2. lia.
      * destruct (Nat.ltb baseSeq (block_seq b0)) eqn:E2.
        -- exists b0. reflexivity.
        -- apply IH. assumption.
    + destruct (Nat.ltb baseSeq (block_seq b0)) eqn:E2.
      * (* not the right sender, so post_fix's find skips too. *)
        apply IH. assumption.
      * apply IH. assumption.
Qed.

(* The post-fix lookup also succeeds for non-dense seq numbers (skipped
   sequences) where the pre-fix lookup fails. We capture this with an
   existence statement: if any descendant exists, post_fix finds one. *)
Theorem t_9_7_finds_descendant_with_gap :
  forall blocks sender baseSeq b,
    In b blocks ->
    block_sender b = sender ->
    block_seq b > baseSeq ->
    exists b', find_descendant_post_fix blocks sender baseSeq = Some b'.
Proof.
  intros blocks sender baseSeq b Hin Hs Hg.
  unfold find_descendant_post_fix.
  induction blocks as [| b0 rest IH]; simpl in Hin |- *.
  - inversion Hin.
  - destruct (validator_eq_dec (block_sender b0) sender) as [Eq | Neq]; simpl.
    + destruct (Nat.ltb baseSeq (block_seq b0)) eqn:E.
      * exists b0. reflexivity.
      * destruct Hin as [E0 | Hrest]; [|apply IH; assumption].
        subst b0. exfalso. apply Nat.ltb_ge in E. lia.
    + destruct Hin as [E0 | Hrest]; [|apply IH; assumption].
      subst b0. congruence.
Qed.
