--------------------- MODULE CertifiedEgressTranscript ---------------------
EXTENDS Naturals, Sequences, TLC

(***************************************************************************
Refinement model for the typed structural-length transcript used by
`CertifiedEgressPreflight`, `CertifiedEgressPlan`, and
`CertifiedEgressReplay`.

The representative generated program deliberately contains all four event
kinds and two equal-length siblings of different kinds. Its outer message is
reserved first, descendants force an inline-to-spill transition, and the
outer slot is completed only after those descendants. Consequently the model
checks the production lifecycle rather than a flat length list:

  Reserve / PushCompleted  -> reserve_length / par_encoded_len
  Complete                 -> complete_length
  Seal                     -> CertifiedEgressPlan::for_root
  StartEmission / Consume  -> replay / next_body_len
  FinishReplay             -> replay.finish
  Publish                  -> return the complete private FFI frame

The existing `ParBagInFrameMemo` model is orthogonal: its census sequence is
the Par-owner projection of this sealed structural transcript.
***************************************************************************)

CONSTANT InlineCap
ASSUME InlineCap = 3

Kinds == {"MessageBody", "ParBody", "MapEntryBody", "EPathMapSnapshotBody"}
Phases == {"preflight", "sealed", "emitting", "ready", "committed"}
StorageModes == {"inline", "spill"}
PublicImages == {"old", "new", "partial"}

GeneratedKinds ==
    <<"MessageBody", "MapEntryBody", "ParBody",
      "EPathMapSnapshotBody", "ParBody">>
GeneratedLengths == <<7, 7, 3, 7, 5>>
GeneratedEventCount == Len(GeneratedKinds)
ExpectedBytes == 29

Event(kind, bodyLen, completed, completions) ==
    [kind |-> kind,
     bodyLen |-> bodyLen,
     completed |-> completed,
     completions |-> completions]

VARIABLES phase, preflightPc, events, storageMode, inlineStorage, spillStorage,
          replayIndex, replayed, privateBytes, publicImage, publicationCount

vars == <<phase, preflightPc, events, storageMode, inlineStorage, spillStorage,
          replayIndex, replayed, privateBytes, publicImage, publicationCount>>

Init ==
    /\ phase = "preflight"
    /\ preflightPc = 1
    /\ events = <<>>
    /\ storageMode = "inline"
    /\ inlineStorage = <<>>
    /\ spillStorage = <<>>
    /\ replayIndex = 0
    /\ replayed = <<>>
    /\ privateBytes = 0
    /\ publicImage = "old"
    /\ publicationCount = 0

AppendEvent(event, nextPc) ==
    /\ events' = Append(events, event)
    /\ preflightPc' = nextPc
    /\ IF storageMode = "inline" /\ Len(events) < InlineCap
          THEN /\ inlineStorage' = Append(inlineStorage, event)
               /\ UNCHANGED <<storageMode, spillStorage>>
          ELSE IF storageMode = "inline"
            THEN /\ storageMode' = "spill"
                 /\ spillStorage' = Append(inlineStorage, event)
                 /\ UNCHANGED inlineStorage
            ELSE /\ spillStorage' = Append(spillStorage, event)
                 /\ UNCHANGED <<storageMode, inlineStorage>>
    /\ UNCHANGED <<phase, replayIndex, replayed, privateBytes,
                    publicImage, publicationCount>>

ReserveGenerated ==
    /\ phase = "preflight"
    /\ \/ /\ preflightPc = 1
           /\ AppendEvent(Event("MessageBody", 0, FALSE, 0), 2)
       \/ /\ preflightPc = 2
           /\ AppendEvent(Event("MapEntryBody", 0, FALSE, 0), 3)
       \/ /\ preflightPc = 5
           /\ AppendEvent(Event("EPathMapSnapshotBody", 0, FALSE, 0), 6)

