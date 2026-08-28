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
* the complete RuntimeBudget accounting snapshot used by this branch's
  consensus COMM cost model and diagnostic reconciliation trace.

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

AccountingSnapshot(seed) == [
  totalCost |-> seed,
  remaining |-> 1 - seed,
  traceDigest |-> IF seed = 0 THEN "trace-empty" ELSE "trace-seeded",
  traceEventCount |-> seed,
  lastOopEvent |-> IF seed = 0 THEN "none" ELSE "seeded-oop",
  perLaneDemand |-> <<seed, seed + 1>>
]
AccountingSnapshots == {AccountingSnapshot(0), AccountingSnapshot(1)}

(***************************************************************************
The exact checkpoint receipt binds authority facts rather than trusting a
Boolean supplied by the implementation.  Digests and semantic authorities
remain opaque finite tokens here; concrete cryptographic authentication is an
executable refinement obligation.
***************************************************************************)
ExactDescriptorAuthority == [
  profile |-> "profile-rspace-v1",
  state |-> "state-1",
  retention |-> RetentionManifest,
  inventory |-> ExactDescriptor
]
ExactPrepareIdentity == [
  sequence |-> 1,
  store |-> "store-0",
  contentDigest |-> "prepare-digest-0",
  recordLength |-> ExpectedPrepareLen,
  descriptorAuthority |-> ExactDescriptorAuthority
]
ExactPrepareAddress == [
  pack |-> 1,
  offset |-> PackHeaderLen,
  length |-> ExpectedPrepareLen
]
ExactIndexAuthority == [
  firstLivePack |-> 1,
  generation |-> 1,
  durableTail |-> ExpectedIndexTail,
  objectCount |-> DataObjects,
  membershipDigest |-> ExactOrderedInventory,
  retentionAuthority |-> RetentionManifest,
  currentBytes |-> CurrentBytes,
  retainedBytes |-> RetainedBytes
]
ExactPriorTerminal == "prior-terminal-0"
ExactCommitReceipt == [
  sequence |-> ExactPrepareIdentity.sequence,
  store |-> ExactPrepareIdentity.store,
  previousTerminal |-> ExactPriorTerminal,
  prepareAddress |-> ExactPrepareAddress,
  prepareDigest |-> ExactPrepareIdentity.contentDigest,
  descriptorAuthority |-> ExactPrepareIdentity.descriptorAuthority,
  indexPublication |-> ExactIndexAuthority
]
ExactBindingState == [
  present |-> TRUE,
  prepareIdentity |-> ExactPrepareIdentity,
  prepareAddress |-> ExactPrepareAddress,
  indexAuthority |-> ExactIndexAuthority,
  previousTerminal |-> ExactPriorTerminal,
  commitReceipt |-> ExactCommitReceipt
]
EmptyBindingState == [ExactBindingState EXCEPT !.present = FALSE]

ReceiptBindsPrepare(binding) ==
  /\ binding.commitReceipt.sequence = binding.prepareIdentity.sequence
  /\ binding.commitReceipt.store = binding.prepareIdentity.store
  /\ binding.commitReceipt.prepareDigest = binding.prepareIdentity.contentDigest
  /\ binding.commitReceipt.descriptorAuthority =
       binding.prepareIdentity.descriptorAuthority
  /\ binding.commitReceipt.prepareAddress = binding.prepareAddress
  /\ binding.prepareAddress.length = binding.prepareIdentity.recordLength

ReceiptBindsIndex(binding) ==
  /\ binding.commitReceipt.indexPublication.firstLivePack =
       binding.indexAuthority.firstLivePack
  /\ binding.commitReceipt.indexPublication.generation =
       binding.indexAuthority.generation
  /\ binding.commitReceipt.indexPublication.durableTail =
       binding.indexAuthority.durableTail
  /\ binding.commitReceipt.indexPublication.objectCount =
       binding.indexAuthority.objectCount
  /\ binding.commitReceipt.indexPublication.membershipDigest =
       binding.indexAuthority.membershipDigest
  /\ binding.commitReceipt.indexPublication.retentionAuthority =
       binding.indexAuthority.retentionAuthority
  /\ binding.commitReceipt.indexPublication.currentBytes =
       binding.indexAuthority.currentBytes
  /\ binding.commitReceipt.indexPublication.retainedBytes =
       binding.indexAuthority.retainedBytes

