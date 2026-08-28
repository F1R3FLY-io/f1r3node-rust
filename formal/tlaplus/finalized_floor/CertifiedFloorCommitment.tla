------------------- MODULE CertifiedFloorCommitment -------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANT
  \* @type: Set(Str);
  ModelValidators,
  \* @type: Bool;
  UseCommittedFloor,
  \* @type: Bool;
  VerifyCertificates,
  \* @type: Bool;
  FetchDependencies,
  \* @type: Bool;
  EnforceParentFloorCompatibility,
  \* @type: Bool;
  CacheSkipsCandidateCompatibility,
  \* @type: Bool;
  BindCandidateAuthorityContext,
  \* @type: Bool;
  UseReceiverLocalFloor

ASSUME /\ ModelValidators \subseteq {"v1", "v2", "v3"}
       /\ Cardinality(ModelValidators) >= 3
       /\ UseCommittedFloor \in BOOLEAN
       /\ VerifyCertificates \in BOOLEAN
       /\ FetchDependencies \in BOOLEAN
       /\ EnforceParentFloorCompatibility \in BOOLEAN
       /\ CacheSkipsCandidateCompatibility \in BOOLEAN
       /\ BindCandidateAuthorityContext \in BOOLEAN
       /\ UseReceiverLocalFloor \in BOOLEAN

Validators == ModelValidators
Nodes == ModelValidators
Blocks == {"G", "A", "S1", "S2", "N", "R1", "R2", "R3", "H", "Bad"}
HistoricalBlocks == {"G", "A", "S1", "S2", "N"}
RebaseBlocks == {"R1", "R2", "R3"}
AttackBlocks == {"H", "Bad"}
CandidateBlocks == RebaseBlocks \union AttackBlocks
Certificates == {"CGenesis", "CA", "CBad"}
NoFloor == "NoFloor"
Effects == {"funding"}
Contexts == {"ContextG", "ContextA"}

RebaseOf(node) ==
  CASE node = "v1" -> "R1"
    [] node = "v2" -> "R2"
    [] OTHER -> "R3"

StaleParentOf(node) ==
  IF node = "v2" THEN "S2" ELSE "S1"

Parents(block) ==
  CASE block = "G" -> {}
    [] block = "A" -> {"G"}
    [] block \in {"S1", "S2"} -> {"A"}
    [] block = "N" -> {"A"}
    [] block = "R1" -> {"S1"}
    [] block = "R2" -> {"S2"}
    [] block = "R3" -> {"S1"}
    [] block = "H" -> {"N"}
    [] block = "Bad" -> {"G"}

StructuralFloor(block) ==
  IF block \in RebaseBlocks THEN "G"
  ELSE IF block = "G" \/ block = "A" THEN "G"
  ELSE "G"

CommittedFloor(block) ==
  IF block \in RebaseBlocks /\ UseCommittedFloor THEN "A"
  ELSE IF block \in AttackBlocks THEN "G"
  ELSE StructuralFloor(block)

CertificateFor(block) ==
  IF block \in RebaseBlocks /\ UseCommittedFloor THEN "CA"
  ELSE IF block = "H" THEN "CGenesis"
  ELSE IF block = "Bad" THEN "CBad"
  ELSE ""

CertificateTarget(certificate) ==
  IF certificate = "CA" THEN "A" ELSE "G"

CertificatePredecessor(certificate) ==
  IF certificate \in {"CGenesis", "CA"} THEN "G" ELSE "A"

CertificateSound(certificate) ==
  \/ /\ certificate = "CGenesis"
        /\ CertificatePredecessor(certificate) = "G"
        /\ CertificateTarget(certificate) = "G"
  \/ /\ certificate = "CA"
        /\ CertificatePredecessor(certificate) = "G"
        /\ CertificateTarget(certificate) = "A"

BlockEffects(block) ==
  IF block = "G" THEN {}
  ELSE {"funding"}

