-------------------------- MODULE FreeMapJournal ---------------------------
EXTENDS Integers, Naturals, Sequences, FiniteSets, TLC

(******************************************************************************
Finite refinement model for the drive-local FreeMap undo journal.

referenceMap and snapshots model the predecessor that cloned a complete map at
every matcher frame.  journalMap, journal, and checkpoints model the replacement
that records one previous value per write.  The transition system covers nested
frame commit and rollback, move-out delta extraction, validate-before-apply for
list and PathMap relation deltas, successful completion, root refusal, and Drop
unwinding.  The refinement invariant requires both maps to agree after every
action, while every live checkpoint must reconstruct its reference snapshot.

The finite domains deliberately contain repeated writes, absent and resident
levels, conflicting added-variable deltas, and nested frames.  The witness and
mutation invariants make the principal obligations discriminating rather than
vacuous.
******************************************************************************)

CONSTANTS MaxActions, MaxDepth

ASSUME /\ MaxActions \in Nat \ {0}
       /\ MaxDepth \in Nat \ {0}

Levels == {"L0", "L1"}
LevelOrder == <<"L0", "L1">>
Values == {"A", "B"}
Absent == "Absent"
NoDelta == "NoDelta"
CellValues == Values \union {Absent}
DeltaValues == Values \union {NoDelta}

FreeMaps == [Levels -> CellValues]
Deltas == [Levels -> DeltaValues]
UndoEntries == [level : Levels, previous : CellValues]
Statuses == {"active", "finished", "refused", "dropped"}

EmptyMap == [level \in Levels |-> Absent]
EmptyDelta == [level \in Levels |-> NoDelta]

MapPut(map, level, value) == [map EXCEPT ![level] = value]

Last(sequence) == sequence[Len(sequence)]
Pop(sequence) == SubSeq(sequence, 1, Len(sequence) - 1)

JournalPut(state, level, value) ==
    [current |-> MapPut(state.current, level, value),
     log |-> Append(state.log,
                    [level |-> level, previous |-> state.current[level]])]

RECURSIVE RollbackTo(_, _, _)
RollbackTo(current, log, keep) ==
    IF Len(log) = keep
    THEN current
    ELSE LET undo == log[Len(log)]
         IN RollbackTo(
              MapPut(current, undo.level, undo.previous),
              Pop(log),
              keep)

DeltaFrom(baseline, current) ==
    [level \in Levels |->
       IF current[level] = baseline[level]
       THEN NoDelta
       ELSE current[level]]

