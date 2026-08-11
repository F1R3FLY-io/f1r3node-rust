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

\* CA-P-171 group-B disjoint acceptance-gate instance: a SINGLE deploy (b1) in
\* canonical order, demanding 1 token (Δ_sB = 1), drawing on a SEPARATE signature
\* pool Σ⟦sB⟧ = 2. This pool is DISJOINT from group A's Σ⟦s⟧ = 5 (different
\* signature ⇒ ChannelSeparation / lane_pool_disjoint). Group B is FULLY FUNDED
\* (cumulative 1 <= 2 admits its whole order), so group B reaches "settled" with
\* its deploy admitted & executed, EVEN THOUGH group A is oversubscribed and
\* rejects its tail. This is the disjoint-pool concurrent-admission witness: A's
\* partial admission does NOT block B's full admission, and the two share no lock
\* (independent per-pool fairness only). A single-deploy group B keeps the state
\* space within the bounded-memory envelope while remaining a faithful second,
\* disjoint, fully-funded pool. (DemandB is total over Bodies; only b1 is in B's
\* canonical order, so only b1's demand is ever drawn.)
MC_CanonOrderB   == <<b1>>
MC_DemandB       == [b \in MC_Bodies |-> 1]
MC_PoolSupplyB   == 2

=============================================================================
