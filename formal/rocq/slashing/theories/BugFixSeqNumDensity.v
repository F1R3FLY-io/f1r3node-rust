(* ═══════════════════════════════════════════════════════════════════════════
   BugFixSeqNumDensity.v — Proof for Bug Fix #7 (T-9.7)

   Bug. equivocation_detector.rs:400 (Scala counterpart at
   EquivocationDetector.scala:336) uses baseSeqNum + 1 to find a
   validator's child block, assuming per-sender seq numbers are dense.
   If a validator skips a sequence number, the exact lookup fails.

   Fix. Replace baseSeqNum + 1 with a canonical walk over the visible
   creator/self-justification chain.  The returned child is the oldest visible
   block on that branch whose seq number is still strictly greater than
   baseSeq.  This detects skipped seq numbers without counting two blocks on
   the same branch as two distinct equivocation children.

   Theorem T-9.7 (SeqNum density). Under the canonical self-chain walk,
   equivocation detection succeeds even when per-sender seq numbers are
   non-dense, while preserving dense-sequence behavior.

   Companion doc: slashing-verification.md §9.7.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
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

(* Post-fix search predicate: a same-sender block strictly above baseSeq. *)
Definition descendant_candidate
  (sender : Validator) (baseSeq : nat) (b : Block) : bool :=
  andb
    (if validator_eq_dec (block_sender b) sender then true else false)
    (Nat.ltb baseSeq (block_seq b)).

(* Legacy abstraction retained for subsumption: find any descendant by sender
   with seq > baseSeq.  The production authority is the canonical self-chain
   definition below. *)
Definition find_descendant_post_fix
  (blocks : list Block) (sender : Validator) (baseSeq : nat) : option Block :=
  find (descendant_candidate sender baseSeq) blocks.

(* Canonical post-fix abstraction: [chain] is ordered from the viewed latest
   block toward older self-justifications.  The result is the oldest contiguous
   same-sender block above [baseSeq]. *)
Fixpoint canonical_child_post_fix
  (chain : list Block) (sender : Validator) (baseSeq : nat) : option Block :=
  match chain with
  | [] => None
  | b :: rest =>
      if descendant_candidate sender baseSeq b then
        match canonical_child_post_fix rest sender baseSeq with
        | Some c => Some c
        | None => Some b
        end
      else None
  end.

Definition canonical_cache_consistent
  (chain : list Block) (sender : Validator) (baseSeq : nat)
  (cached : option Block) : Prop :=
  match cached with
  | Some b => canonical_child_post_fix chain sender baseSeq = Some b
  | None => True
  end.

Definition canonical_child_memoized
  (cached : option Block) (chain : list Block)
  (sender : Validator) (baseSeq : nat) : option Block :=
  match cached with
  | Some b => Some b
  | None => canonical_child_post_fix chain sender baseSeq
  end.

Definition canonical_candidate_prop
  (sender : Validator) (baseSeq : nat) (b : Block) : Prop :=
  block_sender b = sender /\ block_seq b > baseSeq.

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
	        -- exists b. unfold descendant_candidate.
	           destruct (validator_eq_dec (block_sender b) sender) as [_ | Hneq];
	             [rewrite E2; reflexivity|contradiction].
	        -- exfalso. apply Nat.ltb_ge in E2. lia.
	      * destruct (Nat.ltb baseSeq (block_seq b0)) eqn:E2.
	        -- exists b0. unfold descendant_candidate.
	           destruct (validator_eq_dec (block_sender b0) sender) as [_ | Hneq];
	             [rewrite E2; reflexivity|contradiction].
	        -- unfold descendant_candidate.
	           destruct (validator_eq_dec (block_sender b0) sender) as [_ | Hneq];
	             [rewrite E2; apply IH; assumption|contradiction].
	    + destruct (Nat.ltb baseSeq (block_seq b0)) eqn:E2.
	      * unfold descendant_candidate.
	        destruct (validator_eq_dec (block_sender b0) sender) as [Heq | _];
	          [contradiction|apply IH; assumption].
	      * unfold descendant_candidate.
	        destruct (validator_eq_dec (block_sender b0) sender) as [Heq | _];
	          [contradiction|apply IH; assumption].
Qed.

Theorem t_9_7_canonical_dense_subsumes_pre_fix :
  forall b sender baseSeq,
    block_sender b = sender ->
    block_seq b = S baseSeq ->
    canonical_child_post_fix [b] sender baseSeq = Some b.
Proof.
  intros b sender baseSeq Hs Hseq.
  simpl. unfold descendant_candidate.
  destruct (validator_eq_dec (block_sender b) sender) as [_ | Hneq].
  - rewrite Hseq.
    assert (Hlt : baseSeq < S baseSeq) by lia.
    apply Nat.ltb_lt in Hlt. rewrite Hlt. reflexivity.
  - contradiction.
Qed.

Theorem t_9_7_canonical_child_sound :
  forall chain sender baseSeq b,
    canonical_child_post_fix chain sender baseSeq = Some b ->
    In b chain /\ block_sender b = sender /\ block_seq b > baseSeq.
Proof.
  induction chain as [| b0 rest IH]; intros sender baseSeq b Hcanon.
  - discriminate.
  - simpl in Hcanon.
    destruct (descendant_candidate sender baseSeq b0) eqn:Hcand; [|discriminate].
    destruct (canonical_child_post_fix rest sender baseSeq) as [c |] eqn:Hrest.
    + inversion Hcanon. subst c.
      apply IH in Hrest as [Hin [Hs Hseq]].
      split; [right; assumption|split; assumption].
    + inversion Hcanon. subst b0.
      unfold descendant_candidate in Hcand.
      apply andb_true_iff in Hcand. destruct Hcand as [Hsender Hseq].
      destruct (validator_eq_dec (block_sender b) sender) as [Hs | Hneq].
      * split; [left; reflexivity|].
        apply Nat.ltb_lt in Hseq. split; assumption.
      * discriminate.
Qed.

Theorem t_9_7_canonical_finds_visible_descendant_with_gap :
  forall chain sender baseSeq b,
    In b chain ->
    Forall (canonical_candidate_prop sender baseSeq) chain ->
    exists c, canonical_child_post_fix chain sender baseSeq = Some c.
Proof.
  induction chain as [| b0 rest IH]; intros sender baseSeq b Hin Hall.
  - inversion Hin.
  - inversion Hall as [| x xs Hhead Htail]. subst x xs.
    simpl. unfold descendant_candidate.
    destruct Hhead as [Hs Hseq].
    destruct (validator_eq_dec (block_sender b0) sender) as [_ | Hneq].
    + apply Nat.ltb_lt in Hseq. rewrite Hseq.
      destruct (canonical_child_post_fix rest sender baseSeq) as [c |] eqn:Hrest.
      * exists c. reflexivity.
      * exists b0. reflexivity.
    + contradiction.
Qed.

Theorem t_9_7_canonical_prefix_stability :
  forall prefix chain sender baseSeq b,
    Forall (canonical_candidate_prop sender baseSeq) prefix ->
    canonical_child_post_fix chain sender baseSeq = Some b ->
    canonical_child_post_fix (prefix ++ chain) sender baseSeq = Some b.
Proof.
  induction prefix as [| p rest IHp]; intros chain sender baseSeq b Hall Hcanon.
  - simpl. assumption.
  - inversion Hall as [| x xs Hhead Htail]. subst x xs.
    simpl. unfold descendant_candidate.
    destruct Hhead as [Hs Hseq].
    destruct (validator_eq_dec (block_sender p) sender) as [_ | Hneq].
    + apply Nat.ltb_lt in Hseq. rewrite Hseq.
      rewrite (IHp chain sender baseSeq b Htail Hcanon). reflexivity.
    + contradiction.
Qed.

Theorem t_9_7_canonical_same_branch_unique :
  forall chain sender baseSeq b1 b2,
    canonical_child_post_fix chain sender baseSeq = Some b1 ->
    canonical_child_post_fix chain sender baseSeq = Some b2 ->
    block_hash b1 = block_hash b2.
Proof.
  intros chain sender baseSeq b1 b2 H1 H2.
  rewrite H1 in H2. inversion H2. reflexivity.
Qed.

Theorem t_9_7_canonical_memoized_equivalent :
  forall chain sender baseSeq cached,
    canonical_cache_consistent chain sender baseSeq cached ->
    canonical_child_memoized cached chain sender baseSeq =
    canonical_child_post_fix chain sender baseSeq.
Proof.
  intros chain sender baseSeq cached Hconsistent.
  destruct cached as [b |].
  - simpl in *. symmetry. assumption.
  - reflexivity.
Qed.

(* The legacy lookup also succeeds for non-dense seq numbers (skipped
   sequences) where the pre-fix lookup fails. This theorem is retained as a
   compatibility lemma; the canonical self-chain theorems above are the
   production proof obligations. *)
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
      * exists b0. unfold descendant_candidate.
        destruct (validator_eq_dec (block_sender b0) sender) as [_ | Hneq];
          [rewrite E; reflexivity|contradiction].
      * unfold descendant_candidate.
        destruct (validator_eq_dec (block_sender b0) sender) as [_ | Hneq];
          [rewrite E|contradiction].
        destruct Hin as [E0 | Hrest]; [|apply IH; assumption].
        subst b0. exfalso. apply Nat.ltb_ge in E. lia.
    + unfold descendant_candidate.
      destruct (validator_eq_dec (block_sender b0) sender) as [Heq | _];
        [contradiction|].
      destruct Hin as [E0 | Hrest]; [|apply IH; assumption].
      subst b0. congruence.
Qed.
