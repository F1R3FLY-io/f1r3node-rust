------------------------- MODULE MC_EquivocationDetector -------------------------
(****************************************************************************)
(* Model-checking instance for EquivocationDetector.                        *)
(* Two validators (v1, v2), seq numbers 1..2, up to 2 blocks per (v,s) pair *)
(* — i.e. 2 × 2 × 2 = 8 (validator, seq, block) triples. This keeps the      *)
(* state space small enough to exhaust in a few seconds on TLC -workers 12.  *)
(****************************************************************************)

EXTENDS EquivocationDetector, TLC

CONSTANTS v1, v2

MC_Validators        == {v1, v2}
MC_MaxSeqNum         == 2
MC_MaxBlocksPerSeq   == 2

============================================================================
