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
\* Protocol 6 is the fresh-genesis cost-accounted wire protocol. Protocol 5 is
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
    RejectUnsupportedApprovedVersion,
    GenesisUsesProtocolOccurrenceIdentity,
    GenesisUsesProtocolExecutionIdentity,
    GenesisReplayUsesProtocolExecutionIdentity,
    GenesisPrincipalProjectsGroundCustody

LegacyProtocol == 5
CurrentProtocol == 6
UnsupportedProtocol == 7
NoVersion == 0
Versions == {LegacyProtocol, CurrentProtocol, UnsupportedProtocol}
SupportedVersions == {CurrentProtocol}

NoIdentity == "none"
LegacyBlessedIdentity == "legacy-blessed"
ProtocolEnvelopeIdentity == "protocol-envelope"
Identities == {NoIdentity, LegacyBlessedIdentity, ProtocolEnvelopeIdentity}

NoCustody == "none"
GroundCustody == "ground-custody"
RejectedCustody == "rejected-custody"
Custodies == {NoCustody, GroundCustody, RejectedCustody}

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
    acceptedBy,
    genesisOccurrenceIdentity,
    genesisConstructionIdentity,
    genesisReplayIdentity,
    genesisCustody

vars == <<
    phase,
    candidateVersion,
    approvals,
    approvedVersion,
    runningVersions,
    proposedVersion,
    receiverExpectedVersions,
    acceptedBy,
    genesisOccurrenceIdentity,
    genesisConstructionIdentity,
    genesisReplayIdentity,
    genesisCustody
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
    /\ genesisOccurrenceIdentity = NoIdentity
    /\ genesisConstructionIdentity = NoIdentity
    /\ genesisReplayIdentity = NoIdentity
    /\ genesisCustody = NoCustody

Ceremony ==
    /\ phase = "ceremony"
    /\ phase' = "candidate"
    /\ candidateVersion' =
        IF CeremonyUsesConfiguredVersion
        THEN ConfiguredVersion(Master)
        ELSE LegacyProtocol
    /\ genesisOccurrenceIdentity' =
        IF GenesisUsesProtocolOccurrenceIdentity
        THEN ProtocolEnvelopeIdentity
        ELSE LegacyBlessedIdentity
    /\ genesisConstructionIdentity' =
        IF GenesisUsesProtocolExecutionIdentity
        THEN ProtocolEnvelopeIdentity
        ELSE LegacyBlessedIdentity
    /\ genesisCustody' =
        IF GenesisPrincipalProjectsGroundCustody
        THEN GroundCustody
        ELSE RejectedCustody
    /\ UNCHANGED <<approvals, approvedVersion, runningVersions,
                    proposedVersion, receiverExpectedVersions, acceptedBy,
                    genesisReplayIdentity>>

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
                    receiverExpectedVersions, acceptedBy,
                    genesisOccurrenceIdentity, genesisConstructionIdentity,
                    genesisReplayIdentity, genesisCustody>>

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
                    proposedVersion, receiverExpectedVersions, acceptedBy,
                    genesisOccurrenceIdentity, genesisConstructionIdentity,
                    genesisReplayIdentity, genesisCustody>>

Propose ==
    /\ phase = "running"
    /\ phase' = "proposed"
    /\ proposedVersion' =
        IF ProposerUsesRunningVersion
        THEN runningVersions[Proposer]
        ELSE ConfiguredVersion(Proposer)
    /\ UNCHANGED <<candidateVersion, approvals, approvedVersion,
                    runningVersions, receiverExpectedVersions, acceptedBy,
                    genesisOccurrenceIdentity, genesisConstructionIdentity,
                    genesisReplayIdentity, genesisCustody>>

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
                    runningVersions, proposedVersion,
                    genesisOccurrenceIdentity, genesisConstructionIdentity,
                    genesisReplayIdentity, genesisCustody>>

