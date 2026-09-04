(* ═══════════════════════════════════════════════════════════════════════════
   InvalidBlock.v — The InvalidBlock taxonomy and is_slashable predicate

   Mirrors the 29-variant Rust enum at
     casper/src/rust/block_status.rs:31-74
   and the parallel Scala enum at
     coop/rchain/casper/BlockStatus.scala (case classes extending InvalidBlock).

   Proves: T-3 (slashable taxonomy correctness) — is_slashable returns TRUE
   exactly on the two objective-equivocation variants.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Constructor             │ Rust Variant                  │ Slashable?
   ─────────────────────────────┼───────────────────────────────┼──────────
   IBAdmissibleEquivocation     │ AdmissibleEquivocation        │ yes
   IBIgnorableEquivocation      │ IgnorableEquivocation         │ yes (post-fix #1)
   IBNeglectedEquivocation      │ NeglectedEquivocation         │ no
   IBNeglectedInvalidBlock      │ NeglectedInvalidBlock         │ no
   IBJustificationRegression    │ JustificationRegression       │ no
   IBInvalidParents             │ InvalidParents                │ no
   IBInvalidFollows             │ InvalidFollows                │ no
   IBInvalidBlockNumber         │ InvalidBlockNumber            │ no
   IBInvalidSequenceNumber      │ InvalidSequenceNumber         │ no
   IBInvalidShardId             │ InvalidShardId                │ no
   IBInvalidRepeatDeploy        │ InvalidRepeatDeploy           │ no
   IBDeployNotSigned            │ DeployNotSigned               │ no
   IBInvalidTransaction         │ InvalidTransaction            │ no
   IBInvalidBondsCache          │ InvalidBondsCache             │ no
   IBInvalidEquivocationEvidence│ InvalidEquivocationEvidence   │ no
   IBInvalidBlockHash           │ InvalidBlockHash              │ no
   IBUnauthorizedSlashDeploy    │ UnauthorizedSlashDeploy       │ no
   IBInvalidRejectedDeploy      │ InvalidRejectedDeploy         │ no
   IBPrematureDeployRetry       │ PrematureDeployRetry          │ no
   IBContainsExpiredDeploy      │ ContainsExpiredDeploy         │ no
   IBContainsTimeExpiredDeploy  │ ContainsTimeExpiredDeploy     │ no
   IBContainsFutureDeploy       │ ContainsFutureDeploy          │ no
   IBInvalidFormat              │ InvalidFormat                 │ no
   IBInvalidSignature           │ InvalidSignature              │ no
   IBInvalidSender              │ InvalidSender                 │ no
   IBInvalidVersion             │ InvalidVersion                │ no
   IBInvalidTimestamp           │ InvalidTimestamp              │ no
   IBNotOfInterest              │ NotOfInterest                 │ no
   IBLowDeployCost              │ LowDeployCost                 │ no
   ─────────────────────────────────────────────────────────────────────────
   Cardinality: 29 variants total, 2 slashable, 27 non-slashable.

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
  | IBInvalidEquivocationEvidence : InvalidBlock
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
  | IBPrematureDeployRetry       : InvalidBlock
  | IBNotOfInterest              : InvalidBlock
  | IBLowDeployCost              : InvalidBlock.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Pre-fix is_slashable (historical dev/Scala behavior)
   ═══════════════════════════════════════════════════════════════════════════

   This models the historical 17-element slashable set (pre bug fix #1).
   IgnorableEquivocation is intentionally non-slashable here. The current Rust
   predicate narrows economic evidence to the two objective-equivocation
   variants. The historical predicate remains for proof comparisons only. *)

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

   The two-element slashable set mirrors the exhaustive
   `InvalidBlock::is_slashable` match. Only two signed blocks from one validator
   incarnation at one sequence number establish delivery-order-independent
   economic evidence. *)

Definition is_slashable (ib : InvalidBlock) : bool :=
  match ib with
  | IBAdmissibleEquivocation
  | IBIgnorableEquivocation => true
  | _ => false
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — T-3 — Slashable taxonomy correctness
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem slashable_current_exact :
  forall ib,
    is_slashable ib = true <->
    ib = IBAdmissibleEquivocation \/ ib = IBIgnorableEquivocation.
Proof.
  intros ib. destruct ib; simpl; split; intro H;
    try solve [left; reflexivity | right; reflexivity | reflexivity | discriminate];
    destruct H as [H | H]; discriminate.
Qed.

Theorem slashable_current_is_historical_or_ignorable :
  forall ib,
    is_slashable ib = true ->
    is_slashable_pre_fix ib = true \/ ib = IBIgnorableEquivocation.
Proof.
  intros ib H. apply slashable_current_exact in H.
  destruct H as [-> | ->]; [left | right]; reflexivity.
Qed.

Theorem ignorable_pre_fix_not_slashable :
  is_slashable_pre_fix IBIgnorableEquivocation = false.
Proof. reflexivity. Qed.

Theorem ignorable_post_fix_slashable :
  is_slashable IBIgnorableEquivocation = true.
Proof. reflexivity. Qed.

Theorem unauthorized_pre_fix_not_slashable :
  is_slashable_pre_fix IBUnauthorizedSlashDeploy = false.
Proof. reflexivity. Qed.

Theorem unauthorized_current_not_slashable :
  is_slashable IBUnauthorizedSlashDeploy = false.
Proof. reflexivity. Qed.

Theorem invalid_sequence_current_not_slashable :
  is_slashable IBInvalidSequenceNumber = false.
Proof. reflexivity. Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — Decidable equality for InvalidBlock
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition invalid_block_eq_dec :
  forall (ib1 ib2 : InvalidBlock), {ib1 = ib2} + {ib1 <> ib2}.
Proof. decide equality. Defined.