ReceiptBindsPrior(binding) ==
  binding.commitReceipt.previousTerminal = binding.previousTerminal

ReceiptValid(binding) ==
  binding.present /\
  ReceiptBindsPrepare(binding) /\
  ReceiptBindsIndex(binding) /\
  ReceiptBindsPrior(binding)

ArtifactsExact(binding) ==
  /\ binding.prepareIdentity = ExactPrepareIdentity
  /\ binding.prepareAddress = ExactPrepareAddress
  /\ binding.indexAuthority = ExactIndexAuthority
  /\ binding.previousTerminal = ExactPriorTerminal

BindingFacts == {
  "sequence", "store", "previous-terminal",
  "prepare-pack", "prepare-offset", "prepare-length", "prepare-digest",
  "descriptor", "index-first-live", "index-generation", "index-tail",
  "index-count", "index-digest", "index-retention",
  "index-current-bytes", "index-retained-bytes"
}

MutateBinding(binding, fact) ==
  CASE fact = "sequence" ->
         [binding EXCEPT !.commitReceipt.sequence = 2]
    [] fact = "store" ->
         [binding EXCEPT !.commitReceipt.store = "store-mutant"]
    [] fact = "previous-terminal" ->
         [binding EXCEPT !.commitReceipt.previousTerminal = "prior-mutant"]
    [] fact = "prepare-pack" ->
         [binding EXCEPT !.commitReceipt.prepareAddress.pack = 2]
    [] fact = "prepare-offset" ->
         [binding EXCEPT !.commitReceipt.prepareAddress.offset = PackHeaderLen + 1]
    [] fact = "prepare-length" ->
         [binding EXCEPT !.commitReceipt.prepareAddress.length = ExpectedPrepareLen + 1]
    [] fact = "prepare-digest" ->
         [binding EXCEPT !.commitReceipt.prepareDigest = "prepare-digest-mutant"]
    [] fact = "descriptor" ->
         [binding EXCEPT
            !.commitReceipt.descriptorAuthority.profile = "profile-mutant"]
    [] fact = "index-first-live" ->
         [binding EXCEPT !.commitReceipt.indexPublication.firstLivePack = 2]
    [] fact = "index-generation" ->
         [binding EXCEPT !.commitReceipt.indexPublication.generation = 2]
    [] fact = "index-tail" ->
         [binding EXCEPT
            !.commitReceipt.indexPublication.durableTail = ExpectedIndexTail + 1]
    [] fact = "index-count" ->
         [binding EXCEPT !.indexAuthority.objectCount = DataObjects + 1]
    [] fact = "index-digest" ->
         [binding EXCEPT
            !.indexAuthority.membershipDigest = [
              domain |-> "mutant", version |-> 1, objects |-> DataObjects,
              canonicalBytes |-> DataBytes, transcript |-> ExpectedMembers
            ]]
    [] fact = "index-retention" ->
         [binding EXCEPT !.indexAuthority.retentionAuthority = X]
    [] fact = "index-current-bytes" ->
         [binding EXCEPT !.indexAuthority.currentBytes = CurrentBytes + 1]
    [] fact = "index-retained-bytes" ->
         [binding EXCEPT !.indexAuthority.retainedBytes = RetainedBytes + 1]

BindingStates ==
  {ExactBindingState} \union
  {MutateBinding(ExactBindingState, fact) : fact \in BindingFacts}

