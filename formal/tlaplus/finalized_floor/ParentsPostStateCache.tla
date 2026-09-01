---------------------- MODULE ParentsPostStateCache ----------------------
EXTENDS FiniteSets, Integers, Sequences

CONSTANT
    \* @type: Bool;
    BindMainParent

ASSUME BindMainParent \in BOOLEAN

Parents == {"p0", "p1", "p2"}
\* @type: Seq(Str);
RequestA == <<"p0", "p1", "p2">>
\* @type: Seq(Str);
RequestATail == <<"p0", "p2", "p1">>
\* @type: Seq(Str);
RequestB == <<"p1", "p0", "p2">>
Requests == {RequestA, RequestATail, RequestB}

\* @type: (Seq(Str)) => Set(Str);
ParentSet(request) == {request[1], request[2], request[3]}
\* @type: (Seq(Str)) => Set(Str);
SecondarySet(request) == {request[2], request[3]}
\* @type: (Seq(Str)) => Str;
ExpectedState(request) == request[1]
\* @type: (Seq(Str)) => <<Str, Set(Str)>>;
CacheKey(request) ==
    IF BindMainParent
    THEN <<request[1], SecondarySet(request)>>
    ELSE <<"all", ParentSet(request)>>

VARIABLES
    \* @type: Bool;
    cacheValid,
    \* @type: <<Str, Set(Str)>>;
    cacheKey,
    \* @type: Str;
    cacheState,
    \* @type: Set(<<Seq(Str), Str>>);
    observations

vars == <<cacheValid, cacheKey, cacheState, observations>>

Init ==
    /\ cacheValid = FALSE
    /\ cacheKey = CacheKey(RequestA)
    /\ cacheState = ExpectedState(RequestA)
    /\ observations = {}

Publish(request) ==
    /\ request \in Requests
    /\ cacheValid' = TRUE
    /\ cacheKey' = CacheKey(request)
    /\ cacheState' = ExpectedState(request)
    /\ UNCHANGED observations

Read(request) ==
    /\ request \in Requests
    /\ cacheValid
    /\ observations' = observations \union {
        <<request,
          IF cacheKey = CacheKey(request)
          THEN cacheState
          ELSE ExpectedState(request)>>
       }
    /\ UNCHANGED <<cacheValid, cacheKey, cacheState>>

Next ==
    \/ \E request \in Requests : Publish(request)
    \/ \E request \in Requests : Read(request)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ cacheValid \in BOOLEAN
    /\ \E request \in Requests : cacheKey = CacheKey(request)
    /\ cacheState \in Parents
    /\ observations \subseteq Requests \X Parents

CacheKeySound ==
    \A left \in Requests, right \in Requests :
        CacheKey(left) = CacheKey(right)
        => ExpectedState(left) = ExpectedState(right)

SecondaryPermutationConfluence ==
    /\ CacheKey(RequestA) = CacheKey(RequestATail)
    /\ ExpectedState(RequestA) = ExpectedState(RequestATail)

CachedStatePreservesMainParent ==
    \A observation \in observations : observation[2] = ExpectedState(observation[1])

=============================================================================
