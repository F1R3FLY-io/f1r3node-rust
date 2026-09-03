---------------- MODULE EvaluationBoundary ----------------
EXTENDS FiniteSets

CONSTANTS
    \* @type: Bool;
    AbortChildrenOnRootCancellation,
    \* @type: Bool;
    RetainPermitUntilQuiescence,
    \* @type: Bool;
    CheckpointRequiresExclusivePermit

Root == "root"
Child1 == "child-1"
Child2 == "child-2"
Participants == {Root, Child1, Child2}
Children == {Child1, Child2}

VARIABLES
    \* @type: Set(Str);
    active,
    \* @type: Set(Str);
    cancelRequested,
    \* @type: Set(Str);
    mutations,
    \* @type: Bool;
    permitHeld,
    \* @type: Bool;
    rootCancelled,
    \* @type: Bool;
    checkpointed,
    \* @type: Set(Str);
    checkpointMutations

vars == <<
    active,
    cancelRequested,
    mutations,
    permitHeld,
    rootCancelled,
    checkpointed,
    checkpointMutations
>>

PermitFor(nextActive) ==
    IF RetainPermitUntilQuiescence
    THEN nextActive /= {}
    ELSE FALSE

Init ==
    /\ active = Participants
    /\ cancelRequested = {}
    /\ mutations = {}
    /\ permitHeld = TRUE
    /\ rootCancelled = FALSE
    /\ checkpointed = FALSE
    /\ checkpointMutations = {}

CancelRoot ==
    /\ Root \in active
    /\ ~rootCancelled
    /\ LET nextActive == active \ {Root}
       IN /\ active' = nextActive
          /\ cancelRequested' =
              IF AbortChildrenOnRootCancellation
              THEN nextActive
              ELSE {}
          /\ permitHeld' = PermitFor(nextActive)
    /\ rootCancelled' = TRUE
    /\ UNCHANGED <<mutations, checkpointed, checkpointMutations>>

Complete(participant) ==
    /\ participant \in active
    /\ participant \notin cancelRequested
    /\ LET nextActive == active \ {participant}
       IN /\ active' = nextActive
          /\ cancelRequested' = cancelRequested \ {participant}
          /\ mutations' =
              IF participant \in Children
              THEN mutations \union {participant}
              ELSE mutations
          /\ permitHeld' = PermitFor(nextActive)
    /\ UNCHANGED <<rootCancelled, checkpointed, checkpointMutations>>

AbortChild(child) ==
    /\ child \in active
    /\ child \in cancelRequested
    /\ LET nextActive == active \ {child}
       IN /\ active' = nextActive
          /\ cancelRequested' = cancelRequested \ {child}
          /\ permitHeld' = PermitFor(nextActive)
    /\ UNCHANGED <<mutations, rootCancelled, checkpointed,
                    checkpointMutations>>

Checkpoint ==
    /\ ~checkpointed
    /\ IF CheckpointRequiresExclusivePermit THEN ~permitHeld ELSE TRUE
    /\ checkpointed' = TRUE
    /\ checkpointMutations' = mutations
    /\ UNCHANGED <<active, cancelRequested, mutations, permitHeld,
                    rootCancelled>>

AbortRequested == \E child \in Children : AbortChild(child)

Next ==
    CancelRoot
    \/ (\E participant \in Participants : Complete(participant))
    \/ AbortRequested
    \/ Checkpoint

Spec == Init /\ [][Next]_vars /\ WF_vars(AbortRequested)

TypeOK ==
    /\ active \subseteq Participants
    /\ cancelRequested \subseteq Children
    /\ cancelRequested \subseteq active
    /\ mutations \subseteq Children
    /\ permitHeld \in BOOLEAN
    /\ rootCancelled \in BOOLEAN
    /\ checkpointed \in BOOLEAN
    /\ checkpointMutations \subseteq Children

Inv_PermitTracksEvaluationLifetime ==
    RetainPermitUntilQuiescence => (permitHeld = (active /= {}))

Inv_CancelledRootOwnsNoRunnableChildren ==
    (rootCancelled /\ AbortChildrenOnRootCancellation) =>
        active \subseteq cancelRequested

Inv_CheckpointAtEvaluationQuiescence ==
    checkpointed => active = {}

Inv_CheckpointContainsEveryCommittedMutation ==
    checkpointed => checkpointMutations = mutations

StructuredCancellationReachesQuiescence ==
    (rootCancelled /\ AbortChildrenOnRootCancellation) ~> (active = {})

=============================================================================
