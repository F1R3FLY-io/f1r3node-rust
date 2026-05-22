--------------------- MODULE MC_EquivocationDetectorEager_3v ---------------------
(****************************************************************************)
(* Eager spec at LARGER bounds: 3 validators, 3 seqnums, 2 blocks.          *)
(* Demonstrates the headroom enabled by the eager rewrite. The original     *)
(* spec OOMed even at 2v×2s×2b; this rewrite handles 3v×3s×2b comfortably.  *)
(****************************************************************************)

EXTENDS EquivocationDetectorEager, TLC

CONSTANTS v1, v2, v3

MC_Validators        == {v1, v2, v3}
MC_MaxSeqNum         == 3
MC_MaxBlocksPerSeq   == 2

SymmetryV == Permutations(MC_Validators)

============================================================================
