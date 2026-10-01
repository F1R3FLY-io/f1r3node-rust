-------------------- MODULE NativeAttemptConflicts --------------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANT EnforceConflictOrder

Workers == {1, 2, 3, 4}
Cells == {1, 2, 3}
Limit == 1
Reads(w) == CASE w = 1 -> {1, 2}
                 [] w = 2 -> {1}
                 [] w = 3 -> {3}
                 [] OTHER -> {2}
Removes(w) == IF w = 4 THEN {} ELSE Reads(w)
Cost(w) == CASE w = 1 -> 2 [] w = 2 -> 1 [] OTHER -> 0

VARIABLES phase, prepared, played, trace, used, live, running, done, replayLive,
          replayUsed, unavailable
vars == <<phase, prepared, played, trace, used, live, running, done, replayLive,
          replayUsed, unavailable>>

TraceWorkers == {trace[i].worker : i \in 1..Len(trace)}
Position(w) == CHOOSE i \in 1..Len(trace) : trace[i].worker = w
Entry(w) == trace[Position(w)]
Writes(w) == IF Entry(w).accepted THEN Removes(w) ELSE {}
Conflict(a, b) ==
  Reads(a) \cap Writes(b) # {} \/ Writes(a) \cap Reads(b) # {}
Predecessors(w) == {a \in TraceWorkers :
  Position(a) < Position(w) /\ Conflict(a, w)}
Ready(w) == ~EnforceConflictOrder \/ Predecessors(w) \subseteq done
LocksFree(w) == \A active \in running : Reads(w) \cap Reads(active) = {}

Init ==
  /\ phase = "Play"
  /\ prepared = {}
  /\ played = {}
  /\ trace = <<>>
  /\ used = 0
  /\ live = Cells
  /\ running = {}
  /\ done = {}
  /\ replayLive = Cells
  /\ replayUsed = 0
  /\ unavailable = {}

Prepare(w) ==
  /\ phase = "Play"
  /\ w \notin prepared
  /\ prepared' = prepared \cup {w}
  /\ UNCHANGED <<phase, played, trace, used, live, running, done, replayLive,
                  replayUsed, unavailable>>

Attempt(w) ==
  /\ phase = "Play"
  /\ w \in prepared \ played
  /\ played' = played \cup {w}
  /\ IF Reads(w) \subseteq live
     THEN LET accepted == used + Cost(w) <= Limit
          IN /\ trace' = Append(trace, [worker |-> w, accepted |-> accepted])
             /\ used' = IF accepted THEN used + Cost(w) ELSE used
             /\ live' = IF accepted THEN live \ Removes(w) ELSE live
     ELSE UNCHANGED <<trace, used, live>>
  /\ UNCHANGED <<phase, prepared, running, done, replayLive, replayUsed, unavailable>>

Seal ==
  /\ phase = "Play"
  /\ played = Workers
  /\ phase' = "Replay"
  /\ UNCHANGED <<prepared, played, trace, used, live, running, done, replayLive,
                  replayUsed, unavailable>>

Acquire(w) ==
  /\ phase = "Replay"
  /\ w \in TraceWorkers \ (done \cup running)
  /\ Ready(w)
  /\ LocksFree(w)
  /\ running' = running \cup {w}
  /\ UNCHANGED <<phase, prepared, played, trace, used, live, done, replayLive,
                  replayUsed, unavailable>>

Replay(w) ==
  /\ phase = "Replay"
  /\ w \in running
  /\ running' = running \ {w}
  /\ done' = done \cup {w}
  /\ unavailable' = IF Reads(w) \subseteq replayLive
                     THEN unavailable ELSE unavailable \cup {w}
  /\ replayLive' = replayLive \ Writes(w)
  /\ replayUsed' = replayUsed + (IF Entry(w).accepted THEN Cost(w) ELSE 0)
  /\ UNCHANGED <<phase, prepared, played, trace, used, live>>

Finish ==
  /\ phase = "Replay"
  /\ done = TraceWorkers
  /\ running = {}
  /\ phase' = "Done"
  /\ UNCHANGED <<prepared, played, trace, used, live, running, done, replayLive,
                  replayUsed, unavailable>>

Next == (\E w \in Workers : Prepare(w) \/ Attempt(w) \/ Acquire(w) \/ Replay(w)) \/ Seal \/ Finish
Spec == Init /\ [][Next]_vars
FairSpec == Spec /\ WF_vars(Seal) /\ WF_vars(Finish)
  /\ (\A w \in Workers : WF_vars(Prepare(w)) /\ WF_vars(Attempt(w))
        /\ WF_vars(Acquire(w)) /\ WF_vars(Replay(w)))

TypeOK ==
  /\ phase \in {"Play", "Replay", "Done"}
  /\ prepared \subseteq Workers
  /\ played \subseteq prepared
  /\ trace \in Seq([worker : Workers, accepted : BOOLEAN])
  /\ Len(trace) = Cardinality(TraceWorkers)
  /\ done \subseteq TraceWorkers
  /\ running \subseteq TraceWorkers \ done
  /\ live \subseteq Cells
  /\ replayLive \subseteq Cells
  /\ unavailable \subseteq Workers
  /\ used \in 0..Limit
  /\ replayUsed \in 0..Limit

ReplaySourcesAvailable == unavailable = {}
ReplayStateAgreement == phase = "Done" => replayLive = live /\ replayUsed = used
ConflictOrderPreserved == \A w \in done : Predecessors(w) \subseteq done
NoDependencyWaitUnderLock == \A w \in running : Predecessors(w) \subseteq done
ExclusiveConflictingLocks == \A a, b \in running : a # b => Reads(a) \cap Reads(b) = {}
ReplayCanProgress == phase = "Replay" /\ done # TraceWorkers =>
  \E w \in TraceWorkers \ done : ENABLED Acquire(w) \/ ENABLED Replay(w)
IndependentReplayEnabled == phase = "Replay" =>
  \A w \in TraceWorkers \ (done \cup running) :
    Predecessors(w) \subseteq done /\ LocksFree(w) => ENABLED Acquire(w)
EventuallyFinished == <> (phase = "Done")
=============================================================================
