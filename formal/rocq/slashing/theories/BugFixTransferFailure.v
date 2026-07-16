(* ═══════════════════════════════════════════════════════════════════════════
   BugFixTransferFailure.v — Proof for Bug Fix #4 (T-9.4)

   Bug. PoS.rhox:469 has "FIXME handle transfer failing case". If
   posVault!("transfer", ...) fails, the for(_ <- transferDoneCh)
   continuation never fires; no error path back to returnCh.

   Fix. Add an alternate continuation that returns
   (false, "transfer failed") on returnCh deterministically when the
   transfer fails or times out.

   Theorem T-9.4 (Transfer-failure safety). The slash transition either
   succeeds with T-7/T-8 or returns false in finite time.

   Companion doc: slashing-verification.md §9.4.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Slashing Require Import Validator PoSContract.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Slash with transfer-success oracle
   ═══════════════════════════════════════════════════════════════════════════

   The post-fix slash takes an explicit transfer-success oracle and
   returns the new state plus a Boolean. *)

Definition slash_with_transfer_oracle
  (ps : PoSState) (v : Validator) (transfer_ok : bool) : PoSState * bool :=
  if transfer_ok
  then slash ps v
  else (ps, false).

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — T-9.4: Either succeeds with T-7/T-8 or returns false deterministically
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem t_9_4_transfer_failure_safety :
  forall ps v transfer_ok,
    let result := slash_with_transfer_oracle ps v transfer_ok in
    let ps' := fst result in
    let ok := snd result in
    (ok = true /\ bm_lookup (ps_allBonds ps') v = 0)
    \/ (ok = false /\ ps' = ps).
Proof.
  intros ps v transfer_ok.
  unfold slash_with_transfer_oracle.
  destruct transfer_ok; simpl.
  - left.
    pose proof (slash_zeros_bond ps v) as Hzero.
    unfold slash in *.
    destruct (Nat.eq_dec (bm_lookup (ps_allBonds ps) v) 0) as [E | NE]; simpl in *.
    + split; [reflexivity | assumption].
    + split; [reflexivity | assumption].
  - right. split; reflexivity.
Qed.
