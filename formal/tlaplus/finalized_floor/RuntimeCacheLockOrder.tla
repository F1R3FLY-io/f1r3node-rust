----------------------- MODULE RuntimeCacheLockOrder -----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Readers, IndexedReaders, HasIndexLock, Defect
VARIABLES pc, ep, shardReaders, shardWriter, orderOwner, indexOwner,
          present, observed, copied

vars == <<pc, ep, shardReaders, shardWriter, orderOwner, indexOwner,
          present, observed, copied>>
None == "none"
Evictor == "evictor"
Actors == Readers \cup {Evictor}
Value == "cached-value"

ASSUME /\ Readers # {}
       /\ IsFiniteSet(Readers)
       /\ IndexedReaders \subseteq Readers
       /\ HasIndexLock \in BOOLEAN
       /\ ~HasIndexLock => IndexedReaders = {}
       /\ None \notin Actors
       /\ Evictor \notin Readers
       /\ Defect \in {"Safe", "HoldReadAcrossTouch", "ShadowCloneWithoutDrop"}

Init ==
    /\ pc = [r \in Readers |-> "Index"]
    /\ ep = "Index"
    /\ shardReaders = {} /\ shardWriter = FALSE
    /\ orderOwner = None /\ indexOwner = None
    /\ present = TRUE
    /\ observed = [r \in Readers |-> None]
    /\ copied = [r \in Readers |-> None]

ReaderIndex(r) ==
    /\ pc[r] = "Index"
    /\ r \notin IndexedReaders \/ indexOwner = None
    /\ indexOwner' = IF r \in IndexedReaders THEN r ELSE indexOwner
    /\ pc' = [pc EXCEPT ![r] = "Read"]
    /\ UNCHANGED <<ep, shardReaders, shardWriter, orderOwner, present, observed, copied>>

ReaderRead(r) ==
    /\ pc[r] = "Read" /\ ~shardWriter
    /\ shardReaders' = IF present THEN shardReaders \cup {r} ELSE shardReaders
    /\ observed' = [observed EXCEPT ![r] = IF present THEN Value ELSE None]
    /\ pc' = [pc EXCEPT ![r] = IF ~present THEN "ReleaseIndex"
        ELSE IF Defect = "HoldReadAcrossTouch" THEN "RequestOrder" ELSE "Copy"]
    /\ UNCHANGED <<ep, shardWriter, orderOwner, indexOwner, present, copied>>

ReaderCopy(r) ==
    /\ pc[r] = "Copy"
    /\ copied' = [copied EXCEPT ![r] = observed[r]]
    /\ pc' = [pc EXCEPT ![r] = IF Defect = "Safe" THEN "ReleaseRead" ELSE "RequestOrder"]
    /\ UNCHANGED <<ep, shardReaders, shardWriter, orderOwner, indexOwner, present, observed>>

ReaderReleaseRead(r) ==
    /\ pc[r] = "ReleaseRead"
    /\ shardReaders' = shardReaders \ {r}
    /\ pc' = [pc EXCEPT ![r] = "RequestOrder"]
    /\ UNCHANGED <<ep, shardWriter, orderOwner, indexOwner, present, observed, copied>>

ReaderOrder(r) ==
    /\ pc[r] = "RequestOrder" /\ orderOwner = None
    /\ orderOwner' = r
    /\ pc' = [pc EXCEPT ![r] = "Touch"]
    /\ UNCHANGED <<ep, shardReaders, shardWriter, indexOwner, present, observed, copied>>

ReaderTouch(r) ==
    /\ pc[r] = "Touch" /\ orderOwner = r
    /\ pc' = [pc EXCEPT ![r] = "ReleaseOrder"]
    /\ UNCHANGED <<ep, shardReaders, shardWriter, orderOwner, indexOwner,
                   present, observed, copied>>

