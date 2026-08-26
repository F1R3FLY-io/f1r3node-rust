----------------------- MODULE CertifiedObjectiveEquivocation -----------------------
EXTENDS Naturals, Integers, FiniteSets, TLC

CONSTANTS
  \* @type: Set(Int);
  Replicas,
  \* @type: Bool;
  TrustHeaderGeneration,
  \* @type: Bool;
  UsePostStateAuthority,
  \* @type: Bool;
  RepairDuplicateInsert,
  \* @type: Bool;
  GateEvidenceOnLocalInvalid,
  \* @type: Bool;
  GateEvidenceOnEligibleSequence,
  \* @type: Bool;
  CanonicalLatestMessageTie,
  \* @type: Bool;
  CoreSiblingOnly,
  \* @type: Bool;
  SequenceBoundaryOnly

ASSUME Replicas /= {}

\* @type: Str;
BlockA == "A"
\* @type: Str;
BlockB == "B"
\* @type: Str;
BlockC == "C"
\* @type: Str;
MalformedBlock == "F"
\* @type: Str;
NegativeSequenceBlock == "N"
\* @type: Set(Str);
Blocks == {BlockA, BlockB, BlockC, MalformedBlock, NegativeSequenceBlock}
\* @type: Set(Str);
EnabledBlocks ==
  IF SequenceBoundaryOnly
  THEN {NegativeSequenceBlock}
  ELSE IF CoreSiblingOnly THEN {BlockA, BlockB} ELSE Blocks

\* @type: Str;
RootZero == "RootZero"
\* @type: Str;
RootOne == "RootOne"
\* @type: Set(Str);
Roots == {RootZero, RootOne}

\* @type: Str;
ValidatorOne == "ValidatorOne"
\* @type: Set(Str);
Validators == {ValidatorOne}
\* @type: Str;
NoBlock == "NoBlock"
\* @type: Str;
NoRoot == "NoRoot"

\* @type: (Str) => Str;
BlockSender(_block) == ValidatorOne
\* @type: (Str) => Int;
BlockSequence(block) == IF block = NegativeSequenceBlock THEN -2 ELSE 7

\* @type: (Str) => Str;
ParentRoot(block) ==
  IF block \in {BlockA, BlockB, MalformedBlock, NegativeSequenceBlock}
  THEN RootZero
  ELSE RootOne

\* @type: (Str) => Str;
PostRoot(block) ==
  IF block = MalformedBlock THEN RootOne ELSE ParentRoot(block)

\* @type: (Str) => Int;
HeaderGeneration(block) ==
  IF block \in {BlockA, BlockB, NegativeSequenceBlock} THEN 0 ELSE 1

\* @type: (Str) => Int;
AuthorityGeneration(root) == IF root = RootZero THEN 0 ELSE 1
\* @type: (Str, Str) => Int;
AuthorityStake(root, _validator) == IF root \in Roots THEN 1 ELSE 0

\* @type: (Str) => Int;
HashRank(block) ==
  IF block = BlockA THEN 1
  ELSE IF block = BlockB THEN 2
  ELSE IF block = BlockC THEN 3
  ELSE IF block = MalformedBlock THEN 4
  ELSE 5

\* @type: (Str, Str) => Bool;
SameClaimedFault(left, right) ==
  /\ left /= right
  /\ BlockSender(left) = BlockSender(right)
  /\ BlockSequence(left) = BlockSequence(right)
  /\ HeaderGeneration(left) = HeaderGeneration(right)

\* @type: (Str) => Str;
DerivedRoot(block) == IF UsePostStateAuthority THEN PostRoot(block) ELSE ParentRoot(block)

\* @type: (Str) => Int;
DerivedGeneration(block) ==
  IF TrustHeaderGeneration
  THEN HeaderGeneration(block)
  ELSE AuthorityGeneration(DerivedRoot(block))

\* @type: (Str) => Bool;
CertificateMatchesHeader(block) ==
  HeaderGeneration(block) = DerivedGeneration(block)

\* @type: (Str) => Bool;
CertificateAuthorityPositive(block) ==
  AuthorityStake(DerivedRoot(block), BlockSender(block)) > 0

