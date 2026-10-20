(* G1-7 (DR-65 item 5, design section 8.3): the held-capacity guard of one
   offered member at its settlement.

   For every purse p that the member debits or that carries a later hold,
   the balance at the settlement root of the member must cover the member's
   debit plus the later holds:

     balance_after(p) - debit(p) >= sum of the later holds on p.

   The guard implies the condition under which SystemVault reserves the
   debit, and with one offer per block (no later holds) it is exactly that
   condition. If the guard fails, the member is a charged user failure: it
   rolls back to its execution root and pays its billable work and fee from
   the restored balance. When realized use is at most the estimate, that
   charge fits the restored balance.

   Replay must recompute the guard of a committed rollback. A replay that
   trusts the failure class of the evidence lets a producer roll back a
   successful deploy and still charge it (arbitration M2).

   Three negative controls refute plausible wrong rules: the vault-only
   condition, settlement without the guard, and the trusting replay. The
   amounts are binary naturals, so the example of design section 8.3 computes
   with its real numbers. *)

From Stdlib Require Import NArith Bool.Bool Lists.List Lia.
Import ListNotations.
Open Scope N_scope.

(** * The guard *)

(** One purse in the scope of the guard: its balance at the settlement root
    of the member, the member's debit with the fee included, and the sum of
    the holds of the later members of the block. *)
Record purse := {
  balance_after : N;
  debit : N;
  later_holds : N;
}.

(** The guard at one purse, and over every purse of its scope. *)
Definition guard_at (p : purse) : bool := debit p + later_holds p <=? balance_after p.

Definition guard (purses : list purse) : bool := forallb guard_at purses.

(** The condition under which SystemVault reserves the debit of one purse. *)
Definition vault_at (p : purse) : bool := debit p <=? balance_after p.

Definition vault_condition (purses : list purse) : bool := forallb vault_at purses.

(** The balance of a purse after the debit. *)
Definition remaining (p : purse) : N := balance_after p - debit p.

Lemma guard_true_iff : forall purses,
  guard purses = true <->
  Forall (fun p => debit p + later_holds p <= balance_after p) purses.
Proof.
  intros purses. unfold guard. rewrite forallb_forall, Forall_forall.
  split; intros holds p member; specialize (holds p member).
  - unfold guard_at in holds. apply N.leb_le in holds. exact holds.
  - unfold guard_at. apply N.leb_le. exact holds.
Qed.

(** The guard keeps every later hold covered after the debit. The statement
    has no settlement floor (design section 12): the remaining balance alone
    covers the later holds. *)
Theorem held_capacity_guard_preserves_later_holds : forall purses,
  guard purses = true -> Forall (fun p => later_holds p <= remaining p) purses.
Proof.
  intros purses held. apply guard_true_iff in held.
  eapply Forall_impl; [|exact held].
  intros p fits. cbv beta in *. unfold remaining. lia.
Qed.

(** The guard implies the condition of SystemVault, because the later holds
    are not negative. *)
Theorem guard_implies_vault_reservation : forall purses,
  guard purses = true -> vault_condition purses = true.
Proof.
  intros purses held. apply guard_true_iff in held.
  unfold vault_condition. apply forallb_forall. intros p member.
  rewrite Forall_forall in held. specialize (held p member).
  unfold vault_at. apply N.leb_le. lia.
Qed.

(** With one offer per block there are no later holds, and the guard is
    exactly the condition of SystemVault. *)
Theorem single_member_guard_is_vault_condition : forall purses,
  Forall (fun p => later_holds p = 0) purses -> guard purses = vault_condition purses.
Proof.
  induction purses as [|p rest IH]; intros alone; [reflexivity|].
  apply Forall_cons_iff in alone. destruct alone as [none tail].
  change (guard (p :: rest)) with (guard_at p && guard rest).
  change (vault_condition (p :: rest)) with (vault_at p && vault_condition rest).
  rewrite (IH tail). unfold guard_at, vault_at. rewrite none, N.add_0_r. reflexivity.
Qed.

(** * The rollback charge *)

(** The charge of a run of [units] of billable work at [price], with the
    fee of one atomic unit. *)
Definition charge (units price : N) : N := units * price + 1.

(** The check before execution placed the hold of the estimate within the
    balance that the rollback restores. When realized use is at most the
    estimate, the rollback charge fits that balance. *)
Theorem rollback_charge_fits_restored_balance : forall realized estimate price restored,
  realized <= estimate ->
  charge estimate price <= restored ->
  charge realized price <= restored.
Proof.
  intros realized estimate price restored within held. unfold charge in *.
  pose proof (N.mul_le_mono_r realized estimate price within). lia.
Qed.

(** * Settlement of one member *)

(** The outcome of a settlement: the charge and the final balance of the
    purse, or a rejection that charges nothing. *)
Inductive outcome :=
| Settled (paid final : N)
| RolledBack (paid final : N)
| RejectedFree.

(** Settlement with the guard. [restored] is the balance at the execution
    root, [drained] the balance after the run, [cost] the charge of the run
    and [later] the later holds on the purse. A failing guard rolls the run
    back and charges it from the restored balance. *)
Definition settle_with_guard (restored drained cost later : N) : outcome :=
  if cost + later <=? drained then Settled cost (drained - cost)
  else if cost <=? restored then RolledBack cost (restored - cost)
  else RejectedFree.

(** A rollback charges the run and keeps the later holds covered whenever
    the restored balance covers the charge and the later holds. *)
Theorem rollback_keeps_later_holds : forall restored drained cost later paid final,
  cost + later <= restored ->
  settle_with_guard restored drained cost later = RolledBack paid final ->
  paid = cost /\ later <= final.
