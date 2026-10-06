-------------------- MODULE MC_EquivocationDetectorEager_3v2s --------------------
(****************************************************************************)
(* Eager spec at NIGHTLY-TIER bounds: 3 validators, 2 seqnums, 2 blocks.    *)
(*                                                                          *)
(* Three validators is the point: equivocation detection is inherently      *)
(* multi-validator, and with MC_EquivocationDetectorEager_3v (3v×3s×2b)     *)
(* parked in the exhaustive tier, the nightly tier otherwise checks the     *)
(* detector at ≤2 validators. Sequence depth is what shrinks to fit the     *)
(* per-config CI cap; the Eager rewrite already checks liveness as the      *)
(* Inv_LivenessAsSafety invariant, so no temporal property is needed.       *)
(*                                                                          *)
(* The full 3v×3s×2b headroom demonstration remains at                      *)
(* MC_EquivocationDetectorEager_3v (exhaustive tier, RUN_EXHAUSTIVE_TLA=1). *)
(****************************************************************************)

EXTENDS EquivocationDetectorEager, TLC

CONSTANTS v1, v2, v3

MC_Validators        == {v1, v2, v3}
MC_MaxSeqNum         == 2
MC_MaxBlocksPerSeq   == 2

SymmetryV == Permutations(MC_Validators)

============================================================================