(***************************************************************************
Resource preflight selects one exact boundary or required-minus-one witness;
all other resources remain at their exact boundary.  This keeps TLC finite
while quantifying over every positive-demand durable, checkpoint-WAL, and
semantic-admission resource family.
***************************************************************************)
DurableResourceKinds == {
  "durable-live-packs", "durable-pack-bytes", "durable-scan-bytes",
  "durable-object-records", "durable-index-records", "durable-index-bytes"
}
WalResourceKinds == {
  "wal-objects", "wal-object-bytes", "wal-largest-object", "wal-record-bytes"
}
AdmissionResourceKinds == {
  "admission-retained-states", "admission-union-objects",
  "admission-union-bytes", "admission-root-visits",
  "admission-references", "admission-object-reads",
  "admission-object-read-bytes", "admission-per-state-objects",
  "admission-per-state-bytes", "admission-per-state-references",
  "admission-per-state-worklist", "admission-logical-nodes",
  "admission-endpoints", "admission-path-bytes", "admission-payload-bytes",
  "admission-join-lookups", "admission-join-steps",
  "admission-semantic-worklist"
}
ResourceKinds ==
  DurableResourceKinds \union WalResourceKinds \union AdmissionResourceKinds
ResourceModes == {"exact", "minus-one"}

ResourceFamily(resource) ==
  IF resource \in DurableResourceKinds THEN "durable"
  ELSE IF resource \in WalResourceKinds THEN "wal"
  ELSE "admission"

RequiredResource(resource) ==
  CASE resource = "durable-live-packs" -> 1
    [] resource \in {"durable-pack-bytes", "durable-scan-bytes"} ->
         ExpectedPackTail
    [] resource \in {"durable-object-records", "durable-index-records"} ->
         DataObjects
    [] resource = "durable-index-bytes" -> ExpectedIndexTail
    [] resource = "wal-objects" -> DataObjects
    [] resource = "wal-object-bytes" -> DataBytes
    [] resource = "wal-largest-object" -> 17
    [] resource = "wal-record-bytes" -> ExpectedPrepareLen
    [] resource = "admission-retained-states" -> 2
    [] resource \in {
         "admission-union-objects", "admission-per-state-objects",
         "admission-logical-nodes", "admission-endpoints"
       } -> DataObjects
    [] resource \in {
         "admission-union-bytes", "admission-object-read-bytes",
         "admission-per-state-bytes", "admission-path-bytes",
         "admission-payload-bytes"
       } -> DataBytes
    [] resource \in {
         "admission-root-visits", "admission-references",
         "admission-object-reads", "admission-per-state-references",
         "admission-per-state-worklist", "admission-join-lookups",
         "admission-join-steps", "admission-semantic-worklist"
       } -> 3
    [] OTHER -> 0

MinimumResource(resource) ==
  IF resource \in DurableResourceKinds THEN 1 ELSE 0

ExactBoundary(resource) ==
  IF RequiredResource(resource) < MinimumResource(resource)
  THEN MinimumResource(resource)
  ELSE RequiredResource(resource)

SelectedLimit(resource, mode) ==
  IF mode = "exact"
  THEN ExactBoundary(resource)
  ELSE ExactBoundary(resource) - 1

ResourceWithinLimit(resource, limit) ==
  /\ MinimumResource(resource) <= limit
  /\ RequiredResource(resource) <= limit

ZeroDemandDurableKinds == {
  "durable-transactions", "durable-full-comparison-bytes",
  "durable-aggregate-comparison-bytes"
}
ZeroDemandWalKinds == {
  "wal-mutation-bytes", "wal-mutation-entries", "wal-key-bytes"
}
ZeroDemandRequired(resource) == 0
ZeroDemandMinimum(resource) ==
  IF resource \in ZeroDemandDurableKinds THEN 1 ELSE 0
ZeroDemandExactBoundary(resource) == ZeroDemandMinimum(resource)

