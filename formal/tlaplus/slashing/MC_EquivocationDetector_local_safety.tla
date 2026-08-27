---------------- MODULE MC_EquivocationDetector_local_safety ----------------

EXTENDS EquivocationDetector, TLC

CONSTANTS v1, v2

MC_Validators        == {v1}
MC_MaxSeqNum         == 2
MC_MaxBlocksPerSeq   == 2

=============================================================================
