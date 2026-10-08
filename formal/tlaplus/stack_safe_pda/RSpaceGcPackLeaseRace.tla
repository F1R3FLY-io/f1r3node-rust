------------------------ MODULE RSpaceGcPackLeaseRace ------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANT ExactKeyOnly
ASSUME ExactKeyOnly \in BOOLEAN

FirstKey == [epoch |-> 2, first |-> 1, last |-> 2, index |-> 2]
OlderSameKey == [epoch |-> 1, first |-> 1, last |-> 2, index |-> 2]
PartialKey == [epoch |-> 3, first |-> 2, last |-> 3, index |-> 3]
SecondKey == [epoch |-> 4, first |-> 3, last |-> 4, index |-> 4]
OutsideKey == [epoch |-> 5, first |-> 5, last |-> 5, index |-> 5]
Readers == {"older", "target", "partial", "successor", "outside"}
KeyOf(reader) ==
  CASE reader = "older" -> OlderSameKey
    [] reader = "target" -> FirstKey
    [] reader = "partial" -> PartialKey
    [] reader = "successor" -> SecondKey
    [] reader = "outside" -> OutsideKey
FirstPack(key) == key.first
LastPack(key) == key.last
Packs(key) == FirstPack(key)..LastPack(key)
Overlaps(left, right) ==
  FirstPack(left) <= LastPack(right) /\ FirstPack(right) <= LastPack(left)
FirstPacks == Packs(FirstKey)
SecondPacks == Packs(SecondKey)
Max2(left, right) == IF left >= right THEN left ELSE right

VARIABLES leases, retiredThrough, priorFrontier, stage, deleted,
          deferredOlder, retiredAfterOlder, witnessComplete

vars == <<leases, retiredThrough, priorFrontier, stage, deleted,
          deferredOlder, retiredAfterOlder, witnessComplete>>

Blocking(key) ==
  {reader \in leases :
    IF ExactKeyOnly THEN KeyOf(reader) = key ELSE Overlaps(KeyOf(reader), key)}

Init ==
  /\ leases = {}
  /\ retiredThrough = 0
  /\ priorFrontier = 0
  /\ stage = 0
  /\ deleted = {}
  /\ deferredOlder = FALSE
  /\ retiredAfterOlder = FALSE
  /\ witnessComplete = FALSE

Acquire(reader) ==
  /\ reader \in Readers \ leases
  /\ FirstPack(KeyOf(reader)) > retiredThrough
  /\ leases' = leases \cup {reader}
  /\ UNCHANGED <<retiredThrough, priorFrontier, stage, deleted,
                 deferredOlder, retiredAfterOlder, witnessComplete>>

Release(reader) ==
  /\ reader \in leases
  /\ leases' = leases \ {reader}
  /\ UNCHANGED <<retiredThrough, priorFrontier, stage, deleted,
                 deferredOlder, retiredAfterOlder, witnessComplete>>

BlockFirst ==
  /\ stage = 0
  /\ Blocking(FirstKey) # {}
  /\ deferredOlder' = (deferredOlder \/ ("older" \in Blocking(FirstKey)))
  /\ UNCHANGED <<leases, retiredThrough, priorFrontier, stage, deleted,
                 retiredAfterOlder, witnessComplete>>

RetireFirst ==
  /\ stage = 0
  /\ Blocking(FirstKey) = {}
  /\ stage' = 1
  /\ priorFrontier' = retiredThrough
  /\ retiredThrough' = Max2(retiredThrough, LastPack(FirstKey))
  /\ retiredAfterOlder' = (deferredOlder /\ "older" \notin leases)
  /\ UNCHANGED <<leases, deleted, deferredOlder, witnessComplete>>

DeleteFirst ==
  /\ stage >= 1
  /\ \E pack \in FirstPacks \ deleted :
       /\ deleted' = deleted \cup {pack}
       /\ witnessComplete' =
            (witnessComplete \/
             (retiredAfterOlder /\ FirstPacks \subseteq deleted \cup {pack}))
  /\ UNCHANGED <<leases, retiredThrough, priorFrontier, stage,
                 deferredOlder, retiredAfterOlder>>

RetireSecond ==
  /\ stage = 1
  /\ FirstPacks \subseteq deleted
  /\ Blocking(SecondKey) = {}
  /\ stage' = 2
  /\ priorFrontier' = retiredThrough
  /\ retiredThrough' = Max2(retiredThrough, LastPack(SecondKey))
  /\ UNCHANGED <<leases, deleted, deferredOlder, retiredAfterOlder,
                 witnessComplete>>

DeleteSecond ==
  /\ stage = 2
  /\ \E pack \in SecondPacks \ deleted : deleted' = deleted \cup {pack}
  /\ UNCHANGED <<leases, retiredThrough, priorFrontier, stage,
                 deferredOlder, retiredAfterOlder, witnessComplete>>

Next ==
  (\E reader \in Readers : Acquire(reader)) \/
  (\E reader \in Readers : Release(reader)) \/
  BlockFirst \/ RetireFirst \/ DeleteFirst \/ RetireSecond \/ DeleteSecond

Spec == Init /\ [][Next]_vars

TypeInvariant ==
  /\ leases \subseteq Readers
  /\ retiredThrough \in 0..4
  /\ priorFrontier \in 0..4
  /\ stage \in 0..2
  /\ deleted \subseteq FirstPacks \cup SecondPacks
  /\ deferredOlder \in BOOLEAN
  /\ retiredAfterOlder \in BOOLEAN
  /\ witnessComplete \in BOOLEAN

PhysicalOverlapTopology ==
  /\ OlderSameKey # FirstKey
  /\ OlderSameKey.index = FirstKey.index
  /\ Packs(OlderSameKey) = FirstPacks
  /\ PartialKey # FirstKey
  /\ Packs(PartialKey) # FirstPacks
  /\ Overlaps(PartialKey, FirstKey)
  /\ ~Overlaps(SecondKey, FirstKey)
  /\ ~Overlaps(OutsideKey, SecondKey)
RetiredFrontierMonotone == priorFrontier <= retiredThrough
RetiredFrontierMatchesStage ==
  (stage = 0 => retiredThrough = 0) /\
  (stage = 1 => retiredThrough = 2) /\
  (stage = 2 => retiredThrough = 4)
LiveLeaseOutsideRetiredFrontier ==
  \A reader \in leases : FirstPack(KeyOf(reader)) > retiredThrough
DeletedPackUnleased ==
  \A pack \in deleted : \A reader \in leases : pack \notin Packs(KeyOf(reader))
DeletionFollowsRetirement ==
  /\ (deleted \cap FirstPacks # {} => stage >= 1)
  /\ (deleted \cap SecondPacks # {} => stage = 2)
  /\ \A pack \in deleted : pack <= retiredThrough
OlderEpochPinDefersFirst == "older" \in leases => stage = 0
PartialOverlapPinDefersFirst == "partial" \in leases => stage = 0
OlderEpochPackSafe == "older" \in leases => deleted \cap FirstPacks = {}
DisjointSuccessorReadRemainsPossible ==
  (stage = 1 /\ "successor" \notin leases) => ENABLED Acquire("successor")
DisjointPackReadRemainsPossible ==
  "outside" \notin leases => ENABLED Acquire("outside")
OlderLifecycleHistorySound ==
  /\ (retiredAfterOlder => deferredOlder /\ stage >= 1)
  /\ (witnessComplete => retiredAfterOlder /\ FirstPacks \subseteq deleted)
OlderLifecycleAbsent == ~witnessComplete

=============================================================================
