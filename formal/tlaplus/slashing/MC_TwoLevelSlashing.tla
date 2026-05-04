--------------------------- MODULE MC_TwoLevelSlashing ---------------------------
(****************************************************************************)
(* Model-checking instance for TwoLevelSlashing.                            *)
(* Four validators (so F = ⌊3/3⌋ = 1, quorum lower bound = 3); MaxLevel 4.  *)
(****************************************************************************)

EXTENDS TwoLevelSlashing, TLC

CONSTANTS v1, v2, v3, v4

MC_Validators == {v1, v2, v3, v4}
MC_MaxLevel   == 4

============================================================================
