-------------------- MODULE SignedFloorReplayReadiness --------------------
EXTENDS FiniteSets, Naturals, TLC

CONSTANT
  \* @type: Str;
  Defect

ASSUME Defect \in {
  "None",
  "FloorSubstitution",
  "HashMismatchAcceptance",
  "StateMismatchAcceptance",
  "HeightMismatchAcceptance",
  "RejectedOccurrenceAcceptance",
  "StoredBlockFloorMismatchAcceptance",
  "StoredBlockStateMismatchAcceptance",
  "StoredBlockHeightMismatchAcceptance",
  "StoredMetadataFloorMismatchAcceptance",
  "StoredMetadataStateMismatchAcceptance",
  "StoredMetadataHeightMismatchAcceptance",
  "CommitmentStatePairMismatchAcceptance",
  "CommitmentHeightPairMismatchAcceptance",
  "SingleAuthorityContext",
  "CertificateDigestOmission",
  "TargetCommitteeForCertificate",
  "PredecessorCommitteeForProposal",
  "InclusiveFtt",
  "CertificateThresholdMismatch",
  "MissingLocalStateProposal",
  "CertificateHeightNotDigestBound",
  "AncestryOmission",
  "MissingRootAcceptance",
  "MissingRootInvalidation",
  "CandidateCommittee",
  "IncompleteLatestAcceptance",
  "InactiveSenderAcceptance",
  "UnsignedMutation",
  "TornCapture",
  "FinalizerCancellation",
  "ReceiverDisagreement"
}

Nodes == {"n1", "n2"}
Validators == {"v1", "v2", "v3"}
Floors == {"G", "A"}
States == {"SG", "SA", "SX"}
Artifacts == {"Certificate", "FloorBlock", "Metadata", "State"}
ProposalPhases == {"Idle", "HeadRead", "DagRead", "Retry", "Captured", "Created", "Cancelled"}
ValidationOutcomes == {
  "Idle",
  "DeferredCertificate",
  "DeferredBlock",
  "DeferredMetadata",
  "DeferredState",
  "Invalid",
  "Accepted"
}
RetryableOutcomes == ValidationOutcomes \ {"Invalid", "Accepted"}
NoRevision == 2
NoHeight == 2
NoFloor == "NoFloor"
NoState == "NoState"
NoValidator == "NoValidator"
NoCommittee == {}
NoDigest == "NoDigest"
AuthorityDigests == {"AuthG", "AuthA", "AuthX", NoDigest}
CertificateDigests == {"CertG", "CertA", "CertX", NoDigest}
ShardFttNumerator == 1
ShardFttDenominator == 10

StateOf(floor) == IF floor = "G" THEN "SG" ELSE "SA"
HeightOf(floor) == IF floor = "G" THEN 0 ELSE 1
CommitteeOfState(state) ==
  IF state = "SG" THEN Validators
  ELSE IF state = "SA" THEN {"v1", "v2"}
  ELSE {"v1"}
CommitteeOf(floor) == CommitteeOfState(StateOf(floor))
PredecessorOf(floor) == "G"
AuthorityDigestOf(floor) == IF floor = "G" THEN "AuthG" ELSE "AuthA"
CertificateDigestOf(predecessor, floor, state, height,
                    decisionDigest, fttNumerator, fttDenominator) ==
  IF predecessor = PredecessorOf(floor)
     /\ state = StateOf(floor)
     /\ height = HeightOf(floor)
     /\ decisionDigest = AuthorityDigestOf(predecessor)
     /\ fttNumerator = ShardFttNumerator
     /\ fttDenominator = ShardFttDenominator
  THEN IF floor = "G" THEN "CertG" ELSE "CertA"
  ELSE "CertX"
OtherFloor(floor) == IF floor = "G" THEN "A" ELSE "G"
OtherHeight(height) == IF height = 0 THEN 1 ELSE 0

VARIABLES
  \* @type: Int;
  durableRevision,
  \* @type: Str;
  durableFloor,
  \* @type: Str;
  durableState,
  \* @type: Set(Str);
  candidateCommittee,
  \* @type: Str -> Str;
  proposalPhase,
  \* @type: Str -> Int;
  headBefore,
  \* @type: Str -> Int;
  captureRevision,
  \* @type: Str -> Int;
  headAfter,
  \* @type: Str -> Str;
  captureFloor,
  \* @type: Str -> Str;
  captureState,
  \* @type: Str -> Int;
  captureHeight,
  \* @type: Str -> Bool;
  proposalStateAvailable,
  \* @type: Str -> Set(Str);
  decisionAuthorityCommittee,
  \* @type: Str -> Set(Str);
  decisionAuthorityLatestDomain,
  \* @type: Str -> Str;
  decisionAuthorityFloor,
  \* @type: Str -> Str;
  decisionAuthorityDigest,
  \* @type: Str -> Set(Str);
  proposalAuthorityCommittee,
  \* @type: Str -> Set(Str);
  proposalAuthorityLatestDomain,
  \* @type: Str -> Str;
  proposalAuthorityFloor,
  \* @type: Str -> Str;
  proposalAuthorityDigest,
  \* @type: Str -> Str;
  captureSender,
  \* @type: Str -> Bool;
  captureSenderActive,
  \* @type: Str -> Str;
  commitmentFloor,
  \* @type: Str -> Str;
  commitmentState,
  \* @type: Str -> Str;
  commitmentCertificateDigest,
  \* @type: Str -> Str;
  certificateDigest,
  \* @type: Str -> Bool;
  commitmentSigned,
  \* @type: Str -> Str;
  certificatePredecessorFloor,
  \* @type: Str -> Str;
  certificateFloor,
  \* @type: Str -> Str;
  certificateState,
  \* @type: Str -> Int;
  certificateHeight,
  \* @type: Str -> Int;
  certificateFttNumerator,
  \* @type: Str -> Int;
  certificateFttDenominator,
  \* @type: Str -> Int;
  certificateAgreeingStake,
  \* @type: Str -> Int;
  certificateCliqueStake,
  \* @type: Str -> Int;
  certificateTotalStake,
  \* @type: Str -> Str;
  storedBlockFloor,
  \* @type: Str -> Str;
  storedBlockState,
  \* @type: Str -> Int;
  storedBlockHeight,
  \* @type: Str -> Str;
  storedMetadataFloor,
  \* @type: Str -> Str;
  storedMetadataState,
  \* @type: Str -> Int;
  storedMetadataHeight,
  \* @type: Str -> Bool;
  storedAccepted,
  \* @type: Str -> Bool;
  parentDescends,
  \* @type: Str -> Set(Str);
  knownArtifacts,
  \* @type: Str -> Str;
  validationOutcome,
  \* @type: Str -> Str;
  replayFloor,
  \* @type: Str -> Str;
  replayState,
  \* @type: Str -> Set(Str);
  replayCommittee,
  \* @type: Set(Str);
  everCreated