FloorRank(floor) == IF floor = "A" THEN 1 ELSE 0

Dominates(left, right) == FloorRank(left) <= FloorRank(right)

ReplayPreservesFloor(block) ==
  BlockEffects(CommittedFloor(block)) \subseteq BlockEffects(block)

RequiresCertificate(block) ==
  block \in CandidateBlocks /\ (UseCommittedFloor \/ block \in AttackBlocks)

EffectiveCommittedFloor(block) ==
  CASE block = "G" -> "G"
    [] block \in {"A", "S1", "S2"} -> "G"
    [] block = "N" -> "A"
    [] OTHER -> CommittedFloor(block)

ParentFloorsPreserved(block) ==
  \A parent \in Parents(block) :
    Dominates(EffectiveCommittedFloor(parent), CommittedFloor(block))

ExpectedCandidateAuthorityContext(block) ==
  IF CommittedFloor(block) = "A" THEN "ContextA" ELSE "ContextG"

CommittedCandidateAuthorityContext(block) ==
  IF BindCandidateAuthorityContext
  THEN ExpectedCandidateAuthorityContext(block)
  ELSE "ContextG"

CandidateAuthorityContextBound(block) ==
  CommittedCandidateAuthorityContext(block) =
    ExpectedCandidateAuthorityContext(block)

CompatibilityAtReceiverFloor(block, receiverFloor) ==
  IF UseReceiverLocalFloor
  THEN Dominates(receiverFloor, CommittedFloor(block))
  ELSE ParentFloorsPreserved(block)

VARIABLES
  \* @type: Set(Str);
  publishedBlocks,
  \* @type: Set(Str);
  publishedCertificates,
  \* @type: Str -> Set(Str);
  knownBlocks,
  \* @type: Str -> Set(Str);
  knownCertificates,
  \* @type: Str -> Set(Str);
  verifiedCertificates,
  \* @type: Set(Str);
  votesForA,
  \* @type: Str -> Set(Str);
  bufferedBlocks,
  \* @type: Str -> Set(Str);
  acceptedBlocks,
  \* @type: Str -> Str;
  proposedAtFloor,
  \* @type: Str -> Str;
  lfb,
  \* @type: Str -> Bool;
  hasAdvanced

vars == <<publishedBlocks, publishedCertificates, knownBlocks,
  knownCertificates, verifiedCertificates, votesForA, bufferedBlocks, acceptedBlocks,
  proposedAtFloor, lfb, hasAdvanced>>

TypeOK ==
  /\ publishedBlocks \subseteq Blocks
  /\ publishedCertificates \subseteq Certificates
  /\ knownBlocks \in [Nodes -> SUBSET Blocks]
  /\ knownCertificates \in [Nodes -> SUBSET Certificates]
  /\ verifiedCertificates \in [Nodes -> SUBSET Certificates]
  /\ votesForA \subseteq Validators
  /\ bufferedBlocks \in [Nodes -> SUBSET CandidateBlocks]
  /\ acceptedBlocks \in [Nodes -> SUBSET Blocks]
  /\ proposedAtFloor \in [CandidateBlocks -> {NoFloor, "G", "A"}]
  /\ lfb \in [Nodes -> {"G", "A"}]
  /\ hasAdvanced \in [Nodes -> BOOLEAN]

Init ==
  /\ publishedBlocks = HistoricalBlocks \union AttackBlocks
  /\ publishedCertificates = {"CGenesis", "CBad"}
  /\ knownBlocks = [node \in Nodes |-> {"G"}]
  /\ knownCertificates =
       [node \in Nodes |-> IF node = "v1" THEN {"CGenesis", "CBad"} ELSE {}]
  /\ verifiedCertificates =
       [node \in Nodes |-> IF node = "v1" THEN {"CGenesis"} ELSE {}]
  /\ votesForA = {}
  /\ bufferedBlocks = [node \in Nodes |-> {}]
  /\ acceptedBlocks = [node \in Nodes |-> {"G"}]
  /\ proposedAtFloor =
       [block \in CandidateBlocks |-> IF block \in AttackBlocks THEN "G" ELSE NoFloor]
  /\ lfb = [node \in Nodes |-> "G"]
  /\ hasAdvanced = [node \in Nodes |-> FALSE]

