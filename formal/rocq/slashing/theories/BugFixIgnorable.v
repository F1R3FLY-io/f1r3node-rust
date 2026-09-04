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

   T-9.1: every current slashable variant is one of the two objective
   equivocation variants. View-relative and replay-relative rejection reasons
   remain invalid without becoming economic evidence. *)

Theorem bug_fix_ignorable_safety :
  forall ib,
    is_slashable ib = true ->
    ib = IBAdmissibleEquivocation \/ ib = IBIgnorableEquivocation.
Proof.
  intros ib H. apply slashable_current_exact. exact H.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — IgnorableEquivocation only fires on real (pointer) equivocations
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem ignorable_only_on_real_equivocation :
  forall cj lm d,
    detect cj lm d = DSIgnorable ->
    equivocates_ptr cj lm = true.
Proof.
  intros cj lm d Hd.
  apply (@detection_sound cj lm d DSIgnorable Hd).
  right. reflexivity.
Qed.

(* Combined: if the post-fix dispatcher slashes on IgnorableEquivocation,
   the arriving block's creator justification really disagreed with the
   sender's latest message (a genuine pointer equivocation). *)
Theorem post_fix_ignorable_implies_equivocation :
  forall cj lm d,
    detect cj lm d = DSIgnorable ->
    is_slashable IBIgnorableEquivocation = true /\ equivocates_ptr cj lm = true.
Proof.
  intros cj lm d Hd.
  split.
  - apply ignorable_post_fix_slashable.
  - apply (ignorable_only_on_real_equivocation _ _ _ Hd).
Qed.
