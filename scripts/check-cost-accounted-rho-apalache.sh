#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_ROOT="$ROOT/formal/tlaplus/cost_accounted_rho"
WORK_ROOT="$ROOT/target/verification/cost-accounted-rho/apalache"
mkdir -p "$WORK_ROOT"
APALACHE_TIMEOUT="${APALACHE_TIMEOUT:-1200}"
if [[ ! "$APALACHE_TIMEOUT" =~ ^[1-9][0-9]*[smhd]?$ ]]; then
  echo "error: APALACHE_TIMEOUT must be a positive timeout duration" >&2
  exit 2
fi

FILTER="${1:-}"
if [[ "$FILTER" == "--filter" ]]; then
  shift
  FILTER="${1:-}"
  shift || true
else
  FILTER=""
fi

if ! command -v apalache-mc >/dev/null 2>&1; then
  echo "error: apalache-mc is required for the cost-accounted-rho formal gate" >&2
  exit 1
fi

outdir="$(mktemp -d "$WORK_ROOT/run.XXXXXX")"
trap 'rm -rf "$outdir"' EXIT

checks_run=0

run_check() {
  local name="$1"
  local detail="$2"
  shift 2
  local output rc
  if [[ -n "$FILTER" && "$name" != *"$FILTER"* ]]; then
    return 0
  fi
  checks_run=$((checks_run + 1))
  output="$(cd "$MODEL_ROOT" && timeout "$APALACHE_TIMEOUT" apalache-mc --out-dir="$outdir/$name" check "$@" 2>&1)"
  rc=$?
  if [ "$rc" -eq 0 ] && grep -qE 'The outcome is: NoError|EXITCODE: OK' <<<"$output"; then
    echo "  PASS $name: $detail"
    return 0
  fi
  echo "  FAIL $name" >&2
  if [ "$rc" -eq 124 ]; then
    echo "  timed out after $APALACHE_TIMEOUT" >&2
  fi
  grep -iE 'error|violat|outcome|EXITCODE' <<<"$output" | tail -20 >&2
  return 1
}

run_expected_violation() {
  local name="$1"
  local detail="$2"
  local invariant="$3"
  shift 3
  local output rc
  if [[ -n "$FILTER" && "$name" != *"$FILTER"* ]]; then
    return 0
  fi
  checks_run=$((checks_run + 1))
  output="$(cd "$MODEL_ROOT" && timeout "$APALACHE_TIMEOUT" apalache-mc --out-dir="$outdir/$name" check "$@" 2>&1)"
  rc=$?
  if [ "$rc" -ne 0 ] \
     && grep -q "found INVARIANTS: $invariant" <<<"$output" \
     && grep -qE 'state invariant [0-9]+ violated' <<<"$output" \
     && grep -q 'The outcome is: Error' <<<"$output"; then
    echo "  PASS $name: $detail"
    return 0
  fi
  echo "  FAIL $name" >&2
  if [ "$rc" -eq 124 ]; then
    echo "  timed out after $APALACHE_TIMEOUT" >&2
  fi
  grep -iE 'error|violat|outcome|EXITCODE|INVARIANTS' <<<"$output" | tail -20 >&2
  return 1
}

echo "Checking cost-accounted rho with Apalache 0.58.3+..."

overall=0
run_check replay-cache-context \
  "concurrent cache lookup, publication, cancellation, eviction, and metadata removal preserve complete replay context" \
  --config=ReplayCacheContext.cfg --length=8 ReplayCacheContext.tla || overall=1
run_expected_violation replay-cache-context-omittimestamp \
  "reject the OmitTimestamp replay-cache defect" \
  ReplayUsesExactContext \
  --config=ReplayCacheContextOmitTimestampUnsafeApalache.cfg --length=8 ReplayCacheContext.tla || overall=1
run_expected_violation replay-cache-context-omitheight \
  "reject the OmitHeight replay-cache defect" \
  ReplayUsesExactContext \
  --config=ReplayCacheContextOmitHeightUnsafeApalache.cfg --length=8 ReplayCacheContext.tla || overall=1
run_expected_violation replay-cache-context-omitinvalidblocks \
  "reject the OmitInvalidBlocks replay-cache defect" \
  ReplayUsesExactContext \
  --config=ReplayCacheContextOmitInvalidBlocksUnsafeApalache.cfg --length=8 ReplayCacheContext.tla || overall=1
run_expected_violation replay-cache-context-ignorehostlimit \
  "reject the IgnoreHostLimit replay-cache defect" \
  BoundedWorkCannotHitCache \
  --config=ReplayCacheContextIgnoreHostLimitUnsafeApalache.cfg --length=8 ReplayCacheContext.tla || overall=1
run_expected_violation replay-cache-context-ignoremetadata \
  "reject the IgnoreMetadata replay-cache defect" \
  HitHadMetadata \
  --config=ReplayCacheContextIgnoreMetadataUnsafeApalache.cfg --length=8 ReplayCacheContext.tla || overall=1
run_expected_violation replay-cache-context-publishunvalidated \
  "reject the PublishUnvalidated replay-cache defect" \
  NoUnvalidatedPublication \
  --config=ReplayCacheContextPublishUnvalidatedUnsafeApalache.cfg --length=8 ReplayCacheContext.tla || overall=1
run_check nary-join \
  "symbolic authority conservation, partition invariance, and no weakening" \
  --init=Init --next=Next --inv=Inv --length=1 NaryJoin.tla || overall=1
run_check threats \
  "bounded replay, settlement, evidence, and slash-authorization safety" \
  --config=CostAccountingThreats.cfg MCCostAccountingThreats.tla || overall=1
run_check search-frontier \
  "bounded witness classification and promotion discipline" \
  --config=CostAccountingSearchFrontier.cfg CostAccountingSearchFrontier.tla || overall=1
run_check replay-root \
  "two-validator, two-deploy root materialization and replay agreement through length 8" \
  --config=ReplayRootMaterializationApalache.cfg --length=8 ReplayRootMaterialization.tla || overall=1
run_check replay-supply-snapshot \
  "maximal-prefix admission and independent root-bound replay preserve exact validator fuel settlement" \
  --config=ReplaySupplySnapshotApalache.cfg --length=12 MCReplaySupplySnapshot.tla || overall=1
run_expected_violation replay-supply-live-query-unsafe \
  "querying validator fuel after replay rigging is independently refuted" \
  ExactRecordedReplayTrace \
  --config=ReplaySupplySnapshotLiveQueryUnsafeApalache.cfg --length=8 MCReplaySupplySnapshot.tla || overall=1
run_expected_violation replay-supply-replay-runtime-unsafe \
  "capturing validator fuel through ReplayRSpace is independently refuted" \
  SnapshotsUseOrdinaryRuntime \
  --config=ReplaySupplySnapshotReplayRuntimeUnsafeApalache.cfg --length=6 MCReplaySupplySnapshot.tla || overall=1
run_expected_violation replay-supply-late-capture-unsafe \
  "capturing validator fuel after certificate rigging is independently refuted" \
  SnapshotsPrecedeRigging \
  --config=ReplaySupplySnapshotLateCaptureUnsafeApalache.cfg --length=6 MCReplaySupplySnapshot.tla || overall=1
run_expected_violation replay-supply-wrong-root-unsafe \
  "capturing validator fuel from a different state root is independently refuted" \
  SnapshotsUseCurrentRoot \
  --config=ReplaySupplySnapshotWrongRootUnsafeApalache.cfg --length=6 MCReplaySupplySnapshot.tla || overall=1
run_expected_violation replay-supply-wrong-proposer-unsafe \
  "binding validator fuel to a different proposer is independently refuted" \
  SnapshotsUseCertifiedProposer \
  --config=ReplaySupplySnapshotWrongProposerUnsafeApalache.cfg --length=6 MCReplaySupplySnapshot.tla || overall=1
run_expected_violation replay-supply-recorded-balance-unsafe \
  "reusing the recorded initial balance at a later root is independently refuted" \
  SnapshotsMatchActualFuel \
  --config=ReplaySupplySnapshotRecordedBalanceUnsafeApalache.cfg --length=10 MCReplaySupplySnapshot.tla || overall=1
run_expected_violation replay-supply-no-settlement-unsafe \
  "trusting a snapshot without applying physical settlement is independently refuted" \
  ActualSettlementRequired \
  --config=ReplaySupplySnapshotNoSettlementUnsafeApalache.cfg --length=8 MCReplaySupplySnapshot.tla || overall=1
run_expected_violation replay-supply-deferred-burn-unsafe \
  "burning validator fuel for a deferred candidate is independently refuted" \
  DeferredCandidatesDoNotBurn \
  --config=ReplaySupplySnapshotDeferredBurnUnsafeApalache.cfg --length=4 MCReplaySupplySnapshot.tla || overall=1