PushCompletedPar ==
    /\ phase = "preflight"
    /\ \/ /\ preflightPc = 3
           /\ AppendEvent(Event("ParBody", 3, TRUE, 1), 4)
       \/ /\ preflightPc = 6
           /\ AppendEvent(Event("ParBody", 5, TRUE, 1), 7)

CompleteSlot(slot, bodyLen, nextPc) ==
    /\ slot \in 1..Len(events)
    /\ ~events[slot].completed
    /\ events' = [events EXCEPT
                    ![slot].bodyLen = bodyLen,
                    ![slot].completed = TRUE,
                    ![slot].completions = @ + 1]
    /\ preflightPc' = nextPc
    /\ IF storageMode = "inline"
          THEN /\ inlineStorage' = [inlineStorage EXCEPT
                        ![slot].bodyLen = bodyLen,
                        ![slot].completed = TRUE,
                        ![slot].completions = @ + 1]
               /\ UNCHANGED spillStorage
          ELSE /\ spillStorage' = [spillStorage EXCEPT
                        ![slot].bodyLen = bodyLen,
                        ![slot].completed = TRUE,
                        ![slot].completions = @ + 1]
               /\ UNCHANGED inlineStorage
    /\ UNCHANGED <<phase, storageMode, replayIndex, replayed, privateBytes,
                    publicImage, publicationCount>>

CompleteGenerated ==
    /\ phase = "preflight"
    /\ \/ /\ preflightPc = 4 /\ CompleteSlot(2, 7, 5)
       \/ /\ preflightPc = 7 /\ CompleteSlot(4, 7, 8)
       \/ /\ preflightPc = 8 /\ CompleteSlot(1, 7, 9)

AllComplete == \A index \in 1..Len(events): events[index].completed

Seal ==
    /\ phase = "preflight"
    /\ preflightPc = 9
    /\ Len(events) = GeneratedEventCount
    /\ AllComplete
    /\ phase' = "sealed"
    /\ UNCHANGED <<preflightPc, events, storageMode, inlineStorage, spillStorage,
                    replayIndex, replayed, privateBytes, publicImage,
                    publicationCount>>

StartEmission ==
    /\ phase = "sealed"
    /\ phase' = "emitting"
    /\ UNCHANGED <<preflightPc, events, storageMode, inlineStorage, spillStorage,
                    replayIndex, replayed, privateBytes, publicImage,
                    publicationCount>>

Consume ==
    /\ phase = "emitting"
    /\ replayIndex < Len(events)
    /\ LET next == replayIndex + 1
           event == events[next]
           expectedKind == GeneratedKinds[next]
       IN /\ event.kind = expectedKind
          /\ replayIndex' = next
          /\ replayed' = Append(replayed,
                                [kind |-> event.kind, bodyLen |-> event.bodyLen])
          /\ privateBytes' = privateBytes + event.bodyLen
    /\ UNCHANGED <<phase, preflightPc, events, storageMode, inlineStorage,
                    spillStorage, publicImage, publicationCount>>

FinishReplay ==
    /\ phase = "emitting"
    /\ replayIndex = Len(events)
    /\ phase' = "ready"
    /\ UNCHANGED <<preflightPc, events, storageMode, inlineStorage, spillStorage,
                    replayIndex, replayed, privateBytes, publicImage,
                    publicationCount>>

Publish ==
    /\ phase = "ready"
    /\ replayIndex = Len(events)
    /\ privateBytes = ExpectedBytes
    /\ phase' = "committed"
    /\ publicImage' = "new"
    /\ publicationCount' = publicationCount + 1
    /\ UNCHANGED <<preflightPc, events, storageMode, inlineStorage, spillStorage,
                    replayIndex, replayed, privateBytes>>

Next == ReserveGenerated \/ PushCompletedPar \/ CompleteGenerated \/ Seal
        \/ StartEmission \/ Consume \/ FinishReplay \/ Publish
Spec == Init /\ [][Next]_vars

EventType(event) ==
    /\ event.kind \in Kinds
    /\ event.bodyLen \in Nat
    /\ event.completed \in BOOLEAN
    /\ event.completions \in Nat