DeltaEntryCount(delta) ==
    Cardinality({level \in Levels : delta[level] # NoDelta})

BatchEntryCount(deltas) ==
    Cardinality({pair \in (1..Len(deltas)) \X Levels :
                   deltas[pair[1]][pair[2]] # NoDelta})

DuplicateAddedLevel(baseline, deltas, level) ==
    /\ baseline[level] = Absent
    /\ Cardinality({index \in 1..Len(deltas) :
                      deltas[index][level] # NoDelta}) > 1

ValidDeltas(baseline, deltas) ==
    ~\E level \in Levels : DuplicateAddedLevel(baseline, deltas, level)

RECURSIVE ApplyDelta(_, _, _), ApplyBatch(_, _, _)
ApplyDelta(state, delta, levelIndex) ==
    IF levelIndex > Len(LevelOrder)
    THEN state
    ELSE LET level == LevelOrder[levelIndex]
         IN IF delta[level] = NoDelta
            THEN ApplyDelta(state, delta, levelIndex + 1)
            ELSE ApplyDelta(
                   JournalPut(state, level, delta[level]),
                   delta,
                   levelIndex + 1)

ApplyBatch(state, deltas, deltaIndex) ==
    IF deltaIndex > Len(deltas)
    THEN state
    ELSE ApplyBatch(
           ApplyDelta(state, deltas[deltaIndex], 1),
           deltas,
           deltaIndex + 1)

ApplyBatchMap(current, deltas) ==
    ApplyBatch([current |-> current, log |-> <<>>], deltas, 1).current

VARIABLES
    initial,
    referenceMap,
    journalMap,
    snapshots,
    checkpoints,
    journal,
    pendingDeltas,
    status,
    actionCount,
    writeCount,
    batchObserved,
    lastBatchBefore,
    lastBatchAfter,
    lastBatchDeltas,
    lastBatchValid

vars ==
    <<initial, referenceMap, journalMap, snapshots, checkpoints, journal,
      pendingDeltas, status, actionCount, writeCount, batchObserved,
      lastBatchBefore, lastBatchAfter, lastBatchDeltas, lastBatchValid>>

Init ==
    /\ initial \in FreeMaps
    /\ referenceMap = initial
    /\ journalMap = initial
    /\ snapshots = <<>>
    /\ checkpoints = <<>>
    /\ journal = <<>>
    /\ pendingDeltas = <<>>
    /\ status = "active"
    /\ actionCount = 0
    /\ writeCount = 0
    /\ batchObserved = FALSE
    /\ lastBatchBefore = initial
    /\ lastBatchAfter = initial
    /\ lastBatchDeltas = <<>>
    /\ lastBatchValid = TRUE

CanAct == status = "active" /\ actionCount < MaxActions

Write ==
    /\ CanAct
    /\ \E level \in Levels, value \in Values :
         LET next == JournalPut(
                       [current |-> journalMap, log |-> journal],
                       level,
                       value)
         IN /\ referenceMap' = MapPut(referenceMap, level, value)
            /\ journalMap' = next.current
            /\ journal' = next.log
    /\ actionCount' = actionCount + 1
    /\ writeCount' = writeCount + 1
    /\ UNCHANGED
         <<initial, snapshots, checkpoints, pendingDeltas, status,
           batchObserved, lastBatchBefore, lastBatchAfter, lastBatchDeltas,
           lastBatchValid>>

OpenFrame ==
    /\ CanAct
    /\ Len(checkpoints) < MaxDepth
    /\ snapshots' = Append(snapshots, referenceMap)
    /\ checkpoints' = Append(checkpoints, Len(journal))
    /\ actionCount' = actionCount + 1
    /\ UNCHANGED
         <<initial, referenceMap, journalMap, journal, pendingDeltas, status,
           writeCount, batchObserved, lastBatchBefore, lastBatchAfter,
           lastBatchDeltas, lastBatchValid>>

CommitFrame ==
    /\ CanAct
    /\ Len(checkpoints) > 0
    /\ snapshots' = Pop(snapshots)
    /\ checkpoints' = Pop(checkpoints)
    /\ actionCount' = actionCount + 1
    /\ UNCHANGED
         <<initial, referenceMap, journalMap, journal, pendingDeltas, status,
           writeCount, batchObserved, lastBatchBefore, lastBatchAfter,
           lastBatchDeltas, lastBatchValid>>

RollbackFrame ==
    /\ CanAct
    /\ Len(checkpoints) > 0
    /\ referenceMap' = Last(snapshots)
    /\ journalMap' = RollbackTo(journalMap, journal, Last(checkpoints))
    /\ journal' = SubSeq(journal, 1, Last(checkpoints))
    /\ snapshots' = Pop(snapshots)
    /\ checkpoints' = Pop(checkpoints)
    /\ actionCount' = actionCount + 1
    /\ UNCHANGED
         <<initial, pendingDeltas, status, writeCount, batchObserved,
           lastBatchBefore, lastBatchAfter, lastBatchDeltas, lastBatchValid>>

CaptureFrameDelta ==
    /\ CanAct
    /\ Len(checkpoints) > 0
    /\ LET baseline == Last(snapshots)
           checkpoint == Last(checkpoints)
           delta == DeltaFrom(baseline, referenceMap)
       IN /\ referenceMap' = baseline
          /\ journalMap' = RollbackTo(journalMap, journal, checkpoint)
          /\ journal' = SubSeq(journal, 1, checkpoint)
          /\ pendingDeltas' = Append(pendingDeltas, delta)
    /\ snapshots' = Pop(snapshots)
    /\ checkpoints' = Pop(checkpoints)
    /\ actionCount' = actionCount + 1
    /\ UNCHANGED
         <<initial, status, writeCount, batchObserved, lastBatchBefore,
           lastBatchAfter, lastBatchDeltas, lastBatchValid>>

ValidateAndApplyBatch ==
    /\ CanAct
    /\ Len(pendingDeltas) > 0
    /\ LET valid == ValidDeltas(referenceMap, pendingDeltas)
           implementation ==
             ApplyBatch(
               [current |-> journalMap, log |-> journal],
               pendingDeltas,
               1)
           referenceAfter == ApplyBatchMap(referenceMap, pendingDeltas)
       IN /\ referenceMap' = IF valid THEN referenceAfter ELSE referenceMap
          /\ journalMap' = IF valid THEN implementation.current ELSE journalMap
          /\ journal' = IF valid THEN implementation.log ELSE journal
          /\ writeCount' =
               IF valid
               THEN writeCount + BatchEntryCount(pendingDeltas)
               ELSE writeCount
          /\ lastBatchBefore' = referenceMap
          /\ lastBatchAfter' = IF valid THEN referenceAfter ELSE referenceMap
          /\ lastBatchDeltas' = pendingDeltas
          /\ lastBatchValid' = valid
    /\ pendingDeltas' = <<>>
    /\ batchObserved' = TRUE
    /\ actionCount' = actionCount + 1
    /\ UNCHANGED <<initial, snapshots, checkpoints, status>>

FinishSuccess ==
    /\ CanAct
    /\ Len(checkpoints) = 0
    /\ Len(pendingDeltas) = 0
    /\ status' = "finished"
    /\ journal' = <<>>
    /\ actionCount' = actionCount + 1
    /\ UNCHANGED
         <<initial, referenceMap, journalMap, snapshots, checkpoints,
           pendingDeltas, writeCount, batchObserved, lastBatchBefore,
           lastBatchAfter, lastBatchDeltas, lastBatchValid>>

RefuseRoot ==
    /\ CanAct
    /\ referenceMap' = initial
    /\ journalMap' = RollbackTo(journalMap, journal, 0)
    /\ journal' = <<>>
    /\ snapshots' = <<>>
    /\ checkpoints' = <<>>
    /\ pendingDeltas' = <<>>
    /\ status' = "refused"
    /\ actionCount' = actionCount + 1
    /\ UNCHANGED
         <<initial, writeCount, batchObserved, lastBatchBefore,
           lastBatchAfter, lastBatchDeltas, lastBatchValid>>

DropAbort ==
    /\ CanAct
    /\ referenceMap' = initial
    /\ journalMap' = RollbackTo(journalMap, journal, 0)
    /\ journal' = <<>>
    /\ snapshots' = <<>>
    /\ checkpoints' = <<>>
    /\ pendingDeltas' = <<>>
    /\ status' = "dropped"
    /\ actionCount' = actionCount + 1
    /\ UNCHANGED
         <<initial, writeCount, batchObserved, lastBatchBefore,
           lastBatchAfter, lastBatchDeltas, lastBatchValid>>

Done == status # "active" \/ actionCount = MaxActions

Next ==
    Write
    \/ OpenFrame
    \/ CommitFrame
    \/ RollbackFrame
    \/ CaptureFrameDelta
    \/ ValidateAndApplyBatch
    \/ FinishSuccess
    \/ RefuseRoot
    \/ DropAbort
    \/ /\ Done
       /\ UNCHANGED vars

MaxWrites == (Cardinality(Levels) + 1) * MaxActions

TypeInvariant ==
    /\ initial \in FreeMaps
    /\ referenceMap \in FreeMaps
    /\ journalMap \in FreeMaps
    /\ snapshots \in Seq(FreeMaps)
    /\ checkpoints \in Seq(0..MaxWrites)
    /\ journal \in Seq(UndoEntries)
    /\ pendingDeltas \in Seq(Deltas)
    /\ status \in Statuses
    /\ actionCount \in 0..MaxActions
    /\ writeCount \in 0..MaxWrites
    /\ batchObserved \in BOOLEAN
    /\ lastBatchBefore \in FreeMaps
    /\ lastBatchAfter \in FreeMaps
    /\ lastBatchDeltas \in Seq(Deltas)
    /\ lastBatchValid \in BOOLEAN

SnapshotJournalLockstep == referenceMap = journalMap

CheckpointShape ==
    /\ Len(snapshots) = Len(checkpoints)
    /\ Len(checkpoints) <= MaxDepth
    /\ \A index \in 1..Len(checkpoints) : checkpoints[index] <= Len(journal)
    /\ \A index \in 2..Len(checkpoints) :
         checkpoints[index - 1] <= checkpoints[index]

CheckpointSnapshotsAgree ==
    \A index \in 1..Len(checkpoints) :
      RollbackTo(journalMap, journal, checkpoints[index]) = snapshots[index]

ActiveRootCanBeRestored ==
    status = "active" => RollbackTo(journalMap, journal, 0) = initial

CompletedStateIsClosed ==
    status # "active" =>
      /\ Len(snapshots) = 0
      /\ Len(checkpoints) = 0
      /\ Len(journal) = 0
      /\ Len(pendingDeltas) = 0
      /\ (status \in {"refused", "dropped"} => journalMap = initial)

ValidationIsAtomic ==
    batchObserved =>
      IF lastBatchValid
      THEN lastBatchAfter = ApplyBatchMap(lastBatchBefore, lastBatchDeltas)
      ELSE lastBatchAfter = lastBatchBefore

JournalBoundedByWrites ==
    /\ Len(journal) <= writeCount
    /\ writeCount <= MaxWrites

(******************************************************************************
Explicit action witnesses and discriminating mutations.
******************************************************************************)

DeltaAt(level, value) ==
    [candidate \in Levels |-> IF candidate = level THEN value ELSE NoDelta]

RECURSIVE ForwardRollback(_, _, _)
ForwardRollback(current, log, next) ==
    IF next > Len(log)
    THEN current
    ELSE LET undo == log[next]
         IN ForwardRollback(
              MapPut(current, undo.level, undo.previous),
              log,
              next + 1)

ApplyBeforeValidateMutation(baseline, deltas) ==
    IF ValidDeltas(baseline, deltas)
    THEN ApplyBatchMap(baseline, deltas)
    ELSE ApplyBatchMap(baseline, <<Head(deltas)>>)

DropWithoutUndoMutation(current) == current

ActionWitnessesAreNonVacuous ==
    LET outer == JournalPut(
                   [current |-> EmptyMap, log |-> <<>>],
                   "L0",
                   "A")
        checkpoint == Len(outer.log)
        inner == JournalPut(outer, "L1", "B")
        delta == DeltaFrom(outer.current, inner.current)
        conflicting == <<DeltaAt("L0", "A"), DeltaAt("L0", "B")>>
    IN /\ MaxDepth >= 2
       /\ checkpoint > 0
       /\ RollbackTo(inner.current, inner.log, checkpoint) = outer.current
       /\ delta["L1"] = "B"
       /\ ~ValidDeltas(EmptyMap, conflicting)
       /\ ApplyBatchMap(EmptyMap, <<DeltaAt("L0", "A")>>)
            # EmptyMap
       /\ RollbackTo(inner.current, inner.log, 0) = EmptyMap

ReverseUndoMutationIsDetected ==
    LET first == JournalPut(
                   [current |-> EmptyMap, log |-> <<>>],
                   "L0",
                   "A")
        second == JournalPut(first, "L0", "B")
    IN /\ RollbackTo(second.current, second.log, 0) = EmptyMap
       /\ ForwardRollback(second.current, second.log, 1) # EmptyMap

RollbackAllFrameMutationIsDetected ==
    LET outer == JournalPut(
                   [current |-> EmptyMap, log |-> <<>>],
                   "L0",
                   "A")
        checkpoint == Len(outer.log)
        inner == JournalPut(outer, "L1", "B")
    IN /\ RollbackTo(inner.current, inner.log, checkpoint) = outer.current
       /\ RollbackTo(inner.current, inner.log, 0) # outer.current

ApplyBeforeValidationMutationIsDetected ==
    LET conflicting == <<DeltaAt("L0", "A"), DeltaAt("L0", "B")>>
    IN /\ ~ValidDeltas(EmptyMap, conflicting)
       /\ ApplyBeforeValidateMutation(EmptyMap, conflicting) # EmptyMap

DropWithoutUndoMutationIsDetected ==
    LET modified == JournalPut(
                     [current |-> EmptyMap, log |-> <<>>],
                     "L0",
                     "A")
    IN /\ RollbackTo(modified.current, modified.log, 0) = EmptyMap
       /\ DropWithoutUndoMutation(modified.current) # EmptyMap

Spec == Init /\ [][Next]_vars

=============================================================================
