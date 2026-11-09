#!/usr/bin/env bash
# scripts/check-reified-rspaces-tla-invariants.sh
#
# TLA+ gate for the Reified RSpaces models (formal/tlaplus/reified_rspaces).
# P1: CheckpointReplay.tla (T1-T4, checkpoint and replay, and T6 of
# decision 36: an honest deploy replays to the failure and the spending of
# play) and TxnLockCoverage.tla (T5, lock coverage of space transactions,
# FR-4). CheckpointReplay.tla also models decision 45: peeks under Rule E
# (a peek fire removes the record, and a re-store puts it back).
# Changed by decision 90: the merge of feature/cost-accounted-rho removes
# the legacy storage charges of Option P, so the Legacy mode of the model,
# U13 (LegacyChargesFollowHost) and its controls are retired.
# P4: HeadOnlyDiscipline.tla (head-only matching of a queue: no pending
# match, a blocked head blocks the rest, FIFO, and the re-entry of a
# re-issued persistent operation at its first-round ord of decision 43:
# SHADOW in flight, INV at quiescent states, one introduction for each
# persistent element), CounterConflict.tla (a merge without a check #1
# race is a serial run: M-1 and A2; in P5 also for stacks, where every
# removal bumps the counter) and NativeHandles.tla (handle
# uniqueness, dangling handles, restarts, handle calls that merge like
# contract calls).
# P5: GenericRSpace.tla (the engine with the bag, set and cell: no pending
# match, pattern enforcement, conservation, no double spend, atomic
# reservation, class retirement, capacity, the peek restore),
# StackDiscipline.tla (LIFO order, no pending match, no lost record, the
# re-issue order, one fire count for each transaction),
# PriorityQueue.tla (the level order, the older head first, no pending
# match, no lost record, the re-issue order, the transaction bound) and the
# allocator of CounterConflict.tla. Wave W3: PathMapPrefix.tla (the PathMap
# delivery on prefixes: no pending match, one marker for each stored datum
# and prefix, and the merge contact on a shared index channel), and
# JoinCascade.tla (the joins of one space: no pending match at a quiescent
# end, the heads of a join, distinct records, the incoming datum bound once,
# the cascade measure, one fire count, no lost record, the re-issued orders
# and the touched counters, on the queue, the stack, the bag and the set).
# Changed by decisions 102 and 103 (decisions 118 and 119): play picks at
# random, and replay follows the record. A record that is not persistent
# fills at most one bind of a fire. A fire holds its incoming record at one
# bind, or at more binds when that record is persistent. The witness
# NoPersistentTwice shows a fire that binds one persistent record twice.
# Changed by decision 103 (decision 118, C6 arbitration), rule (a): on a
# set, a pick that is not persistent claims its record and its class, and a
# claimed class blocks only the members of a class with no persistent
# member. So the record rule covers the set too, and two binds of one set
# class need a persistent member of that class. The control
# FreeClassUnclaimedUnsafe restores the claim rule before rule (a). The
# witness FreeClassTwiceReachable shows a fire that binds a member that is
# not persistent and a persistent member of one set class.
# Decision 47: NativeReplay.tla (T8 NativeReplayOutcomeEqual: native replay
# accepts the journal of play, and only it, with the outcome of play; and
# the scheduling of space intents: NoBlockedReadiness with the key
# [2]||sid, and NoDeadlock with a failed space row that closes the epoch).
# Decision 90 disabled the scheduling of space intents (ScheduleSpec), and
# decision 92 made a missing later row a mismatch.
# P6 step 8c: VectorDb.tla (the VectorDB discipline of one collection: no
# pending match at a quiescent state under LM-b, atomic list fires, Rule E
# conservation of peeked records, and the measure of the tail, with the
# settle jobs and re-issues at the round cap).
# P6 step 8d: ChromaMirror.tla (the ChromaDB mirror of a VectorDB space:
# consensus does not depend on the mirror, replay uses the recorded answers
# and makes the checks of play, a deploy publishes its exact net writes only
# when its result has no errors, publication never waits, and a reconciler
# pass repairs to the finalized view with a watermark that never runs ahead).
# Changed by decisions 98, 104 and 114: the mirror is an oracle store. A play
# publishes the exact net writes of each commit once, whatever its result,
# and no replay publishes. Replay uses the recorded answers and makes no
# check. Publication never waits, and nothing repairs the mirror.
# P7 step 2: SpaceCoordination.tla (the coordination rules of the
# deploy-scoped qualifiers). Per channel, the admitted uses of a seq space
# form one causal chain, and the history of each participant is its causal
# past (decision 91). No delivered message holds a seq name, and gate 3
# creates a HashSet space only with seq and a seq space only in native
# execution. The use context of each participant is its lexical context.
# P9 step 3 (decision 83): the creation job records a seq space before the
# factory replies. Decision 90 retired its scheduling windows
# (ScanViewPerWindow).
# P8 step 6: AgentSpaces.tla (the spaces of user agents: under every
# context the names of a space route to that space and system channels to
# the default space, a join runs on one space or aborts with CrossSpaceJoin,
# a reference names a space under at most one writable bundle, the replay of
# the call log gives the state of the agent, the body of a linear receive
# runs at most once, and the reference agent keeps no pending match).
# Decision 94: JoinScopeLock.tla (the lock of a join scope). Phase A locks
# the own keys with C(sid), and the scope is read under phase A. Phase B
# locks the own keys and the scope keys in one sorted call, and the exact
# phase B keys give the coverage. The model checks that no scope is stale,
# no refusal is false and no deadlock occurs for every stripe map, with
# mutual exclusion.
# Decision 92: SpaceReplayOrder.tla (the replay order of one space). Every
# row of a space has S(sid) in its footprint. The first row of a
# transaction is its own row or an anchor on C(sid), and the readiness
# check reads that first row only. For every play order, the replay of an
# honest journal does not stall or fail closed. It gives the verdicts, the
# final state and the rows of play.
# Added by decisions 102, 117, 119 and 122 (plans v3 and v4): ten more
# scenarios of SpaceReplayOrder.tla (pick, deviate, capfail, k8, rafirst,
# tailret, tailquiet, reoffer0, seqpick and errstop) check the random picks,
# the deviations and their re-issues, the R-A restage, the anchors of the
# re-offers, the seq claims and the error stops. A child job is absent until
# the finish of its parent creates it, and play runs a parent before its
# children (V3-20). An absent job is not live, and the final state is
# compared only over the jobs that exist. SpaceOwnerRows.tla checks the
# owner checks in the rows of a Reified space transaction, in play and in
# replay: the stop rule, CanStop, CanContinue, the units, the mark-free
# finish, the re-issues, the anchors, the book, the retire group, the seq
# claims and the adversary. Each control refutes exactly one rule, and each
# witness shows that a path is reachable. Added by plan v4 errata (V4-18):
# ProduceWide and ConsumeWide check a waiter or a receive of two or more
# binds whose commit may veto (G2-a, G2-b and S1). Added by decision 102
# (plan v4, rule CR-30): the cascade and cascadeveto scenarios of
# SpaceReplayOrder.tla check the cascade lookahead of replay: the pin of the
# least journal index, the DR-75 windows, and the stage call when every
# probe misses.
# Changed by decision 124 (L3, X7): ProduceWide keeps the two-bind tuple of
# play (the guard verdict of the window picks), and the E3-a and S1 rules
# act on every candidate whose commit may veto, with no bind count.
# ConsumeWide is retired. At a consume, Wide acted only through WideK on the
# S1 stop path, and MayVetoK now covers every receive. So ConsumeWide
# checked the same model as Consume, which differs from it only in Wide.
# Changed by decision 124 (L2): play and replay take the DR-75 waiter order
# that Init chooses, so replay pins no source and makes no probe. The
# cascade and cascadeveto scenarios check the cascade of replay: the round
# cap, the DR-75 order of the waiters and of the windows, and the stage
# calls of the first qualifying waiter.
# Added by decision 124 (L3 and F2): the offerstore scenario checks the
# Stored check of a PathMap offer, whose fires and store have different
# sources. Its control OfferStoreSourceUnsafe reads the source of the
# element alone and stalls.
#
# The gate copies the TLA+ directory into a scratch directory and checks it
# there. Legs:
#   1. bounds    - every .cfg starts with a '\* BOUNDS:' header, and every
#                  .cfg belongs to exactly one leg below. Changed by
#                  decision 90: every line of a retired .cfg
#                  (RETIRED_CONFIGS) is a comment, and no leg checks it.
#   2. safe      - TLC explores the complete state space of each safe
#                  configuration (breadth-first; no -simulate, -dfid or
#                  -seed). T1-T5 and the auxiliary invariants must hold.
#   3. unsafe    - each Unsafe configuration refutes exactly its invariant:
#                  scripts/verify-reified/tlc-expect-violation.sh must pass,
#                  and the same checker with --complement must pass: TLC
#                  checks the configuration without that invariant, and the
#                  other invariants hold on the full state space
#   4. apalache  - BASE (Init => Inv, --length=0) and STEP (Inv /\ Next =>
#                  Inv', --length=1) for the inductive invariants IndInv
#                  (CheckpointReplay: T3 and T4, also with peeks, and
#                  decision 90 retires the legacy charges), LockIndInv
#                  (TxnLockCoverage: T5), HeadIndInv (HeadOnlyDiscipline:
#                  the P4 fragment, and decision 43 with re-entries) and
#                  HandleIndInv (NativeHandles). Each negative control
#                  must refute its inductive invariant in the expected
#                  conjunct.
#
# TLC runs through scripts/lib/tlc-run.sh (tlc_bounded, tlc_metadir and the
# source-identity check) with a heap of at most 4g and an on-disk metadir.
# A missing tool is a FAIL, never a SKIP.
#
# Environment:
#   RRS_SANDBOX=1      the job runs inside the verifier scope: no
#                      systemd-run, logs under $RRS_SCRATCH/logs
#   RRS_SCRATCH        scratch root (default <repo>/target/verification/
#                      reified-rspaces/scratch)
#   RRS_MEMORY_MAX     cgroup memory ceiling (default 6G)
#   RRS_CPU_QUOTA      cgroup CPU quota (default 400%)
#   TLC_JAR            default /usr/share/java/tla2tools.jar
#   TLC_HEAP           JVM heap, at most 4g (default 4g)
#   TLC_WORKERS        TLC workers (default 1, as design §5(b) RF4)
#   APALACHE_TIMEOUT   seconds for each Apalache run (default 1800)
#   RRS_TLA_SRC        the TLA+ directory to check (default formal/tlaplus/
#                      reified_rspaces); negative controls use a copy
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="${RRS_TLA_SRC:-$ROOT/formal/tlaplus/reified_rspaces}"
SCRATCH_ROOT="${RRS_SCRATCH:-$ROOT/target/verification/reified-rspaces/scratch}"
if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
  LOG_DIR="$SCRATCH_ROOT/logs/tla"
