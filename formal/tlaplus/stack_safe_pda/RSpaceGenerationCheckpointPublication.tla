---------------- MODULE RSpaceGenerationCheckpointPublication ----------------
EXTENDS FiniteSets, Integers, Naturals, Sequences, TLC

(***************************************************************************
Formal-first model of fresh-generation RSpace checkpoint publication.

This module specifies the target contract before the remaining Rust changes.
It deliberately distinguishes:

* L: the exact current-state structural closure;
* R: the exact retained-root union;
* D: R plus the retention-manifest singleton, in strict digest order;
* physical pack/index/catalog/dead bytes; and
* the semantic introduction/delivery/trace (I/D/T) tariff.

Object references are (digest, declared canonical length) records. Sequences,
not sets, make omission, duplication, substitution, conflicting length, and
reordering observably different. The ordered-inventory record abstracts the
domain-separated cryptographic transcript; executable properties bind it to
the production BLAKE2b-256 fold.

The catalog model keeps volatile and durable inactive-slot images separate.
A crash after slot-write uncertainty may persist the old bytes, torn bytes, or
the complete new bytes. Restart either selects the old authority, selects the
complete restart-grade new authority, or rejects; it never silently rolls a
complete but damaged selected successor back to old.
***************************************************************************)

Digests == {"da", "db", "dc", "dx"}
Ref(digest, declaredLen) == [digest |-> digest, declaredLen |-> declaredLen]
A == Ref("da", 11)
B == Ref("db", 13)
C == Ref("dc", 17)
X == Ref("dx", 19)

CurrentMembers == <<A>>
RetainedMembers == <<A, C>>
RetentionManifest == B
ExpectedMembers == <<A, B, C>>

DigestRank(digest) ==
  CASE digest = "da" -> 1
    [] digest = "db" -> 2
    [] digest = "dc" -> 3
    [] digest = "dx" -> 4
    [] OTHER -> 0

RECURSIVE ByteSum(_)
ByteSum(members) ==
  IF Len(members) = 0
  THEN 0
  ELSE Head(members).declaredLen + ByteSum(Tail(members))

StrictDigestOrder(members) ==
  IF Len(members) < 2
  THEN TRUE
  ELSE \A index \in 1..(Len(members) - 1):
    DigestRank(members[index].digest) < DigestRank(members[index + 1].digest)

NoDuplicateDigests(members) ==
  \A left, right \in 1..Len(members):
    left # right => members[left].digest # members[right].digest

DigestSubset(left, right) ==
  \A leftIndex \in 1..Len(left):
    \E rightIndex \in 1..Len(right): left[leftIndex] = right[rightIndex]

CurrentObjects == Len(CurrentMembers)
CurrentBytes == ByteSum(CurrentMembers)
RetainedObjects == Len(RetainedMembers)
RetainedBytes == ByteSum(RetainedMembers)
DataObjects == Len(ExpectedMembers)
DataBytes == ByteSum(ExpectedMembers)

ExactOrderedInventory == [
  domain |-> "RSpaceCheckpointObjectInventoryV1",
  version |-> 1,
  objects |-> DataObjects,
  canonicalBytes |-> DataBytes,
  transcript |-> ExpectedMembers
]

ExactDescriptor == [
  currentObjects |-> CurrentObjects,
  currentBytes |-> CurrentBytes,
  retainedObjects |-> RetainedObjects,
  retainedBytes |-> RetainedBytes,
  dataObjects |-> DataObjects,
  dataBytes |-> DataBytes,
  orderedInventory |-> ExactOrderedInventory
]

EmptyDescriptor == [
  currentObjects |-> 0,
  currentBytes |-> 0,
  retainedObjects |-> 0,
  retainedBytes |-> 0,
  dataObjects |-> 0,
  dataBytes |-> 0,
  orderedInventory |-> [
    domain |-> "RSpaceCheckpointObjectInventoryV1",
    version |-> 1,
    objects |-> 0,
    canonicalBytes |-> 0,
    transcript |-> <<>>
  ]
]

(***************************************************************************
The target V1 checkpoint-only wire revision adds L_n without changing ordinary
incremental WAL bytes. The descriptor grows 208 -> 216 bytes, the kind-3
fixed prefix grows 280 -> 288 bytes, and the kind-4 commit grows 392 -> 400.
***************************************************************************)
PackHeaderLen == 128
PrepareFixedLen == 288
PrepareReferenceLen == 40
PrepareChecksumLen == 8
IndexHeaderLen == 64
IndexRecordLen == 72
CommitRecordLen == 400
ExpectedPrepareLen ==
  PrepareFixedLen + PrepareReferenceLen * DataObjects + DataBytes + PrepareChecksumLen
