---------------- MODULE EvaluationBoundary ----------------
EXTENDS FiniteSets

CONSTANTS
    \* @type: Bool;
    RetainPermitForDetachedChildren,
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
    mutations,
    permitHeld,
    rootCancelled,
    checkpointed,
    checkpointMutations
>>

Init ==
    /\ active = Participants
    /\ mutations = {}
    /\ permitHeld = TRUE
    /\ rootCancelled = FALSE
    /\ checkpointed = FALSE
    /\ checkpointMutations = {}

CancelRoot ==
    /\ Root \in active
    /\ ~rootCancelled
    /\ active' = active \ {Root}
    /\ rootCancelled' = TRUE
    /\ permitHeld' =
        IF RetainPermitForDetachedChildren
        THEN active' /= {}
        ELSE FALSE
    /\ UNCHANGED <<mutations, checkpointed, checkpointMutations>>

Complete(participant) ==
    /\ participant \in active
    /\ active' = active \ {participant}
    /\ mutations' =
        IF participant \in Children
        THEN mutations \union {participant}
        ELSE mutations
    /\ permitHeld' = (active' /= {})
    /\ UNCHANGED <<rootCancelled, checkpointed, checkpointMutations>>

Checkpoint ==
    /\ ~checkpointed
    /\ IF CheckpointRequiresExclusivePermit THEN ~permitHeld ELSE TRUE
    /\ checkpointed' = TRUE
    /\ checkpointMutations' = mutations
    /\ UNCHANGED <<active, mutations, permitHeld, rootCancelled>>

Next ==
    CancelRoot
    \/ (\E participant \in Participants : Complete(participant))
    \/ Checkpoint

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ active \subseteq Participants
    /\ mutations \subseteq Children
    /\ permitHeld \in BOOLEAN
    /\ rootCancelled \in BOOLEAN
    /\ checkpointed \in BOOLEAN
    /\ checkpointMutations \subseteq Children

Inv_PermitTracksEvaluationLifetime ==
    RetainPermitForDetachedChildren => (permitHeld = (active /= {}))

Inv_CheckpointAtEvaluationQuiescence ==
    checkpointed => active = {}

Inv_CheckpointContainsAllCompletedChildMutations ==
    checkpointed => checkpointMutations = Children

=============================================================================
