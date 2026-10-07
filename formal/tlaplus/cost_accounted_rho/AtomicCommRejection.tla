------------------------ MODULE AtomicCommRejection ------------------------
EXTENDS AtomicCommAccounting

RejectCommandsDef == {sA, rA, u}
RejectEventsDef == {binary}
RejectRequirementsDef == [event \in RejectEventsDef |-> {sA, rA}]

=============================================================================