run_check execution-result-reuse \
  "validation reuses one exact user execution result and publishes it only after complete validation" \
  --config=ExecutionResultReuseApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-deploy-identity-unsafe \
  "reuse without the exact deploy identity is independently refuted" \
  ValidatedReuseIsExact \
  --config=ExecutionResultReuseOmitDeployIdentityUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-pre-state-unsafe \
  "reuse without the exact pre-state is independently refuted" \
  ValidatedReuseIsExact \
  --config=ExecutionResultReuseOmitPreStateUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-schedule-unsafe \
  "reuse without the exact schedule is independently refuted" \
  ValidatedReuseIsExact \
  --config=ExecutionResultReuseOmitScheduleUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-witness-unsafe \
  "reuse without the exact cost witness is independently refuted" \
  ValidatedReuseIsExact \
  --config=ExecutionResultReuseOmitWitnessUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-cost-surface-unsafe \
  "reuse without the exact cost surface is independently refuted" \
  ValidatedReuseIsExact \
  --config=ExecutionResultReuseOmitCostSurfaceUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-grade-unsafe \
  "reuse without the exact resource grade is independently refuted" \
  ValidatedReuseIsExact \
  --config=ExecutionResultReuseOmitGradeUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-context-unsafe \
  "reuse without the exact execution context is independently refuted" \
  ValidatedReuseIsExact \
  --config=ExecutionResultReuseOmitContextUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-user-post-state-unsafe \
  "reuse without the exact user post-state is independently refuted" \
  ValidatedReuseIsExact \
  --config=ExecutionResultReuseOmitUserPostStateUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-mergeable-unsafe \
  "reuse without the exact mergeable evidence is independently refuted" \
  ValidatedReuseIsExact \
  --config=ExecutionResultReuseOmitMergeableUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-cache-first-unsafe \
  "cache publication before durable validation is independently refuted" \
  CacheFollowsExactDurability \
  --config=ExecutionResultReuseCacheBeforeValidationUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-duplicate-user-replay-unsafe \
  "a second user execution during validation is independently refuted" \
  UserExecutionOccursOnce \
  --config=ExecutionResultReuseDuplicateUserReplayUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-persistent-reinstall-unsafe \
  "charging a persistent listener after its first installation is independently refuted" \
  PersistentIntroductionChargedOnce \
  --config=ExecutionResultReuseRechargePersistentIntroductionUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_expected_violation execution-result-reuse-persistent-firing-collapse-unsafe \
  "collapsing distinct persistent firings into one charge is independently refuted" \
  PersistentFiringsChargedSeparately \
  --config=ExecutionResultReuseCollapsePersistentFiringsUnsafeApalache.cfg --length=8 ExecutionResultReuse.tla || overall=1
run_check checkpoint-admission-recertification \
  "parallel validators recertify each raw-prefix attempt and publish one complete final partition" \
  --config=CheckpointAdmissionRecertificationApalache.cfg --length=8 CheckpointAdmissionRecertification.tla || overall=1
run_expected_violation checkpoint-reuse-old-certificate-unsafe \
  "reusing an earlier generation certificate after shrink is independently refuted" \
  CertificateUsesCurrentGeneration \
  --config=CheckpointAdmissionRecertificationReuseOldCertificateUnsafeApalache.cfg --length=3 CheckpointAdmissionRecertification.tla || overall=1
run_expected_violation checkpoint-preserve-old-rejections-unsafe \
  "retaining rejection decisions from an earlier candidate window is independently refuted" \
  CertifiedPartitionIsCompleteAndDisjoint \
  --config=CheckpointAdmissionRecertificationPreserveOldRejectionsUnsafeApalache.cfg --length=3 CheckpointAdmissionRecertification.tla || overall=1
run_expected_violation checkpoint-drop-final-rejections-unsafe \
  "dropping rejections from the successful candidate window is independently refuted" \
  PublishedPartitionIsCompleteAndDisjoint \
  --config=CheckpointAdmissionRecertificationDropFinalRejectionsUnsafeApalache.cfg --length=2 CheckpointAdmissionRecertification.tla || overall=1
run_expected_violation checkpoint-recertify-admitted-only-unsafe \
  "recertifying only previously admitted deploys is independently refuted" \
  CertifiedPartitionIsCompleteAndDisjoint \
  --config=CheckpointAdmissionRecertificationRecertifyAcceptedOnlyUnsafeApalache.cfg --length=3 CheckpointAdmissionRecertification.tla || overall=1
run_expected_violation checkpoint-context-drift-unsafe \
  "changing authenticated context between certification and execution is independently refuted" \
  CertificateUsesFrozenContext \
  --config=CheckpointAdmissionRecertificationContextDriftUnsafeApalache.cfg --length=1 CheckpointAdmissionRecertification.tla || overall=1
run_expected_violation checkpoint-shrink-initially-admitted-unsafe \
  "shrinking a prior admitted set instead of the raw canonical prefix is independently refuted" \
  CertifiedWindowIsCanonicalRawPrefix \
  --config=CheckpointAdmissionRecertificationShrinkInitiallyAdmittedUnsafeApalache.cfg --length=3 CheckpointAdmissionRecertification.tla || overall=1
run_expected_violation checkpoint-drain-original-candidates-unsafe \
  "draining candidates outside the successful terminal partition is independently refuted" \
  StorageDrainsOnlyFinalTerminalUsers \
  --config=CheckpointAdmissionRecertificationDrainOriginalCandidatesUnsafeApalache.cfg --length=4 CheckpointAdmissionRecertification.tla || overall=1
run_expected_violation checkpoint-publish-before-success-unsafe \
  "publishing or settling a failed checkpoint attempt is independently refuted" \
  FailedAttemptsPublishNothing \
  --config=CheckpointAdmissionRecertificationPublishBeforeSuccessUnsafeApalache.cfg --length=2 CheckpointAdmissionRecertification.tla || overall=1
run_expected_violation checkpoint-deferred-as-rejected-unsafe \
  "publishing deferred candidates as rejected is independently refuted" \
  PeerRecomputationMatchesPublication \
  --config=CheckpointAdmissionRecertificationTreatDeferredAsRejectedUnsafeApalache.cfg --length=2 CheckpointAdmissionRecertification.tla || overall=1
run_expected_violation checkpoint-arrival-order-unsafe \
  "using arrival order instead of canonical deploy order is independently refuted" \
  CertifiedWindowIsCanonicalRawPrefix \
  --config=CheckpointAdmissionRecertificationArrivalOrderUnsafeApalache.cfg --length=1 CheckpointAdmissionRecertification.tla || overall=1
run_expected_violation checkpoint-shared-attempt-unsafe \
  "sharing mutable retry state between validators is independently refuted" \
  ValidatorAttemptStateIsIndependent \
  --config=CheckpointAdmissionRecertificationSharedMutableAttemptUnsafeApalache.cfg --length=1 CheckpointAdmissionRecertification.tla || overall=1
run_expected_violation checkpoint-nondecreasing-retry-unsafe \
  "a retry that does not reduce its user window is independently refuted" \
  RetryLimitsStrictlyDecrease \
  --config=CheckpointAdmissionRecertificationNonDecreasingRetryUnsafeApalache.cfg --length=2 CheckpointAdmissionRecertification.tla || overall=1
run_check parallel-stack-materialization \
  "same-configuration declaration barrier, nested causality, conservation, and replay agreement" \
  --config=ParallelStackMaterialization.cfg --length=8 ParallelStackMaterialization.tla || overall=1
run_check canonical-custody-aliasing \
  "shared physical purse capacity, logical attribution, stack authority, and concurrent replay agreement" \
  --config=CanonicalCustodyAliasingApalache.cfg --length=8 MCCanonicalCustodyAliasing.tla || overall=1
run_expected_violation canonical-custody-aliasing-duplicate-lane-capacity-unsafe \
  "duplicating one physical purse across logical authority lanes is independently refuted" \
  NoDoubleCapacity \
  --config=CanonicalCustodyAliasingDuplicateLaneCapacityUnsafeApalache.cfg --length=8 MCCanonicalCustodyAliasing.tla || overall=1
run_check bond-issuance-lifecycle \
  "genesis-only initial allocation and guarded epoch issuance across concurrent validator lifecycles" \
  --config=BondIssuanceLifecycleApalache.cfg --length=10 MC_BondIssuanceLifecycle.tla || overall=1
run_expected_violation bond-issuance-fresh-bond-subsidy-unsafe \
  "an initial grant after a fresh bond is independently refuted" \
  InitialIssuanceOccursOnlyAtGenesis \
  --config=BondIssuanceLifecycleFreshBondSubsidyUnsafeApalache.cfg --length=2 MC_BondIssuanceLifecycle.tla || overall=1
run_expected_violation bond-issuance-rebond-subsidy-unsafe \
  "an initial grant after rebonding is independently refuted" \
  InitialIssuanceOccursOnlyAtGenesis \
  --config=BondIssuanceLifecycleRebondSubsidyUnsafeApalache.cfg --length=5 MC_BondIssuanceLifecycle.tla || overall=1
run_expected_violation bond-issuance-inactive-validator-unsafe \
  "epoch issuance to an inactive validator is independently refuted" \
  EpochIssuanceRequiresActiveMembership \
  --config=BondIssuanceLifecycleInactiveValidatorIssuanceUnsafeApalache.cfg --length=2 MC_BondIssuanceLifecycle.tla || overall=1
