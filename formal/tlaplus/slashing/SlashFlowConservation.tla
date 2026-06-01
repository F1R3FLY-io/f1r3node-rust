----------------------- MODULE SlashFlowConservation -----------------------
(****************************************************************************)
(* TLC-only conservation layer for SlashFlow.                               *)
(*                                                                          *)
(* This leaf module `EXTENDS SlashFlow` and carries the recursive set-sum   *)
(* operators (SumInitialBonds / SumBonds / SumQuarantined) and the          *)
(* quarantine-inclusive total-stake conservation invariant                  *)
(* Inv_StakeConservation that uses them. These were moved here VERBATIM     *)
(* from SlashFlow.tla so that SlashFlow.tla stays free of `RECURSIVE`        *)
(* operator definitions, which tlapm 1.5.0 cannot process (it aborts the    *)
(* whole module — and every module that EXTENDS/INSTANCEs it — at           *)
(* level-computation time, before any proof obligation is generated). The   *)
(* deductive TLAPS proof of Inv_RedeemedValidatorUnhalted therefore lives in *)
(* the RECURSIVE-free SlashFlow.tla and is checked by `tlapm SlashFlow.tla`. *)
(*                                                                          *)
(* TLC coverage is unchanged: MC_SlashFlow `EXTENDS SlashFlowConservation`,  *)
(* so Inv_StakeConservation (and every SlashFlow definition, transitively   *)
(* visible) is still model-checked exactly as before.                       *)
(****************************************************************************)

EXTENDS SlashFlow

\* Recursive sum operator over a set, weighted by InitialBonds.
RECURSIVE SumInitialBonds(_)
SumInitialBonds(S) ==
    IF S = {} THEN 0
    ELSE LET v == CHOOSE x \in S : TRUE
         IN  InitialBonds[v] + SumInitialBonds(S \ {v})

\* Recursive sum operator over a set, weighted by current bonds.
RECURSIVE SumBonds(_)
SumBonds(S) ==
    IF S = {} THEN 0
    ELSE LET v == CHOOSE x \in S : TRUE
         IN  bonds[v] + SumBonds(S \ {v})

\* Recursive sum operator over a set, weighted by current quarantinedStake.
RECURSIVE SumQuarantined(_)
SumQuarantined(S) ==
    IF S = {} THEN 0
    ELSE LET v == CHOOSE x \in S : TRUE
         IN  quarantinedStake[v] + SumQuarantined(S \ {v})

\* Total stake conservation (quarantine-inclusive, Stage-C): the initial stake
\* is fully accounted across the four buckets it can occupy — currently bonded,
\* the coop vault (Guilty penalties), the per-offender quarantine (slashed but
\* not yet adjudicated), and burned (destroyed by a Burned redemption, now
\* posVault protocol surplus). A slash moves bond → quarantine; a Vindicated
\* redeem moves quarantine → bond; a Guilty redeem moves quarantine → coop; a
\* Burned redeem moves quarantine → burned. Every transition conserves the sum.
Inv_StakeConservation ==
    SumBonds(Validators) + coopVaultBalance
      + SumQuarantined(Validators) + burnedStake
    = SumInitialBonds(Validators)

============================================================================
