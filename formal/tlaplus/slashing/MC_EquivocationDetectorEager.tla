--------------------- MODULE MC_EquivocationDetectorEager ---------------------

EXTENDS EquivocationDetectorEager, TLC

CONSTANTS v1, v2

MC_Validators        == {v1, v2}
MC_MaxSeqNum         == 2
MC_MaxBlocksPerSeq   == 2

\* Symmetry over validators: any permutation of {v1, v2} yields an
\* observably equivalent state. TLC quotients the explored state space
\* by this group. With |Validators| = 2 this halves the state count.
SymmetryV == Permutations(MC_Validators)

============================================================================