ExpectedIndexTail == IndexHeaderLen + IndexRecordLen * DataObjects
ExpectedCommitAddress == PackHeaderLen + ExpectedPrepareLen
ExpectedPackTail == ExpectedCommitAddress + CommitRecordLen

Phases == {
  "OldAdmitted", "Preflighted", "CleanupComplete",
  "PrepareWritten", "PrepareDurable", "IndexWritten", "IndexDurable",
  "CommitWritten", "CommitDurable", "ArtifactDirectoryDurable",
  "CasVerified", "CatalogWriteStarted", "CatalogWritten",
  "CatalogFileDurable", "CatalogDirectoryDurable",
  "PhysicalRecovered", "SemanticReadmitted", "Acknowledged",
  "Failed", "RestartPhysicalRecovered", "RestartSemanticReadmitted",
  "RestartCompleted", "RestartRejected"
}

OwnerTokens == {
  "old-writer-0", "publisher-0", "publisher-1",
  "restart-publisher-0", "new-writer-0"
}
SlotImages == {"old-image", "dirty", "torn", "valid-new"}
CatalogImages == {"old-672", "other-672", "new-672"}
Authorities == {"none", "old", "new", "third"}
Outcomes == {
  "none", "definitely-not-published", "indeterminate",
  "published-error", "success"
}

InitialState(initialDescriptor) == [
  phase |-> "OldAdmitted",
  owners |-> <<"old-writer-0">>,
  descriptor |-> initialDescriptor,
  preflighted |-> FALSE,
  cleanupComplete |-> FALSE,
  cleanupExact |-> FALSE,
  cleanupAfterUncertainty |-> FALSE,
  prepareMembers |-> <<>>,
  indexMembers |-> <<>>,
  prepareWritten |-> FALSE,
  prepareDurable |-> FALSE,
  indexWritten |-> FALSE,
  indexDurable |-> FALSE,
  commitWritten |-> FALSE,
  commitDurable |-> FALSE,
  commitBindsPrepare |-> FALSE,
  commitBindsIndex |-> FALSE,
  artifactDirectoryDurable |-> FALSE,
  catalogImage |-> "old-672",
  casObservedImage |-> "none",
  casVerified |-> FALSE,
  volatileSlot |-> "old-image",
  durableSlot |-> "old-image",
  slotWriteStarted |-> FALSE,
  uncertaintyStarted |-> FALSE,
  catalogFileDurable |-> FALSE,
  catalogDirectoryDurable |-> FALSE,
  selectedAuthority |-> "old",
  physicalRecovery |-> "none",
  semanticReadmitted |-> FALSE,
  acknowledged |-> FALSE,
  restartCompleted |-> FALSE,
  outcome |-> "none",
  semanticBefore |-> "state-0",
  semanticAfter |-> "unreadmitted",
  prepareLen |-> 0,
  indexTail |-> 0,
  commitAddress |-> 0,
  packTail |-> 0,
  introductionCharge |-> 0,
  deliveryCharge |-> 0,
  traceCharge |-> 0,
  physicalPackBytes |-> 0,
  physicalIndexBytes |-> 0,
  physicalCatalogBytes |-> 672,
  physicalDeadBytes |-> 0,
  crashFrom |-> "none"
]

VARIABLE state
vars == <<state>>
Init == state = InitialState(EmptyDescriptor)

RestartGrade(s) ==
  /\ s.prepareWritten
  /\ s.prepareDurable
  /\ s.indexWritten
  /\ s.indexDurable
  /\ s.commitWritten
  /\ s.commitDurable
  /\ s.commitBindsPrepare
  /\ s.commitBindsIndex
  /\ s.artifactDirectoryDurable
  /\ s.prepareMembers = ExpectedMembers
  /\ s.indexMembers = ExpectedMembers
  /\ s.descriptor = ExactDescriptor
  /\ s.prepareLen = ExpectedPrepareLen
  /\ s.indexTail = ExpectedIndexTail
  /\ s.commitAddress = ExpectedCommitAddress
  /\ s.packTail = ExpectedPackTail

Preflight ==
  /\ state.phase = "OldAdmitted"
  /\ state.owners = <<"old-writer-0">>
  /\ state' = [state EXCEPT
       !.phase = "Preflighted",
       !.owners = <<"publisher-0">>,
       !.descriptor = ExactDescriptor,
       !.preflighted = TRUE]

