------------------------------- MODULE MCEval ---------------------------------
(****************************************************************************)
(* Model-checking instance for EvalScheduling.                              *)
(****************************************************************************)

EXTENDS EvalScheduling, TLC

CONSTANTS b1, b2, b3

MC_Bodies        == {b1, b2, b3}
MC_CostPerToken  == 1
MC_StorageCostA  == 10   \* e.g., storage_cost_produce
MC_StorageCostB  == 15   \* e.g., storage_cost_consume (different!)
MC_MintAmount    == 1000 \* epochPhlogiston credited per eligible mint
MC_FeeAmount     == 1    \* Stage D: flat per-deploy FeeExtract collected to F_v

\* WD-D2 acceptance-gate instance: three deploys in a fixed canonical order,
\* each demanding 2 tokens (Δ_s = 2), sharing a pool of 5. The gate admits the
\* first 2 (cumulative 4 <= 5) and rejects the 3rd (cumulative 6 > 5) — an
\* OVERSUBSCRIBED block exercising reject-both + the settlement debit
\* (post = 5 - 4 = 1).
MC_CanonOrder    == <<b1, b2, b3>>
MC_Demand        == [b \in MC_Bodies |-> 2]
MC_PoolSupply    == 5

=============================================================================
