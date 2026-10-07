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

   T-9.1 (honest restatement): every InvalidBlock variant that becomes slashable
   under the real current predicate was either
     (a) already slashable in the historical pre-fix taxonomy, or
     (b) IgnorableEquivocation — only emitted by `detect` when the arriving
         block's creator-justification pointer really disagrees with the
         sender's latest message (by Theorem detection_sound, T-1), or
     (c) UnauthorizedSlashDeploy — the 27th Rust variant, raised only when a
         block carries a Slash system deploy that fails the §9.8/§9.13
         authorization predicate (attributable Byzantine behaviour by the
         block's own sender; see BugFixSlashAuthorization / BugFixDispatcher).
   In every case the slash is attributable, so no honest validator is wrongly
   slashed.

   NOTE: the disjunct for UnauthorizedSlashDeploy is FORCED by fix #1 — adding
   the 27th slashable variant makes the pre-fix∨ignorable statement false
   (UnauthorizedSlashDeploy is slashable yet neither pre-fix-slashable nor
   equal to IBIgnorableEquivocation). The no-corruption argument for the
   empty record minted on this branch is `unauth_record_honest_oblivious`
   in BugFixDispatcher.v. *)

Theorem bug_fix_ignorable_safety :
  forall ib,
    is_slashable ib = true ->
    is_slashable_pre_fix ib = true
    \/ ib = IBIgnorableEquivocation
    \/ ib = IBUnauthorizedSlashDeploy.
Proof.
  intros ib H. destruct ib; simpl in H; try discriminate;
    solve [ left; reflexivity
          | right; left; reflexivity
          | right; right; reflexivity ].
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