InitialState(initialDescriptor, initialAccounting, selectedResource, resourceMode) == [
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
  bindingState |-> EmptyBindingState,
  selectedResource |-> selectedResource,
  selectedResourceFamily |-> ResourceFamily(selectedResource),
  resourceMode |-> resourceMode,
  resourceRequired |-> RequiredResource(selectedResource),
  resourceMinimum |-> MinimumResource(selectedResource),
  resourceLimit |-> SelectedLimit(selectedResource, resourceMode),
  resourceDecision |-> "unchecked",
  refusedResource |-> "none",
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
  authoritativeGenerationIntact |-> FALSE,
  physicalIndexArtifact |-> "absent",
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
  accountingBefore |-> initialAccounting,
  accountingAfter |-> initialAccounting,
  physicalPackBytes |-> 0,
  physicalIndexBytes |-> 0,
  physicalCatalogBytes |-> 672,
  physicalDeadBytes |-> 0,
  crashFrom |-> "none"
]

VARIABLE state
vars == <<state>>
Init ==
  \E initialAccounting \in AccountingSnapshots:
  \E selectedResource \in ResourceKinds:
  \E resourceMode \in ResourceModes:
    state = InitialState(
      EmptyDescriptor, initialAccounting, selectedResource, resourceMode)

RestartGrade(s) ==
  /\ s.prepareWritten
  /\ s.prepareDurable
  /\ s.indexWritten
  /\ s.indexDurable
  /\ s.commitWritten
  /\ s.commitDurable
  /\ s.bindingState \in BindingStates
  /\ ReceiptValid(s.bindingState)
  /\ ArtifactsExact(s.bindingState)
  /\ s.artifactDirectoryDurable
  /\ s.prepareMembers = ExpectedMembers
  /\ s.indexMembers = ExpectedMembers
  /\ s.descriptor = ExactDescriptor
  /\ s.prepareLen = ExpectedPrepareLen
  /\ s.indexTail = ExpectedIndexTail
  /\ s.commitAddress = ExpectedCommitAddress
  /\ s.packTail = ExpectedPackTail

AcceptResourcePreflight ==
  /\ state.phase = "OldAdmitted"
  /\ state.owners = <<"old-writer-0">>
  /\ ResourceWithinLimit(state.selectedResource, state.resourceLimit)
  /\ state' = [state EXCEPT
       !.phase = "Preflighted",
       !.owners = <<"publisher-0">>,
       !.descriptor = ExactDescriptor,
       !.preflighted = TRUE,
       !.resourceDecision = "accepted"]

RefuseResourcePreflight ==
  /\ state.phase = "OldAdmitted"
  /\ state.owners = <<"old-writer-0">>
  /\ state.resourceMinimum <= state.resourceLimit
  /\ state.resourceLimit < state.resourceRequired
  /\ state' = [state EXCEPT
       !.phase = "Failed",
       !.owners = <<>>,
       !.resourceDecision = "refused",
       !.refusedResource = state.selectedResource,
       !.outcome = "definitely-not-published",
       !.crashFrom = "ResourcePreflight"]

RejectResourceConfiguration ==
  /\ state.phase = "OldAdmitted"
  /\ state.owners = <<"old-writer-0">>
  /\ state.resourceLimit < state.resourceMinimum
  /\ state' = [state EXCEPT
       !.phase = "Failed",
       !.owners = <<>>,
       !.resourceDecision = "configuration-refused",
       !.refusedResource = state.selectedResource,
       !.outcome = "definitely-not-published",
       !.crashFrom = "ResourceConfiguration"]

CleanupExactSuccessor ==
  /\ state.phase = "Preflighted"
  /\ state.preflighted
  /\ state.resourceDecision = "accepted"
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
       !.physicalIndexArtifact = "valid",
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
       !.bindingState = ExactBindingState,
       !.authoritativeGenerationIntact = TRUE,
       !.commitAddress = ExpectedCommitAddress,
       !.packTail = ExpectedPackTail,
       !.physicalPackBytes = ExpectedPackTail]

