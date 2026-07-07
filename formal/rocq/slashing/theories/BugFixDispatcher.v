(* ═══════════════════════════════════════════════════════════════════════════
   BugFixDispatcher.v — Proof for Bug Fix #3 (T-9.3)

   Bug. multi_parent_casper_impl.rs:1090-1099 only marks slashable invalid
   blocks invalid; no EquivocationRecord is created and no slash effect
   runs unless a future proposer happens to pick up the offender.

   Fix. Dispatch every is_slashable() = true variant through the same
   record-creation path used by AdmissibleEquivocation.

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

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — Bug fix #1 no-corruption: the minted UnauthorizedSlashDeploy record
        does not spuriously neglect-slash an honest validator
   ═══════════════════════════════════════════════════════════════════════════

   Because IBUnauthorizedSlashDeploy is is_slashable (the 27th variant), the
   dispatcher mints a record for it. That record is `mkEqRec offender baseSeq
   nil` — the empty-witness `EquivocationRecord::new(sender, seq-1, {})`
   (equivocation_detector.rs:280,311 context). We show two facts:
     (1) the dispatcher indeed mints the empty-witness record for it, and
     (2) an honest sender's view over an empty record yields
         EquivocationOblivious — never a spurious NeglectedEquivocation. *)

Definition minted_unauth_record (offender : Validator) (baseSeq : nat) : EqRec :=
  mkEqRec offender baseSeq nil.

Lemma minted_unauth_record_no_witness :
  forall offender baseSeq, er_hashes (minted_unauth_record offender baseSeq) = nil.
Proof. reflexivity. Qed.

Lemma dispatch_unauth_mints_empty_record :
  forall offender baseSeq s,
    has_key s (offender, baseSeq) = false ->
    find_by_key
      (dispatch_post_fix IBUnauthorizedSlashDeploy offender baseSeq s)
      (offender, baseSeq)
    = Some (minted_unauth_record offender baseSeq).
Proof.
  intros offender baseSeq s Hk.
  unfold dispatch_post_fix.
  assert (Hslash : is_slashable IBUnauthorizedSlashDeploy = true) by reflexivity.
  rewrite Hslash.
  unfold minted_unauth_record.
  set (r := mkEqRec offender baseSeq nil).
  assert (Hek : er_key r = (offender, baseSeq)) by reflexivity.
  rewrite <- Hek. rewrite <- Hek in Hk.
  apply find_insert_cond_same_absent. assumption.
Qed.

(* No-corruption capstone: for a bonded (stake > 0) honest sender — whose
   observing view carries no detected-hash marker (the minted record's witness
   set is empty) and at most one distinct canonical child (a single self-chain)
   — the empty UnauthorizedSlashDeploy record resolves to EquivocationOblivious,
   so neglected-equivocation detection is NOT corrupted. *)
Theorem unauth_record_honest_oblivious :
  forall stake xs,
    stake > 0 ->
    detected_hash_seen xs = false ->
    Nat.leb 2 (length (nodup Nat.eq_dec (child_hashes xs))) = false ->
    discovery_status_bonded stake (fixed_detectable_view xs) = EDOblivious.
Proof.
  intros stake xs Hstake H1 H2.
  apply honest_empty_record_oblivious; assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — BlockException → InvalidTransaction coercion (block_processor.rs)
   ═══════════════════════════════════════════════════════════════════════════

   block_processor.rs:357-376 coerces a receiver-local `BlockException` (a
   runtime error raised while the RECEIVER validates the block) into the
   SLASHABLE variant `IBInvalidTransaction`, then dispatches it against the
   block's SENDER through the same record-creation path as a genuine invalid
   block. We model the coercion target so the taxonomy FV explicitly covers this
   control-flow edge and record that the coerced variant is slashable — hence a
   slash record is minted (T-9.3 dispatch completeness applies).

   HONEST CAVEAT (not proven here): the SOUNDNESS of attributing a
   receiver-local exception to the sender rests on the `BlockException` being
   DETERMINISTIC across nodes. If a `BlockException` can arise from transient
   receiver-local state (storage error, non-deterministic replay), two nodes may
   disagree on whether the sender is slashable — a divergence risk in the same
   family as the LIVE Tier-0 pollution finding (see
   docs/theory/slashing/design/12-failure-modes.md §12.2.1a). The
   dispatch-completeness theorem below does NOT establish that premise. *)

Inductive BlockOutcome : Type :=
  | BOValid     : BlockOutcome
  | BOInvalid   : InvalidBlock -> BlockOutcome
  | BOException : BlockOutcome.

Definition coerce_block_outcome (o : BlockOutcome) : option InvalidBlock :=
  match o with
  | BOValid      => None
  | BOInvalid ib => Some ib
  | BOException  => Some IBInvalidTransaction   (* block_processor.rs:372 *)
  end.

Theorem block_exception_coerces_to_slashable :
  forall ib,
    coerce_block_outcome BOException = Some ib ->
    is_slashable ib = true.
Proof.
  intros ib H. simpl in H. inversion H. reflexivity.
Qed.

(* The coercion feeds the dispatcher: a receiver-local exception mints a record
   against the sender exactly like an admissible equivocation would. *)
Theorem block_exception_dispatch_records :
  forall offender baseSeq s,
    match coerce_block_outcome BOException with
    | Some ib => has_key (dispatch_post_fix ib offender baseSeq s) (offender, baseSeq) = true
    | None    => True
    end.
Proof.
  intros offender baseSeq s. simpl.
  apply t_9_3_dispatch_complete. reflexivity.
Qed.
