-------------------------- MODULE PendingOwnerRegistry --------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS KeyCount, OwnerCount, HandleCount, Unsafe
Keys == 1..KeyCount
Owners == 1..OwnerCount
Handles == 1..HandleCount

VARIABLES used, allocated, registry, handles, ownerKey, ownerEpoch, ownerValue,
          terminal, diskEpoch, diskValue, lastWriteEpoch, detached
vars == <<used, allocated, registry, handles, ownerKey, ownerEpoch, ownerValue,
          terminal, diskEpoch, diskValue, lastWriteEpoch, detached>>

Referenced(o) == \E h \in Handles : handles[h] = o
Upgradeable(o) == o \in allocated /\ Referenced(o) /\ o \notin terminal
RegistryKeys == {k \in Keys : registry[k] # 0}

Init ==
    /\ used = {} /\ allocated = {} /\ terminal = {} /\ detached = {}
    /\ registry = [k \in Keys |-> 0]
    /\ handles = [h \in Handles |-> 0]
    /\ ownerKey = [o \in Owners |-> 1]
    /\ ownerEpoch = [o \in Owners |-> 1]
    /\ ownerValue = [o \in Owners |-> 0]
    /\ diskEpoch = [k \in Keys |-> 1]
    /\ diskValue = [k \in Keys |-> 0]
    /\ lastWriteEpoch = [k \in Keys |-> 1]

Upgrade(h, k) ==
    /\ handles[h] = 0 /\ Upgradeable(registry[k])
    /\ handles' = [handles EXCEPT ![h] = registry[k]]
    /\ UNCHANGED <<used, allocated, registry, ownerKey, ownerEpoch, ownerValue,
                    terminal, diskEpoch, diskValue, lastWriteEpoch, detached>>

Load(h, k, o) ==
    /\ handles[h] = 0 /\ o \notin used
    /\ ~Upgradeable(registry[k]) \/ Unsafe = "duplicate-load"
    /\ used' = used \cup {o} /\ allocated' = allocated \cup {o}
    /\ handles' = [handles EXCEPT ![h] = o]
    /\ registry' = [q \in Keys |-> IF q = k THEN o
         ELSE IF Unsafe = "unpruned-weak" \/ Upgradeable(registry[q])
              THEN registry[q] ELSE 0]
    /\ ownerKey' = [ownerKey EXCEPT ![o] = k]
    /\ ownerEpoch' = [ownerEpoch EXCEPT ![o] = diskEpoch[k]]
    /\ ownerValue' = [ownerValue EXCEPT ![o] = diskValue[k]]
    /\ UNCHANGED <<terminal, diskEpoch, diskValue, lastWriteEpoch, detached>>

Complete(h) ==
    /\ handles[h] # 0
    /\ LET o == handles[h] IN
       /\ o \notin terminal \/ Unsafe = "terminal-write"
       /\ ownerValue[o] < 2
       /\ ownerValue' = [ownerValue EXCEPT ![o] = @ + 1]
       /\ diskValue' = [diskValue EXCEPT ![ownerKey[o]] = ownerValue[o] + 1]
       /\ lastWriteEpoch' = [lastWriteEpoch EXCEPT ![ownerKey[o]] = ownerEpoch[o]]
    /\ UNCHANGED <<used, allocated, registry, handles, ownerKey, ownerEpoch,
                    terminal, diskEpoch, detached>>

Finish(h) ==
    /\ handles[h] # 0
    /\ LET o == handles[h] IN
       /\ o \notin terminal /\ diskEpoch[ownerKey[o]] = 1
       /\ terminal' = terminal \cup {o}
       /\ diskEpoch' = [diskEpoch EXCEPT ![ownerKey[o]] = 2]
       /\ diskValue' = [diskValue EXCEPT ![ownerKey[o]] = 0]
       /\ lastWriteEpoch' = [lastWriteEpoch EXCEPT ![ownerKey[o]] = 2]
       /\ registry' = [registry EXCEPT ![ownerKey[o]] = 0]
    /\ UNCHANGED <<used, allocated, handles, ownerKey, ownerEpoch, ownerValue, detached>>

Release(h) ==
    /\ handles[h] # 0
    /\ LET o == handles[h]
           remaining == \E other \in Handles \ {h} : handles[other] = o
       IN
       /\ handles' = [handles EXCEPT ![h] = 0]
       /\ allocated' = IF remaining \/ Unsafe = "detached-owner"
                        THEN allocated ELSE allocated \ {o}
       /\ detached' = IF ~remaining /\ Unsafe = "detached-owner"
                       THEN detached \cup {o} ELSE detached
    /\ UNCHANGED <<used, registry, ownerKey, ownerEpoch, ownerValue, terminal,
                    diskEpoch, diskValue, lastWriteEpoch>>

Next == (\E h \in Handles, k \in Keys : Upgrade(h, k))
        \/ (\E h \in Handles, k \in Keys, o \in Owners : Load(h, k, o))
        \/ (\E h \in Handles : Complete(h) \/ Finish(h) \/ Release(h))

TypeOK ==
    /\ used \subseteq Owners /\ allocated \subseteq used /\ terminal \subseteq used
    /\ detached \subseteq allocated
    /\ registry \in [Keys -> 0..OwnerCount] /\ handles \in [Handles -> 0..OwnerCount]
    /\ ownerKey \in [Owners -> Keys] /\ ownerEpoch \in [Owners -> 1..2]
    /\ ownerValue \in [Owners -> 0..2]
    /\ diskEpoch \in [Keys -> 1..2] /\ diskValue \in [Keys -> 0..2]
    /\ lastWriteEpoch \in [Keys -> 1..2]

Inv_HandleLifetime == \A h \in Handles : handles[h] = 0 \/ handles[h] \in allocated
Inv_OwnerWitness == \A o \in allocated : Referenced(o)
Inv_OwnerBound == Cardinality(allocated) <= HandleCount
Inv_WeakBound == Cardinality(RegistryKeys) <= HandleCount
Inv_CanonicalOwner == \A k \in Keys :
    Cardinality({o \in allocated \ terminal : ownerKey[o] = k /\ ownerEpoch[o] = diskEpoch[k]}) <= 1
Inv_CurrentWriter == \A k \in Keys : lastWriteEpoch[k] = diskEpoch[k]
Inv_CoherentPolicy == \A o \in allocated \ terminal :
    ownerEpoch[o] = diskEpoch[ownerKey[o]] /\ ownerValue[o] = diskValue[ownerKey[o]]

Spec == Init /\ [][Next]_vars
=============================================================================
