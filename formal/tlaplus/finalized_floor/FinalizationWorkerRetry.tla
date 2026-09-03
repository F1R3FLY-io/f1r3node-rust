------------------------- MODULE FinalizationWorkerRetry -------------------------
EXTENDS Integers, TLC

CONSTANTS
  \* @type: Int;
  MaxRequests,
  \* @type: Int;
  MaxFailures,
  \* @type: Str;
  Defect

ASSUME /\ MaxRequests \in Nat \ {0}
       /\ MaxFailures \in Nat
       /\ Defect \in {"None", "FailureCompletes"}

Workers == {1, 2}
WorkerPhases == {"Idle", "Running", "Waiting"}

VARIABLES
  \* @type: Int;
  requestedThrough,
  \* @type: Int;
  launchedThrough,
  \* @type: Int;
  completedThrough,
  \* @type: Int;
  succeededThrough,
  \* @type: Bool;
  dispatcherRunning,
  \* @type: Int -> Str;
  workerPhase,
  \* @type: Int -> Int;
  workerCoverage,
  \* @type: Int;
  retryReadyThrough,
  \* @type: Int;
  failures

vars == <<requestedThrough, launchedThrough, completedThrough,
          succeededThrough, dispatcherRunning, workerPhase,
          workerCoverage, retryReadyThrough, failures>>

Max(left, right) == IF left >= right THEN left ELSE right

ActiveWorkers == {worker \in Workers : workerPhase[worker] # "Idle"}

Launchable ==
  requestedThrough > launchedThrough \/ retryReadyThrough > completedThrough

Init ==
  /\ requestedThrough = 0
  /\ launchedThrough = 0
  /\ completedThrough = 0
  /\ succeededThrough = 0
  /\ dispatcherRunning = FALSE
  /\ workerPhase = [worker \in Workers |-> "Idle"]
  /\ workerCoverage = [worker \in Workers |-> 0]
  /\ retryReadyThrough = 0
  /\ failures = 0

Request ==
  /\ requestedThrough < MaxRequests
  /\ requestedThrough' = requestedThrough + 1
  /\ dispatcherRunning' = TRUE
  /\ UNCHANGED <<launchedThrough, completedThrough, succeededThrough,
                  workerPhase, workerCoverage, retryReadyThrough, failures>>

Launch(worker) ==
  /\ dispatcherRunning
  /\ workerPhase[worker] = "Idle"
  /\ Launchable
  /\ LET coverage == Max(requestedThrough, retryReadyThrough) IN
       /\ coverage > completedThrough
       /\ workerCoverage' = [workerCoverage EXCEPT ![worker] = coverage]
       /\ launchedThrough' = Max(launchedThrough, coverage)
  /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Running"]
  /\ retryReadyThrough' = 0
  /\ UNCHANGED <<requestedThrough, completedThrough, succeededThrough,
                  dispatcherRunning, failures>>

Succeed(worker) ==
  /\ workerPhase[worker] = "Running"
  /\ completedThrough' = Max(completedThrough, workerCoverage[worker])
  /\ succeededThrough' = Max(succeededThrough, workerCoverage[worker])
  /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Idle"]
  /\ retryReadyThrough' =
       IF retryReadyThrough <= completedThrough'
       THEN 0
       ELSE retryReadyThrough
  /\ UNCHANGED <<requestedThrough, launchedThrough, dispatcherRunning,
                  workerCoverage, failures>>

Fail(worker) ==
  /\ workerPhase[worker] = "Running"
  /\ failures < MaxFailures
  /\ failures' = failures + 1
  /\ IF Defect = "FailureCompletes"
     THEN
       /\ completedThrough' = Max(completedThrough, workerCoverage[worker])
       /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Idle"]
     ELSE
       /\ UNCHANGED completedThrough
       /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Waiting"]
  /\ UNCHANGED <<requestedThrough, launchedThrough, succeededThrough,
                  dispatcherRunning, workerCoverage, retryReadyThrough>>

RetryDelayExpires(worker) ==
  /\ workerPhase[worker] = "Waiting"
  /\ workerPhase' = [workerPhase EXCEPT ![worker] = "Idle"]
  /\ IF completedThrough >= workerCoverage[worker]
     THEN
       /\ UNCHANGED <<retryReadyThrough, dispatcherRunning>>
     ELSE
       /\ retryReadyThrough' = Max(retryReadyThrough, workerCoverage[worker])
       /\ dispatcherRunning' = TRUE
  /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                  succeededThrough, workerCoverage, failures>>

Park ==
  /\ dispatcherRunning
  /\ ~Launchable
  /\ dispatcherRunning' = FALSE
  /\ UNCHANGED <<requestedThrough, launchedThrough, completedThrough,
                  succeededThrough, workerPhase, workerCoverage,
                  retryReadyThrough, failures>>

Next ==
  \/ Request
  \/ \E worker \in Workers : Launch(worker)
  \/ \E worker \in Workers : Succeed(worker)
  \/ \E worker \in Workers : Fail(worker)
  \/ \E worker \in Workers : RetryDelayExpires(worker)
  \/ Park

Spec == Init /\ [][Next]_vars

FairSpec ==
  /\ Spec
  /\ WF_vars(Request)
  /\ \A worker \in Workers :
       WF_vars(Launch(worker))
         /\ WF_vars(Succeed(worker))
         /\ WF_vars(RetryDelayExpires(worker))

TypeOK ==
  /\ requestedThrough \in 0..MaxRequests
  /\ launchedThrough \in 0..MaxRequests
  /\ completedThrough \in 0..MaxRequests
  /\ succeededThrough \in 0..MaxRequests
  /\ dispatcherRunning \in BOOLEAN
  /\ workerPhase \in [Workers -> WorkerPhases]
  /\ workerCoverage \in [Workers -> 0..MaxRequests]
  /\ retryReadyThrough \in 0..MaxRequests
  /\ failures \in 0..MaxFailures

Inv_CountersMonotonic ==
  /\ succeededThrough <= completedThrough
  /\ completedThrough <= launchedThrough
  /\ launchedThrough <= requestedThrough

Inv_CompletionRequiresSuccess == completedThrough = succeededThrough

Inv_NoLostRetry ==
  requestedThrough > completedThrough =>
    dispatcherRunning \/ ActiveWorkers # {} \/ retryReadyThrough > completedThrough

Safety ==
  /\ TypeOK
  /\ Inv_CountersMonotonic
  /\ Inv_CompletionRequiresSuccess
  /\ Inv_NoLostRetry

Live_AllRequestsComplete ==
  <>(requestedThrough = MaxRequests /\ completedThrough = requestedThrough)

=============================================================================