vars == <<
  durableRevision,
  durableFloor,
  durableState,
  candidateCommittee,
  proposalPhase,
  headBefore,
  captureRevision,
  headAfter,
  captureFloor,
  captureState,
  captureHeight,
  proposalStateAvailable,
  decisionAuthorityCommittee,
  decisionAuthorityLatestDomain,
  decisionAuthorityFloor,
  decisionAuthorityDigest,
  proposalAuthorityCommittee,
  proposalAuthorityLatestDomain,
  proposalAuthorityFloor,
  proposalAuthorityDigest,
  captureSender,
  captureSenderActive,
  commitmentFloor,
  commitmentState,
  commitmentCertificateDigest,
  certificateDigest,
  commitmentSigned,
  certificatePredecessorFloor,
  certificateFloor,
  certificateState,
  certificateHeight,
  certificateFttNumerator,
  certificateFttDenominator,
  certificateAgreeingStake,
  certificateCliqueStake,
  certificateTotalStake,
  storedBlockFloor,
  storedBlockState,
  storedBlockHeight,
  storedMetadataFloor,
  storedMetadataState,
  storedMetadataHeight,
  storedAccepted,
  parentDescends,
  knownArtifacts,
  validationOutcome,
  replayFloor,
  replayState,
  replayCommittee,
  everCreated
>>

Init ==
  /\ durableRevision = 0
  /\ durableFloor = "G"
  /\ durableState = "SG"
  /\ candidateCommittee = CommitteeOf("A")
  /\ proposalPhase = [node \in Nodes |-> "Idle"]
  /\ headBefore = [node \in Nodes |-> NoRevision]
  /\ captureRevision = [node \in Nodes |-> NoRevision]
  /\ headAfter = [node \in Nodes |-> NoRevision]
  /\ captureFloor = [node \in Nodes |-> NoFloor]
  /\ captureState = [node \in Nodes |-> NoState]
  /\ captureHeight = [node \in Nodes |-> NoHeight]
  /\ proposalStateAvailable = [node \in Nodes |-> FALSE]
  /\ decisionAuthorityCommittee = [node \in Nodes |-> NoCommittee]
  /\ decisionAuthorityLatestDomain = [node \in Nodes |-> NoCommittee]
  /\ decisionAuthorityFloor = [node \in Nodes |-> NoFloor]
  /\ decisionAuthorityDigest = [node \in Nodes |-> NoDigest]
  /\ proposalAuthorityCommittee = [node \in Nodes |-> NoCommittee]
  /\ proposalAuthorityLatestDomain = [node \in Nodes |-> NoCommittee]
  /\ proposalAuthorityFloor = [node \in Nodes |-> NoFloor]
  /\ proposalAuthorityDigest = [node \in Nodes |-> NoDigest]
  /\ captureSender = [node \in Nodes |-> NoValidator]
  /\ captureSenderActive = [node \in Nodes |-> FALSE]
  /\ commitmentFloor = [node \in Nodes |-> NoFloor]
  /\ commitmentState = [node \in Nodes |-> NoState]
  /\ commitmentCertificateDigest = [node \in Nodes |-> NoDigest]
  /\ certificateDigest = [node \in Nodes |-> NoDigest]
  /\ commitmentSigned = [node \in Nodes |-> FALSE]
  /\ certificatePredecessorFloor = [node \in Nodes |-> NoFloor]
  /\ certificateFloor = [node \in Nodes |-> NoFloor]
  /\ certificateState = [node \in Nodes |-> NoState]
  /\ certificateHeight = [node \in Nodes |-> NoHeight]
  /\ certificateFttNumerator = [node \in Nodes |-> 0]
  /\ certificateFttDenominator = [node \in Nodes |-> 0]
  /\ certificateAgreeingStake = [node \in Nodes |-> 0]
  /\ certificateCliqueStake = [node \in Nodes |-> 0]
  /\ certificateTotalStake = [node \in Nodes |-> 0]
  /\ storedBlockFloor = [node \in Nodes |-> NoFloor]
  /\ storedBlockState = [node \in Nodes |-> NoState]
  /\ storedBlockHeight = [node \in Nodes |-> NoHeight]
  /\ storedMetadataFloor = [node \in Nodes |-> NoFloor]
  /\ storedMetadataState = [node \in Nodes |-> NoState]
  /\ storedMetadataHeight = [node \in Nodes |-> NoHeight]
  /\ storedAccepted = [node \in Nodes |-> FALSE]
  /\ parentDescends = [node \in Nodes |-> FALSE]
  /\ knownArtifacts = [node \in Nodes |-> {}]
  /\ validationOutcome = [node \in Nodes |-> "Idle"]
  /\ replayFloor = [node \in Nodes |-> NoFloor]
  /\ replayState = [node \in Nodes |-> NoState]
  /\ replayCommittee = [node \in Nodes |-> NoCommittee]
  /\ everCreated = {}

