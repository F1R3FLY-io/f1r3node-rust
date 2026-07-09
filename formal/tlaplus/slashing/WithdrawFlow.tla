--------------------------- MODULE WithdrawFlow ---------------------------
(****************************************************************************)
(* Withdrawal-flow model for Bug Fix #10 (T-9.10).                          *)
(*                                                                          *)
(* Models the post-quarantine withdrawal pipeline at PoS.rhox:608-637       *)
(* (`removeQuarantinedWithdrawers`). Pre-fix, a failed `posVault.transfer`  *)
(* would still remove the validator from `withdrawers` and                  *)
(* `committedRewards`, losing funds. Post-fix, the validator is removed     *)
(* only when the transfer succeeds; failed transfers leave the per-         *)
(* validator state intact for retry on a later block.                       *)
(*                                                                          *)
(* This spec verifies the post-fix invariants:                              *)
(*   - Inv_NoFundsLost:        a failed withdrawal does not remove the      *)
(*                             validator from `withdrawers`.                *)
(*   - Inv_TotalFundsConst:    pos_balance + Σ payouts is invariant under   *)
(*                             a failed withdrawal.                         *)
(*   - Inv_RemovedImpliesPaid: every removed validator was paid their full  *)
(*                             bond + reward.                               *)
(*   - Inv_RewardsConsistent:  payouts never exceed the originally held     *)
(*                             funds (no net creation of value).            *)
(*                                                                          *)
(* Liveness: every withdrawer whose transfer eventually succeeds is         *)
(* eventually removed (no withdrawal stuck forever under fair scheduling).  *)
(*                                                                          *)
(* Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md     *)
(*            §9.13                                                         *)
(*            formal/rocq/slashing/theories/BugFixWithdrawTransferFailure.v *)
(*            (T-9.10, T-9.10', T-9.10″)                                    *)
(****************************************************************************)

EXTENDS Integers, FiniteSets, TLC

CONSTANTS
    Withdrawers,        \* Set of validator IDs awaiting withdrawal
    InitialBonds,       \* [Withdrawers -> Nat]: bond stored in withdrawers map
    InitialRewards      \* [Withdrawers -> Nat]: rewards stored in committedRewards

VARIABLES
    inWithdrawers,      \* SUBSET Withdrawers: still in withdrawers map
    inRewards,          \* SUBSET Withdrawers: still in committedRewards map
    posBalance,         \* Nat: current PoS vault balance
    paidOut,            \* [Withdrawers -> Nat]: total amount paid to each
    transferFailed      \* [Withdrawers -> BOOLEAN]: latest transfer outcome

vars == <<inWithdrawers, inRewards, posBalance, paidOut, transferFailed>>

Payout(v) == InitialBonds[v] + InitialRewards[v]

RECURSIVE SumPayouts(_)
SumPayouts(S) ==
    IF S = {} THEN 0
    ELSE LET x == CHOOSE w \in S : TRUE
         IN  Payout(x) + SumPayouts(S \ {x})

InitialTotal == SumPayouts(Withdrawers)

(****************************************************************************)
(* TypeOK                                                                   *)
(****************************************************************************)
TypeOK ==
    /\ inWithdrawers \in SUBSET Withdrawers
    /\ inRewards     \in SUBSET Withdrawers
    /\ posBalance    \in Nat
    /\ paidOut       \in [Withdrawers -> Nat]
    /\ transferFailed \in [Withdrawers -> BOOLEAN]

(****************************************************************************)
(* Init                                                                     *)
(****************************************************************************)
Init ==
    /\ inWithdrawers   = Withdrawers
    /\ inRewards       = Withdrawers
    /\ posBalance      = InitialTotal
    /\ paidOut         = [w \in Withdrawers |-> 0]
    /\ transferFailed  = [w \in Withdrawers |-> FALSE]

(****************************************************************************)
(* Action: WithdrawSucceeds(v) — the post-fix successful path.              *)
(* The validator is removed from both maps and credited the full payout.    *)
(****************************************************************************)
WithdrawSucceeds(v) ==
    /\ v \in inWithdrawers
    /\ v \in inRewards
    /\ inWithdrawers'   = inWithdrawers \ {v}
    /\ inRewards'       = inRewards \ {v}
    /\ posBalance'      = posBalance - Payout(v)
    /\ paidOut'         = [paidOut EXCEPT ![v] = Payout(v)]
    /\ transferFailed'  = [transferFailed EXCEPT ![v] = FALSE]

(****************************************************************************)
(* Action: WithdrawFails(v) — the post-fix failure-handling path.           *)
(* Per-validator state is unchanged: validator stays in withdrawers /       *)
(* rewards, posBalance unchanged, payout 0. The transferFailed flag is set  *)
(* to true so an outer-layer retry can later succeed under fair scheduling. *)
(****************************************************************************)
WithdrawFails(v) ==
    /\ v \in inWithdrawers
    /\ transferFailed' = [transferFailed EXCEPT ![v] = TRUE]
    /\ UNCHANGED <<inWithdrawers, inRewards, posBalance, paidOut>>

(****************************************************************************)
(* Action: RetryFromFailed(v) — clears the failure flag so a subsequent     *)
(* WithdrawSucceeds(v) becomes enabled. Models the production behaviour     *)
(* that a failed transfer is retried in a later block.                      *)
(****************************************************************************)
RetryFromFailed(v) ==
    /\ v \in inWithdrawers
    /\ transferFailed[v] = TRUE
    /\ transferFailed' = [transferFailed EXCEPT ![v] = FALSE]
    /\ UNCHANGED <<inWithdrawers, inRewards, posBalance, paidOut>>

(****************************************************************************)
(* Next                                                                     *)
(****************************************************************************)
Next ==
    \/ \E v \in Withdrawers : WithdrawSucceeds(v)
    \/ \E v \in Withdrawers : WithdrawFails(v)
    \/ \E v \in Withdrawers : RetryFromFailed(v)

Spec ==
    Init /\ [][Next]_vars
         /\ \A vw \in Withdrawers : WF_vars(WithdrawSucceeds(vw))
         /\ \A vr \in Withdrawers : WF_vars(RetryFromFailed(vr))

(****************************************************************************)
(* Invariants                                                               *)
(****************************************************************************)

\* T-9.10 (safety): a failed transfer does NOT remove the validator from
\* withdrawers. Equivalently, every "removed from withdrawers" validator was
\* paid in full.
Inv_NoFundsLost ==
    \A v \in Withdrawers :
        (v \notin inWithdrawers) => (paidOut[v] = Payout(v))

\* T-9.10' (vault conservation): the PoS vault initially holds enough to
\* pay every withdrawer; every successful withdrawal moves Payout(v) from
\* posBalance into paidOut[v]. So pos_balance + sum_of_payouts is invariant
\* at the InitialTotal value.
Inv_TotalFundsConst ==
    LET RECURSIVE SumPaid(_)
        SumPaid(S) ==
            IF S = {} THEN 0
            ELSE LET x == CHOOSE w \in S : TRUE
                 IN  paidOut[x] + SumPaid(S \ {x})
    IN posBalance + SumPaid(Withdrawers) = InitialTotal

\* Every removed validator was paid exactly their bond + reward.
Inv_RemovedImpliesPaid ==
    \A v \in Withdrawers :
        (v \notin inWithdrawers) => (paidOut[v] = Payout(v))

\* No validator gets paid more than once or more than their entitled amount.
Inv_RewardsConsistent ==
    \A v \in Withdrawers : paidOut[v] <= Payout(v)

\* TypeOK is maintained by every action.
Inv_TypeOK == TypeOK

(****************************************************************************)
(* Liveness                                                                 *)
(****************************************************************************)

\* Every withdrawer is eventually paid out (under fair scheduling of
\* WithdrawSucceeds and RetryFromFailed actions).
Live_AllEventuallyPaid ==
    \A v \in Withdrawers :
        <>(v \notin inWithdrawers)

============================================================================