ReaderReleaseOrder(r) ==
    /\ pc[r] = "ReleaseOrder" /\ orderOwner = r
    /\ orderOwner' = None
    /\ pc' = [pc EXCEPT ![r] = CASE Defect = "HoldReadAcrossTouch" -> "CopyLate"
        [] Defect = "ShadowCloneWithoutDrop" -> "DropRead"
        [] OTHER -> "ReleaseIndex"]
    /\ UNCHANGED <<ep, shardReaders, shardWriter, indexOwner, present, observed, copied>>

ReaderCopyLate(r) ==
    /\ pc[r] = "CopyLate"
    /\ copied' = [copied EXCEPT ![r] = observed[r]]
    /\ pc' = [pc EXCEPT ![r] = "DropRead"]
    /\ UNCHANGED <<ep, shardReaders, shardWriter, orderOwner, indexOwner, present, observed>>

ReaderDropRead(r) ==
    /\ pc[r] = "DropRead"
    /\ shardReaders' = shardReaders \ {r}
    /\ pc' = [pc EXCEPT ![r] = "ReleaseIndex"]
    /\ UNCHANGED <<ep, shardWriter, orderOwner, indexOwner, present, observed, copied>>

ReaderFinish(r) ==
    /\ pc[r] = "ReleaseIndex"
    /\ indexOwner' = IF indexOwner = r THEN None ELSE indexOwner
    /\ pc' = [pc EXCEPT ![r] = "Done"]
    /\ UNCHANGED <<ep, shardReaders, shardWriter, orderOwner, present, observed, copied>>

ReaderStep(r) == ReaderIndex(r) \/ ReaderRead(r) \/ ReaderCopy(r)
    \/ ReaderReleaseRead(r) \/ ReaderOrder(r) \/ ReaderTouch(r)
    \/ ReaderReleaseOrder(r) \/ ReaderCopyLate(r) \/ ReaderDropRead(r) \/ ReaderFinish(r)

EvictorIndex ==
    /\ ep = "Index" /\ (~HasIndexLock \/ indexOwner = None)
    /\ indexOwner' = IF HasIndexLock THEN Evictor ELSE indexOwner
    /\ ep' = "Order"
    /\ UNCHANGED <<pc, shardReaders, shardWriter, orderOwner, present, observed, copied>>

EvictorOrder ==
    /\ ep = "Order" /\ orderOwner = None
    /\ orderOwner' = Evictor /\ ep' = "Write"
    /\ UNCHANGED <<pc, shardReaders, shardWriter, indexOwner, present, observed, copied>>

EvictorWrite ==
    /\ ep = "Write" /\ shardReaders = {} /\ ~shardWriter
    /\ shardWriter' = TRUE /\ ep' = "Remove"
    /\ UNCHANGED <<pc, shardReaders, orderOwner, indexOwner, present, observed, copied>>

EvictorRemove ==
    /\ ep = "Remove" /\ shardWriter /\ orderOwner = Evictor
    /\ present' = FALSE /\ ep' = "ReleaseWrite"
    /\ UNCHANGED <<pc, shardReaders, shardWriter, orderOwner, indexOwner, observed, copied>>

EvictorReleaseWrite ==
    /\ ep = "ReleaseWrite"
    /\ shardWriter' = FALSE /\ ep' = "ReleaseOrder"
    /\ UNCHANGED <<pc, shardReaders, orderOwner, indexOwner, present, observed, copied>>

EvictorReleaseOrder ==
    /\ ep = "ReleaseOrder"
    /\ orderOwner' = None /\ ep' = "ReleaseIndex"
    /\ UNCHANGED <<pc, shardReaders, shardWriter, indexOwner, present, observed, copied>>

EvictorFinish ==
    /\ ep = "ReleaseIndex"
    /\ indexOwner' = IF indexOwner = Evictor THEN None ELSE indexOwner
    /\ ep' = "Done"
    /\ UNCHANGED <<pc, shardReaders, shardWriter, orderOwner, present, observed, copied>>