CaptureHead(node) ==
  /\ proposalPhase[node] \in {"Idle", "Retry"}
  /\ proposalPhase' = [proposalPhase EXCEPT ![node] = "HeadRead"]
  /\ headBefore' = [headBefore EXCEPT ![node] = durableRevision]
  /\ captureRevision' = [captureRevision EXCEPT ![node] = NoRevision]
  /\ headAfter' = [headAfter EXCEPT ![node] = NoRevision]
  /\ captureFloor' = [captureFloor EXCEPT ![node] = NoFloor]
  /\ captureState' = [captureState EXCEPT ![node] = NoState]
  /\ captureHeight' = [captureHeight EXCEPT ![node] = NoHeight]
  /\ proposalStateAvailable' = [proposalStateAvailable EXCEPT ![node] = FALSE]
  /\ decisionAuthorityCommittee' = [decisionAuthorityCommittee EXCEPT ![node] = NoCommittee]
  /\ decisionAuthorityLatestDomain' = [decisionAuthorityLatestDomain EXCEPT ![node] = NoCommittee]
  /\ decisionAuthorityFloor' = [decisionAuthorityFloor EXCEPT ![node] = NoFloor]
  /\ decisionAuthorityDigest' = [decisionAuthorityDigest EXCEPT ![node] = NoDigest]
  /\ proposalAuthorityCommittee' = [proposalAuthorityCommittee EXCEPT ![node] = NoCommittee]
  /\ proposalAuthorityLatestDomain' = [proposalAuthorityLatestDomain EXCEPT ![node] = NoCommittee]
  /\ proposalAuthorityFloor' = [proposalAuthorityFloor EXCEPT ![node] = NoFloor]
  /\ proposalAuthorityDigest' = [proposalAuthorityDigest EXCEPT ![node] = NoDigest]
  /\ captureSender' = [captureSender EXCEPT ![node] = NoValidator]
  /\ captureSenderActive' = [captureSenderActive EXCEPT ![node] = FALSE]
  /\ UNCHANGED <<durableRevision, durableFloor, durableState, candidateCommittee,
       commitmentFloor, commitmentState, commitmentCertificateDigest,
       certificateDigest, commitmentSigned, certificatePredecessorFloor,
       certificateFloor, certificateState, certificateHeight,
       certificateFttNumerator, certificateFttDenominator,
       certificateAgreeingStake, certificateCliqueStake, certificateTotalStake,
       storedBlockFloor,
       storedBlockState, storedBlockHeight, storedMetadataFloor,
       storedMetadataState, storedMetadataHeight, storedAccepted, parentDescends,
       knownArtifacts, validationOutcome, replayFloor, replayState,
       replayCommittee, everCreated>>

CaptureDag(node) ==
  LET targetCommittee ==
        IF Defect = "CommitmentStatePairMismatchAcceptance"
        THEN CommitteeOfState("SX")
        ELSE CommitteeOfState(durableState)
      predecessor == PredecessorOf(durableFloor)
      predecessorCommittee == CommitteeOf(predecessor)
  IN
  /\ proposalPhase[node] = "HeadRead"
  /\ proposalPhase' = [proposalPhase EXCEPT ![node] = "DagRead"]
  /\ captureRevision' = [captureRevision EXCEPT ![node] = durableRevision]
  /\ captureFloor' = [captureFloor EXCEPT ![node] = durableFloor]
  /\ captureState' = [captureState EXCEPT ![node] = durableState]
  /\ captureHeight' = [captureHeight EXCEPT ![node] = HeightOf(durableFloor)]
  /\ proposalStateAvailable' = [proposalStateAvailable EXCEPT
       ![node] = Defect # "MissingLocalStateProposal"]
  /\ decisionAuthorityCommittee' = [decisionAuthorityCommittee EXCEPT
       ![node] = IF Defect = "TargetCommitteeForCertificate"
                 THEN targetCommittee ELSE predecessorCommittee]
  /\ decisionAuthorityLatestDomain' = [decisionAuthorityLatestDomain EXCEPT
       ![node] = predecessorCommittee]
  /\ decisionAuthorityFloor' = [decisionAuthorityFloor EXCEPT ![node] = predecessor]
  /\ decisionAuthorityDigest' = [decisionAuthorityDigest EXCEPT
       ![node] = IF Defect = "SingleAuthorityContext"
                 THEN AuthorityDigestOf(durableFloor)
                 ELSE AuthorityDigestOf(predecessor)]
  /\ proposalAuthorityCommittee' = [proposalAuthorityCommittee EXCEPT
       ![node] = IF Defect = "PredecessorCommitteeForProposal"
                 THEN predecessorCommittee ELSE targetCommittee]
  /\ proposalAuthorityLatestDomain' = [proposalAuthorityLatestDomain EXCEPT
       ![node] = IF Defect = "IncompleteLatestAcceptance"
                 THEN targetCommittee \ {"v3"}
                 ELSE targetCommittee]
  /\ proposalAuthorityFloor' = [proposalAuthorityFloor EXCEPT ![node] = durableFloor]
  /\ proposalAuthorityDigest' = [proposalAuthorityDigest EXCEPT
       ![node] = AuthorityDigestOf(durableFloor)]
  /\ captureSender' = [captureSender EXCEPT ![node] = "v1"]
  /\ captureSenderActive' = [captureSenderActive EXCEPT
       ![node] = Defect # "InactiveSenderAcceptance"]
  /\ UNCHANGED <<durableRevision, durableFloor, durableState, candidateCommittee,
       headBefore, headAfter, commitmentFloor, commitmentState,
       commitmentCertificateDigest, certificateDigest, commitmentSigned,
       certificatePredecessorFloor, certificateFloor, certificateState,
       certificateHeight, certificateFttNumerator, certificateFttDenominator,
       certificateAgreeingStake, certificateCliqueStake, certificateTotalStake,
       storedBlockFloor, storedBlockState, storedBlockHeight,
       storedMetadataFloor, storedMetadataState, storedMetadataHeight,
       storedAccepted, parentDescends, knownArtifacts, validationOutcome,
       replayFloor, replayState, replayCommittee, everCreated>>

CaptureIsCoherent(node) ==
  headBefore[node] = captureRevision[node] /\ captureRevision[node] = durableRevision

