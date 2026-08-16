--------------------- MODULE ProtocolVersionLifecycle ---------------------
EXTENDS Naturals, FiniteSets

\* This model covers the protocol-version lifecycle that precedes block merge:
\*
\* Ceremony               ApproveBlockProtocolFactory::create
\* Approve                BlockApproverProtocolImpl::validate_candidate
\* Adopt                  hash_set_casper / adopt_approved_protocol_version
\* Propose                block_creator::create
\* ReceiverExpectedVersion BlockProcessor::check_if_of_interest
\* Receive                Validate::version
\*
\* Protocol 2 is the fresh-genesis cost-accounted wire protocol. Protocol 1 is
\* retained as historical encoding metadata but is not runnable by this binary.
\* Each Boolean constant disables one production obligation so the associated
\* unsafe configuration must reproduce its named counterexample.

CONSTANTS
    RecoverApprovedBlock,
    RecoveryApprovedVersion,
    CeremonyUsesConfiguredVersion,
    ApproversCheckConfiguredVersion,
    NodesAdoptApprovedVersion,
    ProposerUsesRunningVersion,
    ReceiverUsesRunningVersion,
    RejectUnsupportedApprovedVersion

LegacyProtocol == 1
CurrentProtocol == 2
UnsupportedProtocol == 3
NoVersion == 0
Versions == {LegacyProtocol, CurrentProtocol, UnsupportedProtocol}
SupportedVersions == {CurrentProtocol}

Master == "master"
Validator1 == "validator-1"
Validator2 == "validator-2"
Validator3 == "validator-3"
Observer == "observer"
Nodes == {Master, Validator1, Validator2, Validator3, Observer}
Validators == {Validator1, Validator2, Validator3}
Proposer == Validator1
Quorum == 2

ConfiguredVersion(node) == CurrentProtocol

VARIABLES
    phase,
    candidateVersion,
    approvals,
    approvedVersion,
    runningVersions,
    proposedVersion,
    receiverExpectedVersions,
    acceptedBy

vars == <<
    phase,
    candidateVersion,
    approvals,
    approvedVersion,
    runningVersions,
    proposedVersion,
    receiverExpectedVersions,
    acceptedBy
>>

Init ==
    /\ phase = IF RecoverApprovedBlock THEN "approved" ELSE "ceremony"
    /\ candidateVersion =
        IF RecoverApprovedBlock THEN RecoveryApprovedVersion ELSE NoVersion
    /\ approvals = IF RecoverApprovedBlock THEN Validators ELSE {}
    /\ approvedVersion =
        IF RecoverApprovedBlock THEN RecoveryApprovedVersion ELSE NoVersion
    /\ runningVersions = [node \in Nodes |-> NoVersion]
    /\ proposedVersion = NoVersion
    /\ receiverExpectedVersions = [node \in Nodes |-> NoVersion]
    /\ acceptedBy = {}

Ceremony ==
    /\ phase = "ceremony"
    /\ phase' = "candidate"
    /\ candidateVersion' =
        IF CeremonyUsesConfiguredVersion
        THEN ConfiguredVersion(Master)
        ELSE LegacyProtocol
    /\ UNCHANGED <<approvals, approvedVersion, runningVersions,
                    proposedVersion, receiverExpectedVersions, acceptedBy>>

EligibleApprovals ==
    {validator \in Validators :
        ~ApproversCheckConfiguredVersion
        \/ candidateVersion = ConfiguredVersion(validator)}

Approve ==
    /\ phase = "candidate"
    /\ approvals' = EligibleApprovals
    /\ phase' =
        IF Cardinality(EligibleApprovals) >= Quorum
        THEN "approved"
        ELSE "stalled"
    /\ approvedVersion' =
        IF Cardinality(EligibleApprovals) >= Quorum
        THEN candidateVersion
        ELSE NoVersion
    /\ UNCHANGED <<candidateVersion, runningVersions, proposedVersion,
                    receiverExpectedVersions, acceptedBy>>

ApprovedVersionUnsupported == approvedVersion \notin SupportedVersions

