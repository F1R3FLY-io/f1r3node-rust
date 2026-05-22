------------------------- MODULE MC_EquivocationDetector -------------------------
(****************************************************************************)
(* Model-checking instance for EquivocationDetector.                        *)
(* Three validators, seq numbers 1..3, up to 2 blocks per (v,s) pair.       *)
(* This bounds the state space at ~1.5 × 10⁵ — sufficient for confidence,   *)
(* small enough to finish in a few seconds on TLC -workers 12.              *)
(****************************************************************************)

EXTENDS EquivocationDetector, TLC

CONSTANTS v1, v2

MC_Validators        == {v1, v2}
MC_MaxSeqNum         == 2
MC_MaxBlocksPerSeq   == 2

============================================================================
