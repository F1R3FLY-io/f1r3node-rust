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
run_check parallel-stack-materialization \
  "same-configuration declaration barrier, nested causality, conservation, and replay agreement" \
  --config=ParallelStackMaterialization.cfg --length=8 ParallelStackMaterialization.tla || overall=1
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
run_check deterministic-evaluation-boundary \
  "the evaluation epoch permit covers detached children and excludes partial checkpoints" \
  --config=../deterministic_parallel_reduction/MC_EvaluationBoundary.cfg --length=4 ../deterministic_parallel_reduction/EvaluationBoundary.tla || overall=1
run_expected_violation deterministic-evaluation-boundary-cancel-unsafe \
  "releasing the evaluation permit when only the root future is cancelled is independently refuted" \
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
