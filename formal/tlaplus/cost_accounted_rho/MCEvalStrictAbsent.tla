---------------------------- MODULE MCEvalStrictAbsent ------------------------
(****************************************************************************)
(* #13b focused model-checking instance for EvalScheduling — the SPEC-STRICT *)
(* reject-when-ABSENT case (§7.6 step 5).                                    *)
(*                                                                          *)
(* Task #13a switched the WD-D2 gate to its strict mode, where an ABSENT     *)
(* supply pool is treated as a present pool with balance 0 (the paper's      *)
(* supply(s) = 0). This instance pins PoolSupply = 0 and gives every deploy   *)
(* a POSITIVE demand (Δ = 2), so the gate — admitting only the largest prefix *)
(* whose cumulative demand fits the pool — admits NOTHING. It exercises       *)
(* Inv_StrictRejectsAbsent NON-vacuously (the PoolSupply = 0 antecedent       *)
(* holds), confirming no Δ>0 deploy is ever admitted against an absent pool.  *)
(* This is the TLA+ analogue of the Rust strict branch + the Rocq corollary   *)
(* strict_reject_when_underfunded; #13b SEEDS client pools at genesis so a     *)
(* strict shard does NOT reject the clients it intends to fund (PoolSupply>0). *)
(*                                                                          *)
(* Pair with EvalStrictAbsent.cfg, run via                                   *)
(*   tlc -deadlock -config EvalStrictAbsent.cfg MCEvalStrictAbsent.tla        *)
(* (the check script's WRAPPED_BY map performs this pairing).                 *)
(****************************************************************************)

EXTENDS EvalScheduling, TLC

CONSTANTS b1, b2, b3

MC_Bodies        == {b1, b2, b3}
MC_CostPerToken  == 1
MC_StorageCostA  == 10
MC_StorageCostB  == 15
MC_MintAmount    == 1000
MC_FeeAmount     == 1

\* The strict reject-when-absent instance: three deploys in canonical order,
\* each demanding 2 tokens (Δ_s = 2 > 0), sharing an ABSENT pool (Σ⟦s⟧ = 0). The
\* strict gate admits NOTHING (cumulative 2 > 0 for the very first deploy), so
\* AdmittedSet is empty and Inv_StrictRejectsAbsent holds non-vacuously.
MC_CanonOrder    == <<b1, b2, b3>>
MC_Demand        == [b \in MC_Bodies |-> 2]
MC_PoolSupply    == 0

\* CA-P-171: this focused #13b instance is about group A's ABSENT pool, so group
\* B is given a trivial DISJOINT, fully-funded SINGLE-deploy instance (b1, Δ_sB = 1,
\* Σ⟦sB⟧ = 2) purely to satisfy the (now mandatory) group-B constants. It does not
\* affect the strict reject-when-absent headline (Inv_StrictRejectsAbsent ranges
\* over group A's PoolSupply = 0). Group B simply admits its one deploy and settles.
MC_CanonOrderB   == <<b1>>
MC_DemandB       == [b \in MC_Bodies |-> 1]
MC_PoolSupplyB   == 2

=============================================================================