else
  LOG_DIR="$ROOT/target/verification/reified-rspaces/logs/tla"
fi
RRS_MEMORY_MAX="${RRS_MEMORY_MAX:-6G}"
RRS_CPU_QUOTA="${RRS_CPU_QUOTA:-400%}"
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
TLC_HEAP="${TLC_HEAP:-4g}"
TLC_WORKERS="${TLC_WORKERS:-1}"
APALACHE_TIMEOUT="${APALACHE_TIMEOUT:-1800}"

# <module> <config>
SAFE_CONFIGS=(
  "CheckpointReplay MC_CheckpointReplay.cfg"
  "CheckpointReplay MC_CheckpointReplay_TwoBlock.cfg"
  "CheckpointReplay MC_CheckpointReplay_Peek.cfg"
  # Retired by decision 90 (DR-134 of the base): CheckpointReplay.tla has no Legacy mode.
  # MC_CheckpointReplay_Peek_TwoBlock.cfg checks the two-block scenario in a token deploy.
  # "CheckpointReplay MC_CheckpointReplay_Legacy.cfg"
  # "CheckpointReplay MC_CheckpointReplay_Legacy_TwoBlock.cfg"
  "CheckpointReplay MC_CheckpointReplay_Peek_TwoBlock.cfg"
  "TxnLockCoverage MC_TxnLockCoverage.cfg"
  "HeadOnlyDiscipline MC_HeadOnlyDiscipline.cfg"
  "HeadOnlyDiscipline MC_HeadOnlyDiscipline_Reissue.cfg"
  "CounterConflict MC_CounterConflict.cfg"
  "CounterConflict MC_CounterConflict_Allocator.cfg"
  "CounterConflict MC_CounterConflict_Stacks.cfg"
  "PriorityQueue MC_PriorityQueue.cfg"
  "PriorityQueue MC_PriorityQueue_Deep.cfg"
  "NativeHandles MC_NativeHandles.cfg"
  "GenericRSpace MC_GenericRSpace_Bag.cfg"
  "GenericRSpace MC_GenericRSpace_Set.cfg"
  "GenericRSpace MC_GenericRSpace_Cell.cfg"
  "StackDiscipline MC_StackDiscipline.cfg"
  "PathMapPrefix MC_PathMapPrefix.cfg"
  "PathMapPrefix MC_PathMapPrefix_Peek.cfg"
  "PathMapPrefix MC_PathMapPrefix_Merge.cfg"
  "JoinCascade MC_JoinCascade_Queue.cfg"
  "JoinCascade MC_JoinCascade_Stack.cfg"
  "JoinCascade MC_JoinCascade_Bag.cfg"
  "JoinCascade MC_JoinCascade_Set.cfg"
  "NativeReplay MC_NativeReplay_Outcome.cfg"
  # Disabled by decision 90 (DR-132): ScheduleSpec models the removed scheduler. The
  # readiness wait of space transactions is checked by the "txn..." scenarios of
  # cost_accounted_rho/NativeReplayStall.tla (scripts/check-cost-accounted-rho-tla-invariants.sh).
  # "NativeReplay MC_NativeReplay_Schedule.cfg"
  # "NativeReplay MC_NativeReplay_Schedule_NativeKey.cfg"
  "VectorDb MC_VectorDb_List.cfg"
  "VectorDb MC_VectorDb_Persist.cfg"
  "VectorDb MC_VectorDb_Plain.cfg"
  "ChromaMirror MC_ChromaMirror.cfg"
  "SpaceCoordination MC_SpaceCoordination.cfg"
  "AgentSpaces MC_AgentSpaces.cfg"
  # Added by decision 94.
  "JoinScopeLock MC_JoinScopeLock.cfg"
  # Added by decision 92: every play order of the mixed scenario, and the re-offers.
  "SpaceReplayOrder MC_SpaceReplayOrder.cfg"
  "SpaceReplayOrder MC_SpaceReplayOrder_Reoffers.cfg"
  # Added by decisions 102, 117, 119 and 122 (plans v3 and v4).
  "SpaceReplayOrder MC_SpaceReplayOrder_Pick.cfg"
  "SpaceReplayOrder MC_SpaceReplayOrder_Deviate.cfg"
  "SpaceReplayOrder MC_SpaceReplayOrder_CapFail.cfg"
  "SpaceReplayOrder MC_SpaceReplayOrder_K8.cfg"
  "SpaceReplayOrder MC_SpaceReplayOrder_RaFirst.cfg"
  "SpaceReplayOrder MC_SpaceReplayOrder_TailRet.cfg"
  "SpaceReplayOrder MC_SpaceReplayOrder_TailQuiet.cfg"
  "SpaceReplayOrder MC_SpaceReplayOrder_Reoffer0.cfg"
  "SpaceReplayOrder MC_SpaceReplayOrder_SeqPick.cfg"
  "SpaceReplayOrder MC_SpaceReplayOrder_ErrStop.cfg"
  # Added by decision 102 (plan v4, rule CR-30): the cascade lookahead.
  "SpaceReplayOrder MC_SpaceReplayOrder_Cascade.cfg"
  "SpaceReplayOrder MC_SpaceReplayOrder_CascadeVeto.cfg"
  # Added by decision 124 (L3 and F2): the Stored check of an offer over all
  # its sources.
  "SpaceReplayOrder MC_SpaceReplayOrder_OfferStore.cfg"
  "SpaceOwnerRows MC_SpaceOwnerRows_ProduceAdversary.cfg"
  "SpaceOwnerRows MC_SpaceOwnerRows_ProduceSeq.cfg"
  "SpaceOwnerRows MC_SpaceOwnerRows_ProduceRacer.cfg"
  "SpaceOwnerRows MC_SpaceOwnerRows_Consume.cfg"
  "SpaceOwnerRows MC_SpaceOwnerRows_Ranked.cfg"
  "SpaceOwnerRows MC_SpaceOwnerRows_Head.cfg"
  "SpaceOwnerRows MC_SpaceOwnerRows_Class.cfg"
  # Added by plan v4 errata (V4-18): the wide waiter and the wide receive.
  # Changed by decision 124 (L3, X7): the wide waiter.
  "SpaceOwnerRows MC_SpaceOwnerRows_ProduceWide.cfg"
  # Retired by decision 124 (L3, X7): Wide has no effect at a consume, so the row checked the Consume model.
)
# <module> <config> <the one invariant that it must refute>
UNSAFE_CONFIGS=(
  "CheckpointReplay UuidUnsafe.cfg ReplayPrefixEqual"
  "CheckpointReplay RootIgnoresVolatileUnsafe.cfg SameRootIffSameState"
  "CheckpointReplay DoublePairUnsafe.cfg PairedOnce"
  "CheckpointReplay JoinResidueUnsafe.cfg NoVolatileResidue"
  "CheckpointReplay StagingDivergenceUnsafe.cfg ReplayOutcomeEqual"
  "CheckpointReplay AcceptUnpairedUnsafe.cfg ReplayPrefixEqual"
  # Retired by decision 90 (DR-134 of the base): the controls of the Legacy mode.
  # "CheckpointReplay LegacyPreAfterObserveUnsafe.cfg ReplayOutcomeEqual"
  # "CheckpointReplay LegacyTailDroppedUnsafe.cfg LegacyChargesFollowHost"
  # "CheckpointReplay RetainPeekUnsafe.cfg LegacyChargesFollowHost"
  "TxnLockCoverage UncoveredChannelUnsafe.cfg Serializable"
  "TxnLockCoverage UnjoinedChannelUnsafe.cfg Serializable"
  "HeadOnlyDiscipline HeadOnlyScanUnsafe.cfg HeadBlocks"
  "HeadOnlyDiscipline ReissueFreshOrdUnsafe.cfg IntroducedOnce"
  "HeadOnlyDiscipline ReentryAtHeadUnsafe.cfg FifoOrder"
  "HeadOnlyDiscipline NonPersistentFirstOrdUnsafe.cfg QuiescentNoPendingMatch"
  "CounterConflict CounterDroppedUnsafe.cfg NoMergedOrderFabrication"
  "CounterConflict PeekCounterFreeUnsafe.cfg NoMergedOrderFabrication"
  "CounterConflict LazyAllocatorUnsafe.cfg NoDuplicateIndex"
  "CounterConflict RemovalCounterFreeUnsafe.cfg NoMergedOrderFabrication"
  "PriorityQueue PqNoSettleUnsafe.cfg PqNoPendingMatch"
  "PriorityQueue PqAnyOrderUnsafe.cfg PqOldestFirst"
  "PriorityQueue PqReentryAtHeadUnsafe.cfg PqLevelOrder"
  "PriorityQueue PqPerPhaseCountUnsafe.cfg TxnFiresBounded"
  "PriorityQueue PqPlanOpensAtCapUnsafe.cfg TxnFiresBounded"
  "NativeHandles JoinDescriptorUnsafe.cfg HandleCallsMergeLikeContracts"
  "GenericRSpace WildcardUnsafe.cfg PatternEnforcement"
  "GenericRSpace SetRetireOneUnsafe.cfg SetClassRetired"
  "GenericRSpace SetNoProduceRetireUnsafe.cfg SetClassRetired"
  "GenericRSpace CellNoCapacityUnsafe.cfg CellCapacity"
  "GenericRSpace PeekNoRestoreUnsafe.cfg Conservation"
  "GenericRSpace RestoreOpAuthorityUnsafe.cfg PeekRestoresRecord"
  "GenericRSpace RestoreNoReofferUnsafe.cfg PeekRestoresRecord"
  "StackDiscipline StackNoSettleUnsafe.cfg StackNoPendingMatch"
  "StackDiscipline StackReentryAtTopUnsafe.cfg LifoOrder"
  "StackDiscipline StackPerPhaseCountUnsafe.cfg TxnFiresBounded"
  "StackDiscipline StackPlanOpensAtCapUnsafe.cfg TxnFiresBounded"
  "PathMapPrefix SpaceWideIndexUnsafe.cfg MergeDetectsPrefixContact"
  "PathMapPrefix PathMapExactPathOnlyUnsafe.cfg NoMatchRemains"
  "PathMapPrefix PathMapNoRetireUnsafe.cfg MarkerCount"
  "PathMapPrefix PathMapMarkerNotRestoredUnsafe.cfg MarkerCount"
  "JoinCascade NoLiftUnsafe.cfg QuiescentNoPendingMatch"
  "JoinCascade NoSetCascadeUnsafe.cfg QuiescentNoPendingMatch"
  "JoinCascade PlainRestoreUnsafe.cfg QuiescentNoPendingMatch"
  # Changed by decision 103 (decision 118): the two invariants are restated.
  # "JoinCascade StoredRecordTwiceUnsafe.cfg DistinctRecordsPerBind"
  # "JoinCascade IncomingTwiceUnsafe.cfg IncomingBoundOnce"
  "JoinCascade StoredRecordTwiceUnsafe.cfg NonPersistentBoundOnce"
  "JoinCascade IncomingTwiceUnsafe.cfg IncomingHeld"
  "JoinCascade HeadOnIncomingOnlyUnsafe.cfg JoinFiresHeadsOnly"
  "JoinCascade JoinPerPhaseCountUnsafe.cfg TxnFiresBounded"
  "JoinCascade RestoreDroppedUnsafe.cfg NoRecordLost"
  "JoinCascade JoinReissueFreshOrdsUnsafe.cfg ReissueKeepsBindOrds"
  "JoinCascade LazyTouchUnsafe.cfg TouchedCountersBumped"
  "JoinCascade PeekPersistUnsafe.cfg CascadeMeasureDecreases"
  # Added by decision 103 (decision 118): the control of a claimed persistent
  # record, and the witness of a fire that binds a persistent record twice.
  "JoinCascade PersistentClaimedUnsafe.cfg QuiescentNoPendingMatch"
  "JoinCascade JoinPersistentTwiceReachable.cfg NoPersistentTwice"
  # Added by decision 103 (decision 118, C6 arbitration): the control of the
  # claim rule before rule (a), and the witness of a fire that binds a member
  # that is not persistent and a persistent member of one set class.
  "JoinCascade FreeClassUnclaimedUnsafe.cfg NonPersistentBoundOnce"
  "JoinCascade FreeClassTwiceReachable.cfg NoFreeClassTwice"
  "NativeReplay TamperedOutcomeUnsafe.cfg NativeReplayOutcomeEqual"
  # Disabled by decision 90 (DR-132): the controls of ScheduleSpec (see above).
  # "NativeReplay NoSpaceKeyUnsafe.cfg NoBlockedReadiness"
  # "NativeReplay WaitUnderLocksUnsafe.cfg NoDeadlock"
  # "NativeReplay NoInvalidateUnsafe.cfg NoDeadlock"
  "VectorDb VdbNoCascadeUnsafe.cfg QuiescentNoPendingMatch"
  "VectorDb VdbParkOnlyWhileEmptyUnsafe.cfg QuiescentNoPendingMatch"
  "VectorDb VdbNoRetireUnsafe.cfg ListAtomic"
  "VectorDb VdbDropOwedUnsafe.cfg PeekConservation"
  "VectorDb VdbUncountedCascadeUnsafe.cfg CascadeTerminates"
  "ChromaMirror MirrorWriteInTxnUnsafe.cfg ConsensusIndependentOfMirror"
  "ChromaMirror UnrecordedAdvisoryUnsafe.cfg AdvisoryRecorded"
  # Retired by decision 104: replay makes no check, as the base driver (DR-114).
  # "ChromaMirror AnnChecksOnlyInPlayUnsafe.cfg EqualHostWork"
  # Retired by decision 98: a play publishes at each commit, whatever its result (DR-114).
  # "ChromaMirror PublishFailedDeployUnsafe.cfg PublishOnlyOnSuccess"
  # "ChromaMirror BeginScopeKeepsJournalUnsafe.cfg PublishOnlyOnSuccess"
  "ChromaMirror RuleEUpsertUnsafe.cfg NetWritesExact"
  "ChromaMirror BlockingPublishUnsafe.cfg PublishNeverWaits"
  # Retired by decision 114: nothing repairs the mirror (File I/O oracle mode).
  # "ChromaMirror OrphansKeptUnsafe.cfg ReconciledEqualsFinalizedView"
  # "ChromaMirror WatermarkAfterFailureUnsafe.cfg WatermarkHonest"
  # Added by decision 98: no replay publishes, and a commit publishes once.
  "ChromaMirror ReplayPublishesUnsafe.cfg ReplayNeverPublishes"
  "ChromaMirror SettleAlsoPublishesUnsafe.cfg OnePublicationPerCommit"
  # Changed by decision 91: the prefix chain of participant paths is a causal chain.
  # "SpaceCoordination SeqGuardToctouUnsafe.cfg SeqExclusive"
  "SpaceCoordination SeqGuardToctouUnsafe.cfg SeqCausalChain"
  "SpaceCoordination SeqScanSkippedUnsafe.cfg SeqNotSent"
  "SpaceCoordination QualifierSupportUnsafe.cfg QualifierSupport"
  "SpaceCoordination UseBalanceUnsafe.cfg UseBalance"
  # Retired by decision 90 (DR-132): racing evaluation has no scheduling windows.
  # "SpaceCoordination RecordInWindowUnsafe.cfg ScanViewPerWindow"
  # Added by decision 91: the causal histories of the seq claims.
  "SpaceCoordination ClaimsNeedContextUnsafe.cfg NoFalseRefusal"
  "SpaceCoordination NoMergeUnsafe.cfg HistIsCausalPast"
  "SpaceCoordination NoInheritUnsafe.cfg HistIsCausalPast"
  "SpaceCoordination TwoUseChainReachable.cfg NoTwoUseChain"
  "AgentSpaces SystemRouteUnsafe.cfg SystemChannelsDefault"
  "AgentSpaces FaultyAgentUnsafe.cfg NoMatchRemains"
  # Added by decision 94: the escalation before it, and one control for each
  # rule of the two-phase lock.
  "JoinScopeLock EscalationRebeginUnsafe.cfg NoStaleScope"
  "JoinScopeLock PhaseAWithoutConfigUnsafe.cfg NoStaleScope"
  "JoinScopeLock PhaseBSplitUnsafe.cfg NoDeadlock"
  "JoinScopeLock PhaseAAfterPhaseBUnsafe.cfg NoDeadlock"
  "JoinScopeLock CoverageByHostUseUnsafe.cfg NoFalseRefusal"
  # Added by decision 92: one control for each rule of the first-row check,
  # and the witnesses of a completed replay and of a repeated failure.
  "SpaceReplayOrder NoFootprintKeyUnsafe.cfg NoStall"
  "SpaceReplayOrder NoAnchorRowlessUnsafe.cfg NoStall"
  "SpaceReplayOrder AnchorAppendedUnsafe.cfg NoFailClosed"
  "SpaceReplayOrder UnknownSourceFailsUnsafe.cfg NoFailClosed"
  "SpaceReplayOrder SpaceReplayCompletionReachable.cfg NoHonestCompletion"
  "SpaceReplayOrder SpaceReplayFailureVerdictReachable.cfg NoFailureVerdict"
  # Added by decisions 102, 117, 119 and 122 (plans v3 and v4): one control
  # for each rule, and the witnesses.
  "SpaceReplayOrder CanonicalStagingUnsafe.cfg NoFailClosed"
  "SpaceReplayOrder EmptyFilterFailsUnsafe.cfg NoFailClosed"
  "SpaceReplayOrder DeviationRowsUnsafe.cfg NoFailClosed"
  "SpaceReplayOrder ReissueOwnFirstUnsafe.cfg NoFailClosed"
  "SpaceReplayOrder NoCanonicalRestageUnsafe.cfg NoStall"
  "SpaceReplayOrder SpaceReplayRestageReachable.cfg NoRestagedCommit"
  "SpaceReplayOrder RestageDropsAnchorUnsafe.cfg NoFailClosed"
  "SpaceReplayOrder MarkedFinishUnsafe.cfg NoStall"
  "SpaceReplayOrder MarkedSettleUnsafe.cfg ExactRows"
  "SpaceReplayOrder ReofferOwnFirstUnsafe.cfg NoFailClosed"
  "SpaceReplayOrder SeqPickAbortUnsafe.cfg NoStall"
  "SpaceReplayOrder FollowErrorFailsUnsafe.cfg NoStall"
  "SpaceReplayOrder SpaceReplayMarkFreeReturnReachable.cfg NoReturnAfterDeviation"
  "SpaceReplayOrder AnchorOnlyErrorCutUnsafe.cfg NoFailClosed"
  "SpaceReplayOrder AnchorOnlyRecordEndUnsafe.cfg NoFailClosed"
  # Changed by decision 124 (L2): play and replay take the DR-75 waiter order,
  # so the controls of the pin and of the stage call at a miss are retired.
  # The control of the DR-75 window of play (F3) and the witness of a
  # completed cascade replay stay.
  # Retired by decision 124 (L2): replay pins no source (CascadeLeastIndex is retired).
  "SpaceReplayOrder CascadeRandomWindowUnsafe.cfg NoStall"
  # Retired by decision 124 (L2): replay makes no probe (CascadeMissStages is retired).
  "SpaceReplayOrder SpaceReplayCascadeCompletionReachable.cfg NoHonestCompletion"
  # Added by decision 124 (L3 and F2): the Stored check that reads the
  # element source alone.
  "SpaceReplayOrder OfferStoreSourceUnsafe.cfg NoStall"
  "SpaceOwnerRows SpaceOwnerMarkedFinishUnsafe.cfg TxnPlayReplayAgree"
  "SpaceOwnerRows SpaceOwnerGuardedSearchUnsafe.cfg TxnPlayReplayAgree"
  "SpaceOwnerRows SpaceOwnerSearchBookUnsafe.cfg TxnPlayReplayAgree"
  "SpaceOwnerRows SpaceOwnerRowStopUnsafe.cfg TxnPlayReplayAgree"
  "SpaceOwnerRows SpaceOwnerUncheckedStoredUnsafe.cfg TxnStoredClaimIsHonest"
  "SpaceOwnerRows SpaceOwnerUncheckedRefusalUnsafe.cfg TxnRefusalNeedsShortfall"
  "SpaceOwnerRows SpaceOwnerUnfilteredPickUnsafe.cfg AllowedChoice"
  "SpaceOwnerRows SpaceOwnerVetoExcuseUnsafe.cfg AllowedChoice"
  "SpaceOwnerRows SpaceOwnerUncheckedGuardUnsafe.cfg AllowedChoice"
  "SpaceOwnerRows SpaceOwnerUncheckedStopUnsafe.cfg StopIsHonest"
  "SpaceOwnerRows SpaceOwnerUncheckedContinueUnsafe.cfg ContinueIsHonest"
  "SpaceOwnerRows SpaceOwnerContinueAfterDeviationUnsafe.cfg HostParity"
  "SpaceOwnerRows SpaceOwnerNoBookUnsafe.cfg RefusedPairNotRefired"
  "SpaceOwnerRows SpaceOwnerCascadeIgnoresBookUnsafe.cfg RefusedPairNotRefired"
  "SpaceOwnerRows SpaceOwnerNoItemBookUnsafe.cfg ItemFailedPairNotRefired"
  "SpaceOwnerRows SpaceOwnerOwnerPaidPutUnsafe.cfg OneCounter"
  "SpaceOwnerRows SpaceOwnerPlainLiftUnsafe.cfg NoLostRecord"
  "SpaceOwnerRows SpaceOwnerPartialClassFireUnsafe.cfg RefusedTupleKeepsMembers"
  "SpaceOwnerRows SpaceOwnerStagedRetiresUnsafe.cfg SpentClassRetired"
  "SpaceOwnerRows SpaceOwnerStagedClaimsUnsafe.cfg SeqClaimsAgree"
  "SpaceOwnerRows SpaceOwnerAlternativeReachable.cfg NoAlternativeFire"
  "SpaceOwnerRows SpaceOwnerRandomPickReachable.cfg NoRandomPickCompletion"
  "SpaceOwnerRows SpaceOwnerStopReissueReachable.cfg NoStopReissue"
  "SpaceOwnerRows SpaceOwnerRankedSkipReachable.cfg NoRankedAlternative"
  # Added by plan v4 errata (V4-17): replay books the refused pairs of every row.
  "SpaceOwnerRows SpaceOwnerReplayBookOnMarkUnsafe.cfg TxnPlayReplayAgree"
  # Added by plan v4 errata (V4-17): the refused park on the Consume base, and the row search of
  # an E3-a park.
  "SpaceOwnerRows SpaceOwnerReplayBookOnMarkConsumeUnsafe.cfg TxnPlayReplayAgree"
  "SpaceOwnerRows SpaceOwnerParkWithoutSearchUnsafe.cfg TxnPlayReplayAgree"
  # Added by plan v4 errata (V4-18): G2-a, G2-b and S1, and the witnesses of H1 and H2.
  "SpaceOwnerRows SpaceOwnerWideSendSearchUnsafe.cfg TxnPlayReplayAgree"
  "SpaceOwnerRows SpaceOwnerWideItemBookOnReturnUnsafe.cfg TxnPlayReplayAgree"
  "SpaceOwnerRows SpaceOwnerWideStopByGuardUnsafe.cfg TxnPlayReplayAgree"
  "SpaceOwnerRows SpaceOwnerWideStoredDeviationReachable.cfg NoWideStoredDeviation"
  "SpaceOwnerRows SpaceOwnerWideOrderStoreReachable.cfg NoWideOrderStore"
)
# <name>|<module>|<config>|<length>|<expected outcome>|<marker in the violated formula>
APALACHE_CHECKS=(
  "base|CheckpointReplay|MC_CheckpointReplay_IndBase_Apalache.cfg|0|NoError|"
  "step|CheckpointReplay|MC_CheckpointReplay_IndStep_Apalache.cfg|1|NoError|"
  "step-doublepair-unsafe|CheckpointReplay|DoublePairUnsafe_IndStep_Apalache.cfg|1|Error|vStaged["
  "step-joinresidue-unsafe|CheckpointReplay|JoinResidueUnsafe_IndStep_Apalache.cfg|1|Error|Swept\""
  # Changed by decision 90 (DR-134 of the base): the checks with peeks run in a token deploy.
  # "legacy-base|CheckpointReplay|MC_CheckpointReplay_Legacy_IndBase_Apalache.cfg|0|NoError|"
  # "legacy-step|CheckpointReplay|MC_CheckpointReplay_Legacy_IndStep_Apalache.cfg|1|NoError|"
  "peek-base|CheckpointReplay|MC_CheckpointReplay_Peek_IndBase_Apalache.cfg|0|NoError|"
  "peek-step|CheckpointReplay|MC_CheckpointReplay_Peek_IndStep_Apalache.cfg|1|NoError|"
  "lock-base|TxnLockCoverage|MC_TxnLockCoverage_IndBase_Apalache.cfg|0|NoError|"
  "lock-step|TxnLockCoverage|MC_TxnLockCoverage_IndStep_Apalache.cfg|1|NoError|"
  "lock-step-uncovered-unsafe|TxnLockCoverage|UncoveredChannelUnsafe_IndStep_Apalache.cfg|1|Error|stagedOne"
  "lock-step-unjoined-unsafe|TxnLockCoverage|UnjoinedChannelUnsafe_IndStep_Apalache.cfg|1|Error|stagedOne"
  "head-base|HeadOnlyDiscipline|MC_HeadOnlyDiscipline_IndBase_Apalache.cfg|0|NoError|"
  "head-step|HeadOnlyDiscipline|MC_HeadOnlyDiscipline_IndStep_Apalache.cfg|1|NoError|"
  "head-step-scan-unsafe|HeadOnlyDiscipline|HeadOnlyScanUnsafe_IndStep_Apalache.cfg|1|Error|~partnersAtHead"
  "head-reissue-base|HeadOnlyDiscipline|MC_HeadOnlyDiscipline_Reissue_IndBase_Apalache.cfg|0|NoError|"
  "head-reissue-step|HeadOnlyDiscipline|MC_HeadOnlyDiscipline_Reissue_IndStep_Apalache.cfg|1|NoError|"
  "handle-base|NativeHandles|MC_NativeHandles_IndBase_Apalache.cfg|0|NoError|"
  "handle-step|NativeHandles|MC_NativeHandles_IndStep_Apalache.cfg|1|NoError|"
  "handle-step-joindescriptor-unsafe|NativeHandles|JoinDescriptorUnsafe_IndStep_Apalache.cfg|1|Error|\"user\""
)
# Added by decision 90 (DR-132): <config>. A retired configuration stays in
# the directory as a comment with its restore command. No leg checks it.
RETIRED_CONFIGS=(
  # ScheduleSpec of NativeReplay.tla and its controls.
  "MC_NativeReplay_Schedule.cfg"
  "MC_NativeReplay_Schedule_NativeKey.cfg"
  "NoSpaceKeyUnsafe.cfg"
  "WaitUnderLocksUnsafe.cfg"
  "NoInvalidateUnsafe.cfg"
  # The scheduling windows of SpaceCoordination.tla.
  "RecordInWindowUnsafe.cfg"
  # The Legacy mode of CheckpointReplay.tla (Option P of decision 45) and its controls.
  "MC_CheckpointReplay_Legacy.cfg"
  "MC_CheckpointReplay_Legacy_TwoBlock.cfg"
  "MC_CheckpointReplay_Legacy_IndBase_Apalache.cfg"
  "MC_CheckpointReplay_Legacy_IndStep_Apalache.cfg"
  "LegacyPreAfterObserveUnsafe.cfg"
  "LegacyTailDroppedUnsafe.cfg"
  "RetainPeekUnsafe.cfg"
  # Added by decisions 98, 104 and 114: the retired controls of ChromaMirror.tla.
  "PublishFailedDeployUnsafe.cfg"
  "BeginScopeKeepsJournalUnsafe.cfg"
  "AnnChecksOnlyInPlayUnsafe.cfg"
  "OrphansKeptUnsafe.cfg"
  "WatermarkAfterFailureUnsafe.cfg"
  # Added by decision 124 (L2): the retired controls of the cascade pin and of
  # the stage call at a miss (SpaceReplayOrder.tla).
  "CascadeKeyOrderUnsafe.cfg"
  "CascadeMissContinueUnsafe.cfg"
  # Added by decision 124 (L3, X7): the wide receive of SpaceOwnerRows.tla. It
  # checked the same model as MC_SpaceOwnerRows_Consume.cfg.
  "MC_SpaceOwnerRows_ConsumeWide.cfg"
)