SealCapture(node) ==
  LET targetFloor == captureFloor[node]
      targetState == IF Defect = "CommitmentStatePairMismatchAcceptance"
                     THEN "SX" ELSE captureState[node]
      predecessor == PredecessorOf(targetFloor)
      certifiedFloor == IF Defect = "HashMismatchAcceptance"
                        THEN OtherFloor(targetFloor) ELSE targetFloor
      certifiedState == IF Defect = "StateMismatchAcceptance"
                        THEN "SX" ELSE targetState
      certifiedHeight == IF Defect \in {
                              "HeightMismatchAcceptance",
                              "CommitmentHeightPairMismatchAcceptance",
                              "CertificateHeightNotDigestBound"}
                         THEN OtherHeight(captureHeight[node])
                         ELSE captureHeight[node]
      fttNumerator == IF Defect = "CertificateThresholdMismatch"
                      THEN 0 ELSE ShardFttNumerator
      fttDenominator == ShardFttDenominator
      computedDigest == CertificateDigestOf(
        predecessor,
        certifiedFloor,
        certifiedState,
        certifiedHeight,
        decisionAuthorityDigest[node],
        fttNumerator,
        fttDenominator)
      wireCertificateDigest ==
        IF Defect = "CertificateHeightNotDigestBound"
        THEN CertificateDigestOf(
          predecessor,
          certifiedFloor,
          certifiedState,
          captureHeight[node],
          decisionAuthorityDigest[node],
          fttNumerator,
          fttDenominator)
        ELSE computedDigest
      committedDigest ==
        IF Defect = "CertificateDigestOmission"
        THEN IF wireCertificateDigest = "CertG" THEN "CertA" ELSE "CertG"
        ELSE wireCertificateDigest
  IN
  /\ proposalPhase[node] = "DagRead"
  /\ headAfter' = [headAfter EXCEPT ![node] = durableRevision]
  /\ IF (CaptureIsCoherent(node) /\ proposalStateAvailable[node])
         \/ Defect \in {"TornCapture", "MissingLocalStateProposal"}
     THEN
       /\ proposalPhase' = [proposalPhase EXCEPT ![node] = "Captured"]
       /\ commitmentFloor' = [commitmentFloor EXCEPT ![node] = targetFloor]
       /\ commitmentState' = [commitmentState EXCEPT ![node] = targetState]
       /\ commitmentCertificateDigest' = [commitmentCertificateDigest EXCEPT
            ![node] = committedDigest]
       /\ certificateDigest' = [certificateDigest EXCEPT
            ![node] = wireCertificateDigest]
       /\ commitmentSigned' = [commitmentSigned EXCEPT
            ![node] = Defect # "UnsignedMutation"]
       /\ certificatePredecessorFloor' = [certificatePredecessorFloor EXCEPT
            ![node] = predecessor]
       /\ certificateFloor' = [certificateFloor EXCEPT ![node] = certifiedFloor]
       /\ certificateState' = [certificateState EXCEPT ![node] = certifiedState]
       /\ certificateHeight' = [certificateHeight EXCEPT ![node] = certifiedHeight]
       /\ certificateFttNumerator' = [certificateFttNumerator EXCEPT
            ![node] = fttNumerator]
       /\ certificateFttDenominator' = [certificateFttDenominator EXCEPT
            ![node] = fttDenominator]
       /\ certificateAgreeingStake' = [certificateAgreeingStake EXCEPT
            ![node] = 60]
       /\ certificateCliqueStake' = [certificateCliqueStake EXCEPT
            ![node] = IF Defect = "InclusiveFtt" THEN 55 ELSE 60]
       /\ certificateTotalStake' = [certificateTotalStake EXCEPT ![node] = 100]
       /\ storedBlockFloor' = [storedBlockFloor EXCEPT
            ![node] = IF Defect = "StoredBlockFloorMismatchAcceptance"
                      THEN OtherFloor(targetFloor)
                      ELSE targetFloor]
       /\ storedBlockState' = [storedBlockState EXCEPT
            ![node] = IF Defect = "StoredBlockStateMismatchAcceptance"
                      THEN "SX"
                      ELSE IF Defect = "CommitmentStatePairMismatchAcceptance"
                           THEN "SX"
                      ELSE targetState]
       /\ storedBlockHeight' = [storedBlockHeight EXCEPT
            ![node] = IF Defect = "StoredBlockHeightMismatchAcceptance"
                      THEN OtherHeight(captureHeight[node])
                      ELSE IF Defect \in {
                          "HeightMismatchAcceptance",
                          "CommitmentHeightPairMismatchAcceptance",
                          "CertificateHeightNotDigestBound"}
                           THEN certifiedHeight
                      ELSE captureHeight[node]]
       /\ storedMetadataFloor' = [storedMetadataFloor EXCEPT
            ![node] = IF Defect = "StoredMetadataFloorMismatchAcceptance"
                      THEN OtherFloor(targetFloor)
                      ELSE targetFloor]
       /\ storedMetadataState' = [storedMetadataState EXCEPT
            ![node] = IF Defect = "StoredMetadataStateMismatchAcceptance"
                      THEN "SX"
                      ELSE IF Defect = "CommitmentStatePairMismatchAcceptance"
                           THEN "SX"
                      ELSE targetState]
       /\ storedMetadataHeight' = [storedMetadataHeight EXCEPT
            ![node] = IF Defect = "StoredMetadataHeightMismatchAcceptance"
                      THEN OtherHeight(captureHeight[node])
                      ELSE IF Defect \in {
                          "HeightMismatchAcceptance",
                          "CommitmentHeightPairMismatchAcceptance",
                          "CertificateHeightNotDigestBound"}
                           THEN certifiedHeight
                      ELSE captureHeight[node]]
       /\ storedAccepted' = [storedAccepted EXCEPT
            ![node] = Defect # "RejectedOccurrenceAcceptance"]
       /\ parentDescends' = [parentDescends EXCEPT
            ![node] = Defect # "AncestryOmission"]
     ELSE
       /\ proposalPhase' = [proposalPhase EXCEPT ![node] = "Retry"]
       /\ UNCHANGED <<commitmentFloor, commitmentState,
            commitmentCertificateDigest, certificateDigest, commitmentSigned,
            certificatePredecessorFloor, certificateFloor, certificateState,
            certificateHeight, certificateFttNumerator,
            certificateFttDenominator, certificateAgreeingStake,
            certificateCliqueStake, certificateTotalStake,
            storedBlockFloor, storedBlockState,
            storedBlockHeight, storedMetadataFloor, storedMetadataState,
            storedMetadataHeight, storedAccepted, parentDescends>>
  /\ UNCHANGED <<durableRevision, durableFloor, durableState, candidateCommittee,
       headBefore, captureRevision, captureFloor, captureState, captureHeight,
       proposalStateAvailable, decisionAuthorityCommittee,
       decisionAuthorityLatestDomain, decisionAuthorityFloor,
       decisionAuthorityDigest, proposalAuthorityCommittee,
       proposalAuthorityLatestDomain, proposalAuthorityFloor,
       proposalAuthorityDigest, captureSender, captureSenderActive,
       knownArtifacts, validationOutcome, replayFloor,
       replayState, replayCommittee, everCreated>>

