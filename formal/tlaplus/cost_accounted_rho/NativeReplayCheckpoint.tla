-------------------- MODULE NativeReplayCheckpoint --------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS Workers, Channels, MaxSerial, UnlockedCapture, StoreOnlyRestore,
          EarlyRelease, FailedPrepareWrites, DrainCounters
VARIABLES stage, serial, channel, granted, tuples, counters, undo, nextSerial,
          epoch, boundary, token, capturedTuples, capturedCounters,
          target, stableSnapshots, preparePreserved, closed
vars == <<stage, serial, channel, granted, tuples, counters, undo, nextSerial,
          epoch, boundary, token, capturedTuples, capturedCounters,
          target, stableSnapshots, preparePreserved, closed>>

Active == {w \in Workers : stage[w] \in {"Waiting", "Reserved", "Prepared", "Applied"}}
ChannelOwners == {w \in Active : stage[w] # "Waiting"}
Zero == [c \in Channels |-> 0]
EmptyToken == [epoch |-> 0, prefix |-> <<>>, tuples |-> Zero, counters |-> Zero]
SumAt(log, c) == Cardinality({i \in 1..Len(log) : channel[log[i].worker] = c /\ granted[log[i].worker]})
Projection(log) == [c \in Channels |-> SumAt(log, c)]
AppliedAt(c) == Cardinality({w \in Active : stage[w] = "Applied" /\ channel[w] = c /\ granted[w]})
PrefixValid(t) == /\ t.epoch = epoch
                  /\ Len(t.prefix) <= Len(undo)
                  /\ t.prefix = SubSeq(undo, 1, Len(t.prefix))
BoundaryIdle == boundary = "Idle"
MayEnter == ~closed /\ (BoundaryIdle
             \/ (UnlockedCapture /\ boundary \in {"CaptureStore", "CaptureLedger"})
             \/ (EarlyRelease /\ boundary = "ReleasedRestore"))

Init ==
  /\ stage = [w \in Workers |-> "Idle"]
  /\ serial = [w \in Workers |-> 0]
  /\ channel \in [Workers -> Channels]
  /\ granted \in [Workers -> BOOLEAN]
  /\ tuples = Zero
  /\ counters = Zero
  /\ undo = <<>>
  /\ nextSerial = 1
  /\ epoch = 1
  /\ boundary = "Idle"
  /\ token = EmptyToken
  /\ capturedTuples = Zero
  /\ capturedCounters = Zero
  /\ target = EmptyToken
  /\ stableSnapshots = TRUE
  /\ preparePreserved = TRUE
  /\ closed = FALSE

Enter(w) ==
  /\ MayEnter /\ stage[w] = "Idle"
  /\ stage' = [stage EXCEPT ![w] = "Waiting"]
  /\ UNCHANGED <<serial, channel, granted, tuples, counters, undo, nextSerial, epoch,
                 boundary, token, capturedTuples, capturedCounters, target, stableSnapshots, preparePreserved>>

Reserve(w) ==
  /\ stage[w] = "Waiting"
  /\ nextSerial <= MaxSerial
  /\ \A v \in ChannelOwners : channel[v] # channel[w]
  /\ stage' = [stage EXCEPT ![w] = "Reserved"]
  /\ serial' = [serial EXCEPT ![w] = nextSerial]
  /\ nextSerial' = nextSerial + 1
  /\ UNCHANGED <<channel, granted, tuples, counters, undo, epoch, boundary,
                 token, capturedTuples, capturedCounters, target, stableSnapshots, preparePreserved>>

Prepare(w) ==
  /\ stage[w] = "Reserved"
  /\ stage' = [stage EXCEPT ![w] = "Prepared"]
  /\ UNCHANGED <<serial, channel, granted, tuples, counters, undo, nextSerial, epoch,
                 boundary, token, capturedTuples, capturedCounters, target, stableSnapshots, preparePreserved>>

Cancel(w) ==
  /\ stage[w] \in {"Waiting", "Reserved", "Prepared"}
  /\ stage' = [stage EXCEPT ![w] = "Idle"]
  /\ UNCHANGED <<serial, channel, granted, tuples, counters, undo, nextSerial, epoch,
                 boundary, token, capturedTuples, capturedCounters, target, stableSnapshots, preparePreserved>>

Apply(w) ==
  /\ stage[w] = "Prepared"
  /\ stage' = [stage EXCEPT ![w] = "Applied"]
  /\ tuples' = IF granted[w] THEN [tuples EXCEPT ![channel[w]] = @ + 1] ELSE tuples
  /\ counters' = IF granted[w] THEN [counters EXCEPT ![channel[w]] = @ + 1] ELSE counters
  /\ UNCHANGED <<serial, channel, granted, undo, nextSerial, epoch, boundary,
                 token, capturedTuples, capturedCounters, target, stableSnapshots, preparePreserved>>

Publish(w) ==
  /\ stage[w] = "Applied"
  /\ undo' = Append(undo, [worker |-> w, serial |-> serial[w]])
  /\ stage' = [stage EXCEPT ![w] = "Done"]
  /\ UNCHANGED <<serial, channel, granted, tuples, counters, nextSerial, epoch, boundary,
                 token, capturedTuples, capturedCounters, target, stableSnapshots, preparePreserved>>

StartCapture ==
  /\ BoundaryIdle /\ ~closed
  /\ Active = {} \/ UnlockedCapture
  /\ boundary' = "CaptureStore"
  /\ UNCHANGED <<stage, serial, channel, granted, tuples, counters, undo, nextSerial,
                 epoch, token, capturedTuples, capturedCounters, target, stableSnapshots, preparePreserved>>

CaptureStore ==
  /\ boundary = "CaptureStore"
  /\ capturedTuples' = tuples
  /\ capturedCounters' = counters
  /\ counters' = IF DrainCounters THEN Zero ELSE counters
  /\ boundary' = "CaptureLedger"
  /\ UNCHANGED <<stage, serial, channel, granted, tuples, undo, nextSerial,
                 epoch, token, target, stableSnapshots, preparePreserved>>

CaptureLedger ==
  /\ boundary = "CaptureLedger"
  /\ token' = [epoch |-> epoch, prefix |-> undo, tuples |-> capturedTuples, counters |-> capturedCounters]
  /\ stableSnapshots' = (capturedTuples = Projection(undo) /\ capturedCounters = Projection(undo))
  /\ boundary' = "Idle"
  /\ UNCHANGED <<stage, serial, channel, granted, tuples, counters, undo, nextSerial,
                 epoch, capturedTuples, capturedCounters, target, preparePreserved>>

StartRestore(root) ==
  /\ BoundaryIdle /\ Active = {} /\ ~closed
  /\ boundary' = "RestorePrepare"
  /\ target' = IF root THEN [epoch |-> epoch, prefix |-> <<>>, tuples |-> Zero, counters |-> Zero] ELSE token
  /\ UNCHANGED <<stage, serial, channel, granted, tuples, counters, undo, nextSerial,
                 epoch, token, capturedTuples, capturedCounters, stableSnapshots, preparePreserved>>

PrepareRestore ==
  /\ boundary = "RestorePrepare" /\ PrefixValid(target)
  /\ boundary' = "RestoreStore"
  /\ UNCHANGED <<stage, serial, channel, granted, tuples, counters, undo, nextSerial,
                 epoch, token, capturedTuples, capturedCounters, target, stableSnapshots, preparePreserved>>

AbortRestore ==
  /\ boundary = "RestorePrepare"
  /\ tuples' = IF FailedPrepareWrites THEN target.tuples ELSE tuples
  /\ counters' = IF FailedPrepareWrites THEN target.counters ELSE counters
  /\ preparePreserved' = (tuples' = tuples /\ counters' = counters)
  /\ boundary' = "Idle"
  /\ UNCHANGED <<stage, serial, channel, granted, undo, nextSerial, epoch,
                 token, capturedTuples, capturedCounters, target, stableSnapshots>>

RestoreStore ==
  /\ boundary = "RestoreStore"
  /\ tuples' = target.tuples
  /\ counters' = target.counters
  /\ boundary' = IF EarlyRelease THEN "ReleasedRestore"
                 ELSE IF StoreOnlyRestore THEN "Idle" ELSE "RestoreLedger"
  /\ UNCHANGED <<stage, serial, channel, granted, undo, nextSerial, epoch,
                 token, capturedTuples, capturedCounters, target, stableSnapshots, preparePreserved>>

RestoreLedger ==
  /\ boundary \in {"RestoreLedger", "ReleasedRestore"}
  /\ undo' = target.prefix
  /\ stage' = [w \in Workers |-> IF w \in {target.prefix[i].worker : i \in 1..Len(target.prefix)} THEN "Done" ELSE "Idle"]
  /\ boundary' = "Idle"
  /\ UNCHANGED <<serial, channel, granted, tuples, counters, nextSerial, epoch,
                 token, capturedTuples, capturedCounters, target, stableSnapshots, preparePreserved>>

ReplaceEpoch ==
  /\ BoundaryIdle /\ Active = {} /\ epoch = 1 /\ ~closed
  /\ epoch' = 2
  /\ tuples' = Zero /\ counters' = Zero /\ undo' = <<>>
  /\ stage' = [w \in Workers |-> "Idle"]
  /\ serial' = [w \in Workers |-> 0]
  /\ nextSerial' = 1
  /\ UNCHANGED <<channel, granted, boundary, token, capturedTuples, capturedCounters,
                 target, stableSnapshots, preparePreserved>>

Close ==
  /\ BoundaryIdle /\ Active = {} /\ ~closed
  /\ closed' = TRUE
  /\ UNCHANGED <<stage, serial, channel, granted, tuples, counters, undo, nextSerial,
                 epoch, boundary, token, capturedTuples, capturedCounters, target, stableSnapshots, preparePreserved>>

Advance == (\E w \in Workers : Enter(w) \/ Reserve(w) \/ Prepare(w) \/ Cancel(w) \/ Apply(w) \/ Publish(w))
     \/ StartCapture \/ CaptureStore \/ CaptureLedger
     \/ StartRestore(TRUE) \/ StartRestore(FALSE) \/ PrepareRestore \/ AbortRestore
     \/ RestoreStore \/ RestoreLedger \/ ReplaceEpoch
Next == (Advance /\ UNCHANGED closed) \/ Close
Spec == Init /\ [][Next]_vars

TypeOK == /\ stage \in [Workers -> {"Idle", "Waiting", "Reserved", "Prepared", "Applied", "Done"}]
          /\ tuples \in [Channels -> 0..Cardinality(Workers)]
          /\ counters \in [Channels -> 0..Cardinality(Workers)]
          /\ epoch \in 1..2
          /\ nextSerial \in 1..(MaxSerial + 1)
          /\ closed \in BOOLEAN
PrivateBoundary == boundary # "Idle" => Active = {}
SharedChannelExclusion == \A w, v \in ChannelOwners : w = v \/ channel[w] # channel[v]
ClosedAccess == closed => (Active = {} /\ BoundaryIdle /\ \A w \in Workers : ~ENABLED Enter(w))
StableSnapshots == stableSnapshots
FailedPreparationPreservesState == preparePreserved
TupleLedgerAgreement == boundary \notin {"RestoreLedger", "RestoreStore"} =>
  \A c \in Channels : tuples[c] = SumAt(undo, c) + AppliedAt(c)
CounterAgreement == tuples = counters
SlotOwnership == {w \in Workers : stage[w] = "Done"} = {undo[i].worker : i \in 1..Len(undo)}
UniqueCompletions == Cardinality({undo[i].worker : i \in 1..Len(undo)}) = Len(undo)
PreparedAncestry == boundary \in {"RestoreStore", "RestoreLedger"} => PrefixValid(target)
IndependentProgress == \A w \in Workers :
  (stage[w] = "Waiting" /\ nextSerial <= MaxSerial
   /\ \A v \in ChannelOwners : channel[v] # channel[w]) => ENABLED Reserve(w)
=============================================================================