TypeInvariant ==
    /\ phase \in Phases
    /\ preflightPc \in 1..9
    /\ events \in Seq([kind: Kinds, bodyLen: Nat,
                        completed: BOOLEAN, completions: Nat])
    /\ \A index \in 1..Len(events): EventType(events[index])
    /\ Len(events) \in 0..GeneratedEventCount
    /\ storageMode \in StorageModes
    /\ inlineStorage \in Seq([kind: Kinds, bodyLen: Nat,
                               completed: BOOLEAN, completions: Nat])
    /\ spillStorage \in Seq([kind: Kinds, bodyLen: Nat,
                              completed: BOOLEAN, completions: Nat])
    /\ replayIndex \in 0..Len(events)
    /\ replayed \in Seq([kind: Kinds, bodyLen: Nat])
    /\ Len(replayed) = replayIndex
    /\ privateBytes \in Nat
    /\ publicImage \in PublicImages
    /\ publicationCount \in Nat

ReservedPreorderIsGeneratedPrefix ==
    \A index \in 1..Len(events): events[index].kind = GeneratedKinds[index]

CompletionAfterDescendants ==
    /\ (Len(events) >= 1 /\ events[1].completed => preflightPc = 9)
    /\ (Len(events) >= 2 /\ events[2].completed => preflightPc >= 5)
    /\ (Len(events) >= 4 /\ events[4].completed => preflightPc >= 8)

CompletionExactlyOnce ==
    \A index \in 1..Len(events):
        events[index].completions \in 0..1
        /\ (events[index].completed <=> events[index].completions = 1)

SealIffWalkDoneAndAllComplete ==
    phase \in {"sealed", "emitting", "ready", "committed"}
    => /\ preflightPc = 9
       /\ Len(events) = GeneratedEventCount
       /\ AllComplete

StorageRefinesLogicalTranscript ==
    IF storageMode = "inline"
      THEN /\ inlineStorage = events
           /\ spillStorage = <<>>
           /\ Len(events) <= InlineCap
      ELSE /\ spillStorage = events
           /\ Len(inlineStorage) = InlineCap
           /\ Len(events) > InlineCap

TypedReplayConsumesExactPrefix ==
    /\ replayIndex <= Len(events)
    /\ \A index \in 1..replayIndex:
          /\ replayed[index].kind = events[index].kind
          /\ replayed[index].kind = GeneratedKinds[index]
          /\ replayed[index].bodyLen = events[index].bodyLen

FinishIffExactExhaustion ==
    phase \in {"ready", "committed"} => replayIndex = Len(events)

AtomicPublication ==
    /\ publicImage # "partial"
    /\ (phase = "committed"
        => /\ replayIndex = Len(events)
           /\ privateBytes = ExpectedBytes
           /\ publicImage = "new"
           /\ publicationCount = 1)
    /\ (phase # "committed"
        => /\ publicImage = "old"
           /\ publicationCount = 0)

(***************************************************************************
Calibrated negative controls. Each configuration enables one mutant and
requires TLC to produce the named invariant witness.
***************************************************************************)

IncompleteSealMutant ==
    /\ phase = "preflight"
    /\ preflightPc = 8
    /\ ~events[1].completed
    /\ phase' = "sealed"
    /\ preflightPc' = 9
    /\ UNCHANGED <<events, storageMode, inlineStorage, spillStorage,
                    replayIndex, replayed, privateBytes, publicImage,
                    publicationCount>>

DuplicateCompletionMutant ==
    /\ phase = "preflight"
    /\ \E slot \in 1..Len(events):
        /\ events[slot].completed
        /\ events' = [events EXCEPT ![slot].completions = @ + 1]
        /\ IF storageMode = "inline"
              THEN /\ inlineStorage' = [inlineStorage EXCEPT
                            ![slot].completions = @ + 1]
                   /\ UNCHANGED spillStorage
              ELSE /\ spillStorage' = [spillStorage EXCEPT
                            ![slot].completions = @ + 1]
                   /\ UNCHANGED inlineStorage
    /\ UNCHANGED <<phase, preflightPc, storageMode, replayIndex, replayed,
                    privateBytes, publicImage, publicationCount>>

