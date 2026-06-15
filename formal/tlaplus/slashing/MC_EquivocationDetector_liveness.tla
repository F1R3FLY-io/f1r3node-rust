--------------------- MODULE MC_EquivocationDetector_liveness ---------------------
(****************************************************************************)
(* Liveness model for EquivocationDetector at REDUCED bounds.               *)
(* Bounds: 1 validator, 1 seqnum, 2 blocks. State space ~50 distinct.       *)
(* This is small enough that liveness checking does not blow the heap.      *)
(****************************************************************************)

EXTENDS EquivocationDetector, TLC

CONSTANTS v1

MC_Validators        == {v1}
MC_MaxSeqNum         == 1
MC_MaxBlocksPerSeq   == 2

============================================================================
