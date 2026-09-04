------------------------ MODULE TargetDeployTerminality ------------------------
EXTENDS Naturals

\* Observer-only refinement; it consumes, but never recomputes, Casper status.
\* ObserveStatus maps to pyf1r3fly/f1r3fly/polling.py:
\* wait_for_deploy_finalized -> deploy_finalization_status.
\* ObserveLfb maps to the same helper -> last_finalized_block.
\* Tick maps to monotonic elapsed time between bounded polling attempts.

CONSTANTS
  \* @type: Int;
  MaxTime,
  \* @type: Int;
  StallTimeout,
  \* @type: Int;
  AbsoluteTimeout,
  \* @type: Int;
  MaxLfbHeight,
  \* @type: Int;
  MaxRevision,
  \* @type: Bool;
  UseProgressAwareDeadline,
  \* @type: Bool;
  DetectHistoryCorruption,
  \* @type: Bool;
  TreatLfbProgressAsSuccess,
  \* @type: Bool;
  DeadlinePrecedesObservation,
  \* @type: Bool;
  FirstObservationRenews

Statuses == {"Pending", "Finalized", "Failed", "Expired"}
Outcomes == {"Waiting", "Succeeded", "TerminalError", "TimedOut",
             "HistoryCorruption"}
ObservationClasses == {"None", "Baseline", "Stable", "StrictProgress",
                       "Regression", "Revision"}

ASSUME /\ StallTimeout \in 1..AbsoluteTimeout
       /\ AbsoluteTimeout <= MaxTime
       /\ MaxLfbHeight > 0
       /\ MaxRevision > 0
       /\ UseProgressAwareDeadline \in BOOLEAN
       /\ DetectHistoryCorruption \in BOOLEAN
       /\ TreatLfbProgressAsSuccess \in BOOLEAN
       /\ DeadlinePrecedesObservation \in BOOLEAN
       /\ FirstObservationRenews \in BOOLEAN

VARIABLES
  \* @type: Int;
  time,
  \* @type: Str;
  targetStatus,
  \* @type: Bool;
  baselineKnown,
  \* @type: Int;
  observedHeight,
  \* @type: Int;
  observedRevision,
  \* @type: Int;
  lastProgressTime,
  \* @type: Str;
  lastObservationClass,
  \* @type: Str;
  waitOutcome

vars == <<time, targetStatus, baselineKnown, observedHeight, observedRevision,
          lastProgressTime, lastObservationClass, waitOutcome>>

ObservationClass(nextHeight, nextRevision) ==
  IF ~baselineKnown
  THEN "Baseline"
  ELSE IF nextHeight < observedHeight
       THEN "Regression"
       ELSE IF nextHeight = observedHeight /\ nextRevision # observedRevision
            THEN "Revision"
            ELSE IF nextHeight > observedHeight
                 THEN "StrictProgress"
                 ELSE "Stable"

ProgressTime(nextTime, observationClass) ==
  IF observationClass = "StrictProgress" \/
     (observationClass = "Baseline" /\ FirstObservationRenews)
  THEN nextTime
  ELSE lastProgressTime

StallExpired(nextTime, nextProgressTime) ==
  IF UseProgressAwareDeadline
  THEN nextTime - nextProgressTime >= StallTimeout
  ELSE nextTime >= StallTimeout

BudgetExpired(nextTime) ==
  nextTime >= AbsoluteTimeout \/ StallExpired(nextTime, lastProgressTime)

NextOutcome(nextTime, nextStatus, observationClass) ==
  IF waitOutcome # "Waiting"
  THEN waitOutcome
  ELSE IF DeadlinePrecedesObservation /\ BudgetExpired(nextTime)
       THEN "TimedOut"
       ELSE IF DetectHistoryCorruption /\
          observationClass \in {"Regression", "Revision"}
            THEN "HistoryCorruption"
            ELSE IF nextStatus = "Finalized"
                 THEN "Succeeded"
                 ELSE IF nextStatus \in {"Failed", "Expired"}
                      THEN "TerminalError"
                      ELSE IF TreatLfbProgressAsSuccess /\
                              observationClass = "StrictProgress"
                           THEN "Succeeded"
                           ELSE IF ~DeadlinePrecedesObservation /\
                                   BudgetExpired(nextTime)
                                THEN "TimedOut"
                                ELSE "Waiting"