run_expected_violation bond-issuance-off-boundary-unsafe \
  "epoch issuance outside an epoch boundary is independently refuted" \
  EpochIssuanceRequiresBoundary \
  --config=BondIssuanceLifecycleOffBoundaryIssuanceUnsafeApalache.cfg --length=1 MC_BondIssuanceLifecycle.tla || overall=1
run_expected_violation bond-issuance-halted-validator-unsafe \
  "epoch issuance to a halted validator is independently refuted" \
  HaltedValidatorsReceiveNoIssuance \
  --config=BondIssuanceLifecycleHaltedValidatorIssuanceUnsafeApalache.cfg --length=4 MC_BondIssuanceLifecycle.tla || overall=1
run_expected_violation bond-issuance-duplicate-epoch-unsafe \
  "a second issuance for one validator epoch is independently refuted" \
  AtMostOneCreditPerValidatorEpoch \
  --config=BondIssuanceLifecycleDuplicateEpochIssuanceUnsafeApalache.cfg --length=3 MC_BondIssuanceLifecycle.tla || overall=1
run_expected_violation bond-issuance-generation-epoch-unsafe \
  "changing bond generation during epoch issuance is independently refuted" \
  GenerationChangesOnlyOnSuccessfulBond \
  --config=BondIssuanceLifecycleGenerationDuringEpochUnsafeApalache.cfg --length=2 MC_BondIssuanceLifecycle.tla || overall=1
run_expected_violation bond-issuance-play-only-subsidy-unsafe \
  "a play-only bond subsidy is independently refuted" \
  PlayReplayIssuanceAgree \
  --config=BondIssuanceLifecyclePlayOnlySubsidyUnsafeApalache.cfg --length=2 MC_BondIssuanceLifecycle.tla || overall=1
run_expected_violation bond-issuance-replay-only-subsidy-unsafe \
  "a replay-only bond subsidy is independently refuted" \
  PlayReplayIssuanceAgree \
  --config=BondIssuanceLifecycleReplayOnlySubsidyUnsafeApalache.cfg --length=2 MC_BondIssuanceLifecycle.tla || overall=1
run_check epoch-mint-atomicity \
  "fallible multi-validator epoch minting publishes one complete state or the exact pre-state" \
  --config=EpochMintAtomicityApalache.cfg --length=10 MC_EpochMintAtomicity.tla || overall=1
run_check epoch-mint-zero-issuance \
  "zero epoch issuance records completion without calling the positive-only mint primitive" \
  --config=EpochMintAtomicityZeroApalache.cfg --length=8 MC_EpochMintAtomicity.tla || overall=1
run_expected_violation epoch-mint-swallowed-failure-unsafe \
  "swallowing one validator mint failure is independently refuted" \
  CommitRequiresAllEligibleMints \
  --config=EpochMintAtomicitySwallowFailureUnsafeApalache.cfg --length=8 MC_EpochMintAtomicity.tla || overall=1
run_expected_violation epoch-mint-early-balance-unsafe \
  "publishing a validator balance before epoch commit is independently refuted" \
  NoPartialBalancePublication \
  --config=EpochMintAtomicityEarlyBalanceUnsafeApalache.cfg --length=3 MC_EpochMintAtomicity.tla || overall=1
run_expected_violation epoch-mint-early-receipt-unsafe \
  "publishing a mint receipt before epoch commit is independently refuted" \
  NoPartialReceiptPublication \
  --config=EpochMintAtomicityEarlyReceiptUnsafeApalache.cfg --length=3 MC_EpochMintAtomicity.tla || overall=1
run_expected_violation epoch-mint-missing-rollback-unsafe \
  "retaining staged epoch state after failure is independently refuted" \
  AbortRestoresCommittedState \
  --config=EpochMintAtomicityMissingRuntimeRollbackUnsafeApalache.cfg --length=5 MC_EpochMintAtomicity.tla || overall=1
run_expected_violation epoch-mint-replay-partial-unsafe \
  "replaying only a prefix of validator mints is independently refuted" \
  PlayReplayCommittedStateEqual \
  --config=EpochMintAtomicityReplayPartialUnsafeApalache.cfg --length=8 MC_EpochMintAtomicity.tla || overall=1
run_expected_violation epoch-mint-zero-strict-mint-unsafe \
  "sending zero issuance to a positive-only mint primitive is independently refuted" \
  ZeroIssuanceCompletesWithoutFailure \
  --config=EpochMintAtomicityZeroCallsStrictMintUnsafeApalache.cfg --length=4 MC_EpochMintAtomicity.tla || overall=1
run_check validator-economics-lifecycle \
  "role-separated validator fuel, general custody, stake, issuance, settlement, slash, replay, and concurrent reservations" \
  --config=ValidatorEconomicsLifecycleApalache.cfg --length=4 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-address-role-collapse-unsafe \
  "address-only custody identity is independently refuted" \
  AddressAndRoleIdentifyCustody \
  --config=ValidatorEconomicsLifecycleAddressOnlyRoleCollapseUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-burn-supply-unsafe \
  "burning custody without reducing circulating supply is independently refuted" \
  BurnReducesCirculatingSupply \
  --config=ValidatorEconomicsLifecycleBurnWithoutSupplyReductionUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-general-capacity-unsafe \
  "using general custody for proposer capacity is independently refuted" \
  CapacityUsesValidatorFuel \
  --config=ValidatorEconomicsLifecycleCapacityFromGeneralUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-redemption-mint-unsafe \
  "creating validator fuel during redemption is independently refuted" \
  RedemptionCreatesNoFuel \
  --config=ValidatorEconomicsLifecycleDirectRedemptionMintUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-duplicate-penalty-unsafe \
  "applying a guilty penalty twice is independently refuted" \
  GuiltyResolutionIsIdempotent \
  --config=ValidatorEconomicsLifecycleDuplicateGuiltyPenaltyUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-epoch-general-unsafe \
  "crediting epoch issuance to general custody is independently refuted" \
  EpochCreditsValidatorFuel \
  --config=ValidatorEconomicsLifecycleEpochToGeneralUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-fee-fuel-unsafe \
  "crediting client fees to validator fuel is independently refuted" \
  FeeCreditsGeneralCustody \
  --config=ValidatorEconomicsLifecycleFeeToFuelUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-fresh-bond-subsidy-unsafe \
  "creating validator fuel during a fresh bond is independently refuted" \
  FreshBondCreatesNoFuel \
  --config=ValidatorEconomicsLifecycleFreshBondSubsidyUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-fuel-withdrawal-unsafe \
  "withdrawing validator fuel into general custody is independently refuted" \
  FuelHasNoWithdrawalPath \
  --config=ValidatorEconomicsLifecycleFuelToGeneralWithdrawalUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-global-lock-unsafe \
  "serializing independent validator economics behind one global lock is independently refuted" \
  ValidatorOperationsRemainIndependent \
  --config=ValidatorEconomicsLifecycleGlobalValidatorEconomicsLockUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-handler-general-unsafe \
  "charging validator handlers from general custody is independently refuted" \
  HandlerUsesValidatorFuel \
  --config=ValidatorEconomicsLifecycleHandlerFromGeneralUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-partial-epoch-unsafe \
  "publishing only part of an epoch issuance is independently refuted" \
  EpochPublicationIsAtomic \
  --config=ValidatorEconomicsLifecyclePartialEpochPublicationUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-partial-slash-unsafe \
  "publishing only part of a slash resolution is independently refuted" \
  SlashResolutionIsAtomic \
  --config=ValidatorEconomicsLifecyclePartialSlashRedemptionUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-rebond-subsidy-unsafe \
  "creating validator fuel during rebonding is independently refuted" \
  RebondCreatesNoFuel \
  --config=ValidatorEconomicsLifecycleRebondSubsidyUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-replay-role-unsafe \
  "substituting a custody role during replay is independently refuted" \
  PlayReplayRolesAgree \
  --config=ValidatorEconomicsLifecycleReplayRoleSubstitutionUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-self-funding-unsafe \
  "using same-candidate top-up for its handler is independently refuted" \
  CandidateCannotSelfFundHandler \
  --config=ValidatorEconomicsLifecycleSameCandidateTopUpUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-sibling-overdraw-unsafe \
  "accepting sibling reservations beyond aggregate fuel is independently refuted" \
  SiblingReservationsAreBounded \
  --config=ValidatorEconomicsLifecycleSiblingAggregateFuelOverdrawUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-slash-general-unsafe \
  "quarantining general custody during slash is independently refuted" \
  SlashPreservesGeneralCustody \
  --config=ValidatorEconomicsLifecycleSlashGeneralUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-slash-fuel-unsafe \
  "leaving validator fuel available after slash is independently refuted" \
  SlashRemovesAvailableFuel \
  --config=ValidatorEconomicsLifecycleSlashLeavesFuelUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-stale-generation-unsafe \
  "resolving validator fuel with stale generation evidence is independently refuted" \
  ResolutionUsesCurrentGeneration \
  --config=ValidatorEconomicsLifecycleStaleGenerationResolutionUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_expected_violation validator-economics-top-up-conservation-unsafe \
  "crediting validator fuel without the matching general debit is independently refuted" \
  TopUpConservesCustody \
  --config=ValidatorEconomicsLifecycleTopUpWithoutGeneralDebitUnsafeApalache.cfg --length=3 MC_ValidatorEconomicsLifecycle.tla || overall=1
