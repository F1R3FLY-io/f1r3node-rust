(* ═══════════════════════════════════════════════════════════════════════════
   BugFixSelfRegression.v — Proofs for Bug Fixes #6 (T-9.6) and #9 (T-9.9)

   Bug #6. validate.rs:875-985 ignores regression of the block's own
   sender (deferring to check_equivocations, which only compares creator-
   justification HASH, not sequence-number ordering). A non-equivocating
   but seq-regressed self-justification slips through.

   Fix #6. Add an explicit seq-number order check for the block's own
   sender in justification_regressions.

   Theorem T-9.6 (Self-regression soundness). Under the fix, every
   self-regression is caught.

   Bug #9. Scala Validate.scala:727-732 rejects self-correcting blocks
   (those that include a SlashDeploy targeting their neglected
   justification's validator). Rust's validate.rs:1016-1029 already
   includes the post-fix branch.

   Theorem T-9.9 (Self-correcting block admit). Under the Rust widening,
   a block is rejected as NeglectedInvalidBlock iff it has a neglected
   justification AND does NOT carry a slash deploy for that offender.

   Companion doc: slashing-verification.md §9.6, §9.9.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block InvalidBlock DAGState.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Bug #6: Self-regression check
   ═══════════════════════════════════════════════════════════════════════════ *)

(* Post-fix check: if the block's own sender's previous seq number
   exceeds the seq number cited in the self-justification, that's a
   regression — even without an equivocation. *)

Definition has_self_regression
  (block_seqNum : nat)
  (latest_self_seq : nat)        (* per-sender oracle *)
  (cited_self_seq  : nat)        (* the seq number cited in self-justification *)
  : bool :=
  Nat.ltb cited_self_seq latest_self_seq.

(* T-9.6: the post-fix predicate detects every regression. *)
Theorem t_9_6_self_regression_detected :
  forall blk_sn latest cited,
    cited < latest ->
    has_self_regression blk_sn latest cited = true.
Proof.
  intros. unfold has_self_regression.
  apply Nat.ltb_lt. assumption.
Qed.

Theorem t_9_6_self_regression_complete :
  forall blk_sn latest cited,
    has_self_regression blk_sn latest cited = true ->
    cited < latest.
Proof.
  intros blk_sn latest cited H.
  unfold has_self_regression in H.
  apply Nat.ltb_lt. assumption.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   T-9.6 (DAG-level): the regression check, when applied with [latest]
   computed by the DAG oracle [ds_latest_seq], detects every actual
   sequence-number regression in the block list.

   This closes Gap 9 from the audit: the predicate is now connected to a
   real DAG witness, not just a Boolean tautology. *)

Theorem t_9_6_self_regression_in_dag :
  forall (blocks : list Block) (sender : Validator) (cited : nat) (b : Block),
    In b blocks ->
    block_sender b = sender ->
    block_seq b > cited ->
    has_self_regression 0 (ds_latest_seq blocks sender) cited = true.
Proof.
  intros blocks sender cited b Hin Hs Hg.
  apply t_9_6_self_regression_detected.
  pose proof (@ds_latest_seq_lower_bound blocks sender b Hin Hs) as Hbound.
  lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Bug #9: Self-correcting block admission
   ═══════════════════════════════════════════════════════════════════════════ *)

(* Post-fix: a block with a neglected justification is rejected ONLY if
   it does NOT carry a slash deploy for the offender. *)

Definition rejects_neglected_post_fix
  (has_neglected : bool)
  (has_slash : bool)
  : bool :=
  andb has_neglected (negb has_slash).

(* T-9.9: under the post-fix predicate, the block is rejected iff it has
   a neglected justification AND lacks a corresponding slash deploy. *)
Theorem t_9_9_post_fix_rejection_iff :
  forall hn hs,
    rejects_neglected_post_fix hn hs = true <-> (hn = true /\ hs = false).
Proof.
  intros hn hs. unfold rejects_neglected_post_fix.
  rewrite andb_true_iff. rewrite negb_true_iff. reflexivity.
Qed.

(* The Scala behavior: rejects whenever has_neglected, regardless of slash. *)
Definition rejects_neglected_pre_fix (has_neglected : bool) : bool :=
  has_neglected.

(* T-9.9 corollary: the post-fix admits strictly more blocks (those with
   has_neglected = true /\ has_slash = true). *)
Theorem t_9_9_post_fix_admits_more :
  forall hn hs,
    hn = true -> hs = true ->
    rejects_neglected_pre_fix hn = true
    /\ rejects_neglected_post_fix hn hs = false.
Proof.
  intros hn hs Hn Hs. unfold rejects_neglected_pre_fix, rejects_neglected_post_fix.
  rewrite Hn, Hs. simpl. split; reflexivity.
Qed.
Record HistoryMessage := {
  history_sender : nat;
  history_justifications : nat -> option nat
}.

Definition history_predecessor
    (store : nat -> option HistoryMessage)
    (message : HistoryMessage) : option HistoryMessage :=
  match history_justifications message (history_sender message) with
  | Some hash => store hash
  | None => None
  end.

Record HistorySnapshot := {
  history_store : nat -> option HistoryMessage;
  history_latest : nat -> option nat
}.

Definition snapshot_predecessor (snapshot : HistorySnapshot) message :=
  history_predecessor (history_store snapshot) message.

Theorem history_latest_independence :
  forall store latest1 latest2 message,
    snapshot_predecessor (Build_HistorySnapshot store latest1) message =
    snapshot_predecessor (Build_HistorySnapshot store latest2) message.
Proof. reflexivity. Qed.

Theorem history_signed_predecessor :
  forall store message hash predecessor,
    history_justifications message (history_sender message) = Some hash ->
    store hash = Some predecessor ->
    history_predecessor store message = Some predecessor.
Proof.
  intros store message hash predecessor Hhash Hstore.
  unfold history_predecessor. rewrite Hhash. exact Hstore.
Qed.

Definition history_pair_regresses (pair : nat * (bool * nat)) : bool :=
  let '(new_seq, (invalid, previous_seq)) := pair in
  negb invalid && Nat.ltb new_seq previous_seq.

Definition history_regresses (pairs : list (nat * (bool * nat))) : bool :=
  existsb history_pair_regresses pairs.

Theorem history_regression_exact :
  forall pairs,
    history_regresses pairs = true <->
    exists new_seq previous_seq,
      In (new_seq, (false, previous_seq)) pairs /\ new_seq < previous_seq.
Proof.
  intros pairs. unfold history_regresses. rewrite existsb_exists.
  split.
  - intros [[new_seq [invalid previous_seq]] [Hin Hcheck]].
    unfold history_pair_regresses in Hcheck.
    apply andb_true_iff in Hcheck as [Hvalid Hlt].
    apply negb_true_iff in Hvalid. subst invalid.
    exists new_seq, previous_seq. split; [exact Hin |].
    apply Nat.ltb_lt. exact Hlt.
  - intros [new_seq [previous_seq [Hin Hlt]]].
    exists (new_seq, (false, previous_seq)). split; [exact Hin |].
    unfold history_pair_regresses. simpl. apply Nat.ltb_lt. exact Hlt.
Qed.

Theorem history_invalid_witness_exemption :
  forall new_seq previous_seq,
    history_regresses [(new_seq, (true, previous_seq))] = false.
Proof. reflexivity. Qed.

Print Assumptions history_latest_independence.
Print Assumptions history_signed_predecessor.
Print Assumptions history_regression_exact.
Print Assumptions history_invalid_witness_exemption.