CleanupExactSuccessor ==
  /\ state.phase = "Preflighted"
  /\ state.preflighted
  /\ ~state.uncertaintyStarted
  /\ state.owners = <<"publisher-0">>
  /\ state' = [state EXCEPT
       !.phase = "CleanupComplete",
       !.cleanupComplete = TRUE,
       !.cleanupExact = TRUE,
       !.physicalDeadBytes = 1]

WritePrepare ==
  /\ state.phase = "CleanupComplete"
  /\ state.cleanupComplete
  /\ state.cleanupExact
  /\ state' = [state EXCEPT
       !.phase = "PrepareWritten",
       !.prepareMembers = ExpectedMembers,
       !.prepareWritten = TRUE,
       !.prepareLen = ExpectedPrepareLen,
       !.physicalPackBytes = PackHeaderLen + ExpectedPrepareLen]

SynchronizePrepare ==
  /\ state.phase = "PrepareWritten"
  /\ state.prepareWritten
  /\ state' = [state EXCEPT
       !.phase = "PrepareDurable",
       !.prepareDurable = TRUE]

WriteIndex ==
  /\ state.phase = "PrepareDurable"
  /\ state.prepareDurable
  /\ state' = [state EXCEPT
       !.phase = "IndexWritten",
       !.indexMembers = state.prepareMembers,
       !.indexWritten = TRUE,
       !.indexTail = ExpectedIndexTail,
       !.physicalIndexBytes = ExpectedIndexTail]

SynchronizeIndex ==
  /\ state.phase = "IndexWritten"
  /\ state.indexWritten
  /\ state' = [state EXCEPT
       !.phase = "IndexDurable",
       !.indexDurable = TRUE]

WriteCommit ==
  /\ state.phase = "IndexDurable"
  /\ state.indexDurable
  /\ state' = [state EXCEPT
       !.phase = "CommitWritten",
       !.commitWritten = TRUE,
       !.commitBindsPrepare = TRUE,
       !.commitBindsIndex = TRUE,
       !.commitAddress = ExpectedCommitAddress,
       !.packTail = ExpectedPackTail,
       !.physicalPackBytes = ExpectedPackTail]

SynchronizeCommit ==
  /\ state.phase = "CommitWritten"
  /\ state.commitWritten
  /\ state.commitBindsPrepare
  /\ state.commitBindsIndex
  /\ state' = [state EXCEPT
       !.phase = "CommitDurable",
       !.commitDurable = TRUE]

SynchronizeArtifactDirectory ==
  /\ state.phase = "CommitDurable"
  /\ state.commitDurable
  /\ state' = [state EXCEPT
       !.phase = "ArtifactDirectoryDurable",
       !.artifactDirectoryDurable = TRUE]

VerifyExactCatalogCas ==
  /\ state.phase = "ArtifactDirectoryDurable"
  /\ state.catalogImage = "old-672"
  /\ state' = [state EXCEPT
       !.phase = "CasVerified",
       !.casObservedImage = state.catalogImage,
       !.casVerified = TRUE]

ExternalCatalogChange ==
  /\ state.phase = "ArtifactDirectoryDurable"
  /\ state.catalogImage = "old-672"
  /\ state' = [state EXCEPT !.catalogImage = "other-672"]

RejectStaleCatalogCas ==
  /\ state.phase = "ArtifactDirectoryDurable"
  /\ state.catalogImage # "old-672"
  /\ state.owners = <<"publisher-0">>
  /\ state' = [state EXCEPT
       !.phase = "Failed",
       !.owners = <<>>,
       !.outcome = "definitely-not-published",
       !.crashFrom = "ArtifactDirectoryDurable"]

BeginCatalogSlotWrite ==
  /\ state.phase = "CasVerified"
  /\ state.casVerified
  /\ state.casObservedImage = "old-672"
  /\ state' = [state EXCEPT
       !.phase = "CatalogWriteStarted",
       !.volatileSlot = "dirty",
       !.slotWriteStarted = TRUE,
       !.uncertaintyStarted = TRUE]

FinishCatalogSlotWrite ==
  /\ state.phase = "CatalogWriteStarted"
  /\ state.slotWriteStarted
  /\ state' = [state EXCEPT
       !.phase = "CatalogWritten",
       !.volatileSlot = "valid-new"]