CreateProposal(node) ==
  /\ proposalPhase[node] = "Captured"
  /\ proposalPhase' = [proposalPhase EXCEPT ![node] = "Created"]
  /\ everCreated' = everCreated \union {node}
  /\ UNCHANGED <<durableRevision, durableFloor, durableState,
       candidateCommittee, headBefore, captureRevision, headAfter,
       captureFloor, captureState, captureHeight, proposalStateAvailable,
       decisionAuthorityCommittee, decisionAuthorityLatestDomain,
       decisionAuthorityFloor, decisionAuthorityDigest,
       proposalAuthorityCommittee, proposalAuthorityLatestDomain,
       proposalAuthorityFloor, proposalAuthorityDigest,
       captureSender, captureSenderActive, commitmentFloor, commitmentState,
       commitmentCertificateDigest, certificateDigest, commitmentSigned,
       certificatePredecessorFloor, certificateFloor, certificateState,
       certificateHeight, certificateFttNumerator, certificateFttDenominator,
       certificateAgreeingStake, certificateCliqueStake, certificateTotalStake,
       storedBlockFloor, storedBlockState, storedBlockHeight,
       storedMetadataFloor, storedMetadataState, storedMetadataHeight,
       storedAccepted, parentDescends, knownArtifacts, validationOutcome,
       replayFloor, replayState, replayCommittee>>

PromoteFinalizer ==
  /\ durableRevision = 0
  /\ durableRevision' = 1
  /\ durableFloor' = "A"
  /\ durableState' = "SA"
  /\ candidateCommittee' = CommitteeOf("A")
  /\ proposalPhase' = IF Defect = "FinalizerCancellation"
       THEN [node \in Nodes |-> IF proposalPhase[node] = "Created"
             THEN "Cancelled" ELSE proposalPhase[node]]
       ELSE proposalPhase
  /\ UNCHANGED <<headBefore, captureRevision, headAfter, captureFloor,
       captureState, captureHeight, proposalStateAvailable,
       decisionAuthorityCommittee, decisionAuthorityLatestDomain,
       decisionAuthorityFloor, decisionAuthorityDigest,
       proposalAuthorityCommittee, proposalAuthorityLatestDomain,
       proposalAuthorityFloor, proposalAuthorityDigest,
       captureSender, captureSenderActive, commitmentFloor, commitmentState,
       commitmentCertificateDigest, certificateDigest, commitmentSigned,
       certificatePredecessorFloor, certificateFloor, certificateState,
       certificateHeight, certificateFttNumerator, certificateFttDenominator,
       certificateAgreeingStake, certificateCliqueStake, certificateTotalStake,
       storedBlockFloor, storedBlockState,
       storedBlockHeight, storedMetadataFloor, storedMetadataState,
       storedMetadataHeight, storedAccepted, parentDescends, knownArtifacts,
       validationOutcome, replayFloor, replayState, replayCommittee,
       everCreated>>

DeliverArtifact(node, artifact) ==
  /\ proposalPhase[node] = "Created"
  /\ artifact \in Artifacts
  /\ artifact \notin knownArtifacts[node]
  /\ knownArtifacts' = [knownArtifacts EXCEPT ![node] = @ \union {artifact}]
  /\ UNCHANGED <<durableRevision, durableFloor, durableState,
       candidateCommittee, proposalPhase, headBefore, captureRevision,
       headAfter, captureFloor, captureState, captureHeight,
       proposalStateAvailable, decisionAuthorityCommittee,
       decisionAuthorityLatestDomain, decisionAuthorityFloor,
       decisionAuthorityDigest, proposalAuthorityCommittee,
       proposalAuthorityLatestDomain, proposalAuthorityFloor,
       proposalAuthorityDigest, captureSender, captureSenderActive,
       commitmentFloor, commitmentState, commitmentCertificateDigest,
       certificateDigest, commitmentSigned, certificatePredecessorFloor,
       certificateFloor, certificateState, certificateHeight,
       certificateFttNumerator, certificateFttDenominator,
       certificateAgreeingStake, certificateCliqueStake, certificateTotalStake,
       storedBlockFloor,
       storedBlockState, storedBlockHeight, storedMetadataFloor,
       storedMetadataState, storedMetadataHeight, storedAccepted,
       parentDescends, validationOutcome, replayFloor, replayState,
       replayCommittee, everCreated>>

CertificateTargetExact(node) ==
  /\ certificateFloor[node] = commitmentFloor[node]
  /\ certificateState[node] = commitmentState[node]
  /\ certificateHeight[node] = HeightOf(certificateFloor[node])

CommitmentExact(node) ==
  commitmentState[node] = StateOf(commitmentFloor[node])

CertificateThresholdExact(node) ==
  /\ certificateFttNumerator[node] = ShardFttNumerator
  /\ certificateFttDenominator[node] = ShardFttDenominator

CertificateStrictFtt(node) ==
  /\ certificateTotalStake[node] > 0
  /\ certificateFttDenominator[node] > 0
  /\ 2 * certificateAgreeingStake[node] > certificateTotalStake[node]
  /\ 2 * certificateCliqueStake[node] * certificateFttDenominator[node] >
       certificateTotalStake[node] *
         (certificateFttDenominator[node] + certificateFttNumerator[node])

CertificateDigestExact(node) ==
  /\ certificateDigest[node] = CertificateDigestOf(
       certificatePredecessorFloor[node],
       certificateFloor[node],
       certificateState[node],
       certificateHeight[node],
       decisionAuthorityDigest[node],
       certificateFttNumerator[node],
       certificateFttDenominator[node])
  /\ commitmentCertificateDigest[node] = certificateDigest[node]

OccurrenceExact(node) ==
  /\ storedBlockFloor[node] = commitmentFloor[node]
  /\ storedMetadataFloor[node] = commitmentFloor[node]
  /\ storedBlockState[node] = commitmentState[node]
  /\ storedMetadataState[node] = commitmentState[node]
  /\ storedBlockHeight[node] = certificateHeight[node]
  /\ storedMetadataHeight[node] = certificateHeight[node]
  /\ storedAccepted[node]

DecisionAuthorityExact(node) ==
  /\ decisionAuthorityFloor[node] = certificatePredecessorFloor[node]
  /\ decisionAuthorityCommittee[node] = CommitteeOf(certificatePredecessorFloor[node])
  /\ decisionAuthorityLatestDomain[node] = decisionAuthorityCommittee[node]
  /\ decisionAuthorityDigest[node] = AuthorityDigestOf(decisionAuthorityFloor[node])

