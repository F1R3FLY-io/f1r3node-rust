---------------- MODULE ReplayCacheContext ----------------
EXTENDS Naturals, FiniteSets

CONSTANT
    \* @type: Str;
    Defect

ASSUME Defect \in {
    "None", "OmitTimestamp", "OmitHeight", "OmitInvalidBlocks",
    "IgnoreHostLimit", "IgnoreMetadata", "PublishUnvalidated"
}

Workers == {1, 2}
Contexts == [timestamp : 1..2, height : 1..2, invalidBlocks : 0..1]
EmptyContext == [timestamp |-> 1, height |-> 1, invalidBlocks |-> 0]
Phases == {"Ready", "Miss", "Computed", "Validated", "Stored",
           "Done", "Rejected", "Cancelled"}

\* @typeAlias: replayContext = { timestamp: Int, height: Int, invalidBlocks: Int };
module_typedefs == TRUE

VARIABLES
    \* @type: Int -> $replayContext;
    request,
    \* @type: Int -> Bool;
    limited,
    \* @type: Int -> Str;
    phase,
    \* @type: Set($replayContext);
    cache,
    \* @type: Bool;
    cacheValid,
    \* @type: Set($replayContext);
    metadata,
    \* @type: Int -> $replayContext;
    returned,
    \* @type: Int -> Bool;
    fromCache,
    \* @type: Int -> Bool;
    metadataAtHit

vars == <<request, limited, phase, cache, cacheValid, metadata,
          returned, fromCache, metadataAtHit>>

\* @type: ($replayContext, $replayContext) => Bool;
SameKey(a, b) ==
    /\ (Defect = "OmitTimestamp" \/ a.timestamp = b.timestamp)
    /\ (Defect = "OmitHeight" \/ a.height = b.height)
    /\ (Defect = "OmitInvalidBlocks" \/ a.invalidBlocks = b.invalidBlocks)

Init ==
    /\ request \in [Workers -> Contexts]
    /\ limited \in [Workers -> BOOLEAN]
    /\ phase = [w \in Workers |-> "Ready"]
    /\ cache = {c \in Contexts : FALSE}
    /\ cacheValid = TRUE
    /\ metadata = {c \in Contexts : FALSE}
    /\ returned = [w \in Workers |-> EmptyContext]
    /\ fromCache = [w \in Workers |-> FALSE]
    /\ metadataAtHit = [w \in Workers |-> FALSE]

\* @type: Int => Set($replayContext);
Candidates(w) ==
    {c \in cache :
        SameKey(c, request[w])
        /\ (~limited[w] \/ Defect = "IgnoreHostLimit")
        /\ (c \in metadata \/ Defect = "IgnoreMetadata")}

\* @type: Int => Bool;
Lookup(w) ==
    /\ phase[w] = "Ready"
    /\ IF Candidates(w) = {}
       THEN /\ phase' = [phase EXCEPT ![w] = "Miss"]
            /\ UNCHANGED <<returned, fromCache, metadataAtHit>>
       ELSE \E c \in Candidates(w) :
            /\ phase' = [phase EXCEPT ![w] = "Done"]
            /\ returned' = [returned EXCEPT ![w] = c]
            /\ fromCache' = [fromCache EXCEPT ![w] = TRUE]
            /\ metadataAtHit' = [metadataAtHit EXCEPT ![w] = c \in metadata]
    /\ UNCHANGED <<request, limited, cache, cacheValid, metadata>>

\* @type: Int => Bool;
Compute(w) ==
    /\ phase[w] = "Miss"
    /\ phase' = [phase EXCEPT ![w] = "Computed"]
    /\ UNCHANGED <<request, limited, cache, cacheValid, metadata,
                   returned, fromCache, metadataAtHit>>

\* @type: Int => Bool;
Validate(w) ==
    /\ phase[w] = "Computed"
    /\ phase' = [phase EXCEPT ![w] = "Validated"]
    /\ UNCHANGED <<request, limited, cache, cacheValid, metadata,
                   returned, fromCache, metadataAtHit>>

\* @type: Int => Bool;
Reject(w) ==
    /\ phase[w] = "Computed"
    /\ phase' = [phase EXCEPT ![w] = "Rejected"]
    /\ UNCHANGED <<request, limited, cache, cacheValid, metadata,
                   returned, fromCache, metadataAtHit>>

\* @type: Int => Bool;
Store(w) ==
    /\ phase[w] = "Validated"
    /\ phase' = [phase EXCEPT ![w] = "Stored"]
    /\ metadata' = metadata \cup {request[w]}
    /\ UNCHANGED <<request, limited, cache, cacheValid,
                   returned, fromCache, metadataAtHit>>

\* @type: Int => Bool;
Publish(w) ==
    /\ phase[w] = "Stored"
    /\ phase' = [phase EXCEPT ![w] = "Done"]
    /\ cache' = {request[w]}
    /\ cacheValid' = TRUE
    /\ returned' = [returned EXCEPT ![w] = request[w]]
    /\ UNCHANGED <<request, limited, metadata, fromCache, metadataAtHit>>

\* @type: Int => Bool;
PublishUnvalidated(w) ==
    /\ Defect = "PublishUnvalidated"
    /\ phase[w] = "Computed"
    /\ cache' = {request[w]}
    /\ cacheValid' = FALSE
    /\ UNCHANGED <<request, limited, phase, metadata,
                   returned, fromCache, metadataAtHit>>

\* @type: Int => Bool;
Cancel(w) ==
    /\ phase[w] \notin {"Done", "Rejected", "Cancelled"}
    /\ phase' = [phase EXCEPT ![w] = "Cancelled"]
    /\ UNCHANGED <<request, limited, cache, cacheValid, metadata,
                   returned, fromCache, metadataAtHit>>

Evict ==
    /\ cache # {}
    /\ cache' = {}
    /\ cacheValid' = TRUE
    /\ UNCHANGED <<request, limited, phase, metadata,
                   returned, fromCache, metadataAtHit>>

\* @type: $replayContext => Bool;
DropMetadata(c) ==
    /\ c \in metadata
    /\ metadata' = metadata \ {c}
    /\ UNCHANGED <<request, limited, phase, cache, cacheValid,
                   returned, fromCache, metadataAtHit>>

Next ==
    \/ \E w \in Workers :
        Lookup(w) \/ Compute(w) \/ Validate(w) \/ Reject(w) \/ Store(w)
        \/ Publish(w) \/ PublishUnvalidated(w) \/ Cancel(w)
    \/ Evict
    \/ \E c \in Contexts : DropMetadata(c)

TypeOK ==
    /\ request \in [Workers -> Contexts]
    /\ limited \in [Workers -> BOOLEAN]
    /\ phase \in [Workers -> Phases]
    /\ cache \subseteq Contexts
    /\ Cardinality(cache) <= 1
    /\ cacheValid \in BOOLEAN
    /\ metadata \subseteq Contexts
    /\ returned \in [Workers -> Contexts]
    /\ fromCache \in [Workers -> BOOLEAN]
    /\ metadataAtHit \in [Workers -> BOOLEAN]

ReplayUsesExactContext ==
    \A w \in Workers : phase[w] = "Done" => returned[w] = request[w]

NoUnvalidatedPublication == cache = {} \/ cacheValid

BoundedWorkCannotHitCache ==
    \A w \in Workers : limited[w] => ~fromCache[w]

HitHadMetadata ==
    \A w \in Workers : fromCache[w] => metadataAtHit[w]

Spec == Init /\ [][Next]_vars
============================================================
