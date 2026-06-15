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

From Slashing Require Import Validator InvalidBlock EquivocationRecord.

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