PASS=0
FAIL=0
SKIP=0
pass() { PASS=$((PASS + 1)); echo "  PASS $*"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $*"; }

finish() {
  echo "reified-rspaces tla: PASS=$PASS FAIL=$FAIL SKIP=$SKIP"
  if [[ $FAIL -gt 0 ]]; then
    exit 1
  fi
  exit 0
}

if [[ ! "$TLC_HEAP" =~ ^([1-4])g$|^([0-9]{1,4})m$ ]]; then
  echo "error: TLC_HEAP must be at most 4g (got $TLC_HEAP)" >&2
  exit 2
fi
if [[ ! "$TLC_WORKERS" =~ ^[1-9][0-9]*$ ]]; then
  echo "error: TLC_WORKERS must be a positive number (got $TLC_WORKERS)" >&2
  exit 2
fi
if [[ ! "$APALACHE_TIMEOUT" =~ ^[1-9][0-9]*$ ]]; then
  echo "error: APALACHE_TIMEOUT must be a positive number of seconds" >&2
  exit 2
fi

# Changed by decisions 94 and 92: the banner names every module of the gate.
# echo "== Reified RSpaces TLA+ gate (CheckpointReplay, TxnLockCoverage, HeadOnlyDiscipline, CounterConflict, NativeHandles, GenericRSpace, StackDiscipline, PriorityQueue, PathMapPrefix, JoinCascade, NativeReplay, VectorDb, ChromaMirror, SpaceCoordination)"
# Changed by plans v3 and v4 (decisions 102 and 122): the banner names SpaceOwnerRows.
echo "== Reified RSpaces TLA+ gate (CheckpointReplay, TxnLockCoverage, HeadOnlyDiscipline, CounterConflict, NativeHandles, GenericRSpace, StackDiscipline, PriorityQueue, PathMapPrefix, JoinCascade, NativeReplay, VectorDb, ChromaMirror, SpaceCoordination, AgentSpaces, JoinScopeLock, SpaceReplayOrder, SpaceOwnerRows)"

# ── Tools ───────────────────────────────────────────────────────────────────
tools_ok=true
for tool in java apalache-mc timeout; do
  if command -v "$tool" >/dev/null 2>&1; then
    pass "tool $tool: $(command -v "$tool")"
  else
    fail "tool $tool is missing"
    tools_ok=false
  fi
done
if [[ -f "$TLC_JAR" ]]; then
  pass "tool TLC: $TLC_JAR"
else
  fail "tool TLC: no jar at $TLC_JAR"
  tools_ok=false
fi
if [[ -x "$ROOT/scripts/verify-reified/tlc-expect-violation.sh" ]]; then
  pass "tool tlc-expect-violation.sh"
else
  fail "tool scripts/verify-reified/tlc-expect-violation.sh is missing or not executable"
  tools_ok=false
fi
if [[ "${RRS_SANDBOX:-}" != "1" ]]; then
  if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --scope --quiet true >/dev/null 2>&1; then
    pass "tool systemd-run: user scopes are available"
  else
    fail "tool systemd-run: no user scope (set RRS_SANDBOX=1 inside a verifier scope)"
    tools_ok=false
  fi
fi
if [[ "$tools_ok" != true ]]; then
  finish
fi

bounded() {
  if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
    "$@"
  else
    systemd-run --user --scope --quiet -p "MemoryMax=$RRS_MEMORY_MAX" -p MemorySwapMax=0 \
      -p "CPUQuota=$RRS_CPU_QUOTA" -- "$@"
  fi
}

# ── Scratch copy ────────────────────────────────────────────────────────────
mkdir -p "$SCRATCH_ROOT" "$LOG_DIR" || { fail "cannot create $SCRATCH_ROOT or $LOG_DIR"; finish; }
WORK="$(mktemp -d "$SCRATCH_ROOT/tla.XXXXXX")" || { fail "cannot create a scratch directory"; finish; }
trap 'rm -rf -- "$WORK"' EXIT
export TMPDIR="$WORK/tmp"
mkdir -p "$TMPDIR" "$WORK/tla" "$WORK/jtmp"
cp -- "$SRC"/*.tla "$SRC"/*.cfg "$WORK/tla/" || { fail "cannot copy $SRC"; finish; }
echo "   scratch: $WORK"

export TLC_REPO_ROOT="$ROOT"
export TLC_METADIR_ROOT="$WORK/metadir"
export TLC_RSS="$RRS_MEMORY_MAX"
export TLC_HEAP TLC_WORKERS TLC_JAR
if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
  export ALLOW_UNBOUNDED_TLC=1
fi
# shellcheck source=/dev/null
source "$ROOT/scripts/lib/tlc-run.sh"

# run_tlc <label> <cfg> <module> <log>: exhaustive breadth-first TLC.
run_tlc() {
  local label="$1" cfg="$2" module="$3" log="$4" before after metadir
  before="$(tlc_recovery_identity "$(tlc_source_hash "$cfg" "$module")")" || return 1
  metadir="$(tlc_metadir "$label")"
  tlc_bounded java "-Xmx$TLC_HEAP" -XX:+UseParallelGC "-Djava.io.tmpdir=$WORK/jtmp" \
    -cp "$TLC_JAR" tlc2.TLC -workers "$TLC_WORKERS" -fp 0 -metadir "$metadir" \
    -config "$cfg" "$module" > "$log" 2>&1
  local rc=$?
  after="$(tlc_recovery_identity "$(tlc_source_hash "$cfg" "$module")")" || return 1
  tlc_require_unchanged_identity "$before" "$after" || return $?
  return "$rc"
}

tlc_summary() {
  local log="$1" states depth
  states="$(grep -oE '[0-9,]+ distinct states found' "$log" | tail -1 | grep -oE '^[0-9,]+')"
  depth="$(grep -oE 'depth of the complete state graph search is [0-9]+' "$log" | grep -oE '[0-9]+$')"
  echo "${states:-?} distinct states, depth ${depth:-?}"
}

# ── 1. Bounds headers and leg coverage ─────────────────────────────────────
# Added by decision 90 (DR-132): the retired configurations.
declare -A RETIRED=()
for name in "${RETIRED_CONFIGS[@]}"; do
  RETIRED[$name]=1
done
for cfg in "$WORK"/tla/*.cfg; do
  # Added by decision 90 (DR-132): every line of a retired configuration is a comment.
  if [[ -n "${RETIRED[$(basename "$cfg")]:-}" ]]; then
    if grep -qv -e '^\\\*' -e '^[[:space:]]*$' "$cfg"; then
      fail "bounds $(basename "$cfg"): retired, but a line is not a comment"
    else
      pass "bounds $(basename "$cfg"): retired, every line is a comment"
    fi
    continue
  fi
  if head -n 1 "$cfg" | grep -q '^\\\* BOUNDS:'; then
    pass "bounds $(basename "$cfg"): $(head -n 1 "$cfg" | sed 's/^\\\* BOUNDS: //')"
  else
    fail "bounds $(basename "$cfg"): the first line is not a '\\* BOUNDS:' header"
  fi
done
declare -A IN_LEG=()
for entry in "${SAFE_CONFIGS[@]}" "${UNSAFE_CONFIGS[@]}"; do
  read -r _ cfg _ <<<"$entry"
  IN_LEG[$cfg]=$(( ${IN_LEG[$cfg]:-0} + 1 ))
done
for entry in "${APALACHE_CHECKS[@]}"; do
  IFS='|' read -r _ _ cfg _ _ _ <<<"$entry"
  IN_LEG[$cfg]=$(( ${IN_LEG[$cfg]:-0} + 1 ))
done
for cfg in "$WORK"/tla/*.cfg; do
  name="$(basename "$cfg")"
  # Added by decision 90 (DR-132): no leg checks a retired configuration.
  if [[ -n "${RETIRED[$name]:-}" ]]; then
    case "${IN_LEG[$name]:-0}" in
      0) pass "leg $name: retired, no leg checks it" ;;
      *) fail "leg $name: retired, but ${IN_LEG[$name]} legs check it" ;;
    esac
    continue
  fi
  case "${IN_LEG[$name]:-0}" in
    1) pass "leg $name: checked by exactly one leg" ;;
    0) fail "leg $name: no leg of the gate checks this configuration" ;;
    *) fail "leg $name: ${IN_LEG[$name]} legs check this configuration" ;;
  esac
done

# ── 2. Safe configurations ──────────────────────────────────────────────────
for entry in "${SAFE_CONFIGS[@]}"; do
  read -r module_name cfg <<<"$entry"
  module="$WORK/tla/$module_name.tla"
  log="$LOG_DIR/${cfg%.cfg}.log"
  if [[ ! -f "$WORK/tla/$cfg" || ! -f "$module" ]]; then
    fail "safe $cfg: the configuration or $module_name.tla is missing"
    continue
  fi
  run_tlc "${cfg%.cfg}" "$WORK/tla/$cfg" "$module" "$log"
  rc=$?
  if [[ $rc -eq 0 ]] && grep -q 'Running breadth-first search Model-Checking' "$log" \
     && grep -q 'Model checking completed. No error has been found.' "$log"; then
    pass "safe $cfg ($module_name): $(tlc_summary "$log"); no invariant violated"
  else
    fail "safe $cfg ($module_name): TLC exit code $rc; see $log"
    grep -E 'Error|violated|Exception' "$log" | head -n 10 | sed 's/^/        /'
  fi
done

# ── 3. Unsafe controls ──────────────────────────────────────────────────────
for entry in "${UNSAFE_CONFIGS[@]}"; do
  read -r module_name cfg invariant <<<"$entry"
  module="$WORK/tla/$module_name.tla"
  if [[ ! -f "$WORK/tla/$cfg" || ! -f "$module" ]]; then
    fail "unsafe $cfg: the configuration or $module_name.tla is missing"
    continue
  fi
  out="$(RRS_SCRATCH="$WORK/expect" RRS_SANDBOX="${RRS_SANDBOX:-}" TLC_JAR="$TLC_JAR" TLC_HEAP="$TLC_HEAP" \
         TLC_WORKERS="$TLC_WORKERS" TLC_RSS="$RRS_MEMORY_MAX" \
         "$ROOT/scripts/verify-reified/tlc-expect-violation.sh" "$WORK/tla/$cfg" "$invariant" "$module" 2>&1)"
  rc=$?
  printf '%s\n' "$out" > "$LOG_DIR/${cfg%.cfg}.expect.log"
  if [[ $rc -eq 0 ]]; then
    pass "unsafe $cfg ($module_name): $(printf '%s\n' "$out" | grep '^PASS:' | sed 's/^PASS: //')"
  else
    fail "unsafe $cfg ($module_name): expected exactly a violation of $invariant; see $LOG_DIR/${cfg%.cfg}.expect.log"
    printf '%s\n' "$out" | tail -n 5 | sed 's/^/        /'
  fi

  # The complement: without the refuted invariant, every other invariant
  # holds on the full state space. The same checker runs in the pgmcp
  # verification recipe.
  out="$(RRS_SCRATCH="$WORK/expect" RRS_SANDBOX="${RRS_SANDBOX:-}" TLC_JAR="$TLC_JAR" TLC_HEAP="$TLC_HEAP" \
         TLC_WORKERS="$TLC_WORKERS" TLC_RSS="$RRS_MEMORY_MAX" \
         "$ROOT/scripts/verify-reified/tlc-expect-violation.sh" --complement "$WORK/tla/$cfg" "$invariant" "$module" 2>&1)"
  rc=$?
  printf '%s\n' "$out" > "$LOG_DIR/${cfg%.cfg}.complement.log"
  if [[ $rc -eq 0 ]]; then
    pass "unsafe $cfg without $invariant: $(printf '%s\n' "$out" | grep '^PASS:' | sed 's/^PASS: //')"
  else
    fail "unsafe $cfg without $invariant: another invariant fails or TLC failed; see $LOG_DIR/${cfg%.cfg}.complement.log"
    printf '%s\n' "$out" | tail -n 5 | sed 's/^/        /'
  fi
done

# ── 4. Apalache inductive checks ────────────────────────────────────────────
# Each check runs through scripts/verify-reified/apalache-expect.sh, the same
# checker that the pgmcp verification recipe runs.
for entry in "${APALACHE_CHECKS[@]}"; do
  IFS='|' read -r name module_name cfg length expected marker <<<"$entry"
  if [[ ! -f "$WORK/tla/$cfg" || ! -f "$WORK/tla/$module_name.tla" ]]; then
    fail "apalache $name: $cfg or $module_name.tla is missing"
    continue
  fi
  log="$LOG_DIR/apalache-$name.log"
  if [[ "$expected" == "NoError" ]]; then
    outcome=(NoError)
  else
    outcome=(Error "$marker")
  fi
  RRS_SCRATCH="$WORK" bounded timeout "$APALACHE_TIMEOUT" \
    "$ROOT/scripts/verify-reified/apalache-expect.sh" "$WORK/tla/$module_name.tla" "$cfg" "$length" "${outcome[@]}" \
    > "$log" 2>&1
  rc=$?
  if [[ $rc -eq 0 ]]; then
    pass "apalache $name ($module_name): $(grep -E '^apalache-expect: PASS' "$log" | tail -1 | sed 's/^apalache-expect: PASS //')"
    violation="$(find "$WORK/apalache-expect" -path "*${cfg%.cfg}*" -name violation1.tla -print -quit 2>/dev/null)"
    [[ -n "$violation" ]] && cp -- "$violation" "$LOG_DIR/apalache-$name.violation.tla"
  else
    fail "apalache $name ($module_name): $cfg --length=$length: see $log"
    [[ $rc -eq 124 ]] && echo "        timed out after ${APALACHE_TIMEOUT}s"
    tail -n 6 "$log" | sed 's/^/        /'
  fi
done

echo "   logs: $LOG_DIR"
finish
