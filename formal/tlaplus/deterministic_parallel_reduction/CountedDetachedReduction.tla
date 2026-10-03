---------------- MODULE CountedDetachedReduction ----------------
EXTENDS FiniteSets, Naturals, Sequences

CONSTANTS
    \* @type: Bool;
    CountBeforeSpawn,
    \* @type: Bool;
    CorrectCharge,
    \* @type: Bool;
    RetainPermit

Tasks == {"root", "left", "right", "grandchild"}

Children(task) ==
    CASE task = "root" -> {"left", "right"}
      [] task = "left" -> {"grandchild"}
      [] OTHER -> {}

Weight(task) ==
    CASE task = "root" -> 2
      [] task = "left" -> 3
      [] task = "right" -> 5
      [] OTHER -> 7

Failure(task) == task = "left"

VARIABLES
    \* @type: Set(Str);
    pending,
    \* @type: Int;
    live,
    \* @type: Seq(Str);
    joinedTrace,
    \* @type: Seq(Str);
    detachedTrace,
    \* @type: Int;
    joinedCharge,
    \* @type: Int;
    detachedCharge,
    \* @type: Seq(Str);
    joinedFailures,
    \* @type: Seq(Str);
    detachedFailures,
    \* @type: Bool;
    cancelled,
    \* @type: Bool;
    closed,
    \* @type: Bool;
    permitHeld,
    \* @type: Bool;
    checkpointed,
    \* @type: Int;
    cancelCharge

vars == <<pending, live, joinedTrace, detachedTrace,
          joinedCharge, detachedCharge, joinedFailures, detachedFailures,
          cancelled, closed, permitHeld, checkpointed, cancelCharge>>

Init ==
    /\ pending = {"root"}
    /\ live = 1
    /\ joinedTrace = <<>>
    /\ detachedTrace = <<>>
    /\ joinedCharge = 0
    /\ detachedCharge = 0
    /\ joinedFailures = <<>>
    /\ detachedFailures = <<>>
    /\ cancelled = FALSE
    /\ closed = FALSE
    /\ permitHeld = TRUE
    /\ checkpointed = FALSE
    /\ cancelCharge = 0

Run(task) ==
    /\ task \in pending
    /\ ~cancelled
    /\ ~closed
    /\ LET nextPending == (pending \ {task}) \union Children(task)
           nextFailures ==
               IF Failure(task) THEN Append(joinedFailures, task)
               ELSE joinedFailures
       IN /\ pending' = nextPending
          /\ live' = live - 1 +
              IF CountBeforeSpawn THEN Cardinality(Children(task)) ELSE 0
          /\ joinedFailures' = nextFailures
          /\ detachedFailures' =
              IF Failure(task) THEN Append(detachedFailures, task)
              ELSE detachedFailures
    /\ joinedTrace' = Append(joinedTrace, task)
    /\ detachedTrace' = Append(detachedTrace, task)
    /\ joinedCharge' = joinedCharge + Weight(task)
    /\ detachedCharge' = detachedCharge +
        IF CorrectCharge THEN Weight(task) ELSE Weight(task) + 1
    /\ UNCHANGED <<cancelled, closed, permitHeld, checkpointed,
                    cancelCharge>>

Cancel ==
    /\ ~cancelled
    /\ ~closed
    /\ cancelled' = TRUE
    /\ cancelCharge' = joinedCharge
    /\ permitHeld' = IF RetainPermit THEN permitHeld ELSE FALSE
    /\ UNCHANGED <<pending, live, joinedTrace, detachedTrace,
                    joinedCharge, detachedCharge, joinedFailures,
                    detachedFailures, closed, checkpointed>>

Abort(task) ==
    /\ cancelled
    /\ task \in pending
    /\ pending' = pending \ {task}
    /\ live' = live - 1
    /\ UNCHANGED <<joinedTrace, detachedTrace, joinedCharge,
                    detachedCharge, joinedFailures, detachedFailures,
                    cancelled, closed, permitHeld, checkpointed,
                    cancelCharge>>

Close ==
    /\ pending = {}
    /\ live = 0
    /\ ~closed
    /\ closed' = TRUE
    /\ permitHeld' = FALSE
    /\ UNCHANGED <<pending, live, joinedTrace, detachedTrace,
                    joinedCharge, detachedCharge, joinedFailures,
                    detachedFailures, cancelled, checkpointed,
                    cancelCharge>>

Checkpoint ==
    /\ ~checkpointed
    /\ ~permitHeld
    /\ checkpointed' = TRUE
    /\ UNCHANGED <<pending, live, joinedTrace, detachedTrace,
                    joinedCharge, detachedCharge, joinedFailures,
                    detachedFailures, cancelled, closed, permitHeld,
                    cancelCharge>>

Next ==
    (\E task \in Tasks : Run(task)) \/ Cancel \/
    (\E task \in Tasks : Abort(task)) \/ Close \/ Checkpoint

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ pending \subseteq Tasks
    /\ live \in 0..4
    /\ joinedCharge \in Nat
    /\ detachedCharge \in Nat
    /\ cancelCharge \in Nat
    /\ cancelled \in BOOLEAN
    /\ closed \in BOOLEAN
    /\ permitHeld \in BOOLEAN
    /\ checkpointed \in BOOLEAN

CountExact == live = Cardinality(pending)
SelectedTraceEqual == joinedTrace = detachedTrace
ChargeEqual == joinedCharge = detachedCharge
FailureSinkEqual == joinedFailures = detachedFailures
PermitUntilQuiescence == pending /= {} => permitHeld
CloseAfterQuiescence == closed => pending = {}
CheckpointAfterQuiescence == checkpointed => pending = {}
NoPostCancelCharge == cancelled =>
    joinedCharge = cancelCharge /\ detachedCharge = cancelCharge

=============================================================================
