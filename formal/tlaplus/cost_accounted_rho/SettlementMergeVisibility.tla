------------------------ MODULE SettlementMergeVisibility ------------------------
EXTENDS Naturals, TLC

CONSTANTS OmitSettlementEvent, CollapseInstanceIdentity,
          SkipReplayRemoval, DropTracePrefix

ASSUME /\ OmitSettlementEvent \in BOOLEAN
       /\ CollapseInstanceIdentity \in BOOLEAN
       /\ SkipReplayRemoval \in BOOLEAN
       /\ DropTracePrefix \in BOOLEAN

VARIABLES phase, siblingSameInstance, stateChanged, eventRecorded,
          replayRemoved, mergeConflict, tracePrefixPresent

vars == <<phase, siblingSameInstance, stateChanged, eventRecorded,
          replayRemoved, mergeConflict, tracePrefixPresent>>

Init ==
  /\ phase = "Snapshot"
  /\ siblingSameInstance \in BOOLEAN
  /\ stateChanged = FALSE
  /\ eventRecorded = FALSE
  /\ replayRemoved = FALSE
  /\ mergeConflict = FALSE
  /\ tracePrefixPresent = TRUE

Snapshot ==
  /\ phase = "Snapshot"
  /\ phase' = "Settle"
  /\ UNCHANGED <<siblingSameInstance, stateChanged, eventRecorded,
                 replayRemoved, mergeConflict, tracePrefixPresent>>

Settle ==
  /\ phase = "Settle"
  /\ stateChanged' = TRUE
  /\ eventRecorded' = ~OmitSettlementEvent
  /\ tracePrefixPresent' = ~DropTracePrefix
  /\ phase' = "Replay"
  /\ UNCHANGED <<siblingSameInstance, replayRemoved, mergeConflict>>

Replay ==
  /\ phase = "Replay"
  /\ replayRemoved' = IF SkipReplayRemoval THEN FALSE ELSE eventRecorded
  /\ phase' = "Merge"
  /\ UNCHANGED <<siblingSameInstance, stateChanged, eventRecorded,
                 mergeConflict, tracePrefixPresent>>

Merge ==
  /\ phase = "Merge"
  /\ mergeConflict' =
       eventRecorded /\ (siblingSameInstance \/ CollapseInstanceIdentity)
  /\ phase' = "Done"
  /\ UNCHANGED <<siblingSameInstance, stateChanged, eventRecorded,
                 replayRemoved, tracePrefixPresent>>

Next == Snapshot \/ Settle \/ Replay \/ Merge

Spec == /\ Init
        /\ [][Next]_vars

TypeOK ==
  /\ phase \in {"Snapshot", "Settle", "Replay", "Merge", "Done"}
  /\ siblingSameInstance \in BOOLEAN
  /\ stateChanged \in BOOLEAN
  /\ eventRecorded \in BOOLEAN
  /\ replayRemoved \in BOOLEAN
  /\ mergeConflict \in BOOLEAN
  /\ tracePrefixPresent \in BOOLEAN

SettlementStateChangeIsIndexed ==
  phase \in {"Replay", "Merge", "Done"} => (stateChanged => eventRecorded)

ReplayReproducesRemoval ==
  phase \in {"Merge", "Done"} => (eventRecorded => replayRemoved)

SameInstanceSettlementsConflict ==
  phase = "Done" => (siblingSameInstance => mergeConflict)

DistinctInstancesRemainMergeable ==
  phase = "Done" => (~siblingSameInstance => ~mergeConflict)

SoftCheckpointPreservesTracePrefix ==
  phase \in {"Replay", "Merge", "Done"} => tracePrefixPresent

=============================================================================