DeliverHistoricalBlock(node, block) ==
  /\ node \in Nodes
  /\ block \in HistoricalBlocks
  /\ (block # "N" \/ node = "v1")
  /\ block \in publishedBlocks
  /\ block \notin knownBlocks[node]
  /\ Parents(block) \subseteq knownBlocks[node]
  /\ knownBlocks' = [knownBlocks EXCEPT ![node] = @ \union {block}]
  /\ acceptedBlocks' = [acceptedBlocks EXCEPT ![node] = @ \union {block}]
  /\ UNCHANGED <<publishedBlocks, publishedCertificates, knownCertificates,
       verifiedCertificates,
       votesForA, bufferedBlocks, proposedAtFloor, lfb, hasAdvanced>>

VoteForA(validator) ==
  /\ validator \in Validators
  /\ "A" \in knownBlocks[validator]
  /\ validator \notin votesForA
  /\ votesForA' = votesForA \union {validator}
  /\ UNCHANGED <<publishedBlocks, publishedCertificates, knownBlocks,
       knownCertificates, verifiedCertificates, bufferedBlocks, acceptedBlocks, proposedAtFloor,
       lfb, hasAdvanced>>

PublishCertificateA ==
  /\ "CA" \notin publishedCertificates
  /\ Cardinality(votesForA) * 2 > Cardinality(Validators)
  /\ publishedCertificates' = publishedCertificates \union {"CA"}
  /\ knownCertificates' =
       [knownCertificates EXCEPT !["v1"] = @ \union {"CA"}]
  /\ UNCHANGED <<publishedBlocks, knownBlocks, verifiedCertificates, votesForA, bufferedBlocks,
       acceptedBlocks, proposedAtFloor, lfb, hasAdvanced>>

AdoptCertificate(node, certificate) ==
  /\ node \in Nodes
  /\ certificate \in knownCertificates[node]
  /\ VerifyCertificates => CertificateSound(certificate)
  /\ CertificateTarget(certificate) \in knownBlocks[node]
  /\ Dominates(lfb[node], CertificateTarget(certificate))
  /\ lfb' = [lfb EXCEPT ![node] = CertificateTarget(certificate)]
  /\ hasAdvanced' =
       [hasAdvanced EXCEPT ![node] = @ \/ CertificateTarget(certificate) = "A"]
  /\ UNCHANGED <<publishedBlocks, publishedCertificates, knownBlocks,
       knownCertificates, verifiedCertificates, votesForA, bufferedBlocks, acceptedBlocks,
       proposedAtFloor>>

ProposalReady(node) ==
  /\ lfb[node] = "A"
  /\ StaleParentOf(node) \in knownBlocks[node]
  /\ proposedAtFloor[RebaseOf(node)] = NoFloor
  /\ Dominates(lfb[node], CommittedFloor(RebaseOf(node)))
  /\ ReplayPreservesFloor(RebaseOf(node))

ProposeRebase(node) ==
  /\ node \in Nodes
  /\ ProposalReady(node)
  /\ publishedBlocks' = publishedBlocks \union {RebaseOf(node)}
  /\ knownBlocks' = [knownBlocks EXCEPT ![node] = @ \union {RebaseOf(node)}]
  /\ acceptedBlocks' =
       [acceptedBlocks EXCEPT ![node] = @ \union {RebaseOf(node)}]
  /\ proposedAtFloor' =
       [proposedAtFloor EXCEPT ![RebaseOf(node)] = lfb[node]]
  /\ UNCHANGED <<publishedCertificates, knownCertificates, verifiedCertificates, votesForA,
       bufferedBlocks, lfb, hasAdvanced>>

CertificateChainAccepted(node, certificate) ==
  \/ certificate \in verifiedCertificates[node]
  \/ IF VerifyCertificates THEN CertificateSound(certificate) ELSE TRUE

BlockCertificateReady(node, block) ==
  /\ RequiresCertificate(block)
  /\ CertificateFor(block) \in knownCertificates[node]
  /\ CertificateChainAccepted(node, CertificateFor(block))
  /\ CertificateTarget(CertificateFor(block)) = CommittedFloor(block)

CandidateSpecificCompatibility(node, block) ==
  /\ (~EnforceParentFloorCompatibility
       \/ CompatibilityAtReceiverFloor(block, lfb[node]))
  /\ CandidateAuthorityContextBound(block)

BlockUseCompatible(node, block) ==
  IF CacheSkipsCandidateCompatibility
       /\ CertificateFor(block) \in verifiedCertificates[node]
  THEN TRUE
  ELSE CandidateSpecificCompatibility(node, block)

BlockAdmissible(node, block) ==
  /\ Parents(block) \subseteq knownBlocks[node]
  /\ ReplayPreservesFloor(block)
  /\ proposedAtFloor[block] # NoFloor
  /\ Dominates(proposedAtFloor[block], CommittedFloor(block))
  /\ BlockUseCompatible(node, block)
  /\ (~RequiresCertificate(block) \/ BlockCertificateReady(node, block))

ReceiveRebase(node, block) ==
  /\ node \in Nodes
  /\ block \in CandidateBlocks
  /\ (block \in RebaseBlocks \/ node = "v1")
  /\ block \in publishedBlocks
  /\ block \notin knownBlocks[node]
  /\ Parents(block) \subseteq knownBlocks[node]
  /\ IF RequiresCertificate(block)
        /\ CertificateFor(block) \notin knownCertificates[node]
     THEN /\ bufferedBlocks' =
                [bufferedBlocks EXCEPT ![node] = @ \union {block}]
          /\ UNCHANGED <<knownBlocks, acceptedBlocks>>
     ELSE /\ BlockAdmissible(node, block)
          /\ knownBlocks' = [knownBlocks EXCEPT ![node] = @ \union {block}]
          /\ acceptedBlocks' =
                [acceptedBlocks EXCEPT ![node] = @ \union {block}]
          /\ bufferedBlocks' =
                [bufferedBlocks EXCEPT ![node] = @ \ {block}]
  /\ UNCHANGED <<publishedBlocks, publishedCertificates, knownCertificates,
       verifiedCertificates,
       votesForA, proposedAtFloor, lfb, hasAdvanced>>

FetchCertificate(node, block) ==
  /\ FetchDependencies
  /\ node \in Nodes
  /\ block \in bufferedBlocks[node]
  /\ CertificateFor(block) \in publishedCertificates
  /\ CertificateFor(block) \notin knownCertificates[node]
  /\ knownCertificates' =
       [knownCertificates EXCEPT ![node] = @ \union {CertificateFor(block)}]
  /\ UNCHANGED <<publishedBlocks, publishedCertificates, knownBlocks,
       verifiedCertificates,
       votesForA, bufferedBlocks, acceptedBlocks, proposedAtFloor, lfb,
       hasAdvanced>>

ValidateBuffered(node, block) ==
  /\ node \in Nodes
  /\ block \in bufferedBlocks[node]
  /\ BlockAdmissible(node, block)
  /\ bufferedBlocks' = [bufferedBlocks EXCEPT ![node] = @ \ {block}]
  /\ knownBlocks' = [knownBlocks EXCEPT ![node] = @ \union {block}]
  /\ acceptedBlocks' =
       [acceptedBlocks EXCEPT ![node] = @ \union {block}]
  /\ UNCHANGED <<publishedBlocks, publishedCertificates, knownCertificates,
       verifiedCertificates,
       votesForA, proposedAtFloor, lfb, hasAdvanced>>

Next ==
  \/ \E node \in Nodes, block \in HistoricalBlocks :
       DeliverHistoricalBlock(node, block)
  \/ \E validator \in Validators : VoteForA(validator)
  \/ PublishCertificateA
  \/ \E node \in Nodes, certificate \in Certificates :
       AdoptCertificate(node, certificate)
  \/ \E node \in Nodes : ProposeRebase(node)
  \/ \E node \in Nodes, block \in CandidateBlocks : ReceiveRebase(node, block)
  \/ \E node \in Nodes, block \in CandidateBlocks : FetchCertificate(node, block)
  \/ \E node \in Nodes, block \in CandidateBlocks : ValidateBuffered(node, block)

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A node \in Nodes, block \in HistoricalBlocks :
       WF_vars(DeliverHistoricalBlock(node, block))
  /\ \A validator \in Validators : WF_vars(VoteForA(validator))
  /\ WF_vars(PublishCertificateA)
  /\ \A node \in Nodes, certificate \in Certificates :
       WF_vars(AdoptCertificate(node, certificate))
  /\ \A node \in Nodes : WF_vars(ProposeRebase(node))
  /\ \A node \in Nodes, block \in CandidateBlocks :
       WF_vars(ReceiveRebase(node, block))
  /\ \A node \in Nodes, block \in CandidateBlocks :
       WF_vars(FetchCertificate(node, block))
  /\ \A node \in Nodes, block \in CandidateBlocks :
       WF_vars(ValidateBuffered(node, block))

CertifiedFloorNeverRegresses ==
  \A node \in Nodes : hasAdvanced[node] => lfb[node] = "A"

PublishedRebasesBindProposalFloor ==
  \A block \in RebaseBlocks :
    block \in publishedBlocks =>
      /\ proposedAtFloor[block] # NoFloor
      /\ Dominates(proposedAtFloor[block], CommittedFloor(block))

AcceptedReplayPreservesCommittedFloor ==
  \A node \in Nodes :
    \A block \in acceptedBlocks[node] : ReplayPreservesFloor(block)

AcceptedCertifiedRebasesHaveEvidence ==
  \A node \in Nodes :
    \A block \in acceptedBlocks[node] \cap CandidateBlocks :
      RequiresCertificate(block) =>
        /\ CertificateFor(block) \in knownCertificates[node]
        /\ CertificateSound(CertificateFor(block))
        /\ CertificateTarget(CertificateFor(block)) = CommittedFloor(block)

AcceptedCandidatesPreserveEveryParentFloor ==
  \A node \in Nodes :
    \A block \in acceptedBlocks[node] \cap CandidateBlocks :
      ParentFloorsPreserved(block)

AcceptedCandidatesBindAuthorityContext ==
  \A node \in Nodes :
    \A block \in acceptedBlocks[node] \cap CandidateBlocks :
      CandidateAuthorityContextBound(block)

CertificateCacheIsCandidateTransparent ==
  \A node \in Nodes :
    \A block \in acceptedBlocks[node] \cap CandidateBlocks :
      CertificateFor(block) \in verifiedCertificates[node] =>
        CandidateSpecificCompatibility(node, block)

ReceiverLocalFloorDoesNotChangeCompatibility ==
  \A block \in CandidateBlocks :
    CompatibilityAtReceiverFloor(block, "G") =
      CompatibilityAtReceiverFloor(block, "A")

BufferedBlocksAreNotAccepted ==
  \A node \in Nodes : bufferedBlocks[node] \cap acceptedBlocks[node] = {}

AllValidatorsAdvance == <> (\A node \in Nodes : lfb[node] = "A")

AllValidatorsRebase ==
  <> (\A node \in Nodes : acceptedBlocks[node] \cap RebaseBlocks # {})

=============================================================================