\* @type: (Str) => Bool;
CertificateEligible(block) ==
  CertificateMatchesHeader(block) /\ CertificateAuthorityPositive(block)

\* @type: (Str) => Bool;
EvidenceEligible(block) == BlockSequence(block) >= 0

VARIABLES
  \* @type: Int -> Set(Str);
  received,
  \* @type: Int -> Set(Str);
  localInvalid,
  \* @type: Int -> Set(Str);
  certificationAttempted,
  \* @type: Int -> (Str -> Int);
  volatileGeneration,
  \* @type: Int -> Set(Str);
  metadata,
  \* @type: Int -> (Str -> Int);
  certifiedGeneration,
  \* @type: Int -> Set(Str);
  evidenceIndex,
  \* @type: Int -> Set(Str);
  retryCompleted,
  \* @type: Int -> Bool;
  reconciled,
  \* @type: Int -> Str;
  latestMessage,
  \* @type: Int -> Int;
  crashCount

vars ==
  <<received, localInvalid, certificationAttempted, volatileGeneration,
    metadata, certifiedGeneration, evidenceIndex, retryCompleted,
    reconciled, latestMessage, crashCount>>

\* @type: Int -> (Str -> Int);
EmptyGenerationMap == [replica \in Replicas |-> [block \in Blocks |-> -1]]

Init ==
  /\ received = [replica \in Replicas |-> {}]
  /\ localInvalid = [replica \in Replicas |-> {}]
  /\ certificationAttempted = [replica \in Replicas |-> {}]
  /\ volatileGeneration = EmptyGenerationMap
  /\ metadata = [replica \in Replicas |-> {}]
  /\ certifiedGeneration = EmptyGenerationMap
  /\ evidenceIndex = [replica \in Replicas |-> {}]
  /\ retryCompleted = [replica \in Replicas |-> {}]
  /\ reconciled = [replica \in Replicas |-> FALSE]
  /\ latestMessage = [replica \in Replicas |-> NoBlock]
  /\ crashCount = [replica \in Replicas |-> 0]

\* @type: (Set(Str)) => Str;
CanonicalLatest(blocks) ==
  IF blocks = {}
  THEN NoBlock
  ELSE CHOOSE block \in blocks :
         \A other \in blocks : HashRank(block) <= HashRank(other)

\* @type: (Int, Str) => Str;
UpdateLatest(replica, block) ==
  IF CanonicalLatestMessageTie
  THEN CanonicalLatest(metadata[replica] \cup {block})
  ELSE block

Receive(replica, block) ==
  /\ block \notin received[replica]
  /\ received' = [received EXCEPT ![replica] = @ \cup {block}]
  /\ localInvalid' =
       IF \E prior \in received[replica] : SameClaimedFault(prior, block)
       THEN [localInvalid EXCEPT ![replica] = @ \cup {block}]
       ELSE localInvalid
  /\ UNCHANGED <<certificationAttempted, volatileGeneration, metadata,
                  certifiedGeneration, evidenceIndex, retryCompleted,
                  reconciled, latestMessage, crashCount>>

Certify(replica, block) ==
  /\ block \in received[replica]
  /\ block \notin certificationAttempted[replica]
  /\ certificationAttempted' =
       [certificationAttempted EXCEPT ![replica] = @ \cup {block}]
  /\ volatileGeneration' =
       IF CertificateEligible(block)
       THEN [volatileGeneration EXCEPT ![replica][block] = DerivedGeneration(block)]
       ELSE volatileGeneration
  /\ UNCHANGED <<received, localInvalid, metadata, certifiedGeneration,
                  evidenceIndex, retryCompleted, reconciled, latestMessage,
                  crashCount>>

PersistMetadata(replica, block) ==
  /\ volatileGeneration[replica][block] >= 0
  /\ block \notin metadata[replica]
  /\ metadata' = [metadata EXCEPT ![replica] = @ \cup {block}]
  /\ certifiedGeneration' =
       [certifiedGeneration EXCEPT
          ![replica][block] = volatileGeneration[replica][block]]
  /\ latestMessage' =
       [latestMessage EXCEPT ![replica] = UpdateLatest(replica, block)]
  /\ reconciled' = [reconciled EXCEPT ![replica] = FALSE]
  /\ UNCHANGED <<received, localInvalid, certificationAttempted,
                  volatileGeneration, evidenceIndex, retryCompleted,
                  crashCount>>

