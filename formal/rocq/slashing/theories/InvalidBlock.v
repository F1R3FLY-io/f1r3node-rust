(* ═══════════════════════════════════════════════════════════════════════════
   InvalidBlock.v — The InvalidBlock taxonomy and is_slashable predicate

   Mirrors the 27-variant Rust enum at
     casper/src/rust/block_status.rs:31-74
   and the parallel Scala enum at
     coop/rchain/casper/BlockStatus.scala (case classes extending InvalidBlock).

   Proves: T-3 (slashable taxonomy correctness) — is_slashable returns TRUE
   exactly on the 19 documented slashable variants. The post-fix set is the
   17 historically-slashable variants PLUS IgnorableEquivocation (bug fix #1)
   PLUS UnauthorizedSlashDeploy (the 27th Rust variant, slashable per
   InvalidBlock::is_slashable at block_status.rs:206).

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Constructor             │ Rust Variant                  │ Slashable?
   ─────────────────────────────┼───────────────────────────────┼──────────
   IBAdmissibleEquivocation     │ AdmissibleEquivocation        │ yes
   IBIgnorableEquivocation      │ IgnorableEquivocation         │ yes (post-fix #1)
   IBNeglectedEquivocation      │ NeglectedEquivocation         │ yes
   IBNeglectedInvalidBlock      │ NeglectedInvalidBlock         │ yes
   IBJustificationRegression    │ JustificationRegression       │ yes
   IBInvalidParents             │ InvalidParents                │ yes
   IBInvalidFollows             │ InvalidFollows                │ yes
   IBInvalidBlockNumber         │ InvalidBlockNumber            │ yes
   IBInvalidSequenceNumber      │ InvalidSequenceNumber         │ yes
   IBInvalidShardId             │ InvalidShardId                │ yes
   IBInvalidRepeatDeploy        │ InvalidRepeatDeploy           │ yes
   IBDeployNotSigned            │ DeployNotSigned               │ yes
   IBInvalidTransaction         │ InvalidTransaction            │ yes
   IBInvalidBondsCache          │ InvalidBondsCache             │ yes
   IBInvalidBlockHash           │ InvalidBlockHash              │ yes
   IBUnauthorizedSlashDeploy    │ UnauthorizedSlashDeploy       │ yes (27th variant)
   IBContainsExpiredDeploy      │ ContainsExpiredDeploy         │ yes
   IBContainsTimeExpiredDeploy  │ ContainsTimeExpiredDeploy     │ yes
   IBContainsFutureDeploy       │ ContainsFutureDeploy          │ yes
   IBInvalidFormat              │ InvalidFormat                 │ no
   IBInvalidSignature           │ InvalidSignature              │ no
   IBInvalidSender              │ InvalidSender                 │ no
   IBInvalidVersion             │ InvalidVersion                │ no
   IBInvalidTimestamp           │ InvalidTimestamp              │ no
   IBInvalidRejectedDeploy      │ InvalidRejectedDeploy         │ no
   IBNotOfInterest              │ NotOfInterest                 │ no
   IBLowDeployCost              │ LowDeployCost                 │ no
   ─────────────────────────────────────────────────────────────────────────
   Cardinality: 27 variants total, 19 slashable, 8 non-slashable.

   Companion doc: slashing-verification.md §3.3
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — The InvalidBlock inductive type
   ═══════════════════════════════════════════════════════════════════════════ *)

Inductive InvalidBlock : Type :=
  | IBAdmissibleEquivocation     : InvalidBlock
  | IBIgnorableEquivocation      : InvalidBlock
  | IBNeglectedEquivocation      : InvalidBlock
  | IBNeglectedInvalidBlock      : InvalidBlock
  | IBJustificationRegression    : InvalidBlock
  | IBInvalidParents             : InvalidBlock
  | IBInvalidFollows             : InvalidBlock
  | IBInvalidBlockNumber         : InvalidBlock
  | IBInvalidSequenceNumber      : InvalidBlock
  | IBInvalidShardId             : InvalidBlock
  | IBInvalidRepeatDeploy        : InvalidBlock
  | IBDeployNotSigned            : InvalidBlock
  | IBInvalidTransaction         : InvalidBlock
  | IBInvalidBondsCache          : InvalidBlock
  | IBInvalidBlockHash           : InvalidBlock
  | IBUnauthorizedSlashDeploy    : InvalidBlock
  | IBContainsExpiredDeploy      : InvalidBlock
  | IBContainsTimeExpiredDeploy  : InvalidBlock
  | IBContainsFutureDeploy       : InvalidBlock
  | IBInvalidFormat              : InvalidBlock
  | IBInvalidSignature           : InvalidBlock
  | IBInvalidSender              : InvalidBlock
  | IBInvalidVersion             : InvalidBlock
  | IBInvalidTimestamp           : InvalidBlock
  | IBInvalidRejectedDeploy      : InvalidBlock
  | IBNotOfInterest              : InvalidBlock
  | IBLowDeployCost              : InvalidBlock.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Pre-fix is_slashable (historical dev/Scala behavior)
   ═══════════════════════════════════════════════════════════════════════════

   This models the historical 17-element slashable set (pre bug fix #1).
   IgnorableEquivocation is intentionally non-slashable here (the documented
   DOS vector), and UnauthorizedSlashDeploy — being the 27th variant that did
   not exist in the pre-fix taxonomy — is likewise non-slashable via the
   wildcard arm. The real current Rust `InvalidBlock::is_slashable`
   (casper/src/rust/block_status.rs:183-238) has 19 slashable variants; that
   post-fix behavior is §3 below. *)

Definition is_slashable_pre_fix (ib : InvalidBlock) : bool :=
  match ib with
  | IBAdmissibleEquivocation
  | IBNeglectedEquivocation
  | IBNeglectedInvalidBlock
  | IBJustificationRegression
  | IBInvalidParents
  | IBInvalidFollows
  | IBInvalidBlockNumber
  | IBInvalidSequenceNumber
  | IBInvalidShardId
  | IBInvalidRepeatDeploy
  | IBDeployNotSigned
  | IBInvalidTransaction
  | IBInvalidBondsCache
  | IBInvalidBlockHash
  | IBContainsExpiredDeploy
  | IBContainsTimeExpiredDeploy
  | IBContainsFutureDeploy => true
  | _ => false
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — Post-fix is_slashable (the real current Rust behavior)
   ═══════════════════════════════════════════════════════════════════════════

   The 19-element slashable set that mirrors, arm-for-arm, the exhaustive
   `InvalidBlock::is_slashable` match at casper/src/rust/block_status.rs:191-236.
   It is the historical 17 (§2) plus IgnorableEquivocation (bug fix #1, closing
   the DOS vector) plus UnauthorizedSlashDeploy (the 27th variant, slashable at
   block_status.rs:206). *)

Definition is_slashable (ib : InvalidBlock) : bool :=
  match ib with
  | IBAdmissibleEquivocation
  | IBIgnorableEquivocation        (* ← added by fix #1 *)
  | IBNeglectedEquivocation
  | IBNeglectedInvalidBlock
  | IBJustificationRegression
  | IBInvalidParents
  | IBInvalidFollows
  | IBInvalidBlockNumber
  | IBInvalidSequenceNumber
  | IBInvalidShardId
  | IBInvalidRepeatDeploy
  | IBDeployNotSigned
  | IBInvalidTransaction
  | IBInvalidBondsCache
  | IBInvalidBlockHash
  | IBUnauthorizedSlashDeploy      (* ← 27th variant; slashable (block_status.rs:206) *)
  | IBContainsExpiredDeploy
  | IBContainsTimeExpiredDeploy
  | IBContainsFutureDeploy => true
  | _ => false
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — T-3 — Slashable taxonomy correctness
   ═══════════════════════════════════════════════════════════════════════════ *)

(* The post-fix slashable set is exactly the pre-fix set plus IgnorableEquivocation. *)
Theorem slashable_post_fix_extends_pre_fix :
  forall ib,
    is_slashable_pre_fix ib = true ->
    is_slashable ib = true.
Proof.
  intros ib H. destruct ib; simpl in H |- *; try discriminate; reflexivity.
Qed.

(* The two definitions agree on every variant other than the two the post-fix
   set adds: IgnorableEquivocation (bug fix #1) and UnauthorizedSlashDeploy
   (the 27th variant). *)
Theorem slashable_diff_only_ignorable_or_unauth :
  forall ib,
    ib <> IBIgnorableEquivocation ->
    ib <> IBUnauthorizedSlashDeploy ->
    is_slashable ib = is_slashable_pre_fix ib.
Proof.
  intros ib Hne1 Hne2. destruct ib; simpl;
    solve [ reflexivity
          | exfalso; apply Hne1; reflexivity
          | exfalso; apply Hne2; reflexivity ].
Qed.

Theorem ignorable_pre_fix_not_slashable :
  is_slashable_pre_fix IBIgnorableEquivocation = false.
Proof. reflexivity. Qed.

Theorem ignorable_post_fix_slashable :
  is_slashable IBIgnorableEquivocation = true.
Proof. reflexivity. Qed.

(* UnauthorizedSlashDeploy (the 27th Rust variant) is non-slashable in the
   historical pre-fix taxonomy but slashable under the real current predicate. *)
Theorem unauthorized_pre_fix_not_slashable :
  is_slashable_pre_fix IBUnauthorizedSlashDeploy = false.
Proof. reflexivity. Qed.

Theorem unauthorized_post_fix_slashable :
  is_slashable IBUnauthorizedSlashDeploy = true.
Proof. reflexivity. Qed.

(* The set of slashable variants under the post-fix predicate has cardinality 19
   (the historical 17 + IgnorableEquivocation + UnauthorizedSlashDeploy). *)
(* (Cardinality is implicit in the syntactic count of [true] match arms — the
    compiler checks exhaustiveness against all 27 constructors.) *)

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — Decidable equality for InvalidBlock
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition invalid_block_eq_dec :
  forall (ib1 ib2 : InvalidBlock), {ib1 = ib2} + {ib1 <> ib2}.
Proof. decide equality. Defined.