run_check minted-epoch-frontier-bounded \
  "all epoch values and lifecycle transitions preserve one retained frontier from genesis" \
  --config=MintedEpochFrontierApalache.cfg --length=4 MintedEpochFrontier.tla || overall=1
run_check minted-epoch-frontier-inductive \
  "the frontier invariant is closed under every transition from every invariant state" \
  --config=MintedEpochFrontierInductiveApalache.cfg --length=1 MintedEpochFrontier.tla || overall=1
run_expected_violation minted-epoch-frontier-init-zero-unsafe \
  "initializing the retained frontier as completed epoch zero is independently refuted" \
  InitialFrontierIsUnminted \
  --config=MintedEpochFrontierInitAtZeroUnsafe.cfg --length=0 MintedEpochFrontier.tla || overall=1
run_expected_violation minted-epoch-frontier-reject-bootstrap-unsafe \
  "rejecting the first production epoch from an unminted genesis is independently refuted" \
  RequiredBootstrapIsAccepted \
  --config=MintedEpochFrontierRejectBootstrapUnsafe.cfg --length=1 MintedEpochFrontier.tla || overall=1
run_expected_violation minted-epoch-frontier-gap-unsafe \
  "accepting a close beyond the immediate successor epoch is independently refuted" \
  GapCloseIsRejected \
  --config=MintedEpochFrontierGapUnsafe.cfg --length=1 MintedEpochFrontier.tla || overall=1
run_expected_violation minted-epoch-frontier-double-sibling-unsafe \
  "publishing both sibling epoch-close effects is independently refuted" \
  SiblingCloseKeepsExactlyOne \
  --config=MintedEpochFrontierDoubleSiblingUnsafe.cfg --length=1 MintedEpochFrontier.tla || overall=1
run_expected_violation minted-epoch-frontier-drop-sibling-unsafe \
  "dropping every sibling epoch-close effect is independently refuted" \
  SiblingCloseKeepsExactlyOne \
  --config=MintedEpochFrontierDropSiblingUnsafe.cfg --length=1 MintedEpochFrontier.tla || overall=1
run_expected_violation minted-epoch-frontier-catchup-bond-unsafe \
  "retroactively minting for a new bond is independently refuted" \
  LifecycleDoesNotMint \
  --config=MintedEpochFrontierCatchupBondUnsafe.cfg --length=2 MintedEpochFrontier.tla || overall=1
run_expected_violation minted-epoch-frontier-clear-redemption-unsafe \
  "clearing the retained frontier during redemption is independently refuted" \
  LifecyclePreservesFrontier \
  --config=MintedEpochFrontierClearRedemptionUnsafe.cfg --length=2 MintedEpochFrontier.tla || overall=1
run_expected_violation minted-epoch-frontier-retroactive-redemption-unsafe \
  "retroactively minting during redemption is independently refuted" \
  LifecycleDoesNotMint \
  --config=MintedEpochFrontierRetroactiveMintUnsafe.cfg --length=2 MintedEpochFrontier.tla || overall=1
run_expected_violation parallel-stack-materialization-unsafe \
  "scheduler-dependent sibling reduction before purse materialization is independently refuted" \
  CausallyFundedProgramIsAccepted \
  --config=ParallelStackMaterializationUnsafe.cfg --length=1 ParallelStackMaterialization.tla || overall=1
run_check oslf-located \
  "finite located spatial/modal checking through both independent spends" \
  --config=OslfLocatedTyping.cfg --length=3 OslfLocatedTyping.tla || overall=1
run_expected_violation oslf-contraction-unsafe \
  "linear contraction is independently refuted" \
  LinearNoContraction \
  --config=OslfLocatedTypingContractionUnsafe.cfg --length=0 OslfLocatedTyping.tla || overall=1
run_expected_violation oslf-weakening-unsafe \
  "linear weakening is independently refuted" \
  LinearNoWeakening \
  --config=OslfLocatedTypingWeakeningUnsafe.cfg --length=0 OslfLocatedTyping.tla || overall=1
run_expected_violation oslf-alias-unsafe \
  "cross-surface debit aliasing is independently refuted" \
  LocationIsolation \
  --config=OslfLocatedTypingAliasUnsafe.cfg --length=2 OslfLocatedTyping.tla || overall=1
run_expected_violation oslf-upper-modal-unsafe \
  "treating a conservative upper bound as exact modal evidence is independently refuted" \
  ModalEvidenceSound \
  --config=OslfLocatedTypingUpperModalUnsafe.cfg --length=1 OslfLocatedTyping.tla || overall=1
run_expected_violation oslf-candidate-credit-unsafe \
  "crediting candidate-created supply during authenticated funding is independently refuted" \
  AuthenticatedFundingOnly \
  --config=OslfLocatedTypingCandidateCreditUnsafe.cfg --length=1 OslfLocatedTyping.tla || overall=1
run_check atomic-vault-application-composition \
  "application transfers, physical burn, byte burn, and fees share one aggregate vault bound" \
  --config=MCAtomicVaultSettlementRefinement.cfg --length=4 MCAtomicVaultSettlementRefinement.tla || overall=1
run_expected_violation atomic-vault-application-debit-omission-unsafe \
  "checking aggregate solvency without the application debit is independently refuted" \
  FinalizedAggregateIsFunded \
  --config=AtomicVaultSettlementRefinementApplicationDebitOmissionUnsafe.cfg --length=3 MCAtomicVaultSettlementRefinement.tla || overall=1
run_expected_violation atomic-vault-physical-burn-omission-unsafe \
  "checking aggregate solvency without physical settlement is independently refuted" \
  FinalizedAggregateIsFunded \
  --config=AtomicVaultSettlementRefinementPhysicalBurnOmissionUnsafe.cfg --length=3 MCAtomicVaultSettlementRefinement.tla || overall=1
run_expected_violation atomic-vault-byte-burn-omission-unsafe \
  "checking aggregate solvency without byte settlement is independently refuted" \
  FinalizedAggregateIsFunded \
  --config=AtomicVaultSettlementRefinementByteBurnOmissionUnsafe.cfg --length=3 MCAtomicVaultSettlementRefinement.tla || overall=1
run_expected_violation atomic-vault-fee-omission-unsafe \
  "checking aggregate solvency without fee settlement is independently refuted" \
  FinalizedAggregateIsFunded \
  --config=AtomicVaultSettlementRefinementFeeOmissionUnsafe.cfg --length=3 MCAtomicVaultSettlementRefinement.tla || overall=1
run_check vault-byte-accounting \
  "REV-backed byte tariffs, fixed reservation, atomic rejection, persistence, top-up isolation, and replay" \
  --config=VaultBackedByteAccountingApalache.cfg --length=16 VaultBackedByteAccounting.tla || overall=1
run_expected_violation vault-byte-charge-after-mutation-unsafe \
  "mutation before an unaffordable byte debit is independently refuted" \
  RejectedAttemptIsAtomic \
  --config=VaultBackedByteAccountingChargeAfterMutationUnsafe.cfg --length=2 VaultBackedByteAccounting.tla || overall=1
run_expected_violation vault-byte-trigger-dependent-unsafe \
  "arrival-side-dependent byte charging is independently refuted" \
  ExactCanonicalDebit \
  --config=VaultBackedByteAccountingTriggerDependentUnsafe.cfg --length=4 VaultBackedByteAccounting.tla || overall=1
run_expected_violation vault-byte-join-omission-unsafe \
  "omitting one join participant from transfer accounting is independently refuted" \
  ExactCanonicalDebit \
  --config=VaultBackedByteAccountingOmitJoinParticipantUnsafe.cfg --length=4 VaultBackedByteAccounting.tla || overall=1
run_expected_violation vault-byte-persistent-recharge-unsafe \
  "recharging the original persistent introduction on a repeated delivery is independently refuted" \
  ExactCanonicalDebit \
  --config=VaultBackedByteAccountingRechargePersistentUnsafe.cfg --length=6 VaultBackedByteAccounting.tla || overall=1
run_expected_violation vault-byte-peek-credit-unsafe \
  "crediting a previously charged introduction after a peek is independently refuted" \
  NoRemovalCredit \
  --config=VaultBackedByteAccountingPeekCreditUnsafe.cfg --length=3 VaultBackedByteAccounting.tla || overall=1
run_expected_violation vault-byte-replay-omission-unsafe \
  "omitting committed trace bytes during replay is independently refuted" \
  ReplayPrefixExact \
  --config=VaultBackedByteAccountingReplayOmissionUnsafe.cfg --length=12 VaultBackedByteAccounting.tla || overall=1
