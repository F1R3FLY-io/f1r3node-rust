# TLA+ Model: Cost-Accounted Rho Calculus

Finite-state model checking of the cost-accounted rho calculus token
protocol and eval scheduling, complementing the Rocq mechanization at
`formal/rocq/cost_accounted_rho/`.

## Prerequisites

- Java 17+ (`java -version`)
- TLC through a local `tla2tools.jar`
- Apalache 0.58.3+ (`apalache-mc version`) for SMT-backed bounded cross-checks

Common TLC jar locations:
- `/usr/share/java/tla2tools.jar`
- `/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar`

## Specifications

| File | Purpose | States | Properties |
|---|---|---|---|
| `CostAccountedRho.tla` | Atomic token protocol | 79 (3 procs) | TokenConservation, CostDeterminism, FuelGateSafety |
| `CompoundProtocol.tla` | Full protocol: compound sigs, Splits, nested eval | 63 (4 procs) | TokenConservation, CostDeterminism, FuelGateSafety, SplitOrdering, InnerGateOrdering |
| `EvalScheduling.tla` | Eval loop scheduling comparison | 16 (3 bodies) | InternalizedCostDeterministic, AllEventuallyDone |
| `MC.tla` | Model instance for CostAccountedRho | — | — |
| `MCCompound.tla` | Model instance for CompoundProtocol | — | — |
| `MCEval.tla` | Model instance for EvalScheduling | — | — |
| `FullProtocol.tla` | Generalized protocol: shared channels, arbitrary nesting (depth 0/1/2), Join mediators | 12,960 (7 procs, 12 channels) | TokenConservation, CostDeterminism, FuelGateSafety, GateOrdering, SplitOrdering, NoNegativeTokens |
| `MCFull.tla` | Model instance for FullProtocol | — | — |
| `RuntimeBudgetReplay.tla` | Bounded runtime-budget with WEAKENED schedule-order grants (`ScheduleReady`), OOP truncation (`OopTruncate`), bounded-K reconciliation (`Merge`), replay trace, invalid-event rejection, deploy reset, finalization-read model, and canonical digest-entry diagnostic abstraction. Re-aimed so the consensus quantities (`total_cost`/verdict, reconciled committed multiset) are asserted schedule-independent; the per-op digest is NOT (it is dropped from consensus). | OOP arm: 1,722 distinct / 5,209 generated; non-OOP arm: 908 distinct / 2,799 generated; cap arm: 908 distinct / 2,575 generated (6 events incl. zero-weight, over-source-path, over-primitive-descriptor invalid events) | NoOverspend, OopCommitsBoundary, ReplayTraceSubset, OopNotLogged, PermitsMatchSuccessfulTrace, NoUnpaidPhysicalWork, LiveTraceIsAdmissibleSchedule, FinalizedTraceSequence, FinalizationPreservesActiveBudget, LoggedEventsHavePositiveWeight, LoggedEventsAreValidated, TraceWithinRetentionBound, ResetClearsActiveTraceAfterFinalization, PostOopRejectionsPreserveSingleBoundary, CanonicalDigestEventCountMatches, CanonicalDigestDomainSeparatesOop, CanonicalDigestStableAfterFinalization, **ConsumedAndVerdictScheduleIndependent**, **TotalCostMatchesClampedSum**, **NonOopCommittedMultisetComplete**, **CapTruncatedCommittedIsLowestK**, **ReconciledCommittedWellFormed**, **MergeReadsBoundedKWindow**, ConsumedFollowsReconciliationContract, NoCrossWorkerStateMixing |
| `MCRuntimeBudgetReplay.tla` | Model instance for RuntimeBudgetReplay — OOP arm (budget binds; deploy goes OOP) | — | — |
| `MCRuntimeBudgetReplayNonOop.tla` | Model instance for RuntimeBudgetReplay — non-OOP arm (Σ valid weights ≤ budget; complete commit) | — | — |
| `MCRuntimeBudgetReplayCap.tla` | Model instance for RuntimeBudgetReplay — bounded-K cap arm (MaxTraceEvents < valid-event count; cap binds before budget) | — | — |
| `CostAccountingThreats.tla` | Replay tampering, activation downgrade, unauthorized settlement, evidence-recording, and slash-authorization threat model | 7,696 distinct / 1,056,225 generated | CostAccountedReplayAcceptsOnlyValidPayload, CostAccountedReplayRejectsMissingCommitment, SettlementNeverAddsRuntimeFuel, CostInvalidEvidenceHasViolation, CanonicalSlashCandidateRequiresEvidence, CanonicalSlashCandidateRequiresPositiveBond, SlashAuthorizationUsesParentPreState, AmbientBondDoesNotAuthorizeWithoutParent, ParentPositiveAmbientZeroCanAuthorize, SlashNoopPreservesCostBoundary |
| `MCCostAccountingThreats.tla` | Model instance for CostAccountingThreats | — | — |
| `CostAccountingSearchFrontier.tla` | Witness classification and promotion discipline for generated cost-accounting findings | 34,167 distinct / 266,015 generated | NoSourceFixWithoutRustOrInvariantEvidence, ProjectionRiskHasRustGuard, FormalStrengtheningHasInvariantTarget, ConfirmedBugHasSourceTarget, SourceSemanticWitnessHasFacets, SourceGraphSlashingWitnessHasAuthorizationMetadata |
| `MCCostAccountingSearchFrontier.tla` | Model instance for CostAccountingSearchFrontier | — | — |
| `MergeableChannelAccounting.tla` | Typed mergeable-channel diff/merge behavior and cost-boundary isolation | 2,656 distinct / 8,992 generated | BitmaskDiffMergeRoundTrip, IntegerAddDiffMergeRoundTrip, BitmaskMergeDoesNotDropBits, NonNumericPayloadHasNoNumericDiff, MergeableAccountingPreservesUserCost, SlashSystemEffectPreservesCostBoundary |
| `MCMergeableChannelAccounting.tla` | Model instance for MergeableChannelAccounting | — | — |
| `MergeAggregateAgreement.tla` | Widened simultaneous `IntegerAdd` aggregation shared by survivor selection and trie application | Safe instance explored by `MergeAggregateAgreement.cfg` | SelectionApplicationAgree, AcceptanceIsPermutationInvariant, FinalResultIsMathematicalTotal |
| `MCMergeAggregateAgreement.tla` | Safe and unsafe contribution-order instances | — | — |
| `MergeAggregateAgreementPrefixUnsafe.cfg` | Expected-refutation control that validates every enumeration prefix instead of the final mathematical total | Counterexample required | Violates AcceptanceIsPermutationInvariant |
| `DeployTraceSegmentation.tla` | Soft-checkpoint drain semantics that assign exactly one causal event segment to each deploy | Safe instance explored by `DeployTraceSegmentation.cfg` | CheckpointContainsOnlyItsDeploy, CheckpointClearsActiveTrace; liveness: EventuallyAllDeploysCheckpointed |
| `MCDeployTraceSegmentation.tla` | Three-deploy trace-segmentation instance | — | — |
| `DeployTraceSegmentationRetentionUnsafe.cfg` | Expected-refutation control that retains earlier deploy events in the active trace | Counterexample required | Violates CheckpointContainsOnlyItsDeploy |
| `AtomicCommAccounting.tla` | Calculus execution-unit projection across producer/consumer arrival orders, unmatched introductions, binary matches, joins, finite capacity, and replay; quantitative bytes are modeled separately by `VaultBackedByteAccounting.tla` | Safe instance explored by `AtomicCommAccounting.cfg` | CostEqualsCommittedComms, UnmatchedIntroductionsAreFree, JoinArityDoesNotMultiplyCost, RejectedCommIsAtomic, ReplayMatchesPlayAtCompletion, BudgetNeverOverspent, TerminalRSpaceIsScheduleIndependent; liveness: EventuallyComplete |
| `AtomicCommRejection.tla` | Zero-capacity specialization of the atomic observer boundary | Safe instance explored by `AtomicCommRejection.cfg` | Observer rejection preserves pending state, committed events, cost, and replay trace |
| `MCAtomicCommAccountingIntroductionUnsafe.tla` | Expected-refutation control that assigns a calculus execution unit to an unmatched introduction; this does not prohibit its separate quantitative byte charge | Counterexample required with `AtomicCommAccountingIntroductionUnsafe.cfg` | Violates ExactCommCost |
| `VaultBackedByteAccounting.tla` | Protocol-4 quantitative RSpace refinement: canonical produce/consume identities, exact-once persistent introductions, payload and trace bytes, fixed RevVault reservation, persistent/peek behavior, checked arithmetic, settlement, concurrent top-up isolation, and replay | Safe TLC and Apalache configurations plus eight expected-refutation controls | ExactCanonicalDebit, RejectedAttemptIsAtomic, NoRemovalCredit, ReservationSnapshotImmutable, ReplayPrefixExact, conservation and boundedness invariants |
| `StackIntroductionAtomicity.tla` | Cross-ledger microstep refinement for a located stack production: pending physical reservation, byte acceptance or rejection, RSpace mutation, matched-continuation success or failure, operation commit or abort, enclosing-deployment rollback with retained attempt cost, nested-produce causal extraction, and replay | Safe TLC and Apalache configurations plus six expected-refutation controls | PhysicalCapacityIsConserved, PreparedAuthorityIsNotWitnessVisible, EveryCommittedStackHasOneBirth, RejectedOperationIsEffectFree, CommittedStackIsComplete, EveryCommittedStackIsCausallyExtracted, FailedDeployHasNoLinearEffects, RolledBackWorkRetainsByteCharge, ReplayMatchesCommit; liveness: EventuallyReplayed |
| `StackIntroductionAtomicity*Unsafe.cfg` | Expected-refutation controls that expose prepared authority, omit operation abort, omit birth, omit enclosing-deployment rollback, omit a COMM-nested produce, or omit replay state | Counterexample required for each configuration | Refutes the exact visibility, operation/deployment rollback, birth, causal extraction, attempted-work-retention, or replay invariant excluding the enabled defect |
| `EvaluationTransactionIsolation.tla` | End-to-end evaluation boundary across pre-reset parser rejection, reducer failure after attempted work, play post-validation, replay candidate-checkpoint publication, replay effect/final-state validation, merge-evidence publication, and rollback to the authenticated base | Safe TLC and Apalache configurations plus five expected-refutation controls | ParserFailureHasNoWitness, ReducerFailureRetainsAttemptedWork, RejectedPlayIsStateAtomic, RejectedReplayIsStateAtomic, RejectedReplayPublishesNoEvidence, AcceptedReplayPublishesValidatedEvidence, ExecutionStartsWithFreshWitness |
| `EvaluationTransactionIsolation*Unsafe.cfg` | Expected-refutation controls that reuse a predecessor witness, erase attempted reducer work, retain rejected play state, retain rejected replay state and its pre-validation active checkpoint, or publish merge evidence before final-state acceptance | Counterexample required for each configuration | Refutes the exact witness-freshness, attempt-cost-retention, play rollback, replay rollback, or evidence-publication invariant excluding the enabled defect |
| `ConcurrentEvaluationTransactionIsolation.tla` | Two independently scheduled evaluation transactions split parser, floor/root capture, reset, execution, checkpoint, validation, rejection, acceptance, crash, and restart while sharing an append-only history repository | Safe TLC and Apalache configurations plus eight expected-refutation controls | ExplicitBaseAuthority, AcceptedRootsAreOwnedAndRecorded, CheckpointedRootsRemainRecorded, ParserFailureHasNoWitness, ReducerFailureRetainsAttemptedWork, RejectedTransactionsAreStateAtomic, EvidenceRequiresAcceptance, AcceptedEvidenceIsExact |
| `ConcurrentEvaluation*Unsafe.cfg` | Expected-refutation controls for parser-witness reuse, reducer-work erasure, play/replay rollback omission, early evidence, shared-current-root authority or publication, and foreign-root deletion | Counterexample required for each configuration | Refutes the exact transaction, evidence, or root-authority invariant excluding the enabled defect |
| `MultiShardResourceIsolation.tla` | Four tasks on two shard runtime boundaries share bounded workers while independently capturing versions, retrying stale work, publishing roots, applying charges, accepting top-ups, crashing, and restarting | Non-crash safety+liveness and crash/restart safety TLC configurations, crash-enabled Apalache, plus five expected-refutation controls | ResourceCapacityBounded, ResourceOwnershipExact, RunningPhaseOwnsResource, PerShardConservation, ChargesMatchOwnedCommits, LedgerMatchesOwnedCommits, RecordedRootsMatchLedger, VersionsMatchLedger, CommittedTasksReleaseResources; failure-free liveness: Liveness_AllTasksCommit |
| `MultiShard*Unsafe.cfg` | Expected-refutation controls for blind stale commit, foreign semantic-state write, foreign root publication, foreign debit, and leaked shared-worker ownership | Counterexample required for each configuration | Refutes the exact lost-update, shard-frame, root-ownership, conservation, or resource-lifetime invariant excluding the enabled defect |
| `MergeableEvidenceAuthentication.tla` | Two validators locally replay two equivocations that share the legacy post-state/creator/sequence tuple, receive untrusted peer payloads, insert authenticated merge evidence in arbitrary orders, and project exact fail-closed finalized-entry retirement over full DAG ancestry | Safe TLC and Apalache configurations plus legacy-key, legacy-delete, peer-trust, vacuous-latest, and main-spine-only expected-refutation controls | CompleteKeySeparatesEquivocations, LocallyDerivedEvidenceOnly, ExactLocalReplayLookup, OppositeArrivalOrdersConverge, PeerInputCannotOverwriteReplay, DeletionRemovesExactExecution, DeletionPreservesDistinctExecution, DeletionIsIdempotent, DeletionCommutesWithDistinctReplay, RetirementRequiresEverySafetyGuard, SecondaryParentRetirementComplete |
| `MergeableEvidenceAuthentication*Unsafe.cfg` | Expected-refutation controls that collapse distinct executions into the legacy key, retire through that aliased key, let peer bytes mutate the local store, treat an empty latest-message set as proof of advancement, or ignore secondary-parent advancement | Counterexample required for each configuration | Refutes opposite-order convergence, distinct-execution preservation during retirement, local replay provenance, the complete retirement guard, or full-DAG retirement completeness |
| `IntroductionAuthorityRegistry.tla` | Linearization refinement for concurrent fallback resolution and explicit registration of an introduction sponsor | Safe TLC and Apalache configurations plus one expected-refutation control | ResolvedMatchesCommittedRegistry, RegistrationResultMatchesCommittedRegistry, FallbackResolutionIsPermanent, ExplicitFirstResolutionUsesExplicitPayer; liveness: EventuallyBothComplete |
| `IntroductionAuthorityRegistrySplitFallbackUnsafe.cfg` | Expected-refutation control that separates fallback lookup from insertion and permits an explicit payer to win between those operations | Counterexample required from both TLC and Apalache | Refutes ResolvedMatchesCommittedRegistry by returning a payer different from the committed registry entry |
| `LocatedVaultByteSettlement.tla` | Per-authority refinement of protocol-4 byte settlement across outer, continuation, compound, and envelope purses, including concurrent top-up and exact replay | Safe TLC and Apalache configurations plus four expected-refutation controls | ExactLocatedDebit, CompoundDebitIsComponentwise, UnderfundedContinuationCannotUseAnotherPurse, ReservationSnapshotImmutable, CanonicalValueConserved, ReplayAllocationIsExact, ReplayMatchesPlay; liveness: EventuallyDone |
| `LocatedVaultByteSettlement*Unsafe.cfg` | Expected-refutation controls for envelope collapse, cross-purse rescue, top-up expansion of an in-flight reservation, and replay against the envelope purse | Counterexample required for each configuration | Refutes the local-debit, local-sufficiency, reservation-snapshot, or replay-allocation invariant that excludes the enabled defect |
| `StructuralAuthorityBound.tla` | Scoped non-persistent introductions under arbitrary COMM participant groupings and firing orders | Safe instance explored by `StructuralAuthorityBound.cfg` | TypeOK, RealizedNeverExceedsStructuralDemand |
| `StructuralAuthorityBoundReuseUnsafe.cfg` | Expected-refutation control that reuses one non-persistent introduction in multiple events | Counterexample required | Violates RealizedNeverExceedsStructuralDemand |
| `LocatedAuthoritySettlement.tla` | Integrated native-refinement model for signed interaction regions, exact authority partitions, located purses, lollipop payer transfer, atomic split/combined joins, deployment reservations, cross-deploy funding slots, settlement, and replay | 94,056 generated / 15,323 distinct / depth 33 | ReservationsNeverExceedSupply, RealizedBackedByReservation, NoPartialEventDebit, CommittedEventsHaveExactAuthority, NoAmbientAuthority, LollipopContinuationOrder, LollipopUsesDistinctPayers, WholeJoinConsumesOneCell, SplitJoinConsumesEveryPresentedCell, CombinedJoinConsumesOneCompleteCell, CrossDeploySlotIdentityStable, SettlementConservesEveryPurse, ReplayPreservesAuthority, ReplayMatchesSettlement; liveness: EventuallyDone |
| `LocatedAuthoritySettlement*Unsafe.cfg` | Six expected-refutation controls for authority erasure, ambient-purse substitution, continuation rewrapping, partial multi-purse debit, replay metadata omission, and cross-deploy slot-identity loss | Counterexample required for each configuration | Refutes the reservation, atomicity, replay-authority, or persistent-slot invariant that excludes the enabled defect |
| `WalletFundedLollipop.tla` | Composed native refinement from distinct public outer/continuation purse addresses and a retained unforgeable lollipop capability through atomic two-purse SystemVault funding, post-funding activation, authenticated gateway ingress, validator certification, component-wise realized debits, separate gateway fee transfer, two-purse refunds, and replay | Safe TLC and Apalache configurations | CanonicalCustodyConserved, PublicAddressesAreNotCapability, FundingUsesAddressesWithoutDelegatingDraw, FundingCommitsBothPursesAtomically, ContinuationActivationRequiresFunding, ContinuationRequiresOuter, OnlyGatewayAuthorizesContinuation, UnauthorizedAttemptPreservesPreActivationState, CertifiedPayersAreLocatedPurses, ValidatorsAgreeOnAdmission, CommittedContinuationUsesLocatedPurses, RealizedCostsAndFeeAreSeparated, UnusedCertifiedBoundsAreRefunded, NoProtocolMintDuringFundingOrSettlement, ReplayUsesCanonicalLocatedPayers, ReplayMatchesCommit; TLC liveness: EventuallyDone |
| `WalletFundedLollipop*Unsafe.cfg` | Eight expected-refutation controls for custody copying, first-class Rholang capability leakage, gateway-authentication bypass, per-validator payer collapse, activation before funding, missing outer authority, certified-bound overcharge, and replay omission | Counterexample required from both TLC and Apalache for each configuration | Refutes the exact conservation, ingress authority, attribution, funding/activation staging, refund, or replay invariant that excludes the enabled defect. `AllowCapabilityLeak` means transferring the executable name into an unauthorized continuation; it does not model visibility of deterministic serialized `GPrivate` identity bytes, which cannot be injected into source |
| `FundingSlotBootstrap.tla` | Concrete installer/sponsor/gateway refinement: installer-paid capability scaffold, publication of distinct outer and continuation addresses, atomic wallet funding of both initially absent vaults, authenticated activation, per-purse settlement, and terminal rejection | Safe TLC and Apalache configurations | CustodyConserved, InstallWorkflowIsAdmissible, RejectedInstallIsEffectFree, InstallPublishesOnlyTheScaffold, CandidateCreatedSupplyNeverFundsItsCreator, LocatedContinuationRequiresPriorFunding, FundingCommitCoversEveryLocatedPurse, RejectedFundingIsEffectFree, ActivationRequiresAuthenticatedLocalSufficiency, SettlementUsesDistinctLocatedPurses; TLC liveness: EventuallyDone |
| `FundingSlotBootstrap*Unsafe.cfg` | Five expected-refutation controls for eager installation against empty purses, candidate self-funding, slot-only funding, partial debit, and destination creation during a rejected batch | Counterexample required from both TLC and Apalache for each configuration | Refutes admissible staging, authenticated pre-state capacity, complete two-purse funding, balance atomicity, or vault-registry atomicity |
| `PoSVaultAuthority.tla` | Blessed-template completeness and the PoS stake-vault controller binding from authenticated genesis deployer through unauthorized and authorized transfer | Safe TLC and Apalache configurations | NoCompiledPlaceholder, InstalledBindsAuthenticatedKey, UnauthorizedCannotDebit, VaultConservation; TLC liveness: InstalledEventuallyAuthorizes |
| `PoSVaultAuthority*Unsafe*.cfg` | Expected-refutation controls for the historical literal controller and permissive unresolved-template compilation | Counterexample required from both TLC and Apalache for each configuration | Refutes authenticated controller binding or fail-closed blessed-template compilation |
| `ConcurrentRedemptionCustody.tla` | Per-validator, generation-scoped redemption transactions with separately staged stake, fuel, and PoS lifecycle updates; receipt-based retry semantics; abort; and distinct-validator concurrency | TLC exhausts the full staged two-validator graph. Apalache checks the complete seven-transition slash/resolution/retry or rejection transaction horizon; the missing-lock control independently reaches its two-commit violation through length 11. Eight expected-refutation controls are mandatory. | StakeConserved, FuelConserved, LiveStakePositive, QuarantineCarriesExactOrigin, ReceiptsUseAuthorizedGeneration, AtMostOneResolutionPerIncarnation, RejectedTransactionsPublishNothing, ExactRetriesAreEffectFree, UnauthorizedGenerationsNeverCommit, GuiltyIsStrictlyPartial, ConflictingRetriesAreEffectFree, RestoresExactLifecycle |
| `ConcurrentRedemptionCustody*Unsafe*.cfg` | Expected-refutation controls for missing target ownership, stale generations, total confiscation through `Guilty`, origin collapse, partial stake/fuel publication, lost receipts, and conflicting receipt overwrite | Counterexample required from both TLC and Apalache for each configuration | Refutes the exact concurrency, authorization, lifecycle, atomicity, or idempotence invariant excluding the enabled defect |
| `EndToEndCostConsensus.tla` | Canonical SystemVault genesis funding and replay, unit-authority symmetry for blessed genesis execution, proof-bearing reservation for every signed deployment kind, concurrent realized events, direct payer-to-proposer fee transfer, settlement, replay, recoverable local faults, validation disposition, and DAG finality | Safe instance explored by `EndToEndCostConsensus.cfg` | GenesisCommitIsExact, AdmissionRequiresGenesisAgreement, GenesisExecutionReplayAuthorityAgree, SettlementDoesNotReapplyGenesisFunding, CostReservationBacksEveryChoice, ReservationBacksRealized, EveryExecutedDeploymentWasFunded, SettlementIsExact, SettlementConserves, FeeIsCanonicalTransfer, RefundIsUnusedReservation, ReplayUsesSameCommittedEvents, LocalFaultNeverCreatesSlashEvidence, FinalityUsesDAGAncestry; liveness: EventuallyDoneOrRejected |
| `MCEndToEndCostConsensus.tla` | Model instance for EndToEndCostConsensus | — | — |
| `EndToEndCostConsensusUnsafe.cfg` | Expected-refutation control mapping local faults to slash evidence | Counterexample required | Violates LocalFaultNeverCreatesSlashEvidence |
| `EndToEndCostConsensusFundingBypassUnsafe.cfg` | Expected-refutation control allowing one deployment class to execute without funding | Counterexample required | Violates EveryExecutedDeploymentWasFunded |
| `EndToEndCostConsensusGenesisMismatchUnsafe.cfg` | Expected-refutation control allowing replay to use authority allocations different from the committed genesis allocations | Counterexample required | Violates AdmissionRequiresGenesisAgreement |
| `EndToEndCostConsensusGenesisAuthorityMismatchUnsafe.cfg` | Expected-refutation control that executes genesis with unit authority but replays it with deploy-funder authority | Counterexample required | Violates GenesisExecutionReplayAuthorityAgree |
| `EndToEndCostConsensusDoubleCreditUnsafe.cfg` | Expected-refutation control reapplying canonical genesis SystemVault funding during ordinary settlement | Counterexample required | Violates SettlementDoesNotReapplyGenesisFunding |
| `StateBoundAdmission.tla` | Dependent bounded execution retained as the committed witness, followed by certificate-constrained replay and fee-inclusive settlement across schedule permutations | Safe instance explored by `StateBoundAdmission.cfg` | AdmittedProofCompleted, AdmissionRequiresCompletedProof, AdmittedCostIsFunded, PreflightCommitReplayAgree, EvidenceMatchesCommit, CommitMatchesReplay, SettlementIsExact, ScheduleIndependentCost; liveness: EventuallyDoneOrRejected |
| `MCStateBoundAdmission.tla` | Model instance for StateBoundAdmission | — | — |
| `StateBoundAdmissionStructuralUnsafe.cfg` | Expected-refutation control using submitted-structure cost while the authenticated state contributes ambient events | Counterexample required | Violates EvidenceMatchesCommit |
| `StateBoundAdmissionDriftUnsafe.cfg` | Expected-refutation control performing a second unconstrained play instead of retaining the bounded witness | Counterexample required | Violates EvidenceMatchesCommit |
| `StateBoundAdmissionExhaustionUnsafe.cfg` | Expected-refutation control admitting a proof that exhausted finite capacity | Counterexample required | Violates AdmissionRequiresCompletedProof |
| `StateBoundValidatorConvergence.tla` | Independent validators decide the same certificate under different arrival orders and reducer schedules, including local schedules with different event sets and costs; stale roots or different block contexts are rejected | Safe instance explored by `StateBoundValidatorConvergence.cfg` | ScheduleDiversityIsExercised, AcceptedUsesAuthenticatedContext, AcceptedUsesCanonicalDeployOrder, AcceptedReproducesCertificate, AcceptedValidatorsAgree; liveness: EventuallyAllValidatorsDecide |
| `MCStateBoundValidatorConvergence.tla` | Three-validator model instance for state-bound convergence | — | — |
| `StateBoundValidatorConvergenceContextUnsafe.cfg` | Expected-refutation control accepting a certificate outside its authenticated root/block context | Counterexample required | Violates AcceptedUsesAuthenticatedContext |
| `StateBoundValidatorConvergenceOrderUnsafe.cfg` | Expected-refutation control executing arrival order without checking the certified post-state | Counterexample required | Violates AcceptedUsesCanonicalDeployOrder |
| `StateBoundValidatorConvergenceScheduleUnsafe.cfg` | Expected-refutation control accepting a scheduler-local event trace instead of replaying the certified causal witness | Counterexample required | Violates AcceptedReproducesCertificate |
| `LocatedStackConservation.tla` | Atomic materialization of first-class located stacks as transfers from an authenticated source purse, with fresh transfer identities and byte-identical replay | Safe instances explored by `LocatedStackConservation.cfg` and `LocatedStackConservationCollision.cfg` | UserStackProductionConserves, RejectedTransferIsAtomic, TransferIdentityIsFresh, ReplayMatchesCommittedTransfer |
| `LocatedStackConservation*Unsafe.cfg` | Expected-refutation controls for duplicate-identity minting, partial underfunded transfer, and replay transfer omission | Counterexample required for each configuration | Refutes UserStackProductionConserves or ReplayMatchesCommittedTransfer |
| `ParallelStackMaterialization.tla` | A current `Par` materializes located stacks before concurrent sibling reduction, while continuation-local stacks remain causally after their parent COMM; replay repeats the same phases | Safe instance explored by TLC and symbolically through the complete eight-step horizon by Apalache | MaterializationBarrier, NestedDeclarationFollowsParent, Conservation, CausallyFundedProgramIsAccepted, ReplayAgreement; liveness: Progress |
| `ParallelStackMaterializationUnsafe.cfg` | Expected-refutation control exposing a scheduler-local sibling reduction before its co-present stack declaration | TLC and Apalache counterexample required | Violates CausallyFundedProgramIsAccepted |
| `StateBoundFrontierExpansion.tla` | Finite retry protocol that discovers authenticated pre-state authority at an exhausted scalar or per-lane allocation boundary, reverts the speculative attempt, expands capacity strictly, and replays under the final bound | Safe instance explored by `StateBoundFrontierExpansion.cfg` | CapacityUsesOnlyAuthenticatedBacking, ExpansionIsStrictAndBounded, SpeculativeAttemptsAreEffectFree, AcceptanceRequiresCompleteTrace, CompleteBackedTraceIsAccepted, ReplayUsesTheExpandedBound; liveness: EventuallyDone |
| `StateBoundFrontierExpansion*Unsafe.cfg` | Expected-refutation controls for a frozen initial cap, unbacked frontier credit, leaked speculative effects, and replay under the initial rather than expanded bound | Counterexample required for each configuration | Refutes CompleteBackedTraceIsAccepted, CapacityUsesOnlyAuthenticatedBacking, SpeculativeAttemptsAreEffectFree, or ReplayUsesTheExpandedBound |
| `ReplaySupplySnapshot.tla` | Per-deploy capture of authenticated SystemVault and located-stack supply from ordinary RSpace, followed by trace-only ReplayRSpace execution with no live authority query | Safe instance explored by `ReplaySupplySnapshot.cfg` | SnapshotsAreAuthenticated, ReplayUsesAuthenticatedSnapshots, ExactRecordedReplayTrace, ReplayConservesSupply; liveness: EventuallyReplayCompletes |
| `ReplaySupplySnapshotLiveQueryUnsafe.cfg` | Expected-refutation control that queries live authority through ReplayRSpace and appends the query to the committed replay trace | Counterexample required | Violates ExactRecordedReplayTrace |
| `ReplayRootMaterialization.tla` | Independent producer, validator, and reporter histories replay a three-deployment root chain by alternating ordinary-RSpace purse capture with trace replay and checkpoint materialization | Safe instance explored by `ReplayRootMaterialization.cfg` | SnapshotsFollowCanonicalChain, SnapshotReadsMaterializedRoot, SnapshotsUseOrdinaryRuntime, CursorMatchesReplayPrefix, AcceptedMaterializedPostState, CompletedValidatorsAgree; liveness: EventuallyAllComplete |
| `ReplayRootMaterializationApalache.cfg` | SMT-bounded two-validator, two-deployment instance of the replay-root model | Safe executions through the complete eight-transition horizon | The seven replay-root safety invariants; liveness remains a TLC obligation |
| `ReplayRootMaterialization*Unsafe.cfg` | Expected-refutation controls for eager future-root reads, producer-local-history-dependent validation, and ReplayRSpace purse queries | Counterexample required for each configuration | Refutes SnapshotReadsMaterializedRoot, CompletedValidatorsAgree, or SnapshotsUseOrdinaryRuntime |
| `OslfLocatedTyping.tla` | Finite located OSLF resource checking with exact and conservative observations plus two independent purse spends | Safe instance explored by `OslfLocatedTyping.cfg` and symbolically through length 3 by Apalache | LinearNoContraction, LinearNoWeakening, ModalEvidenceSound, AuthenticatedFundingOnly, LocationIsolation, ModalPoststateExact, LocalSufficiencyComposes, DisjointSpatialSettlement |
| `OslfLocatedTyping*Unsafe.cfg` | Required-refutation controls for contraction, weakening, cross-surface debit aliasing, upper-bound-as-exact modal evidence, and candidate-created funding credit | TLC and Apalache counterexample required for each configuration | Refutes the exact named invariant excluding the enabled defect |
| `AtomicVaultSettlementRefinement.tla` | One native externally atomic SystemVault application composing application transfer, physical burn, quantitative-byte burn, fee transfer, and refund without persistent transient reservation state | Safe instance explored by `MCAtomicVaultSettlementRefinement.cfg` | NoPersistentReservationState, EverySelectedBranchWasStateBoundFunded, FinalizedAggregateIsFunded, AtomicVisibleRefinement, CanonicalValueConserved, FeeCreditIsAConservingTransfer, RejectedAggregateHasNoEffect, ReplayMatchesFinalizedState |
| `AtomicVaultSettlementRefinementGlobalCellUnsafe.cfg` | Expected-refutation control restoring a consensus-visible singleton reservation cell shared by independent branches | Counterexample required | Violates NoPersistentReservationState |
| `AtomicVaultSettlementRefinement{ApplicationDebit,PhysicalBurn,ByteBurn,Fee}OmissionUnsafe.cfg` | Four expected-refutation controls, each dropping one debit dimension from aggregate solvency while retaining the complete committed state transition | Counterexample required for every configuration | Each violates FinalizedAggregateIsFunded |
| `NormalizerEnvironmentRefinement.tla` | Certification, retained execution, and replay use one authenticated deployer/cosigner normalizer environment | Safe instance explored by `NormalizerEnvironmentRefinement.cfg` | CertificationExecutionReplayUseSameEnvironment, AuthenticatedProgramIsAdmitted, ExecutionRequiresAdmission, ReplayMatchesExecution; liveness: EventuallyReplayCompletes |
| `NormalizerEnvironmentRefinementEmptyUnsafe.cfg` | Expected-refutation control certifying a deployer-ID program under an empty environment while execution uses authenticated bindings | Counterexample required | Violates CertificationExecutionReplayUseSameEnvironment |
| `PhysicalSettlementWorklist.tla` | Canonical heap-worklist physical proof search for two independently scheduled deployments, with a bounded native call stack | Safe instance explored by `PhysicalSettlementWorklist.cfg` | NativeStackBound, WorklistUsesNoNativeRecursion, CompletedResultMatchesReference, SearchNodesStayWithinTheFiniteTree; liveness: EventuallyAllDeploymentsComplete |
| `PhysicalSettlementWorklistRecursiveUnsafe.cfg` | Expected-refutation control that consumes one native frame per event instead of using the heap worklist | Counterexample required | Violates NativeStackBound |
| `BlockHeapLifecycle.tla` | Concurrent block creation/validation heap ownership, bounded completion cadence, abstract allocator reclamation, and consensus-semantic noninterference | Safe instance explored by TLC and symbolically through length 12 by Apalache | TypeOK, IdleSlotsOwnNoLiveHeap, CompletedBlocksMatchHistory, ReclamationIsSemanticallyInvisible, ResidentWithinIntervalEnvelope |
| `BlockHeapLifecycleMissingBoundaryUnsafe.cfg` | Expected-refutation control that drops Rust owners but never returns reclaimable pages at a completion boundary | TLC and Apalache counterexample required | Violates ResidentWithinIntervalEnvelope without changing semantic history |

