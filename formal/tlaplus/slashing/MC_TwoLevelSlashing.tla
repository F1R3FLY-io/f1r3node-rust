--------------------------- MODULE MC_TwoLevelSlashing ---------------------------
(****************************************************************************)
(* Model-checking instance for TwoLevelSlashing.                            *)
(* Four validators (so F = ⌊3/3⌋ = 1, quorum lower bound = 3); MaxLevel 4.  *)
(****************************************************************************)

EXTENDS TwoLevelSlashing, TLC

CONSTANTS v1, v2, v3, v4

MC_Validators == {v1, v2, v3, v4}
MC_MaxLevel   == 4
MC_BondWeight == [v \in MC_Validators |-> 1]
MC_CurrentValidators == MC_Validators
MC_EvidenceValidators == MC_Validators
MC_Visibility == [v \in MC_Validators |-> MC_Validators]
MC_Reports == [v \in MC_Validators |-> {}]
MC_ArithmeticBits == 8
MC_MaxBond == 1
MC_InitialVault == 0
MC_ArithmeticLimit == 255
MC_CurrentEpoch == 0
MC_EvidenceEpoch == [v \in MC_Validators |-> 0]
MC_ViewAVisibility == MC_Visibility
MC_ViewAReports == MC_Reports
MC_ViewBVisibility == MC_Visibility
MC_ViewBReports == MC_Reports
MC_CarryoverEnabled == FALSE
MC_CarryoverMappedDirect == {}
MC_EnforceEvidenceRetention == TRUE
MC_RecordSeqBound == 2
MC_BatchFailureSet == {}
MC_EnforceBatchAtomicity == TRUE
MC_BatchOrderA == <<v1, v2>>
MC_BatchOrderB == <<v2, v1>>
MC_ProposerSchedule == <<v1, v2>>
MC_EvidenceObservedBy == {v1}
MC_EvidenceIncludedBy == {v1}
MC_EnforceProposerFairness == TRUE
MC_GossipDelay == 0
MC_InclusionDelay == 1
MC_RetentionWindow == 1
MC_RebondOldNonce == 0
MC_RebondNewNonce == 1
MC_EnforceRecordRetention == TRUE
MC_Renaming ==
    [v \in MC_Validators |->
        IF v = v1 THEN v2
        ELSE IF v = v2 THEN v1
        ELSE v]

============================================================================
