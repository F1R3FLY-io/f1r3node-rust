------------------------ MODULE SoakCleanup ------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS SkipOwnerStop, SkipResidualStop, IgnoreStopFailure, EraseReason
VARIABLES stage, segment, childKnown, alive, ownerDone, residualDone,
          stopFailed, failed, reason, firstReason, passed
vars == <<stage, segment, childKnown, alive, ownerDone, residualDone,
          stopFailed, failed, reason, firstReason, passed>>

Init ==
  /\ stage = "running"
  /\ segment = 1
  /\ childKnown = TRUE
  /\ alive = {"owned", "residual", "unrelated"}
  /\ ownerDone = FALSE
  /\ residualDone = FALSE
  /\ stopFailed = FALSE
  /\ failed = FALSE
  /\ reason = "none"
  /\ firstReason = "none"
  /\ passed = FALSE

End(cause) ==
  /\ stage = "running"
  /\ cause \in {"deadline", "checkpoint", "finalize", "breach"}
  /\ stage' = "ending"
  /\ childKnown' = FALSE
  /\ UNCHANGED <<segment, alive, ownerDone, residualDone, stopFailed,
                   failed, reason, firstReason, passed>>

OwnerStop(ok) ==
  /\ stage = "ending"
  /\ ~ownerDone
  /\ ownerDone' = TRUE
  /\ alive' = IF ok /\ ~SkipOwnerStop THEN alive \ {"owned"} ELSE alive
  /\ stopFailed' = (stopFailed \/ ~ok)
  /\ failed' = (failed \/ (~ok /\ ~IgnoreStopFailure))
  /\ reason' = IF ~ok /\ reason = "none" THEN "writer_stop_failed" ELSE reason
  /\ firstReason' = IF ~ok /\ firstReason = "none" THEN "writer_stop_failed" ELSE firstReason
  /\ UNCHANGED <<stage, segment, childKnown, residualDone, passed>>

ResidualStop(ok) ==
  /\ stage = "ending"
  /\ ownerDone /\ ~residualDone
  /\ residualDone' = TRUE
  /\ alive' = IF ok /\ ~SkipResidualStop THEN alive \ {"residual"} ELSE alive
  /\ stopFailed' = (stopFailed \/ ~ok)
  /\ failed' = (failed \/ (~ok /\ ~IgnoreStopFailure))
  /\ reason' = IF ~ok /\ reason = "none" THEN "writer_stop_failed" ELSE reason
  /\ firstReason' = IF ~ok /\ firstReason = "none" THEN "writer_stop_failed" ELSE firstReason
  /\ UNCHANGED <<stage, segment, childKnown, ownerDone, passed>>

Publish ==
  /\ stage = "ending"
  /\ ownerDone /\ residualDone
  /\ stage' = "reported"
  /\ passed' = ~failed
  /\ reason' = IF EraseReason THEN "none" ELSE reason
  /\ UNCHANGED <<segment, childKnown, alive, ownerDone, residualDone,
                   stopFailed, failed, firstReason>>

Resume ==
  /\ stage = "reported" /\ segment = 1
  /\ segment' = 2
  /\ stage' = IF failed THEN "ending" ELSE "running"
  /\ childKnown' = ~failed
  /\ alive' = IF failed THEN alive ELSE alive \union {"owned", "residual"}
  /\ ownerDone' = FALSE
  /\ residualDone' = FALSE
  /\ passed' = FALSE
  /\ UNCHANGED <<stopFailed, failed, reason, firstReason>>

Next == (\E cause \in {"deadline", "checkpoint", "finalize", "breach"}: End(cause))
        \/ (\E ok \in BOOLEAN: OwnerStop(ok))
        \/ (\E ok \in BOOLEAN: ResidualStop(ok))
        \/ Publish \/ Resume
Spec == Init /\ [][Next]_vars

NoSuccessfulLeak == passed => alive = {"unrelated"}
StopFailurePersists == stopFailed => failed
ReasonPersists == firstReason # "none" => reason = firstReason
UnrelatedSurvives == "unrelated" \in alive
====================================================================