SynchronizeCommit ==
  /\ state.phase = "CommitWritten"
  /\ state.commitWritten
  /\ state.bindingState \in BindingStates
  /\ ReceiptValid(state.bindingState)
  /\ ArtifactsExact(state.bindingState)
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
  /\ state.authoritativeGenerationIntact
  /\ state.physicalIndexArtifact \in {"valid", "repairable", "repaired"}
  /\ state' = [state EXCEPT
       !.phase = "PhysicalRecovered",
       !.physicalRecovery = "new",
       !.physicalIndexArtifact =
          IF state.physicalIndexArtifact = "repairable"
          THEN "repaired"
          ELSE @]

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

(** Authoritative damage is fatal.  A missing or structurally corrupt derived
    index is repairable from the authenticated WAL inventory; an unsafe index
    identity conflict is rejected and never causes fallback to old. *)
DamageSelectedAuthoritativeGeneration ==
  /\ state.phase = "CatalogDirectoryDurable"
  /\ state.authoritativeGenerationIntact
  /\ state' = [state EXCEPT !.authoritativeGenerationIntact = FALSE]

DamageSelectedIndexRepairably ==
  /\ state.phase = "CatalogDirectoryDurable"
  /\ state.physicalIndexArtifact = "valid"
  /\ state' = [state EXCEPT !.physicalIndexArtifact = "repairable"]

DamageSelectedIndexUnsafely ==
  /\ state.phase = "CatalogDirectoryDurable"
  /\ state.physicalIndexArtifact = "valid"
  /\ state' = [state EXCEPT !.physicalIndexArtifact = "unsafe"]

RepairSelectedAuthoritativeGeneration ==
  /\ state.phase = "Failed"
  /\ state.durableSlot = "valid-new"
  /\ ~state.authoritativeGenerationIntact
  /\ state' = [state EXCEPT !.authoritativeGenerationIntact = TRUE]

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
  /\ state.authoritativeGenerationIntact
  /\ state.physicalIndexArtifact \in {"valid", "repairable", "repaired"}
  /\ state' = [state EXCEPT
       !.phase = "RestartPhysicalRecovered",
       !.owners = <<"restart-publisher-0">>,
       !.selectedAuthority = "new",
       !.physicalRecovery = "new",
       !.physicalIndexArtifact =
          IF state.physicalIndexArtifact = "repairable"
          THEN "repaired"
          ELSE @]

