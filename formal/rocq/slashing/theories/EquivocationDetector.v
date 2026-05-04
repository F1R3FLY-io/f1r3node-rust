(* ═══════════════════════════════════════════════════════════════════════════
   EquivocationDetector.v — Detection semantics and soundness/completeness

   Models the detection function of f1r3node's EquivocationDetector at
     casper/src/rust/equivocation_detector.rs:24-104
   and the Scala counterpart
     coop/rchain/casper/EquivocationDetector.scala:25-100

   Theorems:
     T-1 (detection_sound)    — emitted equivocations are real
     T-2 (detection_complete) — real equivocations get detected

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition         │ Paper / Spec Notation       │ Rust Implementation
   ────────────────────────┼─────────────────────────────┼─────────────────────
   detect                  │ detect(S, b)                │ check_equivocations
   DetectorStatus          │ ib ∈ InvalidBlock taxonomy  │ BlockStatus value
   DSValid                 │ Valid                        │ Right(ValidBlock::Valid)
   DSAdmissible            │ AdmissibleEquivocation       │ Left(InvalidBlock::AdmissibleEquivocation)
   DSIgnorable             │ IgnorableEquivocation        │ Left(InvalidBlock::IgnorableEquivocation)
   ─────────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §4.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator Block InvalidBlock EquivocationRecord DAGState.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — DetectorStatus
   ═══════════════════════════════════════════════════════════════════════════ *)

Inductive DetectorStatus : Type :=
  | DSValid       : DetectorStatus
  | DSAdmissible  : DetectorStatus
  | DSIgnorable   : DetectorStatus
  | DSNeglected   : DetectorStatus.

(* Map a DetectorStatus to its corresponding InvalidBlock variant when
   the status is non-valid. *)
Definition status_to_invalid (st : DetectorStatus) : option InvalidBlock :=
  match st with
  | DSValid       => None
  | DSAdmissible  => Some IBAdmissibleEquivocation
  | DSIgnorable   => Some IBIgnorableEquivocation
  | DSNeglected   => Some IBNeglectedEquivocation
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Detection function
   ═══════════════════════════════════════════════════════════════════════════

   The detector takes a DAG state, the arriving block's sender and seqNum,
   and a flag indicating whether the block has been requested as a
   dependency by some other block in the DAG. It returns one of the four
   detector statuses. *)

Definition detect
  (st : DAGState) (v : Validator) (n : nat) (requestedAsDep : bool)
  : DetectorStatus :=
  if equivocates_b st v n
  then if requestedAsDep then DSAdmissible else DSIgnorable
  else DSValid.

(* The Neglected case is detected separately by checkNeglectedEquivocations
   (mirroring f1r3node's check_neglected_equivocations_with_update). It
   takes a record-set witness for the prior equivocation. *)
Definition detect_neglected
  (st : DAGState) (v : Validator) (n : nat) (requestedAsDep : bool)
  (records : EqStore)
  : DetectorStatus :=
  if andb requestedAsDep (has_key records (v, pred n))
  then DSNeglected
  else DSValid.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — T-1: Detection soundness
   ═══════════════════════════════════════════════════════════════════════════

   If detect emits Admissible or Ignorable, the DAG witnesses a real
   equivocation at (v, n). *)

Theorem detection_sound :
  forall st v n d s,
    detect st v n d = s ->
    s = DSAdmissible \/ s = DSIgnorable ->
    equivocates st v n.
Proof.
  intros st v n d s Hd Hs.
  unfold detect in Hd.
  destruct (equivocates_b st v n) eqn:E.
  - unfold equivocates. exact E.
  - destruct d; subst s; destruct Hs as [Hs | Hs]; discriminate.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — T-2: Detection completeness
   ═══════════════════════════════════════════════════════════════════════════

   If equivocates(st, v, n), then detect emits Admissible or Ignorable
   (depending on the requestedAsDep flag). *)

Theorem detection_complete :
  forall st v n d,
    equivocates st v n ->
    detect st v n d = DSAdmissible \/ detect st v n d = DSIgnorable.
Proof.
  intros st v n d He.
  unfold detect, equivocates in *.
  rewrite He.
  destruct d; [left; reflexivity | right; reflexivity].
Qed.

(* Stronger version: the detected status matches the dependency flag. *)
Theorem detection_complete_strong :
  forall st v n d,
    equivocates st v n ->
    detect st v n d = (if d then DSAdmissible else DSIgnorable).
Proof.
  intros st v n d He. unfold detect. unfold equivocates in He.
  rewrite He. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — Correspondence: Status to InvalidBlock
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem detect_status_to_invalid_slashable :
  forall st v n d s ib,
    detect st v n d = s ->
    status_to_invalid s = Some ib ->
    is_slashable ib = true.
Proof.
  intros st v n d s ib Hd Hs.
  unfold detect in Hd. unfold status_to_invalid in Hs.
  destruct (equivocates_b st v n).
  - destruct d; subst s; inversion Hs; subst; reflexivity.
  - subst s. discriminate.
Qed.

(* The above theorem confirms T-3 in the detection context: every
   detector-emitted invalid-block status (post-fix) is in the slashable set. *)

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — detect_neglected: soundness and completeness
   ═══════════════════════════════════════════════════════════════════════════

   detect_neglected returns DSNeglected only when the requested-as-dependency
   flag is set AND a record exists for (v, n-1).  We prove this is sound
   (only fires when the conditions hold) and complete (always fires when the
   conditions hold). *)

Theorem detect_neglected_sound :
  forall st v n d records,
    detect_neglected st v n d records = DSNeglected ->
    d = true /\ has_key records (v, pred n) = true.
Proof.
  intros st v n d records H. unfold detect_neglected in H.
  destruct (andb d (has_key records (v, pred n))) eqn:E.
  - apply andb_true_iff in E. destruct E as [Hd Hk]. tauto.
  - discriminate.
Qed.

Theorem detect_neglected_complete :
  forall st v n records,
    has_key records (v, pred n) = true ->
    detect_neglected st v n true records = DSNeglected.
Proof.
  intros st v n records Hk. unfold detect_neglected.
  rewrite Hk. simpl. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §7 — Detector status decidability
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition detector_status_eq_dec :
  forall (s1 s2 : DetectorStatus), {s1 = s2} + {s1 <> s2}.
Proof. decide equality. Defined.
