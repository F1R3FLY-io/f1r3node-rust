--------------------------- MODULE MC_SlashFlow ---------------------------
(****************************************************************************)
(* Model-checking instance for SlashFlow.                                   *)
(* Three validators, each bonded with 100 stake; max seq number 2.          *)
(****************************************************************************)

EXTENDS SlashFlow, TLC

CONSTANTS v1, v2, v3

MC_Validators    == {v1, v2, v3}
MC_InitialBonds  == (v1 :> 100 @@ v2 :> 100 @@ v3 :> 100)
MC_MaxSeqNum     == 2

============================================================================