RestartRejectDamagedNew ==
  /\ state.phase = "Failed"
  /\ state.durableSlot = "valid-new"
  /\ (~RestartGrade(state) \/
      ~state.authoritativeGenerationIntact \/
      state.physicalIndexArtifact = "unsafe")
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
  AcceptResourcePreflight \/ RefuseResourcePreflight \/
  RejectResourceConfiguration \/ CleanupExactSuccessor \/
  WritePrepare \/ SynchronizePrepare \/
  WriteIndex \/ SynchronizeIndex \/ WriteCommit \/ SynchronizeCommit \/
  SynchronizeArtifactDirectory \/ VerifyExactCatalogCas \/
  ExternalCatalogChange \/ RejectStaleCatalogCas \/ BeginCatalogSlotWrite \/
  FinishCatalogSlotWrite \/ SynchronizeCatalogFile \/
  SynchronizeCatalogDirectory \/ RecoverNewWithHeldLease \/
  ReadmitSemantics \/ Acknowledge \/ FailBeforeUncertainty \/
  FailAnyUncertainWrite \/ FailAfterCatalogFileSync \/
  DamageSelectedAuthoritativeGeneration \/ DamageSelectedIndexRepairably \/
  DamageSelectedIndexUnsafely \/ RepairSelectedAuthoritativeGeneration \/
  RestartSelectOld \/
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
  /\ state.physicalRecovery \in Authorities \union {"reject"}
  /\ state.outcome \in Outcomes
  /\ state.bindingState \in {EmptyBindingState} \union BindingStates
  /\ state.selectedResource \in ResourceKinds
  /\ state.selectedResourceFamily \in {"durable", "wal", "admission"}
  /\ state.selectedResourceFamily = ResourceFamily(state.selectedResource)
  /\ state.resourceMode \in ResourceModes
  /\ state.resourceRequired = RequiredResource(state.selectedResource)
  /\ state.resourceMinimum = MinimumResource(state.selectedResource)
  /\ state.resourceLimit = SelectedLimit(state.selectedResource, state.resourceMode)
  /\ state.resourceDecision \in {
       "unchecked", "accepted", "refused", "configuration-refused"
     }
  /\ state.refusedResource \in ResourceKinds \union {"none"}
  /\ state.authoritativeGenerationIntact \in BOOLEAN
  /\ state.physicalIndexArtifact \in {
       "absent", "valid", "repairable", "unsafe", "repaired"
     }
  /\ state.prepareLen \in Nat
  /\ state.indexTail \in Nat
  /\ state.commitAddress \in Nat
  /\ state.packTail \in Nat
  /\ state.physicalPackBytes \in Nat
  /\ state.physicalIndexBytes \in Nat
  /\ state.physicalCatalogBytes \in Nat
  /\ state.physicalDeadBytes \in 0..1
  /\ state.accountingBefore \in AccountingSnapshots
  /\ state.accountingAfter \in AccountingSnapshots

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

PreflightRequiresExactResourceAdmission ==
  state.preflighted =>
    /\ state.resourceDecision = "accepted"
    /\ ResourceWithinLimit(state.selectedResource, state.resourceLimit)

ResourceRefusalPrecedesAllMutation ==
  state.resourceDecision \in {"refused", "configuration-refused"} =>
    /\ state.refusedResource = state.selectedResource
    /\ ~state.preflighted
    /\ ~state.cleanupComplete
    /\ ~state.prepareWritten
    /\ ~state.indexWritten
    /\ ~state.commitWritten
    /\ ~state.slotWriteStarted
    /\ ~state.uncertaintyStarted
    /\ state.catalogImage = "old-672"
    /\ state.durableSlot = "old-image"
    /\ state.selectedAuthority = "old"
    /\ state.physicalRecovery \in {"none", "old"}
    /\ state.outcome = "definitely-not-published"

UnderprovisionedResourceCannotReachCleanup ==
  state.resourceLimit < state.resourceRequired => ~state.cleanupComplete

ExactResourceBoundaryIsAccepted ==
  state.resourceMode = "exact" =>
    state.resourceDecision \notin {"refused", "configuration-refused"}

ZeroDemandResourceBoundariesAreExact ==
  /\ \A resource \in ZeroDemandDurableKinds:
       /\ ZeroDemandRequired(resource) = 0
       /\ ZeroDemandMinimum(resource) = 1
       /\ ZeroDemandExactBoundary(resource) = 1
  /\ \A resource \in ZeroDemandWalKinds:
       /\ ZeroDemandRequired(resource) = 0
       /\ ZeroDemandMinimum(resource) = 0
       /\ ZeroDemandExactBoundary(resource) = 0

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
       /\ state.bindingState \in BindingStates
       /\ ReceiptValid(state.bindingState)
       /\ ArtifactsExact(state.bindingState)

ReceiptBindingsAreExact ==
  state.commitWritten =>
    /\ state.bindingState \in BindingStates
    /\ ReceiptValid(state.bindingState)
    /\ ArtifactsExact(state.bindingState)

EverySingleBindingMutationIsRejected ==
  \A fact \in BindingFacts:
    ~ReceiptValid(MutateBinding(ExactBindingState, fact)) \/
    ~ArtifactsExact(MutateBinding(ExactBindingState, fact))