SynchronizeCatalogFile ==
  /\ state.phase = "CatalogWritten"
  /\ state.volatileSlot = "valid-new"
  /\ RestartGrade(state)
  /\ state' = [state EXCEPT
       !.phase = "CatalogFileDurable",
       !.durableSlot = "valid-new",
       !.catalogImage = "new-672",
       !.catalogFileDurable = TRUE,
       !.selectedAuthority = "new"]

SynchronizeCatalogDirectory ==
  /\ state.phase = "CatalogFileDurable"
  /\ state.catalogFileDurable
  /\ state.selectedAuthority = "new"
  /\ state' = [state EXCEPT
       !.phase = "CatalogDirectoryDurable",
       !.catalogDirectoryDurable = TRUE]

RecoverNewWithHeldLease ==
  /\ state.phase = "CatalogDirectoryDurable"
  /\ state.owners = <<"publisher-0">>
  /\ state.catalogDirectoryDurable
  /\ state.selectedAuthority = "new"
  /\ RestartGrade(state)
  /\ state' = [state EXCEPT
       !.phase = "PhysicalRecovered",
       !.physicalRecovery = "new"]

ReadmitSemantics ==
  /\ state.phase = "PhysicalRecovered"
  /\ state.physicalRecovery = "new"
  /\ state' = [state EXCEPT
       !.phase = "SemanticReadmitted",
       !.semanticReadmitted = TRUE,
       !.semanticAfter = state.semanticBefore]

Acknowledge ==
  /\ state.phase = "SemanticReadmitted"
  /\ state.owners = <<"publisher-0">>
  /\ state.catalogDirectoryDurable
  /\ state.physicalRecovery = "new"
  /\ state.semanticReadmitted
  /\ state.semanticAfter = state.semanticBefore
  /\ state.outcome = "none"
  /\ state' = [state EXCEPT
       !.phase = "Acknowledged",
       !.owners = <<"new-writer-0">>,
       !.acknowledged = TRUE,
       !.outcome = "success"]

PreUncertaintyPhases == {
  "Preflighted", "CleanupComplete", "PrepareWritten", "PrepareDurable",
  "IndexWritten", "IndexDurable", "CommitWritten", "CommitDurable",
  "ArtifactDirectoryDurable", "CasVerified"
}

FailBeforeUncertainty ==
  /\ state.phase \in PreUncertaintyPhases
  /\ ~state.uncertaintyStarted
  /\ state' = [state EXCEPT
       !.phase = "Failed",
       !.owners = <<>>,
       !.outcome = "definitely-not-published",
       !.crashFrom = state.phase]

FailDuringUncertainWrite(persisted) ==
  /\ state.phase \in {"CatalogWriteStarted", "CatalogWritten"}
  /\ state.uncertaintyStarted
  /\ persisted \in {"old-image", "torn", "valid-new"}
  /\ state' = [state EXCEPT
       !.phase = "Failed",
       !.owners = <<>>,
       !.volatileSlot = IF persisted = "valid-new" THEN "valid-new" ELSE "torn",
       !.durableSlot = persisted,
       !.catalogImage = IF persisted = "valid-new" THEN "new-672" ELSE "old-672",
       !.selectedAuthority = "none",
       !.outcome = "indeterminate",
       !.crashFrom = state.phase]

FailAnyUncertainWrite ==
  \E persisted \in {"old-image", "torn", "valid-new"}:
    FailDuringUncertainWrite(persisted)

PublishedPhases == {
  "CatalogFileDurable", "CatalogDirectoryDurable",
  "PhysicalRecovered", "SemanticReadmitted"
}

FailAfterCatalogFileSync ==
  /\ state.phase \in PublishedPhases
  /\ state.catalogFileDurable
  /\ state.durableSlot = "valid-new"
  /\ state' = [state EXCEPT
       !.phase = "Failed",
       !.owners = <<>>,
       !.selectedAuthority = "new",
       !.outcome = "published-error",
       !.crashFrom = state.phase]

RestartSelectOld ==
  /\ state.phase = "Failed"
  /\ state.durableSlot \in {"old-image", "torn"}
  /\ state' = [state EXCEPT
       !.phase = "RestartPhysicalRecovered",
       !.owners = <<"restart-publisher-0">>,
       !.selectedAuthority = "old",
       !.physicalRecovery = "old"]