\* @type: (Int, Str) => Bool;
MayIndex(replica, block) ==
  block \in metadata[replica] /\
  (~GateEvidenceOnEligibleSequence \/ EvidenceEligible(block)) /\
  (~GateEvidenceOnLocalInvalid \/ block \notin localInvalid[replica])

PersistEvidence(replica, block) ==
  /\ MayIndex(replica, block)
  /\ block \notin evidenceIndex[replica]
  /\ evidenceIndex' = [evidenceIndex EXCEPT ![replica] = @ \cup {block}]
  /\ UNCHANGED <<received, localInvalid, certificationAttempted,
                  volatileGeneration, metadata, certifiedGeneration,
                  retryCompleted, reconciled, latestMessage, crashCount>>

Crash(replica) ==
  /\ crashCount[replica] < 1
  /\ volatileGeneration' =
       [volatileGeneration EXCEPT ![replica] = [block \in Blocks |-> -1]]
  /\ crashCount' = [crashCount EXCEPT ![replica] = @ + 1]
  /\ UNCHANGED <<received, localInvalid, certificationAttempted, metadata,
                  certifiedGeneration, evidenceIndex, retryCompleted,
                  reconciled, latestMessage>>

RetryDuplicate(replica, block) ==
  /\ block \in metadata[replica]
  /\ block \notin retryCompleted[replica]
  /\ retryCompleted' = [retryCompleted EXCEPT ![replica] = @ \cup {block}]
  /\ evidenceIndex' =
       IF RepairDuplicateInsert /\ MayIndex(replica, block)
       THEN [evidenceIndex EXCEPT ![replica] = @ \cup {block}]
       ELSE evidenceIndex
  /\ UNCHANGED <<received, localInvalid, certificationAttempted,
                  volatileGeneration, metadata, certifiedGeneration,
                  reconciled, latestMessage, crashCount>>

\* @type: (Int) => Set(Str);
EligibleMetadata(replica) ==
  IF GateEvidenceOnEligibleSequence
  THEN {block \in metadata[replica] : EvidenceEligible(block)}
  ELSE metadata[replica]

\* @type: (Int) => Set(Str);
ExpectedEvidence(replica) ==
  IF GateEvidenceOnLocalInvalid
  THEN EligibleMetadata(replica) \ localInvalid[replica]
  ELSE EligibleMetadata(replica)

Reconcile(replica) ==
  /\ ~reconciled[replica]
  /\ evidenceIndex' =
       [evidenceIndex EXCEPT ![replica] = ExpectedEvidence(replica)]
  /\ latestMessage' =
       [latestMessage EXCEPT ![replica] = CanonicalLatest(metadata[replica])]
  /\ reconciled' = [reconciled EXCEPT ![replica] = TRUE]
  /\ UNCHANGED <<received, localInvalid, certificationAttempted,
                  volatileGeneration, metadata, certifiedGeneration,
                  retryCompleted, crashCount>>

Next ==
  \/ \E replica \in Replicas, block \in EnabledBlocks : Receive(replica, block)
  \/ \E replica \in Replicas, block \in EnabledBlocks : Certify(replica, block)
  \/ \E replica \in Replicas, block \in EnabledBlocks : PersistMetadata(replica, block)
  \/ \E replica \in Replicas, block \in EnabledBlocks : PersistEvidence(replica, block)
  \/ \E replica \in Replicas : Crash(replica)
  \/ \E replica \in Replicas, block \in EnabledBlocks : RetryDuplicate(replica, block)
  \/ \E replica \in Replicas : Reconcile(replica)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ received \in [Replicas -> SUBSET Blocks]
  /\ localInvalid \in [Replicas -> SUBSET Blocks]
  /\ certificationAttempted \in [Replicas -> SUBSET Blocks]
  /\ volatileGeneration \in [Replicas -> [Blocks -> -1..1]]
  /\ metadata \in [Replicas -> SUBSET Blocks]
  /\ certifiedGeneration \in [Replicas -> [Blocks -> -1..1]]
  /\ evidenceIndex \in [Replicas -> SUBSET Blocks]
  /\ retryCompleted \in [Replicas -> SUBSET Blocks]
  /\ reconciled \in [Replicas -> BOOLEAN]
  /\ latestMessage \in [Replicas -> Blocks \cup {NoBlock}]
  /\ crashCount \in [Replicas -> 0..1]