CrashRecoverySelectsOnlyValidAuthority ==
  /\ state.physicalRecovery = "old" =>
       state.durableSlot \in {"old-image", "torn"}
  /\ state.physicalRecovery = "new" =>
       /\ state.durableSlot = "valid-new"
       /\ RestartGrade(state)
       /\ state.authoritativeGenerationIntact
       /\ state.physicalIndexArtifact \in {"valid", "repaired"}
  /\ state.physicalRecovery = "reject" =>
       /\ state.durableSlot = "valid-new"
       /\ (~RestartGrade(state) \/
           ~state.authoritativeGenerationIntact \/
           state.physicalIndexArtifact = "unsafe")
  /\ state.physicalRecovery # "third"

RepairableIndexRecoveryPreservesSuccessorAuthority ==
  state.physicalIndexArtifact = "repaired" =>
    /\ state.selectedAuthority = "new"
    /\ state.physicalRecovery = "new"
    /\ state.authoritativeGenerationIntact
    /\ RestartGrade(state)

CatalogSelectionRequiresDurableCompleteSlot ==
  state.selectedAuthority = "new" =>
    /\ state.durableSlot = "valid-new"
    /\ (state.catalogFileDurable \/ state.physicalRecovery = "new")

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

CheckpointPreservesSemanticAccounting ==
  state.accountingAfter = state.accountingBefore

PhysicalAccountingIsSeparated ==
  (state.physicalPackBytes + state.physicalIndexBytes +
   state.physicalCatalogBytes + state.physicalDeadBytes > 0) =>
    state.accountingAfter = state.accountingBefore

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
NeverDurableResourceRefusal ==
  ~(state.resourceDecision = "refused" /\
    state.selectedResourceFamily = "durable")
NeverInvalidResourceConfiguration ==
  state.resourceDecision # "configuration-refused"
NeverWalResourceRefusal ==
  ~(state.resourceDecision = "refused" /\ state.selectedResourceFamily = "wal")
NeverAdmissionResourceRefusal ==
  ~(state.resourceDecision = "refused" /\
    state.selectedResourceFamily = "admission")
NeverDurableExactCleanup ==
  ~(state.cleanupComplete /\ state.resourceMode = "exact" /\
    state.selectedResourceFamily = "durable")
NeverWalExactCleanup ==
  ~(state.cleanupComplete /\ state.resourceMode = "exact" /\
    state.selectedResourceFamily = "wal")
NeverAdmissionExactCleanup ==
  ~(state.cleanupComplete /\ state.resourceMode = "exact" /\
    state.selectedResourceFamily = "admission")
NeverRepairableIndexRecovery == state.physicalIndexArtifact # "repaired"
NeverRejectedDamagedSuccessor == state.phase # "RestartRejected"

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

BindingMutationMutant ==
  \E fact \in BindingFacts:
    /\ state.phase = "CommitWritten"
    /\ state.bindingState = ExactBindingState
    /\ state' = [state EXCEPT !.bindingState = MutateBinding(@, fact)]
BindingMutationMutantSpec == Init /\ [][Next \/ BindingMutationMutant]_vars

MinusOneAcceptedMutant ==
  /\ state.phase = "OldAdmitted"
  /\ state.resourceMode = "minus-one"
  /\ state.resourceMinimum <= state.resourceLimit
  /\ state.resourceLimit < state.resourceRequired
  /\ state' = [state EXCEPT
       !.phase = "Preflighted",
       !.owners = <<"publisher-0">>,
       !.descriptor = ExactDescriptor,
       !.preflighted = TRUE,
       !.resourceDecision = "accepted"]
MinusOneAcceptedMutantSpec == Init /\ [][Next \/ MinusOneAcceptedMutant]_vars

ExactBoundaryRejectedMutant ==
  /\ state.phase = "OldAdmitted"
  /\ state.resourceMode = "exact"
  /\ state' = [state EXCEPT
       !.phase = "Failed",
       !.owners = <<>>,
       !.resourceDecision = "refused",
       !.refusedResource = state.selectedResource,
       !.outcome = "definitely-not-published"]
