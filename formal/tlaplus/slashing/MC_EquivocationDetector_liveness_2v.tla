------------------- MODULE MC_EquivocationDetector_liveness_2v -------------------
(****************************************************************************)
(* Liveness model for EquivocationDetector at TWO-VALIDATOR bounds.         *)
(* Bounds: 2 validators, 1 seqnum, 2 blocks — one validator-dimension step  *)
(* up from MC_EquivocationDetector_liveness (1v×1s×2b). Sequence depth      *)
(* stays at 1: the combined 2v×2s×2b liveness graph is what OOM'd at        *)
(* 14.9M distinct states (see MC_EquivocationDetector_safety header).       *)
(****************************************************************************)

EXTENDS EquivocationDetector, TLC

CONSTANTS v1, v2

MC_Validators        == {v1, v2}
MC_MaxSeqNum         == 1
MC_MaxBlocksPerSeq   == 2

============================================================================
