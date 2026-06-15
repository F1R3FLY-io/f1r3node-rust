--------------------------- MODULE MC_WithdrawFlow ---------------------------
(****************************************************************************)
(* Model-checking instance for WithdrawFlow.                                *)
(* Three withdrawers, each owed (bond=50, reward=10).                       *)
(****************************************************************************)

EXTENDS WithdrawFlow, TLC

CONSTANTS w1, w2, w3

MC_Withdrawers    == {w1, w2, w3}
MC_InitialBonds   == (w1 :> 50 @@ w2 :> 50 @@ w3 :> 50)
MC_InitialRewards == (w1 :> 10 @@ w2 :> 10 @@ w3 :> 10)

============================================================================