RestartSelectNew ==
  /\ state.phase = "Failed"
  /\ state.durableSlot = "valid-new"
  /\ RestartGrade(state)
  /\ state' = [state EXCEPT
       !.phase = "RestartPhysicalRecovered",
       !.owners = <<"restart-publisher-0">>,
       !.selectedAuthority = "new",
       !.physicalRecovery = "new"]

RestartRejectDamagedNew ==
  /\ state.phase = "Failed"
  /\ state.durableSlot = "valid-new"
  /\ ~RestartGrade(state)
  /\ state' = [state EXCEPT
       !.phase = "RestartRejected",
       !.owners = <<>>,
       !.selectedAuthority = "none",
       !.physicalRecovery = "reject"]

ReadmitRestart ==
  /\ state.phase = "RestartPhysicalRecovered"
  /\ state.physicalRecovery \in {"old", "new"}
  /\ state.owners = <<"restart-publisher-0">>
  /\ state' = [state EXCEPT
       !.phase = "RestartSemanticReadmitted",
       !.semanticReadmitted = TRUE,
       !.semanticAfter = state.semanticBefore]

CompleteRestart ==
  /\ state.phase = "RestartSemanticReadmitted"
  /\ state.semanticReadmitted
  /\ state.semanticAfter = state.semanticBefore
  /\ state' = [state EXCEPT
       !.phase = "RestartCompleted",
       !.owners = IF state.physicalRecovery = "old"
                  THEN <<"old-writer-0">>
                  ELSE <<"new-writer-0">>,
       !.restartCompleted = TRUE]

(** Finite physical-layout perturbation proves semantic charges do not depend
    on dead physical bytes. *)
AddDeadPhysicalByte ==
  /\ state.phase = "Preflighted"
  /\ state.physicalDeadBytes = 0
  /\ state' = [state EXCEPT !.physicalDeadBytes = 1]

Next ==
  Preflight \/ CleanupExactSuccessor \/ WritePrepare \/ SynchronizePrepare \/
  WriteIndex \/ SynchronizeIndex \/ WriteCommit \/ SynchronizeCommit \/
  SynchronizeArtifactDirectory \/ VerifyExactCatalogCas \/
  ExternalCatalogChange \/ RejectStaleCatalogCas \/ BeginCatalogSlotWrite \/
  FinishCatalogSlotWrite \/ SynchronizeCatalogFile \/
  SynchronizeCatalogDirectory \/ RecoverNewWithHeldLease \/
  ReadmitSemantics \/ Acknowledge \/ FailBeforeUncertainty \/
  FailAnyUncertainWrite \/ FailAfterCatalogFileSync \/ RestartSelectOld \/
  RestartSelectNew \/ RestartRejectDamagedNew \/ ReadmitRestart \/
  CompleteRestart \/ AddDeadPhysicalByte

Spec == Init /\ [][Next]_vars

(***************************************************************************
Certified invariants.
***************************************************************************)

TypeInvariant ==
  /\ state.phase \in Phases
  /\ state.owners \in Seq(OwnerTokens)
  /\ state.prepareMembers \in Seq({A, B, C, X})
  /\ state.indexMembers \in Seq({A, B, C, X})
  /\ state.catalogImage \in CatalogImages
  /\ state.casObservedImage \in CatalogImages \union {"none"}
  /\ state.volatileSlot \in SlotImages
  /\ state.durableSlot \in SlotImages
  /\ state.selectedAuthority \in Authorities
  /\ state.physicalRecovery \in Authorities
  /\ state.outcome \in Outcomes
  /\ state.prepareLen \in Nat
  /\ state.indexTail \in Nat
  /\ state.commitAddress \in Nat
  /\ state.packTail \in Nat
  /\ state.physicalPackBytes \in Nat
  /\ state.physicalIndexBytes \in Nat
  /\ state.physicalCatalogBytes \in Nat
  /\ state.physicalDeadBytes \in 0..1

DescriptorInventoriesAreExact ==
  state.preflighted =>
    /\ state.descriptor.currentObjects = CurrentObjects
    /\ state.descriptor.currentBytes = CurrentBytes
    /\ state.descriptor.retainedObjects = RetainedObjects
    /\ state.descriptor.retainedBytes = RetainedBytes
    /\ state.descriptor.dataObjects = state.descriptor.retainedObjects + 1
    /\ state.descriptor.dataBytes =
         state.descriptor.retainedBytes + RetentionManifest.declaredLen
    /\ state.descriptor.dataObjects = DataObjects
    /\ state.descriptor.dataBytes = DataBytes
    /\ state.descriptor.currentObjects <= state.descriptor.retainedObjects
    /\ state.descriptor.currentBytes <= state.descriptor.retainedBytes