Proof.
  intros restored drained cost later paid final covered settled.
  unfold settle_with_guard in settled.
  destruct (cost + later <=? drained); [discriminate|].
  destruct (cost <=? restored); [|discriminate].
  injection settled as paid_cost final_rest. subst. split; [reflexivity | lia].
Qed.

(** A successful settlement keeps the later holds covered. *)
Theorem settled_keeps_later_holds : forall restored drained cost later paid final,
  settle_with_guard restored drained cost later = Settled paid final ->
  paid = cost /\ later <= final.
Proof.
  intros restored drained cost later paid final settled.
  unfold settle_with_guard in settled.
  destruct (cost + later <=? drained) eqn:fits.
  - injection settled as paid_cost final_rest. subst.
    apply N.leb_le in fits. split; [reflexivity | lia].
  - destruct (cost <=? restored); discriminate.
Qed.

(** * Replay of a committed outcome *)

(** The rollback that an honest producer publishes: the run failed, or the
    guard on the success path failed. *)
Definition honest_rollback (run_failed guard_holds : bool) : bool :=
  run_failed || negb guard_holds.

(** The replay of arbitration M2. A committed rollback for a run without a
    failure stands only if the recomputed guard fails. A committed success
    stands only for a run without a failure whose guard holds: for a
    success, the wallet settlement of SystemVault rejects a short purse, and
    with one offer per block the guard is that condition
    ([single_member_guard_is_vault_condition]). *)
Definition replay_accepts (committed_rollback run_failed guard_holds : bool) : bool :=
  if committed_rollback then run_failed || negb guard_holds
  else negb run_failed && guard_holds.

(** The replay accepts exactly the class that the honest producer
    publishes. *)
Theorem replay_accepts_exactly_the_honest_class : forall committed run_failed guard_holds,
  replay_accepts committed run_failed guard_holds = true <->
  committed = honest_rollback run_failed guard_holds.
Proof.
  intros committed run_failed guard_holds.
  unfold replay_accepts, honest_rollback.
  destruct committed, run_failed, guard_holds; simpl; split; intro accepted;
    first [reflexivity | discriminate].
Qed.

(** * Negative controls *)

(** N1: the vault-only condition loses a later hold. The example of design
    section 8.3: a purse of 1,000,000 carries the holds 500,001 and 390,001.
    The code of member 1 pays 600,000 out of the purse and leaves 400,000,
    and its charge is 20,000. SystemVault alone settles the charge, and only
    380,000 remain for the hold of 390,001. The guard rolls member 1 back,
    and 980,000 remain. *)
Definition settle_vault_only (drained cost : N) : outcome :=
  if cost <=? drained then Settled cost (drained - cost) else RejectedFree.

Theorem nc_vault_only_loses_a_later_hold :
  settle_vault_only 400000 20000 = Settled 20000 380000 /\
  380000 < 390001 /\
  vault_condition [{| balance_after := 400000; debit := 20000; later_holds := 390001 |}] = true /\
  guard [{| balance_after := 400000; debit := 20000; later_holds := 390001 |}] = false /\
  settle_with_guard 1000000 400000 20000 390001 = RolledBack 20000 980000 /\
  390001 <= 980000.
Proof.
  split; [reflexivity|]. split; [lia|]. split; [reflexivity|]. split; [reflexivity|].
  split; [reflexivity | lia].
Qed.

(** N2: without the guard, a self-drain runs free. The deploy leaves one
    atomic unit in its purse, and its charge is 20,001. SystemVault alone
    rejects the settlement with no charge, after all the work ran. The guard
    rolls the run back and charges it from the restored balance. *)
Theorem nc_no_guard_lets_a_self_drain_run_free :
  settle_vault_only 1 20001 = RejectedFree /\
  settle_with_guard 1000000 1 20001 0 = RolledBack 20001 979999.
Proof.
  split; reflexivity.
Qed.

(** N3: a replay that trusts the failure class of the evidence. A producer
    commits a rollback for a successful run whose guard holds. The trusting
    replay accepts it. The effects of the run are then dropped, and the run
    is still charged: the purse ends at 980,000 instead of the 979,000 that
    the successful run with its transfer of 1,000 leaves, and the transfer
    is lost. The checked replay rejects the block. *)
Definition replay_trusting (committed_rollback run_failed _guard_holds : bool) : bool :=
  committed_rollback || negb run_failed.

Theorem nc_replay_trusting_the_failure_class :
  replay_trusting true false true = true /\
  replay_accepts true false true = false /\
  honest_rollback false true = false /\
  settle_with_guard 1000000 999000 20000 0 = Settled 20000 979000 /\
  RolledBack 20000 980000 <> Settled 20000 979000.
Proof.
  split; [reflexivity|]. split; [reflexivity|]. split; [reflexivity|].
  split; [reflexivity | discriminate].
Qed.

Print Assumptions guard_true_iff.
Print Assumptions held_capacity_guard_preserves_later_holds.
Print Assumptions guard_implies_vault_reservation.
Print Assumptions single_member_guard_is_vault_condition.
Print Assumptions rollback_charge_fits_restored_balance.
Print Assumptions rollback_keeps_later_holds.
Print Assumptions settled_keeps_later_holds.
Print Assumptions replay_accepts_exactly_the_honest_class.
Print Assumptions nc_vault_only_loses_a_later_hold.
Print Assumptions nc_no_guard_lets_a_self_drain_run_free.
Print Assumptions nc_replay_trusting_the_failure_class.