run_expected_violation vault-byte-top-up-expansion-unsafe \
  "allowing a concurrent top-up to enlarge an in-flight reservation is independently refuted" \
  ReservationSnapshotImmutable \
  --config=VaultBackedByteAccountingTopUpExpandsBoundUnsafe.cfg --length=2 VaultBackedByteAccounting.tla || overall=1
run_expected_violation vault-byte-overflow-wrap-unsafe \
  "wrapping byte arithmetic instead of rejecting overflow is independently refuted" \
  ExactCanonicalDebit \
  --config=VaultBackedByteAccountingOverflowWrapUnsafe.cfg --length=4 VaultBackedByteAccounting.tla || overall=1
run_check located-vault-byte-settlement \
  "located and compound byte draws, fixed per-purse reservations, top-up isolation, and exact replay" \
  --config=LocatedVaultByteSettlementApalache.cfg --length=12 LocatedVaultByteSettlement.tla || overall=1
run_expected_violation located-vault-byte-envelope-collapse-unsafe \
  "collapsing a located byte draw into the deploy envelope is independently refuted" \
  ExactLocatedDebit \
  --config=LocatedVaultByteSettlementEnvelopeCollapseUnsafe.cfg --length=2 LocatedVaultByteSettlement.tla || overall=1
run_expected_violation located-vault-byte-cross-purse-rescue-unsafe \
  "using outer-purse surplus to rescue an underfunded continuation is independently refuted" \
  UnderfundedContinuationCannotUseAnotherPurse \
  --config=LocatedVaultByteSettlementCrossPurseRescueUnsafe.cfg --length=2 LocatedVaultByteSettlement.tla || overall=1
run_expected_violation located-vault-byte-top-up-expansion-unsafe \
  "allowing a top-up to expand an in-flight local reservation is independently refuted" \
  ReservationSnapshotImmutable \
  --config=LocatedVaultByteSettlementTopUpExpandsReservationUnsafe.cfg --length=2 LocatedVaultByteSettlement.tla || overall=1
run_expected_violation located-vault-byte-replay-envelope-unsafe \
  "replaying a located byte event against the deploy envelope is independently refuted" \
  ReplayAllocationIsExact \
  --config=LocatedVaultByteSettlementReplayUsesEnvelopeUnsafe.cfg --length=10 LocatedVaultByteSettlement.tla || overall=1
run_check wallet-lollipop \
  "atomic two-purse funding, post-funding activation, located certification, exact settlement, and replay" \
  --config=WalletFundedLollipopApalache.cfg --length=10 MCWalletFundedLollipop.tla || overall=1
run_expected_violation wallet-lollipop-funding-copy-unsafe \
  "copying wallet funds into located purses instead of transferring custody is independently refuted" \
  CanonicalCustodyConserved \
  --config=WalletFundedLollipopFundingCopyUnsafe.cfg --length=2 MCWalletFundedLollipop.tla || overall=1
run_expected_violation wallet-lollipop-capability-leak-unsafe \
  "leaking the private draw capability with public funding addresses is independently refuted" \
  FundingUsesAddressesWithoutDelegatingDraw \
  --config=WalletFundedLollipopCapabilityLeakUnsafe.cfg --length=1 MCWalletFundedLollipop.tla || overall=1
run_expected_violation wallet-lollipop-payer-collapse-unsafe \
  "validator-local collapse of either located payer into the gateway purse is independently refuted" \
  CertifiedPayersAreLocatedPurses \
  --config=WalletFundedLollipopPayerCollapseUnsafe.cfg --length=6 MCWalletFundedLollipop.tla || overall=1
run_expected_violation wallet-lollipop-replay-omission-unsafe \
  "omitting either located debit from replay is independently refuted" \
  ReplayMatchesCommit \
  --config=WalletFundedLollipopReplayOmissionUnsafe.cfg --length=9 MCWalletFundedLollipop.tla || overall=1
run_expected_violation wallet-lollipop-missing-outer-unsafe \
  "activating a continuation without its outer linear authority is independently refuted" \
  ContinuationRequiresOuter \
  --config=WalletFundedLollipopMissingOuterUnsafe.cfg --length=4 MCWalletFundedLollipop.tla || overall=1
run_expected_violation wallet-lollipop-bound-charge-unsafe \
  "burning the certified maximum instead of the realized cost is independently refuted" \
  UnusedCertifiedBoundsAreRefunded \
  --config=WalletFundedLollipopBoundChargeUnsafe.cfg --length=8 MCWalletFundedLollipop.tla || overall=1
run_expected_violation wallet-lollipop-gateway-bypass-unsafe \
  "activating the lollipop continuation through an unauthenticated caller is independently refuted" \
  OnlyGatewayAuthorizesContinuation \
  --config=WalletFundedLollipopGatewayAuthBypassUnsafe.cfg --length=4 MCWalletFundedLollipop.tla || overall=1
run_expected_violation wallet-lollipop-activation-before-funding-unsafe \
  "installing the continuation before both located purses are funded is independently refuted" \
  ContinuationActivationRequiresFunding \
  --config=WalletFundedLollipopActivationBeforeFundingUnsafe.cfg --length=3 MCWalletFundedLollipop.tla || overall=1
run_check funding-slot-bootstrap \
  "installer-paid publication precedes atomic dual-purse funding and authenticated located activation" \
  --config=FundingSlotBootstrapApalache.cfg --length=4 MCFundingSlotBootstrap.tla || overall=1
run_expected_violation funding-slot-bootstrap-eager-install-unsafe \
  "eagerly persisting a located lollipop against zero pre-state purses is independently refuted" \
  InstallWorkflowIsAdmissible \
  --config=FundingSlotBootstrapEagerInstallUnsafe.cfg --length=1 MCFundingSlotBootstrap.tla || overall=1
run_expected_violation funding-slot-bootstrap-candidate-self-funding-unsafe \
  "candidate-created purse supply funding the same installation is independently refuted" \
  CandidateCreatedSupplyNeverFundsItsCreator \
  --config=FundingSlotBootstrapCandidateSelfFundingUnsafe.cfg --length=1 MCFundingSlotBootstrap.tla || overall=1
run_expected_violation funding-slot-bootstrap-slot-only-funding-unsafe \
  "marking a grant funded while its outer purse remains empty is independently refuted" \
  FundingCommitCoversEveryLocatedPurse \
  --config=FundingSlotBootstrapSlotOnlyFundingUnsafe.cfg --length=2 MCFundingSlotBootstrap.tla || overall=1
run_expected_violation funding-slot-bootstrap-partial-funding-unsafe \
  "committing the first purse when atomic two-purse funding fails is independently refuted" \
  RejectedFundingIsEffectFree \
  --config=FundingSlotBootstrapPartialFundingUnsafe.cfg --length=2 MCFundingSlotBootstrap.tla || overall=1
run_expected_violation funding-slot-bootstrap-rejected-target-creation-unsafe \
  "creating an empty target vault during rejected funding is independently refuted" \
  RejectedFundingIsEffectFree \
  --config=FundingSlotBootstrapRejectedTargetCreationUnsafe.cfg --length=2 MCFundingSlotBootstrap.tla || overall=1
run_check pos-vault-authority \
  "PoS human control is bound to the authenticated genesis deployer and preserves vault custody" \
  --config=PoSVaultAuthorityApalache.cfg --length=4 PoSVaultAuthority.tla || overall=1
run_expected_violation pos-vault-literal-control-unsafe \
  "binding PoS human control to a literal placeholder instead of the authenticated deployer is independently refuted" \
  InstalledBindsAuthenticatedKey \
  --config=PoSVaultAuthorityLiteralControlUnsafeApalache.cfg --length=2 PoSVaultAuthority.tla || overall=1
run_expected_violation pos-vault-unresolved-template-unsafe \
  "compiling a blessed contract with an unresolved template placeholder is independently refuted" \
  NoCompiledPlaceholder \
  --config=PoSVaultAuthorityUnresolvedTemplateUnsafeApalache.cfg --length=1 PoSVaultAuthority.tla || overall=1
run_check concurrent-redemption-custody \
  "the complete slash, resolution, retry, rejection, and rollback transaction horizon preserves lifecycle, generation, receipt, stake, and fuel invariants; TLC exhausts the full two-validator graph" \
  --config=ConcurrentRedemptionCustodyApalache.cfg --length=7 ConcurrentRedemptionCustody.tla || overall=1
run_expected_violation concurrent-redemption-no-lock-unsafe \
  "two same-incarnation resolutions without a per-validator transaction lock are independently refuted" \
  AtMostOneResolutionPerIncarnation \
  --config=ConcurrentRedemptionCustodyNoTargetLockUnsafeApalache.cfg --length=11 ConcurrentRedemptionCustody.tla || overall=1
run_expected_violation concurrent-redemption-stale-generation-unsafe \
  "accepting a redemption for a stale validator generation is independently refuted" \
  ReceiptsUseAuthorizedGeneration \
  --config=ConcurrentRedemptionCustodyIgnoreGenerationUnsafeApalache.cfg --length=6 ConcurrentRedemptionCustody.tla || overall=1
