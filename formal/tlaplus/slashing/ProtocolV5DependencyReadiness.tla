------------------ MODULE ProtocolV5DependencyReadiness ------------------
EXTENDS FiniteSets, TLC

CONSTANT
    \* @type: Bool;
    InvalidIndexSatisfiesDependency,
    \* @type: Bool;
    TrackerSatisfiesDependency,
    \* @type: Bool;
    OmitObjectivePairSecond,
    \* @type: Bool;
    BufferOmitsHeaderProof

ASSUME /\ InvalidIndexSatisfiesDependency \in BOOLEAN
       /\ TrackerSatisfiesDependency \in BOOLEAN
       /\ OmitObjectivePairSecond \in BOOLEAN
       /\ BufferOmitsHeaderProof \in BOOLEAN

\* @type: Set(Str);
Hashes == {"parent", "justification", "unary", "objective-first",
           "objective-second", "header-first", "header-second"}

\* @type: Set(Str);
ObjectivePair == {"objective-first", "objective-second"}
\* @type: Set(Str);
HeaderProof == {"header-first", "header-second"}
\* @type: Set(Str);
RequiredDependencies == Hashes
\* @type: Set(Str);
InvalidIndexHints == {"unary", "objective-second"}
\* @type: Set(Str);
TrackerHints == {"objective-first"}
\* @type: Set(Str);
Phases == {"Unseen", "Waiting", "Ready"}

\* @type: Set(Str);
DirectRequired ==
    IF OmitObjectivePairSecond
    THEN RequiredDependencies \ {"objective-second"}
    ELSE RequiredDependencies

\* @type: Set(Str);
BufferRequired ==
    IF BufferOmitsHeaderProof
    THEN RequiredDependencies \ {"header-second"}
    ELSE RequiredDependencies

VARIABLES
    \* @type: Set(Str);
    admittedMetadata,
    \* @type: Set(Str);
    invalidIndex,
    \* @type: Set(Str);
    tracker,
    \* @type: Bool;
    submitted,
    \* @type: Str;
    directPhase,
    \* @type: Str;
    bufferPhase,
    \* @type: Set(Str);
    requested

vars == <<admittedMetadata, invalidIndex, tracker, submitted,
          directPhase, bufferPhase, requested>>

Available(required) ==
    required \subseteq
        admittedMetadata
        \cup (IF InvalidIndexSatisfiesDependency THEN invalidIndex ELSE {})
        \cup (IF TrackerSatisfiesDependency THEN tracker ELSE {})

DirectAvailable == Available(DirectRequired)
BufferAvailable == Available(BufferRequired)

Init ==
    /\ admittedMetadata = {}
    /\ invalidIndex = {}
    /\ tracker = {}
    /\ submitted = FALSE
    /\ directPhase = "Unseen"
    /\ bufferPhase = "Unseen"
    /\ requested = {}

Submit ==
    /\ ~submitted
    /\ submitted' = TRUE
    /\ UNCHANGED <<admittedMetadata, invalidIndex, tracker, directPhase,
                    bufferPhase, requested>>

AdmitMetadata(h) ==
    /\ h \in RequiredDependencies
    /\ admittedMetadata' = admittedMetadata \cup {h}
    /\ requested' = requested \ {h}
    /\ UNCHANGED <<invalidIndex, tracker, submitted, directPhase, bufferPhase>>

AdmitAllExcept(h) ==
    /\ h \in RequiredDependencies
    /\ admittedMetadata' = admittedMetadata \cup (RequiredDependencies \ {h})
    /\ requested' = requested \ (RequiredDependencies \ {h})
    /\ UNCHANGED <<invalidIndex, tracker, submitted, directPhase, bufferPhase>>

PublishInvalidIndex(h) ==
    /\ h \in InvalidIndexHints
    /\ invalidIndex' = invalidIndex \cup {h}
    /\ UNCHANGED <<admittedMetadata, tracker, submitted, directPhase,
                    bufferPhase, requested>>

PublishTracker(h) ==
    /\ h \in TrackerHints
    /\ tracker' = tracker \cup {h}
    /\ UNCHANGED <<admittedMetadata, invalidIndex, submitted, directPhase,
                    bufferPhase, requested>>