ProposalAuthorityExact(node) ==
  /\ proposalAuthorityFloor[node] = commitmentFloor[node]
  /\ proposalAuthorityCommittee[node] = CommitteeOfState(commitmentState[node])
  /\ proposalAuthorityLatestDomain[node] = proposalAuthorityCommittee[node]
  /\ proposalAuthorityDigest[node] = AuthorityDigestOf(proposalAuthorityFloor[node])
  /\ captureSender[node] \in proposalAuthorityCommittee[node]
  /\ captureSenderActive[node]

ExactCapture(node) ==
  /\ commitmentSigned[node]
  /\ CommitmentExact(node)
  /\ CertificateTargetExact(node)
  /\ CertificateThresholdExact(node)
  /\ CertificateStrictFtt(node)
  /\ CertificateDigestExact(node)
  /\ OccurrenceExact(node)
  /\ parentDescends[node]
  /\ DecisionAuthorityExact(node)
  /\ ProposalAuthorityExact(node)

ValidationAccepts(node) ==
  /\ (commitmentSigned[node] \/ Defect = "UnsignedMutation")
  /\ (CommitmentExact(node) \/ Defect = "CommitmentStatePairMismatchAcceptance")
  /\ (CertificateTargetExact(node) \/ Defect \in {
       "HashMismatchAcceptance", "StateMismatchAcceptance",
       "HeightMismatchAcceptance", "CommitmentHeightPairMismatchAcceptance",
       "CertificateHeightNotDigestBound"})
  /\ (CertificateThresholdExact(node) \/ Defect = "CertificateThresholdMismatch")
  /\ (CertificateStrictFtt(node) \/ Defect = "InclusiveFtt")
  /\ (CertificateDigestExact(node) \/ Defect \in {
       "CertificateDigestOmission", "CertificateHeightNotDigestBound"})
  /\ (OccurrenceExact(node) \/ Defect \in {
       "RejectedOccurrenceAcceptance",
       "StoredBlockFloorMismatchAcceptance",
       "StoredBlockStateMismatchAcceptance",
       "StoredBlockHeightMismatchAcceptance",
       "StoredMetadataFloorMismatchAcceptance",
       "StoredMetadataStateMismatchAcceptance",
       "StoredMetadataHeightMismatchAcceptance"})
  /\ (parentDescends[node] \/ Defect = "AncestryOmission")
  /\ (DecisionAuthorityExact(node) \/ Defect \in {
       "SingleAuthorityContext", "TargetCommitteeForCertificate"})
  /\ (ProposalAuthorityExact(node) \/ Defect \in {
       "PredecessorCommitteeForProposal", "IncompleteLatestAcceptance",
       "InactiveSenderAcceptance"})

ValidationResult(node) ==
  IF Defect = "ReceiverDisagreement"
  THEN IF node = "n1" THEN "DeferredCertificate" ELSE "DeferredBlock"
  ELSE IF "Certificate" \notin knownArtifacts[node]
       THEN "DeferredCertificate"
       ELSE IF "FloorBlock" \notin knownArtifacts[node]
       THEN "DeferredBlock"
       ELSE IF "Metadata" \notin knownArtifacts[node]
            THEN "DeferredMetadata"
            ELSE IF "State" \notin knownArtifacts[node]
                 THEN IF Defect = "MissingRootAcceptance"
                      THEN "Accepted"
                      ELSE IF Defect = "MissingRootInvalidation"
                           THEN "Invalid"
                           ELSE "DeferredState"
                 ELSE IF ValidationAccepts(node) THEN "Accepted" ELSE "Invalid"

Validate(node) ==
  /\ proposalPhase[node] = "Created"
  /\ validationOutcome[node] \in RetryableOutcomes
  /\ LET outcome == ValidationResult(node)
     IN
       /\ validationOutcome' = [validationOutcome EXCEPT ![node] = outcome]
       /\ replayFloor' = [replayFloor EXCEPT
            ![node] = IF outcome = "Accepted"
                      THEN IF Defect = "FloorSubstitution"
                           THEN durableFloor ELSE commitmentFloor[node]
                      ELSE @]
       /\ replayState' = [replayState EXCEPT
            ![node] = IF outcome = "Accepted"
                      THEN IF Defect = "FloorSubstitution"
                           THEN durableState ELSE commitmentState[node]
                      ELSE @]
       /\ replayCommittee' = [replayCommittee EXCEPT
            ![node] = IF outcome = "Accepted"
                      THEN IF Defect = "CandidateCommittee"
                           THEN candidateCommittee ELSE proposalAuthorityCommittee[node]
                      ELSE @]
  /\ UNCHANGED <<durableRevision, durableFloor, durableState,
       candidateCommittee, proposalPhase, headBefore, captureRevision,
       headAfter, captureFloor, captureState, captureHeight,
       proposalStateAvailable, decisionAuthorityCommittee,
       decisionAuthorityLatestDomain, decisionAuthorityFloor,
       decisionAuthorityDigest, proposalAuthorityCommittee,
       proposalAuthorityLatestDomain, proposalAuthorityFloor,
       proposalAuthorityDigest, captureSender, captureSenderActive,
       commitmentFloor, commitmentState, commitmentCertificateDigest,
       certificateDigest, commitmentSigned, certificatePredecessorFloor,
       certificateFloor, certificateState, certificateHeight,
       certificateFttNumerator, certificateFttDenominator,
       certificateAgreeingStake, certificateCliqueStake, certificateTotalStake,
       storedBlockFloor,
       storedBlockState, storedBlockHeight, storedMetadataFloor,
       storedMetadataState, storedMetadataHeight, storedAccepted,
       parentDescends, knownArtifacts, everCreated>>

Next ==
  \/ PromoteFinalizer
  \/ \E node \in Nodes : CaptureHead(node)
  \/ \E node \in Nodes : CaptureDag(node)
  \/ \E node \in Nodes : SealCapture(node)
  \/ \E node \in Nodes : CreateProposal(node)
  \/ \E node \in Nodes, artifact \in Artifacts : DeliverArtifact(node, artifact)
  \/ \E node \in Nodes : Validate(node)

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(PromoteFinalizer)
  /\ \A node \in Nodes :
       /\ WF_vars(CaptureHead(node))
       /\ WF_vars(CaptureDag(node))
       /\ WF_vars(SealCapture(node))
       /\ WF_vars(CreateProposal(node))
       /\ WF_vars(Validate(node))
  /\ \A node \in Nodes, artifact \in Artifacts :
       WF_vars(DeliverArtifact(node, artifact))

