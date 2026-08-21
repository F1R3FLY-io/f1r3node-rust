------------------------------ MODULE MCEvalNoLockNeg --------------------------
(****************************************************************************)
(* CA-P-171 NEGATIVE-CONTROL model-checking instance for EvalScheduling.     *)
(*                                                                          *)
(* This instance exists to PROVE that the headline liveness property         *)
(* DisjointPoolsAdmitConcurrentlyNoGlobalLock is genuinely CHECKED and        *)
(* NON-VACUOUS — i.e. it is not silently satisfied by a phase advance with    *)
(* nothing admitted. It is the same model as MCEval EXCEPT group B's disjoint *)
(* signature pool is UNFUNDED: PoolSupplyB = 0 while DemandB[b1] = 1 > 0.      *)
(*                                                                          *)
(* With an absent/zero group-B pool, the strict acceptance gate admits        *)
(* NOTHING for group B (cumulative 1 > 0 for its first deploy), so            *)
(* admittedLenB = 0 ≠ Len(CanonOrderB) = 1. Group B's gate still RUNS         *)
(* (AcceptanceGateB → SettleBlockB) and reaches gatePhaseB = "settled", but   *)
(* GroupBAdmittedExecuted is FALSE (its WHOLE order was NOT admitted). Hence   *)
(* DisjointPoolsAdmitConcurrentlyNoGlobalLock (which requires BOTH groups to   *)
(* be admitted+executed) and EachPoolAdmittedIndependently are BOTH REFUTED.   *)
(*                                                                          *)
(* TLC therefore produces a COUNTEREXAMPLE here — that REFUTATION is the       *)
(* EXPECTED, intended outcome of this run (exactly like the companion          *)
(* TokenGatedJoinM2Grief.cfg griefing-vector refutation). It is the proof      *)
(* that the property has TEETH: when the disjoint pool is NOT funded, the      *)
(* concurrent-admission liveness DOES fail, so the green result on the funded  *)
(* MCEval instance is meaningful.                                              *)
(*                                                                          *)
(* This wrapper is DELIBERATELY NOT registered in                             *)
(* scripts/check-cost-accounted-rho-tla-invariants.sh's WRAPPED_BY map — a    *)
(* counterexample is its intended result, not a pass. Run it explicitly:      *)
(*   tlc -deadlock -config EvalNoLockNeg.cfg MCEvalNoLockNeg.tla               *)
(****************************************************************************)

EXTENDS EvalScheduling, TLC

CONSTANTS b1, b2, b3

MC_Bodies        == {b1, b2, b3}
MC_CostPerToken  == 1
MC_StorageCostA  == 10
MC_StorageCostB  == 15
MC_MintAmount    == 1000
MC_FeeAmount     == 1

\* Group A: the SAME oversubscribed instance as MCEval (admits b1,b2; rejects b3).
MC_CanonOrder    == <<b1, b2, b3>>
MC_Demand        == [b \in MC_Bodies |-> 2]
MC_PoolSupply    == 5

\* Group B (NEGATIVE CONTROL): a single deploy b1 with Δ_sB = 1 drawing on an
\* UNFUNDED disjoint pool Σ⟦sB⟧ = 0. The strict gate admits NOTHING for B, so
\* admittedLenB = 0 ≠ Len(CanonOrderB) = 1 ⇒ GroupBAdmittedExecuted is FALSE ⇒
\* the concurrent-admission liveness property is REFUTED (the intended outcome).
MC_CanonOrderB   == <<b1>>
MC_DemandB       == [b \in MC_Bodies |-> 1]
MC_PoolSupplyB   == 0

=============================================================================