ResolveDirect ==
    /\ submitted
    /\ directPhase # "Ready"
    /\ IF DirectAvailable
       THEN
         /\ directPhase' = "Ready"
         /\ UNCHANGED requested
       ELSE
         /\ directPhase' = "Waiting"
         /\ requested' = requested \cup (DirectRequired \ admittedMetadata)
    /\ UNCHANGED <<admittedMetadata, invalidIndex, tracker, submitted,
                    bufferPhase>>

ResolveBuffer ==
    /\ submitted
    /\ bufferPhase # "Ready"
    /\ IF BufferAvailable
       THEN
         /\ bufferPhase' = "Ready"
         /\ UNCHANGED requested
       ELSE
         /\ bufferPhase' = "Waiting"
         /\ requested' = requested \cup (BufferRequired \ admittedMetadata)
    /\ UNCHANGED <<admittedMetadata, invalidIndex, tracker, submitted,
                    directPhase>>

Next ==
    \/ Submit
    \/ \E h \in RequiredDependencies : AdmitMetadata(h)
    \/ \E h \in RequiredDependencies : AdmitAllExcept(h)
    \/ \E h \in InvalidIndexHints : PublishInvalidIndex(h)
    \/ \E h \in TrackerHints : PublishTracker(h)
    \/ ResolveDirect
    \/ ResolveBuffer

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ admittedMetadata \in SUBSET RequiredDependencies
    /\ invalidIndex \in SUBSET InvalidIndexHints
    /\ tracker \in SUBSET TrackerHints
    /\ submitted \in BOOLEAN
    /\ directPhase \in Phases
    /\ bufferPhase \in Phases
    /\ requested \in SUBSET RequiredDependencies

Inv_DirectReadyRequiresAdmittedMetadata ==
    directPhase = "Ready" => RequiredDependencies \subseteq admittedMetadata

Inv_BufferReadyRequiresAdmittedMetadata ==
    bufferPhase = "Ready" => RequiredDependencies \subseteq admittedMetadata

Inv_ObjectivePairComplete ==
    (directPhase = "Ready" \/ bufferPhase = "Ready") =>
        ObjectivePair \subseteq admittedMetadata

Inv_HeaderProofComplete ==
    (directPhase = "Ready" \/ bufferPhase = "Ready") =>
        HeaderProof \subseteq admittedMetadata

Inv_InvalidIndexNoninterference ==
    /\ DirectAvailable = (DirectRequired \subseteq admittedMetadata)
    /\ BufferAvailable = (BufferRequired \subseteq admittedMetadata)

Inv_TrackerNoninterference ==
    /\ DirectAvailable =
          (DirectRequired \subseteq
              admittedMetadata
              \cup (IF InvalidIndexSatisfiesDependency THEN invalidIndex ELSE {}))
    /\ BufferAvailable =
          (BufferRequired \subseteq
              admittedMetadata
              \cup (IF InvalidIndexSatisfiesDependency THEN invalidIndex ELSE {}))

Inv_DirectBufferRulesExact ==
    /\ DirectRequired = RequiredDependencies
    /\ BufferRequired = RequiredDependencies
    /\ DirectAvailable = BufferAvailable

Inv_WaitingTracksAllMissing ==
    /\ directPhase = "Waiting" =>
          RequiredDependencies \ admittedMetadata \subseteq requested
    /\ bufferPhase = "Waiting" =>
          RequiredDependencies \ admittedMetadata \subseteq requested

Inv_ReadyOnlyAfterSubmission ==
    (directPhase = "Ready" \/ bufferPhase = "Ready") => submitted

Safety ==
    /\ TypeOK
    /\ Inv_DirectReadyRequiresAdmittedMetadata
    /\ Inv_BufferReadyRequiresAdmittedMetadata
    /\ Inv_ObjectivePairComplete
    /\ Inv_HeaderProofComplete
    /\ Inv_InvalidIndexNoninterference
    /\ Inv_TrackerNoninterference
    /\ Inv_DirectBufferRulesExact
    /\ Inv_WaitingTracksAllMissing
    /\ Inv_ReadyOnlyAfterSubmission

=============================================================================
