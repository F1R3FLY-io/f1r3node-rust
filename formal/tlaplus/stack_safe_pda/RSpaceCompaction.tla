--------------------------- MODULE RSpaceCompaction ---------------------------
EXTENDS Naturals, FiniteSets, TLC

(******************************************************************************
Bounded contract for retained-root DAG compaction. Objects are abstract digest
identities; `edges` is the canonical ObjectRef relation. The model separates
logical root preservation from physical publication and lease-safe reclamation.
No action may expose a mixed catalog, delete an object needed by a pinned reader,
or publish a closure with a dangling edge.
******************************************************************************)

CONSTANTS Objects, Roots, MaxWork, oldPack, newPack, oldCatalog, newCatalog,
          SkipLeasedClosure

ASSUME Objects # {}
ASSUME Roots # {}
ASSUME MaxWork \in Nat
ASSUME SkipLeasedClosure \in BOOLEAN

edges(x) == IF x = "o0" THEN {"o1"} ELSE IF x = "o1" THEN {"o2"} ELSE {}
rootClosure(r) == IF r = "r0" THEN {"o0"} ELSE {"o2"}
PackNames == {oldPack, newPack}
Catalogs == {oldCatalog, newCatalog}

 VARIABLES leases,
          marked, copied, catalog, published, deleted, work,
          phase, failed

vars == <<leases,
           marked, copied, catalog, published, deleted, work,
           phase, failed>>

(* The executable model uses a bounded explicit frontier; this predicate is
   the denotational closure used by the safety invariants. *)
currentRoots == {"r0"}
retainedRoots == {"r1"}

Closure(seed) ==
  LET Reach(S) == S \cup UNION {edges(x) : x \in S}
      closed == {S \in SUBSET Objects : Reach(S) = S}
  IN CHOOSE S \in closed : seed \subseteq S /\ \A T \in closed : seed \subseteq T => S \subseteq T

AllRetainedRoots == currentRoots \cup retainedRoots \cup leases
LeasedClosure == UNION {Closure(rootClosure(r)) : r \in leases}

Init ==
  /\ Objects = {"o0", "o1", "o2"}
  /\ Roots = {"r0", "r1"}
  /\ leases = {}
  /\ marked = {}
  /\ copied = {}
  /\ catalog = oldCatalog
  /\ published = FALSE
  /\ deleted = {}
  /\ work = {}
  /\ phase = "idle"
  /\ failed = FALSE

StartMark ==
  /\ phase = "idle"
  /\ work' = UNION {rootClosure(r) : r \in AllRetainedRoots}
  /\ marked' = {}
  /\ copied' = {}
  /\ phase' = "marking"
  /\ UNCHANGED <<leases,
                 catalog, published, deleted, failed>>

MarkOne ==
  /\ phase = "marking"
  /\ work # {}
  /\ Cardinality(work) <= MaxWork
  /\ LET x == CHOOSE y \in work : TRUE
         successors == edges(x)
     IN /\ marked' = marked \cup {x}
        /\ work' = (work \ {x}) \cup (successors \ marked)
  /\ UNCHANGED <<leases,
                 copied, catalog, published, deleted, phase, failed>>

FinishMark ==
  /\ phase = "marking"
  /\ work = {}
  /\ marked = UNION {Closure(rootClosure(r)) : r \in AllRetainedRoots}
  /\ phase' = "copying"
  /\ UNCHANGED <<leases,
                 marked, copied, catalog, published, deleted, work, failed>>

CopyOne ==
  /\ phase = "copying"
  /\ marked # copied
  /\ LET x == CHOOSE y \in marked \ copied : TRUE
     IN copied' = copied \cup {x}
  /\ UNCHANGED <<leases,
                 marked, catalog, published, deleted, work, phase, failed>>

Publish ==
  /\ phase = "copying"
  /\ copied = marked
  /\ published = FALSE
  /\ copied = UNION {Closure(rootClosure(r)) : r \in AllRetainedRoots}
  /\ catalog' = newCatalog
  /\ published' = TRUE
  /\ phase' = "published"
  /\ UNCHANGED <<leases,
                 marked, copied, deleted, work, failed>>

DeleteOld ==
  /\ phase = "published"
  /\ published
  /\ deleted' = deleted \cup
       (marked \ (IF SkipLeasedClosure
                  THEN UNION {rootClosure(r) : r \in leases}
                  ELSE LeasedClosure))
  /\ phase' = "done"
  /\ UNCHANGED <<leases,
                 marked, copied, catalog, published, work, failed>>

AcquireLease ==
  /\ phase = "published"
  /\ \E r \in Roots \ leases : leases' = leases \cup {r}
  /\ UNCHANGED <<marked, copied, catalog, published, deleted, work, phase, failed>>

ReleaseLease ==
  /\ leases # {}
  /\ LET r == CHOOSE x \in leases : TRUE
     IN leases' = leases \ {r}
  /\ UNCHANGED <<marked, copied, catalog, published, deleted, work, phase, failed>>

AbortBeforePublish ==
  /\ ~published
  /\ phase \in {"marking", "copying"}
  /\ failed' = TRUE
  /\ phase' = "aborted"
  /\ catalog' = oldCatalog
  /\ published' = FALSE
  /\ UNCHANGED <<leases,
                 marked, copied, deleted, work>>

Next == StartMark \/ MarkOne \/ FinishMark \/ CopyOne \/ Publish \/
        DeleteOld \/ AcquireLease \/ ReleaseLease \/ AbortBeforePublish

Spec == Init /\ [][Next]_vars

TypeInvariant ==
  /\ Objects = {"o0", "o1", "o2"}
  /\ Roots = {"r0", "r1"}
  /\ marked \subseteq Objects
  /\ copied \subseteq Objects
  /\ deleted \subseteq Objects
  /\ catalog \in Catalogs
  /\ phase \in {"idle", "marking", "copying", "published", "done", "aborted"}

NoDanglingEdge ==
  /\ copied = marked => UNION {edges(x) : x \in copied} \subseteq marked

RootClosurePreserved ==
  /\ published => \A r \in AllRetainedRoots :
                     Closure(rootClosure(r)) \subseteq copied

CatalogAtomicity ==
  catalog \in {oldCatalog, newCatalog}

PrePublishRollback ==
  /\ ~published => catalog = oldCatalog

LeaseSafety ==
  deleted \cap LeasedClosure = {}

NoDeletionBeforePublication ==
  deleted # {} => published

BoundedFrontier == Cardinality(work) <= MaxWork

THEOREM Spec => [](
  TypeInvariant /\ CatalogAtomicity /\ PrePublishRollback /\
  RootClosurePreserved /\ LeaseSafety /\
  NoDeletionBeforePublication /\ BoundedFrontier)

=============================================================================