Replay ==
    /\ phase = "received"
    /\ phase' = "replayed"
    /\ genesisReplayIdentity' =
        IF GenesisReplayUsesProtocolExecutionIdentity
        THEN ProtocolEnvelopeIdentity
        ELSE LegacyBlessedIdentity
    /\ UNCHANGED <<candidateVersion, approvals, approvedVersion,
                    runningVersions, proposedVersion, receiverExpectedVersions,
                    acceptedBy, genesisOccurrenceIdentity,
                    genesisConstructionIdentity, genesisCustody>>

Next == Ceremony \/ Approve \/ Adopt \/ Propose \/ Receive \/ Replay

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"ceremony", "candidate", "approved", "stalled",
                   "rejected", "running", "proposed", "received", "replayed"}
    /\ candidateVersion \in Versions \union {NoVersion}
    /\ approvals \subseteq Validators
    /\ approvedVersion \in Versions \union {NoVersion}
    /\ runningVersions \in [Nodes -> Versions \union {NoVersion}]
    /\ proposedVersion \in Versions \union {NoVersion}
    /\ receiverExpectedVersions \in [Nodes -> Versions \union {NoVersion}]
    /\ acceptedBy \subseteq Nodes
    /\ genesisOccurrenceIdentity \in Identities
    /\ genesisConstructionIdentity \in Identities
    /\ genesisReplayIdentity \in Identities
    /\ genesisCustody \in Custodies

Inv_CeremonyCandidateCurrent ==
    (~RecoverApprovedBlock
        /\ phase \in {"candidate", "approved", "running", "proposed", "received", "replayed"})
    => candidateVersion = CurrentProtocol

Inv_ApprovalsBindCurrentCandidate ==
    (~RecoverApprovedBlock
        /\ phase \in {"approved", "running", "proposed", "received", "replayed"})
    => approvals = Validators /\ approvedVersion = CurrentProtocol

Inv_ApprovedVersionSupported ==
    phase \in {"running", "proposed", "received", "replayed"}
    => approvedVersion \in SupportedVersions

Inv_UnsupportedApprovedFailsClosed ==
    approvedVersion \notin SupportedVersions
    => phase \notin {"running", "proposed", "received", "replayed"}

Inv_RunningNodesAdoptApproved ==
    phase \in {"running", "proposed", "received", "replayed"}
    => \A node \in Nodes : runningVersions[node] = approvedVersion

Inv_ProposalUsesApprovedVersion ==
    phase \in {"proposed", "received", "replayed"}
    => proposedVersion = approvedVersion

Inv_ReceiversUseApprovedVersion ==
    phase \in {"received", "replayed"}
    => \A node \in Nodes : receiverExpectedVersions[node] = approvedVersion

Inv_AllReceiversAccept ==
    phase \in {"received", "replayed"} => acceptedBy = Nodes

Inv_CurrentCeremonyEndToEnd ==
    (~RecoverApprovedBlock /\ phase = "replayed")
    => /\ approvedVersion = CurrentProtocol
       /\ proposedVersion = CurrentProtocol
       /\ acceptedBy = Nodes

Inv_CurrentGenesisIdentityUnified ==
    (~RecoverApprovedBlock
        /\ phase \in {"candidate", "approved", "running", "proposed", "received", "replayed"})
    => /\ genesisOccurrenceIdentity = ProtocolEnvelopeIdentity
       /\ genesisConstructionIdentity = ProtocolEnvelopeIdentity

Inv_CurrentGenesisReplayDeterministic ==
    (~RecoverApprovedBlock /\ phase = "replayed")
    => /\ genesisReplayIdentity = ProtocolEnvelopeIdentity
       /\ genesisReplayIdentity = genesisConstructionIdentity

Inv_CurrentGenesisCustodyProjection ==
    (~RecoverApprovedBlock
        /\ phase \in {"candidate", "approved", "running", "proposed", "received", "replayed"})
    => genesisCustody = GroundCustody

Inv_LegacyApprovedFailsClosed ==
    (RecoverApprovedBlock /\ RecoveryApprovedVersion = LegacyProtocol)
    => phase \notin {"running", "proposed", "received", "replayed"}
=============================================================================