EvictorStep == EvictorIndex \/ EvictorOrder \/ EvictorWrite \/ EvictorRemove
    \/ EvictorReleaseWrite \/ EvictorReleaseOrder \/ EvictorFinish
AllDone == ep = "Done" /\ \A r \in Readers : pc[r] = "Done"
Next == (\E r \in Readers : ReaderStep(r)) \/ EvictorStep \/ (AllDone /\ UNCHANGED vars)

WaitEdges ==
    {<<a,b>> \in Actors \X Actors :
        IF a = Evictor THEN
            \/ ep = "Index" /\ HasIndexLock /\ indexOwner = b
            \/ ep = "Order" /\ orderOwner = b
            \/ ep = "Write" /\ b \in shardReaders
        ELSE
            \/ pc[a] = "Index" /\ a \in IndexedReaders /\ indexOwner = b
            \/ pc[a] = "Read" /\ shardWriter /\ b = Evictor
            \/ pc[a] = "RequestOrder" /\ orderOwner = b}

RECURSIVE Reach(_)
Reach(n) == IF n = 0 THEN WaitEdges ELSE
    LET previous == Reach(n-1) IN
    previous \cup {<<a,c>> \in Actors \X Actors :
        \E b \in Actors : <<a,b>> \in previous /\ <<b,c>> \in WaitEdges}

TypeOK ==
    /\ pc \in [Readers -> {"Index", "Read", "Copy", "ReleaseRead", "RequestOrder",
                          "Touch", "ReleaseOrder", "CopyLate", "DropRead", "ReleaseIndex", "Done"}]
    /\ ep \in {"Index", "Order", "Write", "Remove", "ReleaseWrite", "ReleaseOrder", "ReleaseIndex", "Done"}
    /\ shardReaders \subseteq Readers /\ shardWriter \in BOOLEAN
    /\ orderOwner \in Actors \cup {None} /\ indexOwner \in Actors \cup {None}
    /\ present \in BOOLEAN
    /\ observed \in [Readers -> {None,Value}] /\ copied \in [Readers -> {None,Value}]
NoShardGuardAtTouchRequest == \A r \in Readers :
    pc[r] \in {"RequestOrder", "Touch", "ReleaseOrder"} => r \notin shardReaders
NoWaitForCycle == \A a \in Actors : <<a,a>> \notin Reach(Cardinality(Actors))
ReturnedCopyMatchesRead == \A r \in Readers : pc[r] = "Done" => copied[r] = observed[r]
ShardMutualExclusion == shardWriter => shardReaders = {}
GuardPhaseCoherence ==
    /\ \A r \in Readers :
        (r \in shardReaders) <=>
            (pc[r] \in {"Copy", "ReleaseRead", "CopyLate", "DropRead"}
             \/ (Defect # "Safe" /\ pc[r] \in {"RequestOrder", "Touch", "ReleaseOrder"}))
    /\ shardWriter <=> ep \in {"Remove", "ReleaseWrite"}
    /\ \A r \in Readers : (orderOwner = r) <=> pc[r] \in {"Touch", "ReleaseOrder"}
    /\ (orderOwner = Evictor) <=> ep \in {"Write", "Remove", "ReleaseWrite", "ReleaseOrder"}
    /\ \A r \in Readers : (indexOwner = r) <=>
        (r \in IndexedReaders /\ pc[r] \notin {"Index", "Done"})
    /\ (indexOwner = Evictor) <=> (HasIndexLock /\ ep \notin {"Index", "Done"})
NoCompletedGuardOwner ==
    /\ \A r \in Readers : pc[r] = "Done" =>
        r \notin shardReaders /\ orderOwner # r /\ indexOwner # r
    /\ ep = "Done" => ~shardWriter /\ orderOwner # Evictor /\ indexOwner # Evictor
EventuallyCompleted == <>AllDone
Spec == Init /\ [][Next]_vars /\ WF_vars(EvictorStep)
    /\ \A r \in Readers : WF_vars(ReaderStep(r))
=============================================================================
