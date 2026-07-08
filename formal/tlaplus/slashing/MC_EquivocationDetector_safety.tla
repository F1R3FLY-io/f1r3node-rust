--------------------- MODULE MC_EquivocationDetector_safety ---------------------
(****************************************************************************)
(* Model-checking instance for EquivocationDetector — SAFETY ONLY.          *)
(*                                                                          *)
(* Identical bounds to MC_EquivocationDetector.tla (2 validators × 2 seqs   *)
(* × 2 blocks). The companion .cfg checks safety invariants only; the       *)
(* temporal property Live_DetectionComplete is omitted so the liveness      *)
(* graph does not blow up at this state-space size.                         *)
(*                                                                          *)
(* The full bounded liveness check lives at MC_EquivocationDetector_liveness*)
(* (reduced bounds) and the fully-equivalent rewrite at                     *)
(* MC_EquivocationDetectorEager (which converts liveness to a safety        *)
(* invariant, allowing both at full bounds in seconds).                     *)
(*                                                                          *)
(* See slashing-verification.md §10.4 for the rationale.                    *)
(****************************************************************************)

EXTENDS EquivocationDetector, TLC

CONSTANTS v1, v2

MC_Validators        == {v1, v2}
MC_MaxSeqNum         == 2
MC_MaxBlocksPerSeq   == 2

============================================================================