Adopt ==
    /\ phase = "approved"
    /\ phase' =
        IF RejectUnsupportedApprovedVersion /\ ApprovedVersionUnsupported
        THEN "rejected"
        ELSE "running"
    /\ runningVersions' =
        IF RejectUnsupportedApprovedVersion /\ ApprovedVersionUnsupported
        THEN runningVersions
        ELSE [node \in Nodes |->
            IF NodesAdoptApprovedVersion
            THEN approvedVersion
            ELSE ConfiguredVersion(node)]
    /\ UNCHANGED <<candidateVersion, approvals, approvedVersion,
                    proposedVersion, receiverExpectedVersions, acceptedBy>>

Propose ==
    /\ phase = "running"
    /\ phase' = "proposed"
    /\ proposedVersion' =
        IF ProposerUsesRunningVersion
        THEN runningVersions[Proposer]
        ELSE ConfiguredVersion(Proposer)
    /\ UNCHANGED <<candidateVersion, approvals, approvedVersion,
                    runningVersions, receiverExpectedVersions, acceptedBy>>

ReceiverExpectedVersion(node) ==
    IF ReceiverUsesRunningVersion
    THEN runningVersions[node]
    ELSE approvedVersion

Receive ==
    /\ phase = "proposed"
    /\ phase' = "received"
    /\ receiverExpectedVersions' =
        [node \in Nodes |-> ReceiverExpectedVersion(node)]
    /\ acceptedBy' =
        {node \in Nodes : ReceiverExpectedVersion(node) = proposedVersion}
    /\ UNCHANGED <<candidateVersion, approvals, approvedVersion,
                    runningVersions, proposedVersion>>

Next == Ceremony \/ Approve \/ Adopt \/ Propose \/ Receive

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"ceremony", "candidate", "approved", "stalled",
                   "rejected", "running", "proposed", "received"}
    /\ candidateVersion \in Versions \union {NoVersion}
    /\ approvals \subseteq Validators
    /\ approvedVersion \in Versions \union {NoVersion}
    /\ runningVersions \in [Nodes -> Versions \union {NoVersion}]
    /\ proposedVersion \in Versions \union {NoVersion}
    /\ receiverExpectedVersions \in [Nodes -> Versions \union {NoVersion}]
    /\ acceptedBy \subseteq Nodes

Inv_CeremonyCandidateCurrent ==
    (~RecoverApprovedBlock
        /\ phase \in {"candidate", "approved", "running", "proposed", "received"})
    => candidateVersion = CurrentProtocol

Inv_ApprovalsBindCurrentCandidate ==
    (~RecoverApprovedBlock
        /\ phase \in {"approved", "running", "proposed", "received"})
    => approvals = Validators /\ approvedVersion = CurrentProtocol

Inv_ApprovedVersionSupported ==
    phase \in {"running", "proposed", "received"}
    => approvedVersion \in SupportedVersions

Inv_UnsupportedApprovedFailsClosed ==
    approvedVersion \notin SupportedVersions
    => phase \notin {"running", "proposed", "received"}

Inv_RunningNodesAdoptApproved ==
    phase \in {"running", "proposed", "received"}
    => \A node \in Nodes : runningVersions[node] = approvedVersion

Inv_ProposalUsesApprovedVersion ==
    phase \in {"proposed", "received"}
    => proposedVersion = approvedVersion

Inv_ReceiversUseApprovedVersion ==
    phase = "received"
    => \A node \in Nodes : receiverExpectedVersions[node] = approvedVersion

Inv_AllReceiversAccept ==
    phase = "received" => acceptedBy = Nodes

Inv_CurrentCeremonyEndToEnd ==
    (~RecoverApprovedBlock /\ phase = "received")
    => /\ approvedVersion = CurrentProtocol
       /\ proposedVersion = CurrentProtocol
       /\ acceptedBy = Nodes

Inv_LegacyApprovedFailsClosed ==
    (RecoverApprovedBlock /\ RecoveryApprovedVersion = LegacyProtocol)
    => phase \notin {"running", "proposed", "received"}
=============================================================================
