----------------------- MODULE NativeLaunchAdmission -----------------------
EXTENDS Naturals

CONSTANTS VerifyBeforeRelease, QueryAvailable
VARIABLES phase, verified, admitted, unrelatedRunning

vars == <<phase, verified, admitted, unrelatedRunning>>

Init ==
    /\ phase = "created"
    /\ verified = FALSE
    /\ admitted = FALSE
    /\ unrelatedRunning = TRUE

Start ==
    /\ phase = "created"
    /\ phase' = "query"
    /\ admitted' = ~VerifyBeforeRelease
    /\ UNCHANGED <<verified, unrelatedRunning>>

Observe ==
    /\ phase = "query"
    /\ phase' = IF QueryAvailable THEN "verified" ELSE "refused"
    /\ verified' = QueryAvailable
    /\ UNCHANGED <<admitted, unrelatedRunning>>

Release ==
    /\ phase = "verified"
    /\ phase' = "admitted"
    /\ admitted' = TRUE
    /\ UNCHANGED <<verified, unrelatedRunning>>

Next == Start \/ Observe \/ Release
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ VerifyBeforeRelease \in BOOLEAN
    /\ QueryAvailable \in BOOLEAN
    /\ phase \in {"created", "query", "verified", "refused", "admitted"}
    /\ verified \in BOOLEAN
    /\ admitted \in BOOLEAN
    /\ unrelatedRunning \in BOOLEAN

UnavailableQueryPreventsNativeAdmission == phase = "refused" => ~admitted
VerifiedReleaseAdmits == phase = "admitted" => (verified /\ admitted)
UnrelatedWriterPreserved == unrelatedRunning
Completes == <>(phase \in {"refused", "admitted"})
=============================================================================