OrderedInventoryIsExact ==
  state.preflighted =>
    /\ StrictDigestOrder(RetainedMembers)
    /\ NoDuplicateDigests(RetainedMembers)
    /\ DigestSubset(CurrentMembers, RetainedMembers)
    /\ ~\E index \in 1..Len(RetainedMembers):
         RetainedMembers[index].digest = RetentionManifest.digest
    /\ StrictDigestOrder(ExpectedMembers)
    /\ NoDuplicateDigests(ExpectedMembers)
    /\ state.descriptor.orderedInventory = ExactOrderedInventory

WireLayoutIsExact ==
  /\ state.prepareWritten => state.prepareLen = ExpectedPrepareLen
  /\ state.indexWritten => state.indexTail = ExpectedIndexTail
  /\ state.commitWritten =>
       /\ state.commitAddress = ExpectedCommitAddress
       /\ state.packTail = ExpectedPackTail

AtMostOneMutableAuthority ==
  /\ Len(state.owners) <= 1
  /\ \A left, right \in 1..Len(state.owners):
       left # right => state.owners[left] # state.owners[right]

AuthorityIsContinuous ==
  state.phase \notin {"Failed", "RestartRejected"} => Len(state.owners) = 1

PhaseFollowsAffineProtocol ==
  /\ state.prepareDurable => state.prepareWritten
  /\ state.indexWritten => state.prepareDurable
  /\ state.indexDurable => state.indexWritten /\ state.prepareDurable
  /\ state.commitWritten => state.indexDurable
  /\ state.commitDurable => state.commitWritten /\ state.indexDurable
  /\ state.artifactDirectoryDurable => state.commitDurable
  /\ state.slotWriteStarted => state.casVerified /\ state.artifactDirectoryDurable
  /\ state.catalogFileDurable =>
       state.slotWriteStarted /\ state.durableSlot = "valid-new"
  /\ state.catalogDirectoryDurable => state.catalogFileDurable
  /\ state.semanticReadmitted => state.physicalRecovery \in {"old", "new"}
  /\ state.acknowledged => state.semanticReadmitted

PreflightAndCleanupPrecedeMutation ==
  /\ (state.prepareWritten \/ state.indexWritten \/ state.commitWritten \/
       state.slotWriteStarted) => state.preflighted
  /\ state.prepareWritten => state.cleanupComplete /\ state.cleanupExact
  /\ ~state.cleanupAfterUncertainty

CatalogWriteUsesExactOldImage ==
  state.slotWriteStarted =>
    /\ state.casVerified
    /\ state.casObservedImage = "old-672"

NewAuthorityRequiresRestartGradeGeneration ==
  state.selectedAuthority = "new" =>
    /\ state.durableSlot = "valid-new"
    /\ RestartGrade(state)

CheckpointAndIndexExactlyAdmitted ==
  /\ state.prepareDurable => state.prepareMembers = ExpectedMembers
  /\ state.indexDurable => state.indexMembers = ExpectedMembers
  /\ state.commitDurable =>
       /\ state.prepareMembers = ExpectedMembers
       /\ state.indexMembers = ExpectedMembers
       /\ state.commitBindsPrepare
       /\ state.commitBindsIndex

CrashRecoverySelectsOnlyValidAuthority ==
  /\ state.physicalRecovery = "old" =>
       state.durableSlot \in {"old-image", "torn"}
  /\ state.physicalRecovery = "new" =>
       state.durableSlot = "valid-new" /\ RestartGrade(state)
  /\ state.physicalRecovery = "reject" =>
       state.durableSlot = "valid-new" /\ ~RestartGrade(state)
  /\ state.physicalRecovery # "third"

PhysicalRecoveryRequiresCatalogDirectory ==
  state.phase \in {"PhysicalRecovered", "SemanticReadmitted", "Acknowledged"} =>
    state.catalogDirectoryDurable

OutcomeBoundaryIsExact ==
  /\ state.outcome = "definitely-not-published" => ~state.uncertaintyStarted
  /\ state.outcome = "indeterminate" =>
       state.uncertaintyStarted /\ ~state.catalogFileDurable
  /\ state.outcome = "published-error" =>
       state.catalogFileDurable /\ ~state.acknowledged
  /\ state.outcome = "success" <=> state.acknowledged

