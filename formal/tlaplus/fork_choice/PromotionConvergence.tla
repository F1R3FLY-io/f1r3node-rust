------------------------ MODULE PromotionConvergence ------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS SignatureCount, CoverageGate

VARIABLES covered, pending, mode, promotedSig

vars == <<covered, pending, mode, promotedSig>>
Signatures == 1..SignatureCount

Init == /\ covered = {}
        /\ pending = Signatures
        /\ mode = "ghost"
        /\ promotedSig = 0

Eligible(sig) == IF CoverageGate THEN sig \in pending ELSE sig \in Signatures

Promote ==
    /\ mode = "ghost"
    /\ \E sig \in Signatures :
         /\ Eligible(sig)
         /\ mode' = "promoted"
         /\ promotedSig' = sig
         /\ UNCHANGED <<covered, pending>>

Merge ==
    /\ mode = "promoted"
    /\ covered' = covered \union {promotedSig}
    /\ pending' = pending \ {promotedSig}
    /\ mode' = "ghost"
    /\ promotedSig' = 0

Done == /\ CoverageGate
        /\ mode = "ghost"
        /\ pending = {}
        /\ UNCHANGED vars

Next == Promote \/ Merge \/ Done
Spec == Init /\ [][Next]_vars /\ WF_vars(Promote) /\ WF_vars(Merge)

TypeOK == /\ covered \subseteq Signatures
          /\ pending \subseteq Signatures
          /\ mode \in {"ghost", "promoted"}
          /\ promotedSig \in 0..SignatureCount

Inv_CoveragePartition == /\ covered \union pending = Signatures
                         /\ covered \intersect pending = {}
Inv_CoveredCannotPromote == CoverageGate => (pending = {} => ~ENABLED Promote)
Live_GhostRestored == <>[] (mode = "ghost" /\ pending = {})
=============================================================================