run_expected_violation concurrent-redemption-full-guilty-unsafe \
  "using a Guilty verdict for total stake confiscation instead of Burned is independently refuted" \
  GuiltyIsStrictlyPartial \
  --config=ConcurrentRedemptionCustodyFullGuiltyUnsafeApalache.cfg --length=6 ConcurrentRedemptionCustody.tla || overall=1
run_expected_violation concurrent-redemption-origin-collapse-unsafe \
  "restoring every quarantined lifecycle as Bonded is independently refuted" \
  RestoresExactLifecycle \
  --config=ConcurrentRedemptionCustodyRestoreBondedUnsafeApalache.cfg --length=6 ConcurrentRedemptionCustody.tla || overall=1
run_expected_violation concurrent-redemption-stake-checkpoint-unsafe \
  "publishing a staged stake disposition after transaction rejection is independently refuted" \
  RejectedTransactionsPublishNothing \
  --config=ConcurrentRedemptionCustodyCheckpointStakeUnsafeApalache.cfg --length=4 ConcurrentRedemptionCustody.tla || overall=1
run_expected_violation concurrent-redemption-fuel-checkpoint-unsafe \
  "publishing a staged fuel disposition after transaction rejection is independently refuted" \
  RejectedTransactionsPublishNothing \
  --config=ConcurrentRedemptionCustodyCheckpointFuelUnsafeApalache.cfg --length=5 ConcurrentRedemptionCustody.tla || overall=1
run_expected_violation concurrent-redemption-lost-receipt-unsafe \
  "reapplying an accepted Guilty disposition after losing its receipt is independently refuted" \
  ExactRetriesAreEffectFree \
  --config=ConcurrentRedemptionCustodyLostReceiptUnsafeApalache.cfg --length=7 ConcurrentRedemptionCustody.tla || overall=1
run_expected_violation concurrent-redemption-conflict-overwrite-unsafe \
  "overwriting an accepted receipt with a conflicting retry is independently refuted" \
  ConflictingRetriesAreEffectFree \
  --config=ConcurrentRedemptionCustodyOverwriteConflictUnsafeApalache.cfg --length=7 ConcurrentRedemptionCustody.tla || overall=1
run_check introduction-authority-registry \
  "fallback resolution and explicit registration linearize to one committed payer" \
  --config=IntroductionAuthorityRegistryApalache.cfg --length=2 MCIntroductionAuthorityRegistry.tla || overall=1
run_expected_violation introduction-authority-registry-split-fallback-unsafe \
  "returning an uncommitted fallback after an explicit registration is independently refuted" \
  ResolvedMatchesCommittedRegistry \
  --config=IntroductionAuthorityRegistrySplitFallbackUnsafe.cfg --length=3 MCIntroductionAuthorityRegistry.tla || overall=1
run_check stack-introduction-atomicity \
  "physical preparation, byte rejection, RSpace mutation, stack birth, abort, concurrency, and replay compose atomically" \
  --config=StackIntroductionAtomicityApalache.cfg --length=8 StackIntroductionAtomicity.tla || overall=1
run_expected_violation stack-introduction-exposed-preparation-unsafe \
  "exposing a prepared physical transfer before the RSpace produce is independently refuted" \
  PreparedAuthorityIsNotWitnessVisible \
  --config=StackIntroductionAtomicityExposePreparedUnsafe.cfg --length=1 StackIntroductionAtomicity.tla || overall=1
run_expected_violation stack-introduction-omitted-abort-unsafe \
  "retaining a prepared transfer after byte rejection is independently refuted" \
  RejectedOperationIsEffectFree \
  --config=StackIntroductionAtomicityOmitAbortUnsafe.cfg --length=2 StackIntroductionAtomicity.tla || overall=1
run_expected_violation stack-introduction-fallible-birth-unsafe \
  "committing RSpace and physical authority without the stack birth is independently refuted" \
  EveryCommittedStackHasOneBirth \
  --config=StackIntroductionAtomicityFallibleBirthUnsafe.cfg --length=4 StackIntroductionAtomicity.tla || overall=1
run_expected_violation stack-introduction-omitted-nested-produce-unsafe \
  "omitting a matched produce nested inside its COMM from the causal trace is independently refuted" \
  EveryCommittedStackIsCausallyExtracted \
  --config=StackIntroductionAtomicityOmitNestedProduceUnsafe.cfg --length=4 StackIntroductionAtomicity.tla || overall=1
run_expected_violation stack-introduction-omitted-deploy-rollback-unsafe \
  "retaining stack custody after the enclosing deploy rollback is independently refuted" \
  FailedDeployHasNoLinearEffects \
  --config=StackIntroductionAtomicityOmitDeployRollbackUnsafe.cfg --length=7 StackIntroductionAtomicity.tla || overall=1
run_expected_violation stack-introduction-replay-omission-unsafe \
  "omitting the committed stack transaction during replay is independently refuted" \
  ReplayMatchesCommit \
  --config=StackIntroductionAtomicityReplayOmissionUnsafe.cfg --length=8 StackIntroductionAtomicity.tla || overall=1
run_check evaluation-transaction-isolation \
  "parser, reducer, play validation, replay validation, and evidence publication share one failure-atomic witness boundary" \
  --config=EvaluationTransactionIsolationApalache.cfg --length=5 EvaluationTransactionIsolation.tla || overall=1
run_expected_violation evaluation-parser-reuses-prior-witness-unsafe \
  "returning the prior deploy witness from a parser failure is independently refuted" \
  ParserFailureHasNoWitness \
  --config=EvaluationTransactionIsolationParserReuseUnsafe.cfg --length=1 EvaluationTransactionIsolation.tla || overall=1
run_expected_violation evaluation-reducer-erases-attempt-unsafe \
  "erasing attempted work after a reducer failure is independently refuted" \
  ReducerFailureRetainsAttemptedWork \
  --config=EvaluationTransactionIsolationReducerEraseUnsafe.cfg --length=3 EvaluationTransactionIsolation.tla || overall=1
run_expected_violation evaluation-play-validation-no-rollback-unsafe \
  "retaining play state after witness validation failure is independently refuted" \
  RejectedPlayIsStateAtomic \
  --config=EvaluationTransactionIsolationPlayRollbackUnsafe.cfg --length=3 EvaluationTransactionIsolation.tla || overall=1
run_expected_violation evaluation-replay-validation-no-rollback-unsafe \
  "retaining replay state after witness validation failure is independently refuted" \
  RejectedReplayIsStateAtomic \
  --config=EvaluationTransactionIsolationReplayRollbackUnsafe.cfg --length=4 EvaluationTransactionIsolation.tla || overall=1
run_expected_violation evaluation-replay-evidence-before-final-validation-unsafe \
  "publishing mergeable evidence before the block final-state witness is validated is independently refuted" \
  RejectedReplayPublishesNoEvidence \
  --config=EvaluationTransactionIsolationEarlyEvidenceUnsafe.cfg --length=5 EvaluationTransactionIsolation.tla || overall=1
run_check concurrent-evaluation-transaction-isolation \
  "two evaluation transactions interleave explicit-root capture, replay, validation, rollback, crash, and publication without cross-transaction authority" \
  --config=ConcurrentEvaluationTransactionIsolationApalache.cfg --length=6 ConcurrentEvaluationTransactionIsolation.tla || overall=1
run_expected_violation concurrent-evaluation-parser-reuse-unsafe \
  "reusing another evaluation witness after parser rejection is independently refuted" \
  ParserFailureHasNoWitness \
  --config=ConcurrentEvaluationParserReuseUnsafe.cfg --length=1 ConcurrentEvaluationTransactionIsolation.tla || overall=1
run_expected_violation concurrent-evaluation-reducer-erase-unsafe \
  "erasing attempted work in an interleaved reducer failure is independently refuted" \
  ReducerFailureRetainsAttemptedWork \
  --config=ConcurrentEvaluationReducerEraseUnsafe.cfg --length=4 ConcurrentEvaluationTransactionIsolation.tla || overall=1
run_expected_violation concurrent-evaluation-play-rollback-unsafe \
  "retaining a rejected play transaction's local state is independently refuted" \
  RejectedTransactionsAreStateAtomic \
  --config=ConcurrentEvaluationPlayRollbackUnsafe.cfg --length=6 ConcurrentEvaluationTransactionIsolation.tla || overall=1
run_expected_violation concurrent-evaluation-replay-rollback-unsafe \
  "retaining a rejected replay transaction's local state is independently refuted" \
  RejectedTransactionsAreStateAtomic \
  --config=ConcurrentEvaluationReplayRollbackUnsafe.cfg --length=6 ConcurrentEvaluationTransactionIsolation.tla || overall=1
run_expected_violation concurrent-evaluation-early-evidence-unsafe \
  "publishing replay evidence before final acceptance in an interleaved execution is independently refuted" \
  EvidenceRequiresAcceptance \
  --config=ConcurrentEvaluationEarlyEvidenceUnsafe.cfg --length=5 ConcurrentEvaluationTransactionIsolation.tla || overall=1
