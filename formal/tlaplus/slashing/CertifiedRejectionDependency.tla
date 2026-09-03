------------------ MODULE CertifiedRejectionDependency ------------------
EXTENDS Integers

CONSTANT
    \* @type: Bool;
    DropRejectedMetadata,
    \* @type: Bool;
    InvalidIndexSatisfiesDependency,
    \* @type: Bool;
    RejectedCountsAsAccepted,
    \* @type: Bool;
    RejectionCreatesSlashEvidence,
    \* @type: Bool;
    RemoveBufferBeforeMetadata

ASSUME /\ DropRejectedMetadata \in BOOLEAN
       /\ InvalidIndexSatisfiesDependency \in BOOLEAN
       /\ RejectedCountsAsAccepted \in BOOLEAN
       /\ RejectionCreatesSlashEvidence \in BOOLEAN
       /\ RemoveBufferBeforeMetadata \in BOOLEAN

DependencyStates == {"Absent", "Accepted", "Rejected"}
ChildStates == {"Waiting", "Accepted", "Rejected"}

VARIABLES
    \* @type: Str;
    dependencyState,
    \* @type: Bool;
    dependencyBuffered,
    \* @type: Bool;
    dependencyCertified,
    \* @type: Bool;
    invalidIndex,
    \* @type: Bool;
    slashEvidence,
    \* @type: Str;
    childState,
    \* @type: Bool;
    childBuffered,
    \* @type: Bool;
    requestPending,
    \* @type: Int;
    deliveries

vars == <<dependencyState, dependencyBuffered, dependencyCertified,
          invalidIndex, slashEvidence, childState, childBuffered,
          requestPending, deliveries>>

CanonicalDependencyAvailable ==
    dependencyState \in {"Accepted", "Rejected"}

DependencyAvailable ==
    CanonicalDependencyAvailable
    \/ (InvalidIndexSatisfiesDependency /\ invalidIndex)

DependencyAccepted ==
    dependencyState = "Accepted"
    \/ (RejectedCountsAsAccepted /\ dependencyState = "Rejected")

Init ==
    /\ dependencyState = "Absent"
    /\ dependencyBuffered = TRUE
    /\ dependencyCertified = FALSE
    /\ invalidIndex = FALSE
    /\ slashEvidence = FALSE
    /\ childState = "Waiting"
    /\ childBuffered = TRUE
    /\ requestPending = TRUE
    /\ deliveries = 1

PublishInvalidIndex ==
    /\ ~invalidIndex
    /\ invalidIndex' = TRUE
    /\ UNCHANGED <<dependencyState, dependencyBuffered,
                    dependencyCertified, slashEvidence, childState,
                    childBuffered, requestPending, deliveries>>

CertifyNonSlashableRejection ==
    /\ ~dependencyCertified
    /\ dependencyCertified' = TRUE
    /\ invalidIndex' = TRUE
    /\ slashEvidence' = RejectionCreatesSlashEvidence
    /\ dependencyState' =
        IF DropRejectedMetadata \/ RemoveBufferBeforeMetadata
        THEN "Absent"
        ELSE "Rejected"
    /\ dependencyBuffered' = DropRejectedMetadata
    /\ requestPending' =
        IF DropRejectedMetadata \/ RemoveBufferBeforeMetadata
        THEN requestPending
        ELSE FALSE
    /\ UNCHANGED <<childState, childBuffered, deliveries>>

ResolveChild ==
    /\ childState = "Waiting"
    /\ DependencyAvailable
    /\ childState' = IF DependencyAccepted THEN "Accepted" ELSE "Rejected"
    /\ childBuffered' = FALSE
    /\ requestPending' = FALSE
    /\ UNCHANGED <<dependencyState, dependencyBuffered,
                    dependencyCertified, invalidIndex, slashEvidence,
                    deliveries>>

RedeliverDependency ==
    /\ deliveries < 3
    /\ deliveries' = deliveries + 1
    /\ UNCHANGED <<dependencyState, dependencyBuffered,
                    dependencyCertified, invalidIndex, slashEvidence,
                    childState, childBuffered, requestPending>>

Next ==
    PublishInvalidIndex
    \/ CertifyNonSlashableRejection
    \/ ResolveChild
    \/ RedeliverDependency

Spec ==
    Init
    /\ [][Next]_vars
    /\ WF_vars(CertifyNonSlashableRejection)
    /\ WF_vars(ResolveChild)

TypeOK ==
    /\ dependencyState \in DependencyStates
    /\ dependencyBuffered \in BOOLEAN
    /\ dependencyCertified \in BOOLEAN
    /\ invalidIndex \in BOOLEAN
    /\ slashEvidence \in BOOLEAN
    /\ childState \in ChildStates
    /\ childBuffered \in BOOLEAN
    /\ requestPending \in BOOLEAN
    /\ deliveries \in 1..3

CertifiedRejectionIsDurable ==
    dependencyCertified => dependencyState = "Rejected"

NonSlashableRejectionHasNoEvidence ==
    ~slashEvidence

TerminalChildRequiresCanonicalDependency ==
    childState # "Waiting" => CanonicalDependencyAvailable

RejectedDependencyCannotAcceptChild ==
    dependencyState = "Rejected" => childState # "Accepted"

RejectedChildRequiresRejectedDependency ==
    childState = "Rejected" => dependencyState = "Rejected"

BufferRemovalRequiresTerminalMetadata ==
    dependencyCertified /\ ~dependencyBuffered => CanonicalDependencyAvailable

TerminalDependencyStopsRequests ==
    CanonicalDependencyAvailable => ~requestPending

ResolvedChildLeavesBuffer ==
    childState # "Waiting" => ~childBuffered

Safety ==
    /\ TypeOK
    /\ CertifiedRejectionIsDurable
    /\ NonSlashableRejectionHasNoEvidence
    /\ TerminalChildRequiresCanonicalDependency
    /\ RejectedDependencyCannotAcceptChild
    /\ RejectedChildRequiresRejectedDependency
    /\ BufferRemovalRequiresTerminalMetadata
    /\ TerminalDependencyStopsRequests
    /\ ResolvedChildLeavesBuffer

RejectionEventuallyClassifiesChild ==
    <>(/\ dependencyState = "Rejected"
       /\ childState = "Rejected"
       /\ ~dependencyBuffered
       /\ ~childBuffered
       /\ ~requestPending
       /\ ~slashEvidence)

=============================================================================