### Phase 1 / 2 / 3 multi-signature + LL-rich algebra specs

| File | Purpose | Properties |
|---|---|---|
| `MultiSignerProtocol.tla` | Historical Phase 1.7 refinement of staged per-cosigner charge/refund arithmetic; not the production D3 escrow mechanism | MapDomainEqualsInFlightSigners, RefundFinalizes, NoRefundCrossAttribution, PartialFailureNoConsumption, NoNegativeAmounts, ChargedAmountBounded, PhloShareConservation, FailureRevertsCharges, TotalRefundConservation; liveness: EventuallyDoneOrReverted, EventuallyAllRefundsComplete. Production custody is refined by `RuntimeBoundAuthority`, `AtomicVaultSettlementRefinement`, and `WalletFundedLollipop`. |
| `MCMultiSigner.tla` | Phase 4.6 — scaled-up harness (5 cosigners, phlo 8) | (same as base) |
| `ThresholdProtocol.tla` | Phase 2 M-of-N quorum semantics | QuorumThresholdConstraint, QuorumExactness, QuorumNoOverCount, AuthorizedSubsetPresented, PresentedSubsetMembers, RejectionImpliesShortQuorum; liveness: EventuallyTerminates |
| `MCThreshold.tla` | Phase 4.6 — 3-of-5 quorum harness (vs base 2-of-4) | (same) |
| `ThresholdEnvelopeAuthority.tla` | Protocol-v6.1 commitment, principal/custody separation, exact selected-subset authority, witness correspondence, and per-selected-purse funding under two concurrent validators | Safe instance: 2,306 generated / 512 distinct / depth 10; TLC and Apalache also require six unsafe controls to refute subset omission, policy-wide authority, member-zero authority, policy-wide debit, witness mismatch, and duplicate-ground aliasing |
| `ReplayAdmissionPublication.tla` | Protocol-v6.1 exact authenticated admission, typed storage/evidence/reservation/fee/RNG identity, exact evidence consumption, validated state-root publication, compare-and-swap durability, cache ordering, crash/restart, and cross-validator agreement | Safe instance: 1,035,710 generated / 130,321 distinct / depth 37; TLC and Apalache also require fifteen unsafe controls to refute all-admitted, count-only, primary-signature, legacy-wire-field, raw evidence/reservation/fee/RNG identity, unconsumed evidence, caller-context, early-publication, bare-row, peer-byte, conflict-overwrite, and cache-first defects |
| `PlusProtocol.tla` | Phase 3 Sig::Plus additive disjunction (signer's chosen branch) | PlusBranchInRange, PlusBranchWitness, PlusNonChosenUntouched; properties: AdditiveChoiceDeterminism, PlusEventuallyAuthorizes |
| `MCPlus.tla` | Phase 4.6 — PhloPerBranch=8 harness | (same) |
| `WithProtocol.tla` | Phase 3 Sig::With additive conjunction (verifier's chosen branch) | AdditiveCoConservation, WithBothBranchesSigned, WithBranchAvailability, WithUnpickedUntouched; liveness: WithEventuallyPicked |
| `MCWith.tla` | Phase 4.6 — PhloPerBranch=8 harness | (same) |
| `BangProtocol.tla` | Phase 3 Sig::Bang exponential replication (bounded/unbounded) | BangReplicationSafety, BangUsageBound, BangBoundedNonNegative, BangPersistence, BangApprovedBoundedByLimit; liveness: BangBoundedEventuallyExhausts |
| `MCBang.tla` | Phase 4.6 — Bound=5, MaxInvocations=10 harness | (same) |
| `WhyNotProtocol.tla` | Phase 3 Sig::WhyNot exponential optionality | WhyNotOptional, WhyNotEmptyEquiv, WhyNotNoChargeWhenAbsent, WhyNotChargeBounded, WhyNotInvalidImpliesRejection; liveness: WhyNotEventuallyResolves |
| `MCWhyNot.tla` | Phase 4.6 — PhloAvailable=6 harness | (same) |
| `LollyProtocol.tla` | Phase 3 Sig::Lolly linear-implication capability | LollyResourceFlow, LollyNoCreationExNihilo, LollyTransformer, LollyCapabilityRegistered, LollyCapabilityNotRevoked; liveness: LollyEventuallyCompletes |
| `MCLolly.tla` | Phase 4.6 — MaxInvocations=6 harness | (same) |

### Connectives without a dedicated spec (subsumed)

Three of the nine LL connectives have no standalone protocol spec — by design, not omission:

| Connective | Where verified | How |
|---|---|---|
| **And / `⊗`** (tensor) | `CompoundProtocol.tla` / `FullProtocol.tla` plus `RuntimeBoundAuthority.tla`, `AtomicVaultSettlementRefinement.tla`, and `WalletFundedLollipop.tla` | `SplitFires` and `TokenConservation` establish structural decomposition; production state-bound admission and exact settlement require every verified component, isolate payer lanes, and apply physical, byte, and fee debits atomically. `MultiSignerProtocol.tla` remains a historical arithmetic refinement only. |
| **Unit (`1`)** | `CostAccountedRho.tla` / `CompoundProtocol.tla` / `FullProtocol.tla` | Degenerate zero-token case (`TokensPerProc = 0` ⇒ 0 cost). The algebraic unit laws (`1 ⊗ σ ≡ σ`) are checked in Sage / Rocq, not TLA⁺. |
| **atom** | `CostAccountedRho.tla` | The atomic single-token signature is the base case ("atomic signatures"); one-atom ⇒ one-gate ⇒ one-COMM is the generic `FuelGateSafety`. |

So all nine connectives are accounted for: six dedicated specs (Plus, With, Bang, WhyNot, Lolly, Threshold) plus these three subsumptions.

## Running

All TLC execution goes through the repository's bounded runner. It caps the JVM
heap and worker count, stores the state graph below `target/verification/` on
disk, and rejects any unsafe control that does not fail for its exact intended
property. This prevents a large state space from consuming the host's
RAM-backed `/tmp` or an unbounded ergonomic JVM heap. The TLC registry described
below covers the atomic and compound protocols, evaluation scheduling,
generalized nested signatures, runtime-budget replay, threats, search,
state-bound admission and convergence, and every linear connective.

The companion script `scripts/check-cost-accounted-rho-tla-invariants.sh`
runs every registered safe specification sequentially through TLC and runs each
registered unsafe configuration as a required expected refutation. A negative
control passes only when TLC names its exact intended invariant violation.
Per the team's "formal verification is local-only, NOT in CI" policy
this script does NOT live under `scripts/ci/`. Invoke directly from
the repo root:

```bash
bash scripts/check-cost-accounted-rho-tla-invariants.sh
# Or filter:
bash scripts/check-cost-accounted-rho-tla-invariants.sh --filter MC
bash scripts/check-cost-accounted-rho-tla-invariants.sh --filter RuntimeBudget
```

Safe specs report `Model checking completed. No error has been found.` Unsafe
controls report `PASS (refuted <Invariant>)`.

The end-to-end safe model and all six expected-refutation controls run together:

```bash
bash scripts/check-cost-accounted-rho-tla-invariants.sh --filter EndToEndCostConsensus
```

The controls require violations of `LocalFaultNeverCreatesSlashEvidence`,
`EveryExecutedDeploymentWasFunded`, `AdmissionRequiresGenesisAgreement`,
`GenesisExecutionReplayAuthorityAgree`, `ValidationOriginParity`, and
`SettlementDoesNotReapplyGenesisFunding`, respectively.

The atomic-COMM safe models and introduction-charging negative control are:

```bash
bash scripts/check-cost-accounted-rho-tla-invariants.sh --filter AtomicComm
```

The registered control is successful only when TLC reports a violation of
`ExactCommCost`: unmatched introductions are deliberately charged in that model,
reproducing the implementation defect that caused replay disagreement.

The authenticated replay-supply boundary and its live-query negative control are:

```bash
bash scripts/check-cost-accounted-rho-tla-invariants.sh --filter ReplaySupplySnapshot
bash scripts/check-cost-accounted-rho-tla-invariants.sh --filter ReplayRootMaterialization
bash scripts/check-cost-accounted-rho-tla-invariants.sh --filter AtomicVaultSettlementRefinement
```

The safe model captures each deployment's supply before trace replay and proves
that replay consumes the same supply sequence while reproducing the exact
recorded trace. The control is successful only when TLC reports a violation of
`ExactRecordedReplayTrace`: a live SystemVault query through ReplayRSpace is an
extra RSpace event and therefore cannot be part of replay admission.

The root-materialization model gives the producer every intermediate root but
starts the independent validator and reporter with genesis only. The safe loop
must replay and checkpoint deployment $`i`$ before reading deployment $`i+1`$.
Its controls demonstrate that eager reads access absent roots, make acceptance
depend on producer-local history, or cross the ordinary/replay runtime boundary.

The aggregate formal gate runs Apalache over symbolic N-ary join authority,
the threat and search-frontier models, the bounded replay-root model, and the
finite located OSLF safe model plus all five required counterexamples:

```bash
bash scripts/check-cost-accounted-rho-apalache.sh
```

The replay-root cross-check covers all eight steps required for both validators
to capture two authenticated pre-state snapshots, replay both deployments, and
materialize both post-state roots. It checks the seven safety invariants with an
SMT-backed state encoding. TLC separately exhausts the three-node,
three-deployment state space, proves the liveness property under weak fairness,
and requires the three unsafe configurations to exhibit their named defects.
The OSLF cross-check covers admission plus both orders of the two disjoint purse
spends. Its safe leg must report `NoError`; each unsafe leg must report the
configured invariant violation. This prevents a vacuous safe model from passing
after a safeguard or counterexample path is accidentally removed.

## Verified Properties

### AtomicCommAccounting

- **ExactCommCost / CostEqualsCommittedComms**: cost is exactly the cardinality
  of successful semantic COMM transitions.
- **UnmatchedIntroductionsAreFree**: storing an unmatched send or receive
  cannot consume a calculus COMM execution unit. Protocol-4 quantitative
  introduction bytes remain positively charged by `VaultBackedByteAccounting`.
- **JoinArityDoesNotMultiplyCost**: a committed join contributes one unit,
  independent of the number of channels in its requirement set.
- **RejectedCommIsAtomic**: a capacity rejection cannot commit the match or
  mutate pending RSpace state.
- **ReplayMatchesPlayAtCompletion**: replay of the committed semantic events
  produces the exact play cost.
- **TerminalRSpaceIsScheduleIndependent**: with sufficient authority, every
  command-arrival order leaves exactly the unmatched commands resident.

### EndToEndCostConsensus

- **GenesisCommitIsExact**: the authority map committed by genesis is exactly the canonical initial SystemVault funding map.
- **AdmissionRequiresGenesisAgreement**: no deployment can be admitted until replay has reconstructed the same genesis authority map; the mismatch control demonstrates the missing guard with a counterexample.
- **SettlementDoesNotReapplyGenesisFunding**: ordinary settlement cannot apply genesis SystemVault funding again; the double-credit control refutes the invariant.
- **CostReservationBacksEveryChoice**: the certified cost reservation bounds every modeled reachable event subset, not only one schedule.
- **ReservationBacksRealized**: every realized authority event and deterministic fee is covered by the combined reservation.
- **EveryExecutedDeploymentWasFunded**: every client, heartbeat, or dummy deployment that reaches execution first passed the same proof-bearing funding reservation; no deployment class bypasses authority accounting.
- **SettlementIsExact / SettlementConserves**: the post-state burns realized execution cost, transfers the fee, and preserves the global custody identity.
- **FeeIsCanonicalTransfer**: the proposer SystemVault receives exactly the total fee debited from payer SystemVault balances; no intermediate fee pool or conversion exists.
- **RefundIsUnusedReservation**: realized event cost plus unused cost reservation is exactly the certified cost reservation; the fixed fee is reserved and settled separately.
- **ReplayUsesSameCommittedEvents**: reordering replay work cannot change the canonical realized cost or settled balance.
- **LocalFaultNeverCreatesSlashEvidence**: missing local history and other local validation faults never become invalid-block evidence.
- **FinalityUsesDAGAncestry**: parent-array permutation cannot change DAG-based finality advancement.
- **EventuallyDoneOrRejected**: under weak fairness, the finite valid path completes after at most one recoverable local fault; an unprovable or underfunded admission terminates as rejected.

### OslfLocatedTyping

- **LinearNoContraction / LinearNoWeakening**: an accepted linear assertion has
  exactly one live demand; the unsafe controls independently admit multiplicity
  or absence and must refute the corresponding invariant.
- **ModalEvidenceSound / ModalPoststateExact**: a modal spend is never inferred
  from a conservative upper bound, and an exact spend removes one unit from the
  matching supply and demand maps.
- **AuthenticatedFundingOnly**: admission supply is the authenticated pre-state;
  candidate-created supply cannot satisfy its own reservation.
- **LocationIsolation / DisjointSpatialSettlement**: a spend changes only its
  named surface, and completing both independent branches produces the exact
  component-wise residual regardless of their order.
- **LocalSufficiencyComposes**: each realized local spend stays within its purse,
  so the disjoint conjunction is globally funded.

### CostAccountedRho (atomic signatures)

- **TokenConservation**: Total tokens in system (pending + consumed) equals the initial total in every reachable state.
- **NoNegativeFuel**: No channel ever has negative pending tokens.
- **FuelGateSafety**: A process completes its inner COMM only if its fuel gate has fired.
- **CostDeterminism**: In every terminal state, `totalConsumed` equals the expected cost (one token per process that had fuel), regardless of which interleaving TLC explored.
- **AllComplete** (liveness): Every process with available fuel eventually completes.

### CompoundProtocol (compound signatures, Splits, recursive eval)

All properties from CostAccountedRho, plus:
- **TokenConservation** (extended): Accounts for Split redistribution (1 compound token becomes 2 atomic tokens). Invariant: `TotalPending + totalCost - SplitsFired = InitialTotal`.
- **SplitOrdering**: A compound process's outer gate fires only after its Split mediator has fired.
- **InnerGateOrdering**: A compound process's inner gate fires only after its outer gate.
- **CostDeterminism**: Terminal cost accounts for compound processes consuming 2 gates each and atomic processes consuming 1 gate each. The cost is identical across all scheduling orders.
- **AllSpawnedComplete** (liveness): All spawned processes (including recursively spawned children) with available fuel eventually complete.

### FullProtocol (shared channels, arbitrary nesting, Join mediators)

All properties from CompoundProtocol, generalized to arbitrary configurations:
- **TokenConservation** (generalized): Accounts for both Splits and Joins. Invariant: `TotalPending + totalCost - TotalSplitsFired + totalJoinsFired = InitialTotal`. Splits add +1 net token (1 in -> 2 out), Joins remove -1 net token (2 in -> 1 out).
- **Shared Channels**: Multiple processes can listen on the same signature channel. The injectivity assumption from CompoundProtocol is removed. When two processes compete for the same token, only one wins non-deterministically, but total cost remains deterministic.
- **Arbitrary Nesting (depth k)**: A depth-k process requires k cascading Splits and (k+1) gate layers. The model instance tests depth 0 (atomic), depth 1 (compound), and depth 2 (doubly-compound with 2 cascading Splits and 3 gates).
- **GateOrdering**: Gates fire in strict order (layer 1, then 2, ..., then k+1), and each gate's prerequisite Split must have fired.
- **SplitOrdering**: Splits fire in cascading order (level 1 before level 2, etc.), with each level's output feeding the next level's input.
- **Join Mediator**: The JoinFires action combines two atomic tokens into one compound token, the inverse of Split. The Join mediator's output feeds another process's gate channel.
- **CostDeterminism**: In terminal states, `totalCost` equals the expected cost regardless of interleaving order. With shared channels, the expected cost depends on the token supply configuration (specified as `ExpectedTerminalCost`).
- **AllComplete** (liveness): All processes with available fuel eventually complete.

### EvalScheduling (scheduling comparison)

- **InternalizedCostDeterministic**: At termination, `totalCost = |Bodies| * CostPerToken` regardless of execution order.
- **InternalizedCostBounded**: Cost never exceeds the theoretical maximum.
- **AllEventuallyDone** (liveness): All bodies eventually execute.

The `extCost` variable tracks what the externalized (buggy) cost model would produce — it is intentionally NOT checked as an invariant because it IS order-dependent (that's the bug this migration fixes).

### CostAccountingThreats (single-deploy replay/security boundary)

- **CostAccountedReplayAcceptsOnlyValidPayload**: in cost-accounted mode,
  accepted replay implies a present cost-trace commitment with matching
  digest and count.
- **CostAccountedReplayRejectsMissingCommitment**: absent trace
  commitments cannot be accepted after activation.
- **SettlementNeverAddsRuntimeFuel**: authorized and unauthorized
  settlement actions cannot increase runtime fuel.
- **CostInvalidEvidenceHasViolation**: evidence recording is enabled only
  for a modeled cost-invalid violation.
- **ReplayTamperCannotStayAccepted**: after digest/count/commitment
  tampering, cost-accounted replay is no longer accepted.
- **CanonicalSlashCandidateRequiresEvidence**: canonical scanning selects a
  slash candidate only when the invalid-block evidence is present and both its
  epoch and the target activation epoch are current.
- **CanonicalSlashCandidateRequiresPositiveBond**: canonical scanning excludes
  targets whose bond is non-positive in the exact parent pre-state.
- **SlashAuthorizationUsesParentPreState**: slash acceptance is
  authorized by the parent pre-state bond, not by an ambient post-state
  or execution-time bond view.
- **AmbientBondDoesNotAuthorizeWithoutParent**: an ambient bond alone
  cannot authorize a slash when the parent pre-state bond is zero.
- **ParentPositiveAmbientZeroCanAuthorize**: a positive parent pre-state
  bond authorizes current slash evidence even when the ambient bond view
  is zero.
- **SlashNoopPreservesCostBoundary**: a zero execution-bond slash is a
  no-op with respect to the user runtime cost boundary.

### RuntimeBudgetReplay (bounded runtime-budget replay)

- **CanonicalDigestEventCountMatches**: the abstract digest entry set has
  exactly the retained successful trace count plus the single OOP boundary,
  matching the Rust `cost_trace_event_count` contract. Duplicate events with
  the same deploy id, source path, redex id, local index, billable kind,
  primitive descriptor, and weight receive distinct occurrence ordinals.
- **PermitsMatchSuccessfulTrace** and **NoUnpaidPhysicalWork**: successful
  budget commits grant execution permits before modeled physical work
  executes, and OOP does not grant an execution permit for unfunded work.
- **LiveTraceIsAdmissibleSchedule** (replaces the old `CanonicalPermitOrder`):
  under the cost-accounting refactor the live `successTrace` is recorded in
  whatever order the lock-free CAS race produced — it is **not** rank-sorted.
  The firing guard was deliberately weakened (`ScheduleReady` instead of
  `CanonicalReady`) so TLC explores every interleaving of grants and the model
  can witness the schedule-dependence of the live per-op trace under OOP. The
  invariant therefore only asserts that every committed event was
  intrinsically admissible (positive weight, bounded source path / primitive
  descriptor); the canonical order is recovered post-hoc by the `Merge`
  reconciliation, not enforced on the live trace.
- **CanonicalDigestDomainSeparatesOop**: the OOP boundary is tagged
  separately from successful events, so boundary evidence cannot collapse
  into a successful reservation with the same event identity.
- **CanonicalDigestStableAfterFinalization**: finalization reads the same
  canonical digest entries that the active runtime budget retained; deploy
  reset may clear active trace state only after the finalization read.

#### Re-aimed consensus-quantity invariants (digest dropped from consensus)

The refactor **drops the per-operation `cost_trace_digest` from consensus** —
it is not a consensus quantity. The `ReconciledDigestIsPureFunctionOfEventsAnd
Initial` invariant (which asserted the per-op digest was schedule-independent)
is therefore **removed**: it is *false* once the firing guard is weakened and
OOP truncation is modeled. The consensus cost quantity that remains is
`total_cost` (= consumed tokens, clamped to `initial` on OOP) plus the
failed/OOP status. The model now asserts schedule-independence of exactly
those, via a bounded-K `Merge` action that reconciles a schedule-dependent
live attempt log into a pure function of the constants:

- **ConsumedAndVerdictScheduleIndependent** (headline): after `Merge`
  (`frontier = 2`) the reconciled `consumed`/`total_cost` and OOP verdict equal
  `RecConsumed`/`RecOop`, which read only the constants (Events, Weight, Rank,
  InitialBudget) — never the live trace or firing order. Hence every schedule
  reaches the same value. Threshold law (when the cap does not bite):
  Σ(valid weights) > InitialBudget ⇒ OOP ∧ consumed = InitialBudget; otherwise
  ¬OOP ∧ consumed = Σ.
- **TotalCostMatchesClampedSum**: `total_cost` is the clamped sum
  `min(InitialBudget, Σ valid weights)` in the common (non-cap-truncated) case,
  and is always ≤ both bounds.
- **NonOopCommittedMultisetComplete**: when not OOP and the cap does not bite,
  the reconciled committed multiset is the complete intrinsically-valid event
  set — complete and schedule-independent (every term's fresh-counter metering
  child commits; RSpace selection is deterministic).
- **CapTruncatedCommittedIsLowestK**: when the `MAX_COST_TRACE_EVENTS` backstop
  bites, the committed set is the lowest-K canonical prefix (still a pure
  function of the constants).
- **MergeReadsBoundedKWindow** (Milestone 3): the reconciliation reads only the
  lowest K = `min(MaxTraceEvents, InitialBudget+1)` canonical events; the
  committed prefix never exceeds K (or InitialBudget) events.

The **OopTruncate** action models a fork abandoning its remaining pending work
at the schedule-dependent point where the budget is exhausted; it is the
witness that two schedules can reach different *live* committed sets (hence
different per-op digests) under OOP — which is precisely why the per-op digest
is not a consensus quantity, while the reconciled `total_cost`/verdict above
remain invariant.

Three model instances exercise all three regimes:
`MCRuntimeBudgetReplay` (OOP), `MCRuntimeBudgetReplayNonOop` (complete commit,
no OOP), and `MCRuntimeBudgetReplayCap` (bounded-K cap binds before the
budget).

### CostAccountingSearchFrontier (witness classification)

- **NoSourceFixWithoutRustOrInvariantEvidence**: generated witnesses cannot
  directly motivate implementation changes without production Rust reproduction or a
  production-invariant violation.
- **ClassifiedWitnessHasAction**: every terminal classification has a
  non-empty follow-up action.
- **GuardedProjectionDoesNotFixSource**: projection risks promote to guards
  and documentation, not immediate implementation changes.
- **FormalGapDoesNotDirectlyFixSource**: proof/model strengthening witnesses
  promote to formal artifacts before implementation changes.
- **ProjectionRiskHasRustGuard**: projection risks must point at a Rust guard
  target and carry concrete guard evidence.
- **FormalStrengtheningHasInvariantTarget**: proof/model strengthening
  witnesses must carry an expected invariant and promote to Rocq, TLA+, or
  Sage before any implementation action.
- **ConfirmedBugHasSourceTarget**: confirmed current bugs must target a source
  fix and must be backed by Rust reproduction or production-invariant evidence.
- **ClassifiedWitnessHasPromotionTarget**: every terminal classification
  carries a non-empty promotion target, so frontier output is actionable.
- **StatefulCampaignNamesSteps**: v3 stateful campaign witnesses cannot
  terminate without minimized operation steps.
- **ProductionPathWitnessNamesOracle**: source-corpus and production-path
  differential witnesses cannot terminate without a named production path and
  oracle.
- **ExploitCrossProductHasThreatAndSteps**: exploit cross-product witnesses
  cannot terminate without campaign steps, threat-family classification, and
  an expected invariant.
- **TerminalStutter**: once a witness reaches a terminal classification,
  later discovery actions cannot rewrite its action or promotion target.
- **SourceGraphSlashingWitnessHasAuthorizationMetadata**: source-graph
  slashing witnesses cannot terminate without current-evidence and
  parent-pre-state metadata.

### MergeableChannelAccounting (typed mergeable channels)

- **BitmaskDiffMergeRoundTrip**: a `BitmaskOr` diff records newly-set bits
  as `end & !previous`; replaying it with OR reconstructs
  `previous OR end`, not `max(previous, end)`.
- **IntegerAddDiffMergeRoundTrip**: the existing `IntegerAdd` path keeps
  additive diff/merge behavior.
- **BitmaskMergeDoesNotDropBits**: OR merge preserves every bit set in the
  previous value or current value.
- **NonNumericPayloadHasNoNumericDiff**: tagged non-numeric values stay out
  of numeric merge accounting and must use the ordinary conflict path.
- **MergeableAccountingPreservesUserCost** and
  **MergeableAccountingPreservesSettlementCost**: mergeable-channel metadata
  updates do not mutate user runtime cost or fee-settlement cost evidence.
- **SlashSystemEffectPreservesCostBoundary**: slashing system effects compose
  with the typed mergeable-channel model without changing the cost boundary.

## Scope and Limitations

These TLA+ specifications complement the Rocq mechanization at `formal/rocq/cost_accounted_rho/`; neither tool subsumes the other. Readers should understand what TLA+ here establishes, what it does not, and how it relates to the Rocq proofs.

### What these models establish

- **Finite-state reachability**: TLC exhaustively explores every reachable state of each model under every legal scheduling. Any invariant violation or deadlock that can occur within the configured bounds will be reported.
- **Protocol-level correctness at the bounds used**: at the process/channel/token counts listed in each `.cfg`, each model's listed invariants hold in every reachable state. The core protocol models cover the headline token-conservation, fuel-gate-safety, cost-determinism, and nonnegative-token/fuel properties; the replay, threat, and search-frontier models cover their implementation-facing invariants. See the table above for per-model state counts.
- **Scheduling independence of cost**: `EvalScheduling.tla` specifically contrasts the internalized model against the externalized model side-by-side under all 3! = 6 body orderings, confirming that internalized cost is invariant under reordering while externalized cost is not.
- **Compound signature semantics**: `CompoundProtocol.tla` and `FullProtocol.tla` exercise Split-firing ordering, inner/outer gate sequencing, and Join mediators at concrete small depths.

### What these models do NOT establish

- **Properties for unbounded process, channel, or token counts**: TLC is a finite-state model checker. Claims like "cost is deterministic for *every* configuration" are not proven by TLC — only for the configurations in the `.cfg` files. Unbounded results are established in Rocq:
  - `ca_cost_deterministic` (`formal/rocq/cost_accounted_rho/theories/Confluence.v:474`) — deterministic cost for arbitrary systems.
  - `ca_strongly_normalizing` (`StrongNormalization.v:95`) — every system terminates.
  - `token_monotone_reachable` (`TokenConservation.v:98`) — token conservation for arbitrary reachability chains.
  - `fuel_events_consumed_perm` (`FuelEventDecomposition.v:198`) — consumed-event multiset determinism.
- **Refinement to Rust evaluator code**: the TLA+ models are specifications at the *protocol* level; they describe atomic actions (`FuelGateFires`, `InnerCommFires`, `SplitFires`, `JoinFires`, etc.) without modelling substitution, binding, or the RSpace storage layer. Establishing that the actual Rust implementation realizes these specifications is the responsibility of integration tests and property-based testing at implementation time (see migration doc §5.7 for the normalizer validation prescription and §6 for the test plan).
- **Cryptographic assumptions**: signature uniqueness, hash collision resistance, and the three properties of `hash_process` required by Rocq (verification doc §11.1) are assumed as trust-base constants in the models (`sigChannel` is an injective mapping in `CostAccountedRho.tla`). TLC does not verify cryptography.
- **Structural equivalence / normalizer correspondence**: the TLA+ models work with atomic identifiers (process names, channel names) and never encounter `≡`-reordering, so they cannot detect a hypothetical divergence between RSpace's normalizer and the Rocq `≡` relation. That obligation is discharged at implementation time via property-based tests (migration doc §5.7).
- **Unbounded nesting depth**: `FullProtocol.tla` tests depth 0/1/2; arbitrary depth is covered by Rocq induction, not TLC.

### Model-checking bounds used

| Model | Processes | Channels | Max nesting depth | Tokens / proc | Reachable states |
|---|---|---|---|---|---|
| `CostAccountedRho.tla` | 3 | 3 | 0 (atomic only) | 1 | 79 |
| `CompoundProtocol.tla` | 4 (incl. recursive spawn) | 4 | 1 | up to 2 | 63 |
| `FullProtocol.tla` | 7 | 12 | 2 (doubly-compound + Join) | up to 3 | 12,960 |
| `EvalScheduling.tla` | 3 bodies | — | 0 | 1 | 16 |
| `RuntimeBudgetReplay.tla` (OOP arm) | 6 events | — | 0 | bounded budget 6 | 1,722 distinct / 5,209 generated |
| `MCRuntimeBudgetReplayNonOop.tla` (non-OOP arm) | 6 events | — | 0 | bounded budget 12, Σ valid = 9 | 908 distinct / 2,799 generated |
| `MCRuntimeBudgetReplayCap.tla` (bounded-K cap arm) | 6 events | — | 0 | bounded budget 12, trace cap 2 | 908 distinct / 2,575 generated |
| `CostAccountingThreats.tla` | 1 deploy boundary plus slash authorization view | — | 0 | bounded fuel 5, epochs 0..1, bonds 0..1 | 5,408 distinct / 401,025 generated |
| `CostAccountingSearchFrontier.tla` | 11 witness families | — | 0 | — | 34,167 distinct / 266,015 generated |
| `MergeableChannelAccounting.tla` | typed values over 2-bit bitmaps and bounded integers | — | 0 | bounded values 0..3 | 2,656 |
| `StateBoundAdmission.tla` | one authenticated deployment with bounded play, evidence commitment, constrained replay, and settlement phases | — | 0 | three event schedules, supply 4, fee 1 | 162 distinct / 162 generated |
| `StateBoundValidatorConvergence.tla` | 3 independent validators, 2 arrival orders, 3 reducer schedules, 2 roots, and 2 block contexts | — | 0 | local schedules include both permutations and a distinct event/cost trace; acceptance requires the exact certified witness | model-checker count recorded by the bounded runner |
| `LocatedStackConservation.tla` | one source purse, one located target, and bounded repeated transfer attempts | — | 0 | bounded source depth 0..4 and transfer depth 1..3 | model-checker count recorded by the bounded runner |
| `ParallelStackMaterialization.tla` | one payer, one current located purse, one continuation-local purse, forward execution, and replay | one parent COMM | 1 | four conserved cells across declaration, burn, and nested transfer | model-checker count recorded by the bounded runner; Apalache length 8 covers the complete safe horizon |
| `StateBoundFrontierExpansion.tla` | one deployment discovering a finite ordered pre-state authority frontier | — | 0 | three positive backing pools, one fee, and a trace that requires expansion | model-checker count recorded by the bounded runner |
| `ReplaySupplySnapshot.tla` | two sequential deployments with authenticated pre-state supply capture and trace-only replay | — | 0 | initial supply 9, two costs, two committed events | model-checker count recorded by the bounded runner |
| `ReplayRootMaterialization.tla` | producer, validator, and reporter with asymmetric initial root histories | — | 0 | three sequential deployments and four roots | model-checker count recorded by the bounded runner |
| `AtomicVaultSettlementRefinement.tla` | two independently selected paid branches, including a shared payer and a distinct located payer | — | 0 | three purses, two certified maxima, application transfer, physical burn, byte burn, and fee vectors | model-checker count recorded by the bounded runner |
| `NormalizerEnvironmentRefinement.tla` | certification, execution, and replay phases over authenticated versus empty environments | — | 0 | one deployer-ID-dependent program | model-checker count recorded by the bounded runner |
| `PhysicalSettlementWorklist.tla` | two independently scheduled physical allocators exploring a canonical finite candidate tree | — | 0 | event depth 3, binary candidates, native-stack bound 2 | model-checker count recorded by the bounded runner |
| `OslfLocatedTyping.tla` | one proof-check phase followed by two independently ordered located spends | 2 work surfaces / 6 observed surfaces | 0 | finite exact/upper-bound maps; one spend per work surface | model-checker count recorded by the bounded runner; Apalache length 3 covers the complete safe transition horizon |
| `VaultBackedByteAccounting.tla` | one vault purse, two introductions, one COMM, optional persistent delivery, top-up, settlement, and replay | one binary/join interaction | 0 | fixed reservation over bounded canonical byte footprints | model-checker count recorded by the bounded runner; eight unsafe controls exercise each excluded defect |
| `LocatedVaultByteSettlement.tla` | three independent purses, three byte events, an underfunded-continuation probe, concurrent top-up, settlement, and replay | outer and continuation surfaces plus one compound event | 0 | fixed per-purse reservations of 20, 9, and 20 units | 49 distinct / 82 generated / depth 12; four unsafe controls exercise envelope collapse, cross-purse rescue, top-up expansion, and replay misattribution |
| `StackIntroductionAtomicity.tla` | two independently scheduled stack productions, one byte rejection, one success, optional continuation failure, operation abort, enclosing-deployment rollback, retained attempt bytes, commit, and replay | two operations | 0 | three physical cells split 2+1 | model-checker count recorded by the bounded runner; six unsafe controls exercise prepared visibility, operation abort, birth, deployment rollback, nested-produce extraction, and replay |
| `EvaluationTransactionIsolation.tla` | parser rejection, reducer attempt/rejection, play validation, replay candidate-checkpoint publication, final-state validation, and merge-evidence publication followed by acceptance or rejection | one evaluation transaction | 0 | bounded work witness 2, active-state bit, and evidence bit | safe model plus five unsafe controls; Apalache length 5 reaches the final-state-gated evidence boundary |
| `ConcurrentEvaluationTransactionIsolation.tla` | two evaluations with independent parser, capture, reset, execution, checkpoint, validation, crash/restart, and publication phases over one append-only history repository | two evaluation transactions / two roots | 0 | bounded work witnesses and transaction-owned candidate roots | 511 distinct states at depth 21; eight unsafe controls; Apalache checks the split safe transition system and every named authority/rollback defect |
| `MergeableEvidenceAuthentication.tla` | two validators, two equivocations sharing a legacy key, arbitrary local replay and peer-response order, exact per-execution cache lookup, and exact fail-closed finalized-entry retirement over all parent paths | two execution identities / three key slots | 0 | two canonical evidence values plus one forged value | safe model plus five unsafe controls; Apalache length 8 covers every replay/peer action and opposite arrival orders, while the legacy-delete control reaches its counterexample at length 2 and the vacuous-latest and main-spine-only controls fail at the initial state |
| `BlockHeapLifecycle.tla` | two concurrent block slots with allocation, completion, bounded reclamation cadence, retained free heap, and an independent semantic reference | two slots / four blocks | 0 | two transient units per block and production interval `1` | safe model plus one required missing-boundary refutation; Apalache length 12 covers complete sequential and concurrent completion traces |
| `MultiShardResourceIsolation.tla` | four independently scheduled tasks mapped onto two shard-local ledgers and root histories with a shared two-worker pool, top-ups, stale retry, crash, and restart | four tasks / two shard runtime boundaries | 0 | initial funding 2 per shard and one-unit admitted commits | non-crash safety+liveness: 14,305 generated / 4,048 distinct states at depth 19; `MultiShardResourceIsolationCrash.cfg` separately explores crash/restart safety; five unsafe controls expose stale overwrite, foreign state, foreign root, foreign debit, and resource leakage |
| `FundingSlotBootstrap.tla` | installer-paid scaffold, sponsor-funded outer and continuation vaults, authenticated activation, settlement, and replay-independent terminal state | one grant / four purses | 0 | fixed scaffold cost, dual funding vector, and component-wise realized cost | safe model plus five required refutations; Apalache length 4 reaches complete activation and settlement while length 2 exposes partial debit and rejected target creation |

Running on larger bounds has not been attempted — doubly-compound depth-2 already exercises the cascading-Split + Join interactions and is the deepest scenario anticipated by the design.

### When to extend the models

Extend the TLA+ suite (rather than rely on Rocq alone) when introducing:

- **New atomic protocol actions** (e.g., Out-of-Phlogiston revert, checkpoint rollback interleaved with COMM). These are state-machine-shaped and are best captured in TLA+.
- **New concurrency scenarios** (e.g., shared channels with >2 processes per channel). Finite-state exhaustive search catches ordering bugs that Rocq inductive proofs may miss at the protocol level.
- **New invariants to cross-check** against the Rocq proofs. If a theorem's interpretation at the specification level is unclear, encoding it as a TLA+ invariant and model-checking a small instance is a fast sanity check.

Do **not** use TLA+ as a substitute for Rocq when:

- A property must hold for arbitrary configurations.
- The property concerns binding, substitution, or structural equivalence at a fine grain (the TLA+ models treat channel/process identifiers as opaque atoms).