run_expected_violation concurrent-evaluation-shared-root-authority-unsafe \
  "capturing another transaction's speculative shared root as execution authority is independently refuted" \
  ExplicitBaseAuthority \
  --config=ConcurrentEvaluationSharedRootAuthorityUnsafe.cfg --length=5 ConcurrentEvaluationTransactionIsolation.tla || overall=1
run_expected_violation concurrent-evaluation-shared-root-publication-unsafe \
  "publishing the shared root pointer instead of the accepted transaction-owned root is independently refuted" \
  AcceptedRootsAreOwnedAndRecorded \
  --config=ConcurrentEvaluationSharedRootPublicationUnsafe.cfg --length=10 ConcurrentEvaluationTransactionIsolation.tla || overall=1
run_expected_violation concurrent-evaluation-foreign-root-deletion-unsafe \
  "deleting another in-flight transaction's authenticated checkpoint during rollback is independently refuted" \
  CheckpointedRootsRemainRecorded \
  --config=ConcurrentEvaluationForeignRootDeletionUnsafe.cfg --length=9 ConcurrentEvaluationTransactionIsolation.tla || overall=1
run_check multi-shard-resource-isolation \
  "independent shard ledgers share bounded workers under stale retries, top-ups, and interleaved commits without cross-shard effects" \
  --config=MultiShardResourceIsolationApalache.cfg --length=6 MultiShardResourceIsolation.tla || overall=1
run_expected_violation multi-shard-blind-commit-unsafe \
  "blindly committing a stale per-shard snapshot and losing a prior state effect is independently refuted" \
  LedgerMatchesOwnedCommits \
  --config=MultiShardBlindCommitUnsafe.cfg --length=8 MultiShardResourceIsolation.tla || overall=1
run_expected_violation multi-shard-state-write-unsafe \
  "writing one shard's accepted effect into another shard's ledger is independently refuted" \
  LedgerMatchesOwnedCommits \
  --config=MultiShardStateWriteUnsafe.cfg --length=4 MultiShardResourceIsolation.tla || overall=1
run_expected_violation multi-shard-root-publication-unsafe \
  "recording one shard's accepted root in another shard's history is independently refuted" \
  RecordedRootsMatchLedger \
  --config=MultiShardRootPublicationUnsafe.cfg --length=4 MultiShardResourceIsolation.tla || overall=1
run_expected_violation multi-shard-debit-unsafe \
  "charging one shard's accepted execution to another shard is independently refuted" \
  ChargesMatchOwnedCommits \
  --config=MultiShardDebitUnsafe.cfg --length=4 MultiShardResourceIsolation.tla || overall=1
run_expected_violation multi-shard-resource-leak-unsafe \
  "leaking shared worker ownership when a task exits is independently refuted" \
  ResourceOwnershipExact \
  --config=MultiShardResourceLeakUnsafe.cfg --length=3 MultiShardResourceIsolation.tla || overall=1
run_check mergeable-evidence-authentication \
  "complete execution keys, local replay provenance, peer-input exclusion, and opposite arrival-order convergence" \
  --config=MergeableEvidenceAuthenticationApalache.cfg --length=8 MergeableEvidenceAuthentication.tla || overall=1
run_expected_violation mergeable-evidence-legacy-key-unsafe \
  "aliasing equivocations through the legacy post-state/creator/sequence key is independently refuted" \
  OppositeArrivalOrdersConverge \
  --config=MergeableEvidenceAuthenticationLegacyKeyUnsafe.cfg --length=4 MergeableEvidenceAuthentication.tla || overall=1
run_expected_violation mergeable-evidence-legacy-delete-unsafe \
  "retiring one execution through the legacy partial key is independently refuted" \
  DeletionCommutesWithDistinctReplay \
  --config=MergeableEvidenceAuthenticationLegacyDeleteUnsafe.cfg --length=2 MergeableEvidenceAuthentication.tla || overall=1
run_expected_violation mergeable-evidence-peer-trust-unsafe \
  "publishing unauthenticated peer evidence is independently refuted" \
  LocallyDerivedEvidenceOnly \
  --config=MergeableEvidenceAuthenticationPeerTrustUnsafe.cfg --length=1 MergeableEvidenceAuthentication.tla || overall=1
run_expected_violation mergeable-evidence-vacuous-latest-unsafe \
  "retiring evidence without a concrete latest-message witness is independently refuted" \
  RetirementRequiresEverySafetyGuard \
  --config=MergeableEvidenceAuthenticationVacuousLatestUnsafe.cfg --length=0 MergeableEvidenceAuthentication.tla || overall=1
run_expected_violation mergeable-evidence-main-spine-only-unsafe \
  "ignoring advancement through a secondary parent is independently refuted" \
  SecondaryParentRetirementComplete \
  --config=MergeableEvidenceAuthenticationMainSpineUnsafe.cfg --length=0 MergeableEvidenceAuthentication.tla || overall=1
run_check threshold-envelope-authority \
  "exact threshold-subset commitment, signer-only funding, typed custody, and validator agreement" \
  --config=ThresholdEnvelopeAuthorityApalache.cfg --length=9 ThresholdEnvelopeAuthority.tla || overall=1
run_expected_violation threshold-envelope-unbound-subset-unsafe \
  "omitting the selected presence bitmap from deploy identity is independently refuted" \
  DeployIdentityBindsStateTransition \
  --config=ThresholdEnvelopeAuthorityUnboundSubsetUnsafe.cfg --length=0 ThresholdEnvelopeAuthority.tla || overall=1
run_expected_violation threshold-envelope-policy-authority-unsafe \
  "granting unsigned policy members runtime authority is independently refuted" \
  UnsignedMembersHaveNoAuthority \
  --config=ThresholdEnvelopeAuthorityPolicyAuthorityUnsafe.cfg --length=0 ThresholdEnvelopeAuthority.tla || overall=1
run_expected_violation threshold-envelope-member-zero-unsafe \
  "using policy member zero as compound deploy authority is independently refuted" \
  UnsignedMembersHaveNoAuthority \
  --config=ThresholdEnvelopeAuthorityMemberZeroUnsafe.cfg --length=0 ThresholdEnvelopeAuthority.tla || overall=1
run_expected_violation threshold-envelope-policy-debit-unsafe \
  "debiting unsigned policy members is independently refuted" \
  UnsignedMembersAreNeverDebited \
  --config=ThresholdEnvelopeAuthorityPolicyDebitUnsafe.cfg --length=0 ThresholdEnvelopeAuthority.tla || overall=1
run_expected_violation threshold-envelope-witness-unsafe \
  "accepting witnesses outside the committed presence bitmap is independently refuted" \
  WitnessesSelectExactlyTheFunders \
  --config=ThresholdEnvelopeAuthorityWitnessUnsafe.cfg --length=0 ThresholdEnvelopeAuthority.tla || overall=1
run_expected_violation threshold-envelope-ground-alias-unsafe \
  "counting one custody owner twice through two prehash schemes is independently refuted" \
  PolicyGroundOwnersAreUnique \
  --config=ThresholdEnvelopeAuthorityGroundAliasUnsafe.cfg --length=0 ThresholdEnvelopeAuthority.tla || overall=1
run_check replay-admission-publication \
  "parallel validators require exact typed admission before durable replay evidence and cache publication" \
  --config=ReplayAdmissionPublicationApalache.cfg --length=7 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-all-admitted-unsafe \
  "bypassing recomputation for an all-admitted partition is independently refuted" \
  ValidatedReplayUsesExactPartition \
  --config=ReplayAdmissionPublicationAllAdmittedUnsafe.cfg --length=3 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-count-only-unsafe \
  "accepting processed evidence by count instead of ordered identity is independently refuted" \
  ValidatedReplayUsesExactPartition \
  --config=ReplayAdmissionPublicationCountOnlyUnsafe.cfg --length=3 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-primary-identity-unsafe \
  "keying protocol-v6 replay by a legacy primary signature is independently refuted" \
  DeployIdentityIsTypedAndInjective \
  --config=ReplayAdmissionPublicationPrimaryIdentityUnsafe.cfg --length=0 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-legacy-wire-field-unsafe \
  "reading the empty legacy signature field instead of the protocol-v6 deploy ID is independently refuted" \
  StoredIdentityMatchesProtocolIdentity \
  --config=ReplayAdmissionPublicationLegacyWireFieldUnsafe.cfg --length=0 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-raw-evidence-identity-unsafe \
  "keying state-bound evidence by the empty protocol-v6 primary witness is independently refuted" \
  EvidenceIdentityIsTypedAndInjective \
  --config=ReplayAdmissionPublicationRawEvidenceIdentityUnsafe.cfg --length=0 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-raw-reservation-identity-unsafe \
  "keying vault reservations by the empty protocol-v6 primary witness is independently refuted" \
  ReservationIdentityIsTypedAndInjective \
  --config=ReplayAdmissionPublicationRawReservationIdentityUnsafe.cfg --length=0 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-raw-fee-identity-unsafe \
  "keying fee regions by the empty protocol-v6 primary witness is independently refuted" \
  FeeIdentityIsTypedAndInjective \
  --config=ReplayAdmissionPublicationRawFeeIdentityUnsafe.cfg --length=0 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-raw-rng-identity-unsafe \
  "seeding private names from the empty protocol-v6 primary witness is independently refuted" \
  RngIdentityIsTypedAndInjective \
  --config=ReplayAdmissionPublicationRawRngIdentityUnsafe.cfg --length=0 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-unconsumed-evidence-unsafe \
  "accepting unconsumed state-bound evidence is independently refuted" \
  EvidenceConsumptionIsExact \
  --config=ReplayAdmissionPublicationUnconsumedEvidenceUnsafe.cfg --length=0 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-caller-context-unsafe \
  "allowing caller-supplied invalid-block context is independently refuted" \
  ValidatedReplayUsesAuthenticatedContext \
  --config=ReplayAdmissionPublicationCallerContextUnsafe.cfg --length=3 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-early-publish-unsafe \
  "publishing replay evidence before validation is independently refuted" \
  PersistentEvidenceRequiresValidatedReplay \
  --config=ReplayAdmissionPublicationEarlyPublishUnsafe.cfg --length=2 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-bare-row-unsafe \
  "treating a bare storage row as validated replay evidence is independently refuted" \
  PersistentEvidenceRequiresValidatedReplay \
  --config=ReplayAdmissionPublicationBareRowUnsafe.cfg --length=1 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-peer-bytes-unsafe \
  "publishing unauthenticated peer bytes is independently refuted" \
  PersistentEvidenceRequiresValidatedReplay \
  --config=ReplayAdmissionPublicationPeerBytesUnsafe.cfg --length=1 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-conflict-overwrite-unsafe \
  "overwriting a conflicting execution row is independently refuted" \
  ConflictingWritesNeverOverwrite \
  --config=ReplayAdmissionPublicationConflictOverwriteUnsafe.cfg --length=6 ReplayAdmissionPublication.tla || overall=1
