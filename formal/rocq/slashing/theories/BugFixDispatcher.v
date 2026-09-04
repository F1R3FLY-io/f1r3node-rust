(* ═══════════════════════════════════════════════════════════════════════════
   BugFixDispatcher.v — Proof for Bug Fix #3 (T-9.3)

   Bug (since fixed). Marking a slashable invalid block invalid without
   minting an EquivocationRecord means no slash effect runs unless a future
   proposer happens to pick up the offender.

   Fix. validation_dispatcher.rs:502-505 (engine/multi_parent_casper) dispatches
   every is_slashable()=true variant through record_evidence, as AdmissibleEquivocation.

   Theorem T-9.3 (Dispatch completeness). Under the fix, every slashable
   invalid block triggers a record in finite steps. (Liveness gap closed.)

   Companion doc: slashing-verification.md §9.3.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator InvalidBlock EquivocationRecord
  Block DAGState EquivocationDetector.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Dispatch function (post-fix)
   ═══════════════════════════════════════════════════════════════════════════

   Given an invalid-block status, the offender, the base sequence number,
   and the current record store, the dispatcher returns the updated store.
   Pre-fix, this function only updates the store for AdmissibleEquivocation;
   post-fix, it updates for every is_slashable variant. *)

Definition dispatch_post_fix
  (ib : InvalidBlock) (offender : Validator) (baseSeq : nat) (s : EqStore)
  : EqStore :=
  if is_slashable ib
  then insert_cond s (mkEqRec offender baseSeq nil)
  else s.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — T-9.3: Completeness
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem t_9_3_dispatch_complete :
  forall ib offender baseSeq s,
    is_slashable ib = true ->
    has_key (dispatch_post_fix ib offender baseSeq s) (offender, baseSeq) = true.
Proof.
  intros ib offender baseSeq s Hslash.
  unfold dispatch_post_fix. rewrite Hslash.
  set (r := mkEqRec offender baseSeq nil).
  assert (Hek : er_key r = (offender, baseSeq)) by reflexivity.
  destruct (has_key s (er_key r)) eqn:Eold.
  - rewrite (insert_cond_dup_noop _ _ Eold).
    unfold has_key in Eold |- *.
    rewrite Hek in Eold. assumption.
  - assert (Hf : find_by_key (insert_cond s r) (er_key r) = Some r)
      by (apply find_insert_cond_same_absent; assumption).
    rewrite Hek in Hf.
    unfold has_key. rewrite Hf. reflexivity.
Qed.

(* The non-slashable case: no record is created. *)
Theorem t_9_3_dispatch_noop_unslashable :
  forall ib offender baseSeq s,
    is_slashable ib = false ->
    dispatch_post_fix ib offender baseSeq s = s.
Proof.
  intros. unfold dispatch_post_fix. rewrite H. reflexivity.
Qed.

Record CertifiedRejectionProjection : Type := mkCertifiedRejectionProjection {
  rejection_persisted : bool;
  rejection_buffered : bool;
  rejection_evidence : EqStore
}.

Definition dispatch_certified_rejection
  (ib : InvalidBlock) (offender : Validator) (baseSeq : nat)
  (s : EqStore) : CertifiedRejectionProjection :=
  mkCertifiedRejectionProjection true false
    (dispatch_post_fix ib offender baseSeq s).

Theorem certified_objective_rejection_persists :
  forall ib offender baseSeq s,
    rejection_persisted
      (dispatch_certified_rejection ib offender baseSeq s) = true.
Proof. reflexivity. Qed.

Theorem certified_objective_rejection_leaves_buffer :
  forall ib offender baseSeq s,
    rejection_buffered
      (dispatch_certified_rejection ib offender baseSeq s) = false.
Proof. reflexivity. Qed.

Theorem every_certified_rejection_is_terminal :
  forall ib offender baseSeq s,
    rejection_persisted
      (dispatch_certified_rejection ib offender baseSeq s) = true
    /\ rejection_buffered
      (dispatch_certified_rejection ib offender baseSeq s) = false.
Proof. intros. split; reflexivity. Qed.

Theorem certified_non_slashable_rejection_preserves_evidence :
  forall ib offender baseSeq s,
    is_slashable ib = false ->
    rejection_evidence
      (dispatch_certified_rejection ib offender baseSeq s) = s.
Proof.
  intros ib offender baseSeq s H.
  unfold dispatch_certified_rejection. simpl.
  apply t_9_3_dispatch_noop_unslashable. exact H.
Qed.

Inductive DependencyDisposition : Type :=
  | DependencyAbsent
  | DependencyAccepted
  | DependencyRejected.

Definition dependency_ready (disposition : DependencyDisposition) : bool :=
  match disposition with
  | DependencyAbsent => false
  | DependencyAccepted | DependencyRejected => true
  end.

Definition child_can_be_accepted (disposition : DependencyDisposition) : bool :=
  match disposition with
  | DependencyAccepted => true
  | DependencyAbsent | DependencyRejected => false
  end.

Theorem persisted_rejection_is_ready_but_not_accepted :
  dependency_ready DependencyRejected = true /\
  child_can_be_accepted DependencyRejected = false.
Proof. split; reflexivity. Qed.

Theorem invalid_sequence_persists_without_evidence :
  forall offender baseSeq s,
    rejection_persisted
      (dispatch_certified_rejection IBInvalidSequenceNumber offender baseSeq s) = true
    /\ rejection_buffered
      (dispatch_certified_rejection IBInvalidSequenceNumber offender baseSeq s) = false
    /\ rejection_evidence
      (dispatch_certified_rejection IBInvalidSequenceNumber offender baseSeq s) = s.
Proof.
  intros. repeat split; reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — Objective invalidity / local-fault separation (block_processor.rs)
   ═══════════════════════════════════════════════════════════════════════════ *)

Inductive BlockOutcome : Type :=
  | BOValid     : BlockOutcome
  | BOInvalid   : InvalidBlock -> BlockOutcome
  | BOException : BlockOutcome.

Definition classify_block_outcome (o : BlockOutcome) : option InvalidBlock :=
  match o with
  | BOValid      => None
  | BOInvalid ib => Some ib
  | BOException  => None
  end.

Theorem block_exception_is_not_objective_invalidity :
  classify_block_outcome BOException = None.
Proof. reflexivity. Qed.

Theorem explicit_slashable_invalidity_dispatches :
  forall ib offender baseSeq s,
    is_slashable ib = true ->
    match classify_block_outcome (BOInvalid ib) with
    | Some classified =>
        has_key (dispatch_post_fix classified offender baseSeq s) (offender, baseSeq) = true
    | None => False
    end.
Proof.
  intros ib offender baseSeq s Hslash. simpl.
  apply t_9_3_dispatch_complete. exact Hslash.
Qed.