AcknowledgementRequiresRestartPathReadmission ==
  state.acknowledged =>
    /\ state.phase = "Acknowledged"
    /\ state.owners = <<"new-writer-0">>
    /\ state.catalogDirectoryDurable
    /\ state.selectedAuthority = "new"
    /\ state.physicalRecovery = "new"
    /\ state.semanticReadmitted
    /\ state.semanticAfter = state.semanticBefore

RestartCompletionPreservesOriginalOutcome ==
  state.restartCompleted =>
    /\ state.semanticReadmitted
    /\ state.semanticAfter = state.semanticBefore
    /\ state.outcome # "success"
    /\ state.owners \in {<<"old-writer-0">>, <<"new-writer-0">>}

CheckpointHasZeroSemanticCharge ==
  /\ state.introductionCharge = 0
  /\ state.deliveryCharge = 0
  /\ state.traceCharge = 0

SemanticStateIsPreserved ==
  state.semanticReadmitted => state.semanticAfter = state.semanticBefore

(***************************************************************************
Non-vacuity witnesses: each is expected to fail as an invariant under Spec.
***************************************************************************)
NeverAcknowledged == state.phase # "Acknowledged"
NeverRecoveredOld == ~(state.phase = "RestartCompleted" /\ state.physicalRecovery = "old")
NeverRecoveredNew == ~(state.phase = "RestartCompleted" /\ state.physicalRecovery = "new")
NeverPublishedFailure == state.outcome # "published-error"
NeverIndeterminate == state.outcome # "indeterminate"

(***************************************************************************
Negative controls. Each mutant is independently enabled by one configuration
and must violate its named invariant.
***************************************************************************)

AffineCloneMutant ==
  /\ state.phase = "Preflighted"
  /\ state.owners = <<"publisher-0">>
  /\ state' = [state EXCEPT !.owners = <<"publisher-0", "publisher-1">>]
AffineCloneMutantSpec == Init /\ [][Next \/ AffineCloneMutant]_vars

LeaseGapMutant ==
  /\ state.phase = "Preflighted"
  /\ state.owners = <<"publisher-0">>
  /\ state' = [state EXCEPT !.owners = <<>>]
LeaseGapMutantSpec == Init /\ [][Next \/ LeaseGapMutant]_vars

PhaseSkipMutant ==
  /\ state.phase = "CommitWritten"
  /\ ~state.commitDurable
  /\ state' = [state EXCEPT
       !.phase = "ArtifactDirectoryDurable",
       !.artifactDirectoryDurable = TRUE]
PhaseSkipMutantSpec == Init /\ [][Next \/ PhaseSkipMutant]_vars

PrematureCatalogMutant ==
  /\ state.phase = "IndexDurable"
  /\ state' = [state EXCEPT
       !.phase = "CatalogFileDurable",
       !.slotWriteStarted = TRUE,
       !.uncertaintyStarted = TRUE,
       !.durableSlot = "valid-new",
       !.catalogFileDurable = TRUE,
       !.selectedAuthority = "new"]
PrematureCatalogMutantSpec == Init /\ [][Next \/ PrematureCatalogMutant]_vars

BadMemberSequences == {
  <<A, B>>,
  <<A, B, B, C>>,
  <<A, C, B>>,
  <<A, B, X>>,
  <<A, Ref("db", 19), C>>
}
MembershipSubstitutionMutant ==
  \E badMembers \in BadMemberSequences:
    /\ state.phase = "CleanupComplete"
    /\ state' = [state EXCEPT
         !.phase = "PrepareWritten",
         !.prepareMembers = badMembers,
         !.prepareWritten = TRUE,
         !.prepareLen = ExpectedPrepareLen]
MembershipSubstitutionMutantSpec == Init /\ [][Next \/ MembershipSubstitutionMutant]_vars

StaleCasMutant ==
  /\ state.phase = "ArtifactDirectoryDurable"
  /\ state.catalogImage = "other-672"
  /\ state' = [state EXCEPT
       !.phase = "CatalogWriteStarted",
       !.casVerified = TRUE,
       !.casObservedImage = "other-672",
       !.volatileSlot = "dirty",
       !.slotWriteStarted = TRUE,
       !.uncertaintyStarted = TRUE]
StaleCasMutantSpec == Init /\ [][Next \/ StaleCasMutant]_vars

ThirdAuthorityMutant ==
  /\ state.phase = "Failed"
  /\ state.durableSlot = "torn"
  /\ state' = [state EXCEPT
       !.phase = "RestartPhysicalRecovered",
       !.owners = <<"restart-publisher-0">>,
       !.selectedAuthority = "third",
       !.physicalRecovery = "third"]