run_expected_violation replay-admission-cache-first-unsafe \
  "publishing cache state before durable evidence is independently refuted" \
  CacheFollowsDurablePublication \
  --config=ReplayAdmissionPublicationCacheFirstUnsafe.cfg --length=2 ReplayAdmissionPublication.tla || overall=1
run_check merge-tag-binding \
  "authenticated system URI bindings preserve numeric classification across validators and envelope changes" \
  --config=MergeTagBindingApalache.cfg --length=4 MergeTagBinding.tla || overall=1
run_expected_violation merge-tag-binding-envelope-derived-unsafe \
  "deriving the integer-add merge tag from the protocol envelope is independently refuted" \
  Inv_UriRegistryAgreement \
  --config=MergeTagBindingEnvelopeDerivedUnsafeApalache.cfg --length=1 MergeTagBinding.tla || overall=1
run_check deterministic-parallel-reduction \
  "complete causal frontiers preserve canonical COMM results, located-authority exclusion, and disjoint parallelism" \
  --config=../deterministic_parallel_reduction/MC_DeterministicParallelReduction.cfg --length=10 ../deterministic_parallel_reduction/DeterministicParallelReduction.tla || overall=1
run_expected_violation deterministic-parallel-reduction-arrival-unsafe \
  "committing an incomplete arrival frontier is independently refuted" \
  Inv_CommitRequiresCompleteFrontier \
  --config=../deterministic_parallel_reduction/MC_DeterministicParallelReduction_arrival_unsafe_Apalache.cfg --length=2 ../deterministic_parallel_reduction/DeterministicParallelReduction.tla || overall=1
run_expected_violation deterministic-parallel-reduction-order-unsafe \
  "arbitrary commitment inside a conflict component is independently refuted" \
  Inv_ConflictComponentCommitsInOrder \
  --config=../deterministic_parallel_reduction/MC_DeterministicParallelReduction_order_unsafe_Apalache.cfg --length=6 ../deterministic_parallel_reduction/DeterministicParallelReduction.tla || overall=1
run_expected_violation deterministic-parallel-reduction-checkpoint-unsafe \
  "checkpointing before the reduction frontier is quiescent is independently refuted" \
  Inv_CheckpointAtQuiescence \
  --config=../deterministic_parallel_reduction/MC_DeterministicParallelReduction_checkpoint_unsafe_Apalache.cfg --length=1 ../deterministic_parallel_reduction/DeterministicParallelReduction.tla || overall=1
run_expected_violation deterministic-parallel-reduction-serial-unsafe \
  "global serialization that discards independent branch concurrency is independently refuted" \
  Inv_FirstCommitRetainsDisjointParallelism \
  --config=../deterministic_parallel_reduction/MC_DeterministicParallelReduction_serial_unsafe_Apalache.cfg --length=6 ../deterministic_parallel_reduction/DeterministicParallelReduction.tla || overall=1
run_expected_violation deterministic-parallel-reduction-authority-unsafe \
  "classifying operations with an overlapping purse region as disjoint is independently refuted" \
  Inv_SharedAuthorityNeverRunsAsDisjoint \
  --config=../deterministic_parallel_reduction/MC_DeterministicParallelReduction_authority_unsafe_Apalache.cfg --length=7 ../deterministic_parallel_reduction/DeterministicParallelReduction.tla || overall=1
run_check deterministic-reduction-driver-lifecycle \
  "an inline first poll transfers pending work before cancellation and preserves exact driver ownership" \
  --config=../deterministic_parallel_reduction/MC_ReductionDriverLifecycle_Apalache.cfg --length=6 ../deterministic_parallel_reduction/ReductionDriverLifecycle.tla || overall=1
run_expected_violation deterministic-reduction-driver-claim-unsafe \
  "separating frontier submission from driver ownership creates an unowned ready frontier" \
  Inv_ReadyHasDriver \
  --config=../deterministic_parallel_reduction/MC_ReductionDriverLifecycle_claim_unsafe_Apalache.cfg --length=3 ../deterministic_parallel_reduction/ReductionDriverLifecycle.tla || overall=1
run_expected_violation deterministic-reduction-driver-transfer-unsafe \
  "yielding inline work without transferring driver ownership leaves a cancellation-sensitive frontier" \
  Inv_PendingInlineIsTransferable \
  --config=../deterministic_parallel_reduction/MC_ReductionDriverLifecycle_transfer_unsafe_Apalache.cfg --length=3 ../deterministic_parallel_reduction/ReductionDriverLifecycle.tla || overall=1
run_expected_violation deterministic-reduction-driver-reentry-unsafe \
  "executing an internal RSpace commit through the external scheduler is independently refuted" \
  Inv_InternalExecutionNeverResubmits \
  --config=../deterministic_parallel_reduction/MC_ReductionDriverLifecycle_reentry_unsafe_Apalache.cfg --length=4 ../deterministic_parallel_reduction/ReductionDriverLifecycle.tla || overall=1
run_check deterministic-single-participant-fast-path \
  "direct execution refines scheduled commitment only after one live participant remains" \
  --config=../deterministic_parallel_reduction/MC_SingleParticipantFastPath_Apalache.cfg --length=4 ../deterministic_parallel_reduction/SingleParticipantFastPath.tla || overall=1
run_expected_violation deterministic-single-participant-fast-path-unsafe \
  "direct ownership with two live participants is independently refuted" \
  Inv_DirectOwnerRequiresSingleton \
  --config=../deterministic_parallel_reduction/MC_SingleParticipantFastPath_unsafe_Apalache.cfg --length=1 ../deterministic_parallel_reduction/SingleParticipantFastPath.tla || overall=1
run_check deterministic-evaluation-boundary \
  "structured cancellation aborts child tasks before the evaluation permit permits a checkpoint" \
  --config=../deterministic_parallel_reduction/MC_EvaluationBoundary_Apalache.cfg --length=4 ../deterministic_parallel_reduction/EvaluationBoundary.tla || overall=1
run_expected_violation deterministic-evaluation-boundary-cancel-unsafe \
  "detaching child tasks and releasing the evaluation permit is independently refuted" \
  Inv_CheckpointAtEvaluationQuiescence \
  --config=../deterministic_parallel_reduction/MC_EvaluationBoundary_cancel_unsafe_Apalache.cfg --length=2 ../deterministic_parallel_reduction/EvaluationBoundary.tla || overall=1
run_check block-heap-lifecycle \
  "concurrent block completion bounds reclaimable heap without changing committed semantics" \
  --config=BlockHeapLifecycleApalache.cfg --length=12 BlockHeapLifecycle.tla || overall=1
run_expected_violation block-heap-lifecycle-missing-boundary-unsafe \
  "omitting block-boundary allocator reclamation is independently refuted" \
  ResidentWithinIntervalEnvelope \
  --config=BlockHeapLifecycleMissingBoundaryUnsafe.cfg --length=12 BlockHeapLifecycle.tla || overall=1

if [ "$overall" -ne 0 ]; then
  exit 1
fi

if [ "$checks_run" -eq 0 ]; then
  echo "error: no Apalache checks matched filter '$FILTER'" >&2
  exit 2
fi

echo "Apalache cost-accounted-rho cross-witnesses passed."
