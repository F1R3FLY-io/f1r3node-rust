(* ═══════════════════════════════════════════════════════════════════════════
   BugFixIgnorable.v — Proof for Bug Fix #1 (T-9.1)

   Bug. block_status.rs:36-39 carries the TODO:
     "Make IgnorableEquivocation slashable again ... will become a DOS
      vector if not fixed."
   Pre-fix, IgnorableEquivocation is non-slashable (silently dropped).

   Fix. Add IgnorableEquivocation to is_slashable; in handle_invalid_block,
   treat it identically to AdmissibleEquivocation (record evidence).

   Theorem T-9.1. Under the fix, no honest validator is wrongly slashed —
   every validator that gets a slash record really did equivocate.

   Companion doc: slashing-verification.md §9.1.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Slashing Require Import InvalidBlock EquivocationDetector DAGState.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — The fix is conservative (does not introduce false positives)
   ═══════════════════════════════════════════════════════════════════════════

   T-9.1: every InvalidBlock variant that becomes slashable under the fix
   was either (a) already slashable pre-fix, or (b) is IgnorableEquivocation,
   which is only emitted by detect when the underlying equivocation is real
   (by Theorem detection_sound, T-1). Hence no honest validator is wrongly
   slashed. *)

Theorem bug_fix_ignorable_safety :
  forall ib,
    is_slashable ib = true ->
    is_slashable_pre_fix ib = true \/ ib = IBIgnorableEquivocation.
Proof.
  intros ib H. destruct ib; simpl in H; try discriminate.
  all: (left; reflexivity) || (right; reflexivity).
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — IgnorableEquivocation only fires on real equivocations
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem ignorable_only_on_real_equivocation :
  forall st v n d,
    detect st v n d = DSIgnorable ->
    equivocates st v n.
Proof.
  intros st v n d Hd.
  apply (@detection_sound st v n d DSIgnorable Hd).
  right. reflexivity.
Qed.

(* Combined: if the post-fix dispatcher slashes on IgnorableEquivocation,
   the validator did equivocate. *)
Theorem post_fix_ignorable_implies_equivocation :
  forall st v n d,
    detect st v n d = DSIgnorable ->
    is_slashable IBIgnorableEquivocation = true /\ equivocates st v n.
Proof.
  intros st v n d Hd.
  split.
  - apply ignorable_post_fix_slashable.
  - apply (ignorable_only_on_real_equivocation _ _ _ _ Hd).
Qed.