Inv_MetadataCertificatesUseExactParentAuthority ==
  \A replica \in Replicas :
    \A block \in metadata[replica] :
      /\ certifiedGeneration[replica][block] =
           AuthorityGeneration(ParentRoot(block))
      /\ HeaderGeneration(block) = certifiedGeneration[replica][block]
      /\ AuthorityStake(ParentRoot(block), BlockSender(block)) > 0

Inv_EveryIndexedBlockHasDurableCertificate ==
  \A replica \in Replicas : evidenceIndex[replica] \subseteq metadata[replica]

\* @type: (Int, Str, Str) => Bool;
SameCertifiedFault(replica, left, right) ==
  /\ left /= right
  /\ BlockSender(left) = BlockSender(right)
  /\ BlockSequence(left) = BlockSequence(right)
  /\ certifiedGeneration[replica][left] = certifiedGeneration[replica][right]

\* @type: (Int) => Set(<<Str, Str>>);
ObjectivePairs(replica) ==
  {pair \in
     {<<left, right>> :
        left \in evidenceIndex[replica],
        right \in evidenceIndex[replica]} :
     HashRank(pair[1]) < HashRank(pair[2]) /\
     SameCertifiedFault(replica, pair[1], pair[2])}

Inv_EveryObjectivePairHasOneCertifiedIdentity ==
  \A replica \in Replicas :
    \A pair \in ObjectivePairs(replica) :
      SameCertifiedFault(replica, pair[1], pair[2])

Inv_HeaderOnlyClaimNeverBecomesEvidence ==
  \A replica \in Replicas : MalformedBlock \notin metadata[replica]

Inv_CrossGenerationSiblingsNeverPair ==
  \A replica \in Replicas :
    <<BlockA, BlockC>> \notin ObjectivePairs(replica) /\
    <<BlockB, BlockC>> \notin ObjectivePairs(replica)

Inv_DuplicateRetryRepairsEvidence ==
  \A replica \in Replicas :
    \A block \in retryCompleted[replica] :
      (block \in metadata[replica] /\ EvidenceEligible(block)) =>
        block \in evidenceIndex[replica]

Inv_ReconcileRepairsAllCertifiedEvidence ==
  \A replica \in Replicas :
    reconciled[replica] => evidenceIndex[replica] = ExpectedEvidence(replica)

Inv_IneligibleSequenceNeverBecomesEvidence ==
  \A replica \in Replicas : NegativeSequenceBlock \notin evidenceIndex[replica]

Inv_ReconciledSiblingEvidenceIsComplete ==
  \A replica \in Replicas :
    (reconciled[replica] /\ {BlockA, BlockB} \subseteq metadata[replica]) =>
      <<BlockA, BlockB>> \in ObjectivePairs(replica)

Inv_CanonicalLatestMessage ==
  \A replica \in Replicas :
    latestMessage[replica] = CanonicalLatest(metadata[replica])

Inv_EquivalentDurableViewsConverge ==
  \A leftReplica \in Replicas, rightReplica \in Replicas :
    metadata[leftReplica] = metadata[rightReplica] =>
      latestMessage[leftReplica] = latestMessage[rightReplica]

Inv_ReconciledEquivalentViewsHaveSameEvidence ==
  \A leftReplica \in Replicas, rightReplica \in Replicas :
    (reconciled[leftReplica] /\ reconciled[rightReplica] /\
     metadata[leftReplica] = metadata[rightReplica]) =>
      evidenceIndex[leftReplica] = evidenceIndex[rightReplica]

=============================================================================
