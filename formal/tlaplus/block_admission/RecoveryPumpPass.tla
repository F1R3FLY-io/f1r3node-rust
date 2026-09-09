---------------------------- MODULE RecoveryPumpPass ----------------------------
EXTENDS Naturals

CONSTANTS Candidates, PageSize, PreserveFailure, FinishOnlyComplete, RejectWake
ASSUME Candidates > 0 /\ PageSize > 0
VARIABLES remaining, allowance, phase, failed, observedError, capacity, wake,
          passComplete, proposed, visited, selfWake
vars == <<remaining, allowance, phase, failed, observedError, capacity, wake,
          passComplete, proposed, visited, selfWake>>

Init ==
    /\ remaining = Candidates
    /\ allowance = PageSize
    /\ phase = "scan"
    /\ failed = FALSE
    /\ observedError = FALSE
    /\ capacity \in BOOLEAN
    /\ wake = FALSE
    /\ passComplete = FALSE
    /\ proposed = FALSE
    /\ visited = 0
    /\ selfWake = FALSE

Visit(error) ==
    /\ phase = "scan" /\ remaining > 0 /\ allowance > 0 /\ capacity
    /\ remaining' = remaining - 1
    /\ allowance' = allowance - 1
    /\ visited' = visited + 1
    /\ failed' = (failed \/ error)
    /\ observedError' = (observedError \/ error)
    /\ UNCHANGED <<phase, capacity, wake, passComplete, proposed, selfWake>>

PageEnd ==
    /\ phase = "scan" /\ remaining > 0 /\ allowance = 0
    /\ phase' = "yield"
    /\ UNCHANGED <<remaining, allowance, failed, observedError, capacity, wake,
                    passComplete, proposed, visited, selfWake>>

Continue ==
    /\ phase = "yield"
    /\ phase' = "scan"
    /\ allowance' = PageSize
    /\ failed' = IF PreserveFailure THEN failed ELSE FALSE
    /\ UNCHANGED <<remaining, observedError, capacity, wake, passComplete, proposed, visited, selfWake>>

Park ==
    /\ phase = "scan" /\ remaining > 0 /\ ~capacity
    /\ phase' = IF FinishOnlyComplete THEN "park" ELSE "done"
    /\ passComplete' = ~FinishOnlyComplete
    /\ proposed' = (~FinishOnlyComplete /\ ~failed)
    /\ UNCHANGED <<remaining, allowance, failed, observedError, capacity, wake, visited, selfWake>>

ReleaseAdmission ==
    /\ ~capacity
    /\ capacity' = TRUE
    /\ wake' = TRUE
    /\ UNCHANGED <<remaining, allowance, phase, failed, observedError, passComplete, proposed, visited, selfWake>>

CompetingAdmission ==
    /\ capacity
    /\ capacity' = FALSE
    /\ UNCHANGED <<remaining, allowance, phase, failed, observedError, wake, passComplete, proposed, visited, selfWake>>

RejectedReservation ==
    /\ ~capacity
    /\ wake' = (wake \/ RejectWake)
    /\ selfWake' = (selfWake \/ RejectWake)
    /\ UNCHANGED <<remaining, allowance, phase, failed, observedError, capacity, passComplete, proposed, visited>>

ExternalWake ==
    /\ wake' = TRUE
    /\ UNCHANGED <<remaining, allowance, phase, failed, observedError, capacity,
                    passComplete, proposed, visited, selfWake>>

Resume ==
    /\ phase = "park" /\ wake
    /\ wake' = FALSE
    /\ phase' = "scan"
    /\ UNCHANGED <<remaining, allowance, failed, observedError, capacity, passComplete, proposed, visited, selfWake>>

Finish ==
    /\ phase = "scan" /\ remaining = 0
    /\ phase' = "done"
    /\ passComplete' = TRUE
    /\ proposed' = ~failed
    /\ UNCHANGED <<remaining, allowance, failed, observedError, capacity, wake, visited, selfWake>>

Next == (\E error \in BOOLEAN : Visit(error)) \/ PageEnd \/ Continue \/ Park
        \/ ReleaseAdmission \/ CompetingAdmission \/ RejectedReservation \/ ExternalWake \/ Resume \/ Finish
TypeOK ==
    /\ remaining \in 0..Candidates /\ visited \in 0..Candidates
    /\ allowance \in 0..PageSize
    /\ phase \in {"scan", "yield", "park", "done"}
    /\ failed \in BOOLEAN /\ observedError \in BOOLEAN /\ capacity \in BOOLEAN
    /\ wake \in BOOLEAN /\ passComplete \in BOOLEAN /\ proposed \in BOOLEAN /\ selfWake \in BOOLEAN
Inv_VisitConservation == remaining + visited = Candidates
Inv_FailureSticky == observedError => failed
Inv_CompletePass == passComplete => remaining = 0
Inv_ProposalAfterSuccessfulPass == proposed => passComplete /\ ~observedError /\ remaining = 0
Inv_RejectionCannotSelfWake == ~selfWake
Live_FinitePassCompletes == (<>[]capacity) => <>passComplete
Spec == Init /\ [][Next]_vars
        /\ WF_vars(\E error \in BOOLEAN : Visit(error))
        /\ WF_vars(PageEnd) /\ WF_vars(Continue) /\ WF_vars(Park)
        /\ WF_vars(ReleaseAdmission) /\ WF_vars(Resume) /\ WF_vars(Finish)
=============================================================================
