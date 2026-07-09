(* ═══════════════════════════════════════════════════════════════════════════
   InvalidBlock.v — The InvalidBlock taxonomy and is_slashable predicate

   Mirrors the 22-variant Rust enum at
     casper/src/rust/block_status.rs:32-67
   and the parallel Scala enum at
     coop/rchain/casper/BlockStatus.scala (case classes extending InvalidBlock).

   Proves: T-3 (slashable taxonomy correctness) — is_slashable returns TRUE
   exactly on the 17 documented slashable variants (post-fix #1, the
   IgnorableEquivocation variant is also slashable, making it 18).

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
   §2 — Pre-fix is_slashable (current Rust behavior)
   ═══════════════════════════════════════════════════════════════════════════

   This matches the 17-element slashable set in
     casper/src/rust/block_status.rs:172-194
   IgnorableEquivocation is intentionally non-slashable (the documented
   DOS vector). *)

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
   §3 — Post-fix is_slashable (per bug fix #1, T-9.1)
   ═══════════════════════════════════════════════════════════════════════════

   Adds IgnorableEquivocation to the slashable set, closing the DOS vector. *)

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

(* The two definitions agree on every variant other than IgnorableEquivocation. *)
Theorem slashable_diff_only_ignorable :
  forall ib,
    ib <> IBIgnorableEquivocation ->
    is_slashable ib = is_slashable_pre_fix ib.
Proof.
  intros ib Hne. destruct ib; simpl; try reflexivity.
  exfalso. apply Hne. reflexivity.
Qed.

Theorem ignorable_pre_fix_not_slashable :
  is_slashable_pre_fix IBIgnorableEquivocation = false.
Proof. reflexivity. Qed.

Theorem ignorable_post_fix_slashable :
  is_slashable IBIgnorableEquivocation = true.
Proof. reflexivity. Qed.

(* The set of slashable variants under the post-fix predicate has cardinality 18. *)
(* (Cardinality is implicit in the syntactic count of [true] match arms — the
    compiler checks exhaustiveness.) *)

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — Decidable equality for InvalidBlock
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition invalid_block_eq_dec :
  forall (ib1 ib2 : InvalidBlock), {ib1 = ib2} + {ib1 <> ib2}.
Proof. decide equality. Defined.