ThirdAuthorityMutantSpec == Init /\ [][Next \/ ThirdAuthorityMutant]_vars

EarlyAckMutant ==
  /\ state.phase = "CatalogFileDurable"
  /\ state' = [state EXCEPT
       !.phase = "Acknowledged",
       !.owners = <<"new-writer-0">>,
       !.acknowledged = TRUE,
       !.outcome = "success"]
EarlyAckMutantSpec == Init /\ [][Next \/ EarlyAckMutant]_vars

InventoryCountMutant ==
  /\ state.phase = "OldAdmitted"
  /\ state' = [state EXCEPT
       !.phase = "Preflighted",
       !.owners = <<"publisher-0">>,
       !.preflighted = TRUE,
       !.descriptor = [ExactDescriptor EXCEPT !.currentObjects = CurrentObjects + 1]]
InventoryCountMutantSpec == Init /\ [][Next \/ InventoryCountMutant]_vars

InventoryBytesMutant ==
  /\ state.phase = "OldAdmitted"
  /\ state' = [state EXCEPT
       !.phase = "Preflighted",
       !.owners = <<"publisher-0">>,
       !.preflighted = TRUE,
       !.descriptor = [ExactDescriptor EXCEPT !.dataBytes = DataBytes + 1]]
InventoryBytesMutantSpec == Init /\ [][Next \/ InventoryBytesMutant]_vars

InventoryDigestMutant ==
  /\ state.phase = "OldAdmitted"
  /\ state' = [state EXCEPT
       !.phase = "Preflighted",
       !.owners = <<"publisher-0">>,
       !.preflighted = TRUE,
       !.descriptor = [ExactDescriptor EXCEPT
         !.orderedInventory.transcript = <<A, C, B>>]]
InventoryDigestMutantSpec == Init /\ [][Next \/ InventoryDigestMutant]_vars

DirtySlotSelectionMutant ==
  /\ state.phase = "CatalogWriteStarted"
  /\ state.volatileSlot = "dirty"
  /\ state' = [state EXCEPT
       !.selectedAuthority = "new",
       !.physicalRecovery = "new"]
DirtySlotSelectionMutantSpec == Init /\ [][Next \/ DirtySlotSelectionMutant]_vars

CatalogFailureDnpMutant ==
  /\ state.phase = "CatalogFileDurable"
  /\ state' = [state EXCEPT
       !.phase = "Failed",
       !.owners = <<>>,
       !.outcome = "definitely-not-published"]
CatalogFailureDnpMutantSpec == Init /\ [][Next \/ CatalogFailureDnpMutant]_vars

SkipCatalogDirectoryMutant ==
  /\ state.phase = "CatalogFileDurable"
  /\ ~state.catalogDirectoryDurable
  /\ state' = [state EXCEPT
       !.phase = "PhysicalRecovered",
       !.physicalRecovery = "new"]
SkipCatalogDirectoryMutantSpec == Init /\ [][Next \/ SkipCatalogDirectoryMutant]_vars

CleanupAfterUncertaintyMutant ==
  /\ state.phase = "CatalogWriteStarted"
  /\ state.uncertaintyStarted
  /\ state' = [state EXCEPT !.cleanupAfterUncertainty = TRUE]
CleanupAfterUncertaintyMutantSpec == Init /\ [][Next \/ CleanupAfterUncertaintyMutant]_vars

WireLayoutMutant ==
  /\ state.phase = "CleanupComplete"
  /\ state' = [state EXCEPT
       !.phase = "PrepareWritten",
       !.prepareMembers = ExpectedMembers,
       !.prepareWritten = TRUE,
       !.prepareLen = ExpectedPrepareLen + 1]
WireLayoutMutantSpec == Init /\ [][Next \/ WireLayoutMutant]_vars

ChargePollutionMutant ==
  /\ state.phase = "PrepareWritten"
  /\ state' = [state EXCEPT !.introductionCharge = 1]
ChargePollutionMutantSpec == Init /\ [][Next \/ ChargePollutionMutant]_vars

PhysicalMeteringMutant ==
  /\ state.phase = "PrepareWritten"
  /\ state.physicalPackBytes > 0
  /\ state' = [state EXCEPT !.traceCharge = state.physicalPackBytes]
PhysicalMeteringMutantSpec == Init /\ [][Next \/ PhysicalMeteringMutant]_vars

=============================================================================