ExactBoundaryRejectedMutantSpec ==
  Init /\ [][Next \/ ExactBoundaryRejectedMutant]_vars

LateAdmissionRefusalMutant ==
  /\ state.phase = "CleanupComplete"
  /\ state.selectedResourceFamily = "admission"
  /\ state' = [state EXCEPT
       !.phase = "Failed",
       !.owners = <<>>,
       !.resourceDecision = "refused",
       !.refusedResource = state.selectedResource,
       !.outcome = "definitely-not-published"]
LateAdmissionRefusalMutantSpec ==
  Init /\ [][Next \/ LateAdmissionRefusalMutant]_vars

RefusalMutatesAuthorityMutant ==
  /\ state.phase = "OldAdmitted"
  /\ state.resourceMode = "minus-one"
  /\ state' = [state EXCEPT
       !.phase = "Failed",
       !.owners = <<>>,
       !.resourceDecision = "refused",
       !.refusedResource = state.selectedResource,
       !.selectedAuthority = "new",
       !.catalogImage = "new-672",
       !.outcome = "definitely-not-published"]
RefusalMutatesAuthorityMutantSpec ==
  Init /\ [][Next \/ RefusalMutatesAuthorityMutant]_vars

OmittedResourceFamilyMutant(family) ==
  /\ state.phase = "OldAdmitted"
  /\ state.resourceMode = "minus-one"
  /\ state.selectedResourceFamily = family
  /\ state.resourceMinimum <= state.resourceLimit
  /\ state.resourceLimit < state.resourceRequired
  /\ state' = [state EXCEPT
       !.phase = "Preflighted",
       !.owners = <<"publisher-0">>,
       !.descriptor = ExactDescriptor,
       !.preflighted = TRUE,
       !.resourceDecision = "accepted"]
OmittedDurableResourceMutantSpec ==
  Init /\ [][Next \/ OmittedResourceFamilyMutant("durable")]_vars
OmittedWalResourceMutantSpec ==
  Init /\ [][Next \/ OmittedResourceFamilyMutant("wal")]_vars
OmittedAdmissionResourceMutantSpec ==
  Init /\ [][Next \/ OmittedResourceFamilyMutant("admission")]_vars

FallbackDamagedSuccessorToOldMutant ==
  /\ state.phase = "Failed"
  /\ state.durableSlot = "valid-new"
  /\ (~state.authoritativeGenerationIntact \/
      state.physicalIndexArtifact = "unsafe")
  /\ state' = [state EXCEPT
       !.phase = "RestartPhysicalRecovered",
       !.owners = <<"restart-publisher-0">>,
       !.selectedAuthority = "old",
       !.physicalRecovery = "old"]
FallbackDamagedSuccessorToOldMutantSpec ==
  Init /\ [][Next \/ FallbackDamagedSuccessorToOldMutant]_vars

ChargePollutionMutant ==
  /\ state.phase = "PrepareWritten"
  /\ state' = [state EXCEPT !.accountingAfter.totalCost = @ + 1]
ChargePollutionMutantSpec == Init /\ [][Next \/ ChargePollutionMutant]_vars

DiagnosticTracePollutionMutant ==
  /\ state.phase = "PrepareWritten"
  /\ state' = [state EXCEPT !.accountingAfter.traceEventCount = @ + 1]
DiagnosticTracePollutionMutantSpec ==
  Init /\ [][Next \/ DiagnosticTracePollutionMutant]_vars

PhysicalMeteringMutant ==
  /\ state.phase = "PrepareWritten"
  /\ state.physicalPackBytes > 0
  /\ state' = [state EXCEPT
       !.accountingAfter.totalCost = @ + state.physicalPackBytes]
PhysicalMeteringMutantSpec == Init /\ [][Next \/ PhysicalMeteringMutant]_vars

=============================================================================