TypeOK ==
  /\ durableRevision \in 0..1
  /\ durableFloor \in Floors
  /\ durableState \in States
  /\ candidateCommittee \in SUBSET Validators
  /\ proposalPhase \in [Nodes -> ProposalPhases]
  /\ headBefore \in [Nodes -> 0..NoRevision]
  /\ captureRevision \in [Nodes -> 0..NoRevision]
  /\ headAfter \in [Nodes -> 0..NoRevision]
  /\ captureFloor \in [Nodes -> Floors \union {NoFloor}]
  /\ captureState \in [Nodes -> States \union {NoState}]
  /\ captureHeight \in [Nodes -> 0..NoHeight]
  /\ proposalStateAvailable \in [Nodes -> BOOLEAN]
  /\ decisionAuthorityCommittee \in [Nodes -> SUBSET Validators]
  /\ decisionAuthorityLatestDomain \in [Nodes -> SUBSET Validators]
  /\ decisionAuthorityFloor \in [Nodes -> Floors \union {NoFloor}]
  /\ decisionAuthorityDigest \in [Nodes -> AuthorityDigests]
  /\ proposalAuthorityCommittee \in [Nodes -> SUBSET Validators]
  /\ proposalAuthorityLatestDomain \in [Nodes -> SUBSET Validators]
  /\ proposalAuthorityFloor \in [Nodes -> Floors \union {NoFloor}]
  /\ proposalAuthorityDigest \in [Nodes -> AuthorityDigests]
  /\ captureSender \in [Nodes -> Validators \union {NoValidator}]
  /\ captureSenderActive \in [Nodes -> BOOLEAN]
  /\ commitmentFloor \in [Nodes -> Floors \union {NoFloor}]
  /\ commitmentState \in [Nodes -> States \union {NoState}]
  /\ commitmentCertificateDigest \in [Nodes -> CertificateDigests]
  /\ certificateDigest \in [Nodes -> CertificateDigests]
  /\ commitmentSigned \in [Nodes -> BOOLEAN]
  /\ certificatePredecessorFloor \in [Nodes -> Floors \union {NoFloor}]
  /\ certificateFloor \in [Nodes -> Floors \union {NoFloor}]
  /\ certificateState \in [Nodes -> States \union {NoState}]
  /\ certificateHeight \in [Nodes -> 0..NoHeight]
  /\ certificateFttNumerator \in [Nodes -> 0..ShardFttNumerator]
  /\ certificateFttDenominator \in [Nodes -> 0..ShardFttDenominator]
  /\ certificateAgreeingStake \in [Nodes -> 0..100]
  /\ certificateCliqueStake \in [Nodes -> 0..100]
  /\ certificateTotalStake \in [Nodes -> 0..100]
  /\ storedBlockFloor \in [Nodes -> Floors \union {NoFloor}]
  /\ storedBlockState \in [Nodes -> States \union {NoState}]
  /\ storedBlockHeight \in [Nodes -> 0..NoHeight]
  /\ storedMetadataFloor \in [Nodes -> Floors \union {NoFloor}]
  /\ storedMetadataState \in [Nodes -> States \union {NoState}]
  /\ storedMetadataHeight \in [Nodes -> 0..NoHeight]
  /\ storedAccepted \in [Nodes -> BOOLEAN]
  /\ parentDescends \in [Nodes -> BOOLEAN]
  /\ knownArtifacts \in [Nodes -> SUBSET Artifacts]
  /\ validationOutcome \in [Nodes -> ValidationOutcomes]
  /\ replayFloor \in [Nodes -> Floors \union {NoFloor}]
  /\ replayState \in [Nodes -> States \union {NoState}]
  /\ replayCommittee \in [Nodes -> SUBSET Validators]
  /\ everCreated \subseteq Nodes

Inv_CapturedTuplesAreCoherent ==
  \A node \in Nodes :
    proposalPhase[node] \in {"Captured", "Created", "Cancelled"} =>
      headBefore[node] = captureRevision[node] /\ captureRevision[node] = headAfter[node]

Inv_AcceptanceRequiresExactArtifacts ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      /\ knownArtifacts[node] = Artifacts
      /\ ExactCapture(node)

Inv_AcceptanceRequiresSignedCommitment ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" => commitmentSigned[node]

Inv_AcceptanceRequiresCanonicalCommitmentState ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      commitmentState[node] = StateOf(commitmentFloor[node])

Inv_AcceptanceRequiresCanonicalCommitmentHeight ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      certificateHeight[node] = HeightOf(commitmentFloor[node])

Inv_AcceptanceRequiresCertificateHash ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      certificateFloor[node] = commitmentFloor[node]

Inv_AcceptanceRequiresCertificateState ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      certificateState[node] = commitmentState[node]

Inv_AcceptanceRequiresCertificateHeight ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      certificateHeight[node] = HeightOf(certificateFloor[node])

Inv_CommitmentBindsCompleteCertificate ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" => CertificateDigestExact(node)

Inv_CertificateThresholdMatchesShard ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" => CertificateThresholdExact(node)

Inv_CertificateUsesStrictFtt ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" => CertificateStrictFtt(node)

Inv_CertificateAuthorityUsesPredecessorFloor ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" => DecisionAuthorityExact(node)

Inv_ProposalAuthorityUsesTargetFloor ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" => ProposalAuthorityExact(node)

Inv_DistinctCommitteesHaveDistinctAuthorityDigests ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted"
      /\ decisionAuthorityCommittee[node] # proposalAuthorityCommittee[node] =>
        decisionAuthorityDigest[node] # proposalAuthorityDigest[node]

Inv_MissingLocalStatePreventsProposal ==
  \A node \in Nodes :
    proposalPhase[node] \in {"Captured", "Created", "Cancelled"} =>
      proposalStateAvailable[node]

Inv_AcceptanceRequiresAcceptedOccurrence ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" => storedAccepted[node]

Inv_AcceptanceRequiresStoredBlockFloor ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      storedBlockFloor[node] = commitmentFloor[node]

Inv_AcceptanceRequiresStoredBlockState ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      storedBlockState[node] = commitmentState[node]

Inv_AcceptanceRequiresStoredBlockHeight ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      storedBlockHeight[node] = certificateHeight[node]

Inv_AcceptanceRequiresStoredMetadataFloor ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      storedMetadataFloor[node] = commitmentFloor[node]

Inv_AcceptanceRequiresStoredMetadataState ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      storedMetadataState[node] = commitmentState[node]

Inv_AcceptanceRequiresStoredMetadataHeight ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      storedMetadataHeight[node] = certificateHeight[node]