Init ==
  /\ time = 0
  /\ targetStatus = "Pending"
  /\ baselineKnown = FALSE
  /\ observedHeight = 0
  /\ observedRevision = 0
  /\ lastProgressTime = 0
  /\ lastObservationClass = "None"
  /\ waitOutcome = "Waiting"

Tick ==
  LET nextTime == time + 1
  IN /\ waitOutcome = "Waiting"
     /\ time < MaxTime
     /\ time' = nextTime
     /\ lastObservationClass' = "None"
     /\ waitOutcome' =
          NextOutcome(nextTime, targetStatus, "None")
     /\ UNCHANGED <<targetStatus, baselineKnown, observedHeight,
                    observedRevision, lastProgressTime>>

ObserveStatus(nextStatus, nextTime) ==
  /\ waitOutcome = "Waiting"
  /\ nextStatus \in Statuses
  /\ nextTime \in time..MaxTime
  /\ time' = nextTime
  /\ targetStatus' = nextStatus
  /\ lastObservationClass' = "None"
  /\ waitOutcome' =
       NextOutcome(nextTime, nextStatus, "None")
  /\ UNCHANGED <<baselineKnown, observedHeight, observedRevision,
                 lastProgressTime>>

ObserveLfb(nextHeight, nextRevision, nextTime) ==
  LET observationClass == ObservationClass(nextHeight, nextRevision)
      deadlineWins == DeadlinePrecedesObservation /\ BudgetExpired(nextTime)
      nextProgressTime ==
        IF deadlineWins
        THEN lastProgressTime
        ELSE ProgressTime(nextTime, observationClass)
  IN /\ waitOutcome = "Waiting"
     /\ nextHeight \in 0..MaxLfbHeight
     /\ nextRevision \in 0..MaxRevision
     /\ nextTime \in time..MaxTime
     /\ time' = nextTime
     /\ baselineKnown' = IF deadlineWins THEN baselineKnown ELSE TRUE
     /\ observedHeight' = IF deadlineWins THEN observedHeight ELSE nextHeight
     /\ observedRevision' = IF deadlineWins THEN observedRevision ELSE nextRevision
     /\ lastProgressTime' = nextProgressTime
     /\ lastObservationClass' = IF deadlineWins THEN "None" ELSE observationClass
     /\ waitOutcome' =
          NextOutcome(nextTime, targetStatus, observationClass)
     /\ UNCHANGED targetStatus

Next ==
  \/ Tick
  \/ \E nextStatus \in Statuses :
       \E nextTime \in time..MaxTime :
         ObserveStatus(nextStatus, nextTime)
  \/ \E nextHeight \in 0..MaxLfbHeight :
       \E nextRevision \in 0..MaxRevision :
         \E nextTime \in time..MaxTime :
           ObserveLfb(nextHeight, nextRevision, nextTime)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ time \in 0..MaxTime
  /\ targetStatus \in Statuses
  /\ baselineKnown \in BOOLEAN
  /\ observedHeight \in 0..MaxLfbHeight
  /\ observedRevision \in 0..MaxRevision
  /\ lastProgressTime \in 0..MaxTime
  /\ lastObservationClass \in ObservationClasses
  /\ waitOutcome \in Outcomes

Inv_SuccessRequiresExactFinalizedStatus ==
  waitOutcome = "Succeeded" => targetStatus = "Finalized"

Inv_FailedOrExpiredCannotSucceed ==
  targetStatus \in {"Failed", "Expired"} => waitOutcome # "Succeeded"

Inv_TerminalOutcomeWithinBudget ==
  waitOutcome \in {"Succeeded", "TerminalError", "HistoryCorruption"} =>
    time < AbsoluteTimeout /\ ~StallExpired(time, lastProgressTime)

Inv_HistoryAnomalyDetected ==
  lastObservationClass \in {"Regression", "Revision"} =>
    waitOutcome = "HistoryCorruption"

Inv_FirstObservationDoesNotRenew ==
  lastObservationClass = "Baseline" => lastProgressTime = 0

Inv_TimeoutHasExpiredBudget ==
  waitOutcome = "TimedOut" =>
    time >= AbsoluteTimeout \/ time - lastProgressTime >= StallTimeout

Inv_WithinProgressBudgetRemainsLive ==
  /\ targetStatus = "Pending"
  /\ lastObservationClass \notin {"Regression", "Revision"}
  /\ time < AbsoluteTimeout
  /\ time - lastProgressTime < StallTimeout
  => waitOutcome = "Waiting"

Inv_AbsoluteBound ==
  time >= AbsoluteTimeout => waitOutcome # "Waiting"
=============================================================================
