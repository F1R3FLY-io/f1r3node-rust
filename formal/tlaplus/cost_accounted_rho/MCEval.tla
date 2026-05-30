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

=============================================================================