Inv_AcceptanceRequiresFloorAncestry ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" => parentDescends[node]

Inv_AcceptanceRequiresCompleteLatestMessages ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      /\ decisionAuthorityLatestDomain[node] = decisionAuthorityCommittee[node]
      /\ proposalAuthorityLatestDomain[node] = proposalAuthorityCommittee[node]

Inv_AcceptanceRequiresActiveSender ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" => captureSenderActive[node]

Inv_MissingArtifactIsNeverAccepted ==
  \A node \in Nodes :
    knownArtifacts[node] # Artifacts => validationOutcome[node] # "Accepted"

Inv_MissingArtifactIsNeverInvalid ==
  \A node \in Nodes :
    proposalPhase[node] = "Created" /\ knownArtifacts[node] # Artifacts =>
      validationOutcome[node] # "Invalid"

Inv_AcceptedReplayUsesCapturedFloor ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      /\ replayFloor[node] = commitmentFloor[node]
      /\ replayState[node] = commitmentState[node]

Inv_AcceptedAuthorityUsesCapturedFloor ==
  \A node \in Nodes :
    validationOutcome[node] = "Accepted" =>
      /\ replayCommittee[node] = proposalAuthorityCommittee[node]
      /\ replayCommittee[node] = CommitteeOfState(commitmentState[node])

Inv_FinalizerDoesNotCancelCreatedProposal ==
  everCreated \subseteq {node \in Nodes : proposalPhase[node] = "Created"}

SameFrozenEvidence(left, right) ==
  /\ commitmentFloor[left] = commitmentFloor[right]
  /\ commitmentState[left] = commitmentState[right]
  /\ commitmentCertificateDigest[left] = commitmentCertificateDigest[right]
  /\ certificateDigest[left] = certificateDigest[right]
  /\ commitmentSigned[left] = commitmentSigned[right]
  /\ certificatePredecessorFloor[left] = certificatePredecessorFloor[right]
  /\ certificateFloor[left] = certificateFloor[right]
  /\ certificateState[left] = certificateState[right]
  /\ certificateHeight[left] = certificateHeight[right]
  /\ certificateFttNumerator[left] = certificateFttNumerator[right]
  /\ certificateFttDenominator[left] = certificateFttDenominator[right]
  /\ certificateAgreeingStake[left] = certificateAgreeingStake[right]
  /\ certificateCliqueStake[left] = certificateCliqueStake[right]
  /\ certificateTotalStake[left] = certificateTotalStake[right]
  /\ storedBlockFloor[left] = storedBlockFloor[right]
  /\ storedBlockState[left] = storedBlockState[right]
  /\ storedBlockHeight[left] = storedBlockHeight[right]
  /\ storedMetadataFloor[left] = storedMetadataFloor[right]
  /\ storedMetadataState[left] = storedMetadataState[right]
  /\ storedMetadataHeight[left] = storedMetadataHeight[right]
  /\ storedAccepted[left] = storedAccepted[right]
  /\ parentDescends[left] = parentDescends[right]
  /\ decisionAuthorityCommittee[left] = decisionAuthorityCommittee[right]
  /\ decisionAuthorityLatestDomain[left] = decisionAuthorityLatestDomain[right]
  /\ decisionAuthorityFloor[left] = decisionAuthorityFloor[right]
  /\ decisionAuthorityDigest[left] = decisionAuthorityDigest[right]
  /\ proposalAuthorityCommittee[left] = proposalAuthorityCommittee[right]
  /\ proposalAuthorityLatestDomain[left] = proposalAuthorityLatestDomain[right]
  /\ proposalAuthorityFloor[left] = proposalAuthorityFloor[right]
  /\ proposalAuthorityDigest[left] = proposalAuthorityDigest[right]
  /\ captureSender[left] = captureSender[right]
  /\ captureSenderActive[left] = captureSenderActive[right]
  /\ knownArtifacts[left] = knownArtifacts[right]

Inv_EqualEvidenceHasEqualReceiverDecision ==
  \A left, right \in Nodes :
    SameFrozenEvidence(left, right) => ValidationResult(left) = ValidationResult(right)

Safety ==
  /\ TypeOK
  /\ Inv_CapturedTuplesAreCoherent
  /\ Inv_AcceptanceRequiresExactArtifacts
  /\ Inv_AcceptanceRequiresSignedCommitment
  /\ Inv_AcceptanceRequiresCanonicalCommitmentState
  /\ Inv_AcceptanceRequiresCanonicalCommitmentHeight
  /\ Inv_AcceptanceRequiresCertificateHash
  /\ Inv_AcceptanceRequiresCertificateState
  /\ Inv_AcceptanceRequiresCertificateHeight
  /\ Inv_CommitmentBindsCompleteCertificate
  /\ Inv_CertificateThresholdMatchesShard
  /\ Inv_CertificateUsesStrictFtt
  /\ Inv_CertificateAuthorityUsesPredecessorFloor
  /\ Inv_ProposalAuthorityUsesTargetFloor
  /\ Inv_DistinctCommitteesHaveDistinctAuthorityDigests
  /\ Inv_MissingLocalStatePreventsProposal
  /\ Inv_AcceptanceRequiresAcceptedOccurrence
  /\ Inv_AcceptanceRequiresStoredBlockFloor
  /\ Inv_AcceptanceRequiresStoredBlockState
  /\ Inv_AcceptanceRequiresStoredBlockHeight
  /\ Inv_AcceptanceRequiresStoredMetadataFloor
  /\ Inv_AcceptanceRequiresStoredMetadataState
  /\ Inv_AcceptanceRequiresStoredMetadataHeight
  /\ Inv_AcceptanceRequiresFloorAncestry
  /\ Inv_AcceptanceRequiresCompleteLatestMessages
  /\ Inv_AcceptanceRequiresActiveSender
  /\ Inv_MissingArtifactIsNeverAccepted
  /\ Inv_MissingArtifactIsNeverInvalid
  /\ Inv_AcceptedReplayUsesCapturedFloor
  /\ Inv_AcceptedAuthorityUsesCapturedFloor
  /\ Inv_FinalizerDoesNotCancelCreatedProposal
  /\ Inv_EqualEvidenceHasEqualReceiverDecision

CreatedEventuallyAccepted ==
  \A node \in Nodes :
    proposalPhase[node] = "Created" ~> validationOutcome[node] = "Accepted"

=============================================================================