EqualLengthKindSwapMutant ==
    /\ phase = "emitting"
    /\ replayIndex = 0
    /\ events[1].bodyLen = events[2].bodyLen
    /\ replayIndex' = 1
    /\ replayed' = Append(replayed,
                          [kind |-> events[2].kind,
                           bodyLen |-> events[2].bodyLen])
    /\ privateBytes' = privateBytes + events[2].bodyLen
    /\ UNCHANGED <<phase, preflightPc, events, storageMode, inlineStorage,
                    spillStorage, publicImage, publicationCount>>

ReplayOverrunMutant ==
    /\ phase = "ready"
    /\ replayIndex = Len(events)
    /\ replayIndex' = replayIndex + 1
    /\ replayed' = Append(replayed,
                          [kind |-> "ParBody", bodyLen |-> 0])
    /\ UNCHANGED <<phase, preflightPc, events, storageMode, inlineStorage,
                    spillStorage, privateBytes, publicImage, publicationCount>>

RemainingCommitMutant ==
    /\ phase = "emitting"
    /\ replayIndex < Len(events)
    /\ phase' = "committed"
    /\ publicImage' = "new"
    /\ publicationCount' = 1
    /\ UNCHANGED <<preflightPc, events, storageMode, inlineStorage,
                    spillStorage, replayIndex, replayed, privateBytes>>

SpillSlotCorruptionMutant ==
    /\ phase = "preflight"
    /\ preflightPc = 8
    /\ storageMode = "spill"
    /\ ~events[1].completed
    /\ events' = [events EXCEPT
                    ![1].bodyLen = 7,
                    ![1].completed = TRUE,
                    ![1].completions = @ + 1]
    /\ inlineStorage' = [inlineStorage EXCEPT
                    ![1].bodyLen = 7,
                    ![1].completed = TRUE,
                    ![1].completions = @ + 1]
    /\ preflightPc' = 9
    /\ UNCHANGED <<phase, storageMode, spillStorage, replayIndex, replayed,
                    privateBytes, publicImage, publicationCount>>

PartialPublicationMutant ==
    /\ phase = "emitting"
    /\ replayIndex < Len(events)
    /\ publicImage' = "partial"
    /\ UNCHANGED <<phase, preflightPc, events, storageMode, inlineStorage,
                    spillStorage, replayIndex, replayed, privateBytes,
                    publicationCount>>

IncompleteSealMutantNext == Next \/ IncompleteSealMutant
IncompleteSealMutantSpec == Init /\ [][IncompleteSealMutantNext]_vars

DuplicateCompletionMutantNext == Next \/ DuplicateCompletionMutant
DuplicateCompletionMutantSpec == Init /\ [][DuplicateCompletionMutantNext]_vars

EqualLengthKindSwapMutantNext == Next \/ EqualLengthKindSwapMutant
EqualLengthKindSwapMutantSpec == Init /\ [][EqualLengthKindSwapMutantNext]_vars

ReplayOverrunMutantNext == Next \/ ReplayOverrunMutant
ReplayOverrunMutantSpec == Init /\ [][ReplayOverrunMutantNext]_vars

RemainingCommitMutantNext == Next \/ RemainingCommitMutant
RemainingCommitMutantSpec == Init /\ [][RemainingCommitMutantNext]_vars

SpillSlotCorruptionMutantNext == Next \/ SpillSlotCorruptionMutant
SpillSlotCorruptionMutantSpec == Init /\ [][SpillSlotCorruptionMutantNext]_vars

PartialPublicationMutantNext == Next \/ PartialPublicationMutant
PartialPublicationMutantSpec == Init /\ [][PartialPublicationMutantNext]_vars

=============================================================================
