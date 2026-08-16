#!/usr/bin/env bash
# scripts/check-cost-accounted-rho-tla-invariants.sh
#
# Local-only TLA+ invariant runner for the cost_accounted_rho specs.
# Per team policy (memory `feedback_formal_verification_is_local_only_not_ci`),
# formal verification stays local — this script is NOT a CI step.
#
# Runs TLC against every safe .cfg under formal/tlaplus/cost_accounted_rho/
# whose paired .tla module exists, then checks each registered unsafe control
# for its exact expected invariant violation. Every TLC run goes through the shared
# scripts/lib/tlc-run.sh launcher, which enforces a strict memory envelope
# (on-disk metadir — NOT tmpfs; bounded -Xmx heap; bounded workers; and a
# hard systemd-run MemoryMax / MemorySwapMax=0 ceiling) so a single large
# model can't exhaust RAM. Tune via TLC_HEAP / TLC_WORKERS / TLC_RSS /
# TLC_METADIR_ROOT (see the helper header).
#
# Each run is reported as PASS / FAIL based on the TLC output. Exit code 0 iff
# every safe spec reports no errors and every unsafe control reports its named
# counterexample.
#
# Usage:
#   bash scripts/check-cost-accounted-rho-tla-invariants.sh
#   bash scripts/check-cost-accounted-rho-tla-invariants.sh --filter MC

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TLA_DIR="$REPO_ROOT/formal/tlaplus/cost_accounted_rho"

FILTER="${1:-}"
if [[ "$FILTER" == "--filter" ]]; then
    shift
    FILTER="${1:-}"
    shift || true
fi

if [[ ! -d "$TLA_DIR" ]]; then
    echo "ERROR: TLA+ cost_accounted_rho directory not found at $TLA_DIR" >&2
    exit 2
fi

if ! command -v tlc >/dev/null 2>&1; then
    echo "ERROR: tlc binary not on PATH; install tlaplus tooling" >&2
    exit 2
fi

# Shared memory-bounded TLC launcher: on-disk metadir, capped -Xmx heap,
# capped workers, hard systemd MemoryMax ceiling. See scripts/lib/tlc-run.sh.
export TLC_REPO_ROOT="$REPO_ROOT"
source "$REPO_ROOT/scripts/lib/tlc-run.sh"

cd "$TLA_DIR"

# Some protocol .cfg files were authored for use ONLY through their
# MC wrapper module (they reference MC_-prefixed identifiers that
# only resolve when the MC*.tla module is the spec root). The MC
# wrappers have non-trivial naming (e.g., CompoundProtocol.cfg is
# wrapped by MCCompound.tla, NOT MCCompoundProtocol.tla). This
# explicit map records which protocol .cfgs depend on which wrapper.
# The mapping is used to invoke TLC as:
#   tlc -config <base>.cfg <wrapper>.tla
declare -A WRAPPED_BY
WRAPPED_BY[CompoundProtocol]=MCCompound
WRAPPED_BY[CompoundSettlement]=MCCompoundSettlement
WRAPPED_BY[CostAccountedRho]=MC
WRAPPED_BY[CostAccountingSearchFrontier]=MCCostAccountingSearchFrontier
WRAPPED_BY[CostAccountingThreats]=MCCostAccountingThreats
WRAPPED_BY[EvalScheduling]=MCEval
# #13b: focused strict reject-when-absent instance (PoolSupply = 0).
WRAPPED_BY[EvalStrictAbsent]=MCEvalStrictAbsent
WRAPPED_BY[FullProtocol]=MCFull
WRAPPED_BY[MergeableChannelAccounting]=MCMergeableChannelAccounting
WRAPPED_BY[RuntimeBudgetReplay]=MCRuntimeBudgetReplay
WRAPPED_BY[EndToEndCostConsensus]=MCEndToEndCostConsensus
WRAPPED_BY[DeployTraceSegmentation]=MCDeployTraceSegmentation
WRAPPED_BY[ReplaySupplySnapshot]=MCReplaySupplySnapshot
WRAPPED_BY[MergeAggregateAgreement]=MCMergeAggregateAgreement
WRAPPED_BY[StateBoundAdmission]=MCStateBoundAdmission
WRAPPED_BY[StateBoundValidatorConvergence]=MCStateBoundValidatorConvergence
WRAPPED_BY[LocatedAuthoritySettlement]=MCLocatedAuthoritySettlement
WRAPPED_BY[CapacityBoundedTrace]=MCCapacityBoundedTrace
WRAPPED_BY[LocatedStackConservationCollision]=LocatedStackConservation
WRAPPED_BY[StateBoundFrontierExpansion]=MCStateBoundFrontierExpansion
WRAPPED_BY[VaultBackedCostLifecycle]=MCVaultBackedCostLifecycle
WRAPPED_BY[AtomicVaultSettlementRefinement]=MCAtomicVaultSettlementRefinement
WRAPPED_BY[WalletFundedLollipop]=MCWalletFundedLollipop
# MAJOR-5: the token-gated-join sequential-fuel griefing / atomicity obligation.
# TokenGatedJoin.cfg is the NATIVE-model safety suite (must HOLD). The companion
# TokenGatedJoinM2Grief.cfg is an EXPECTED-REFUTATION run (it confirms the griefing
# vector for the TRANSPILER runtime-gate model by producing a counterexample), so
# it is deliberately NOT registered here — a counterexample is its intended result,
# not a pass. Run it explicitly: tlc -config TokenGatedJoinM2Grief.cfg MCTokenGatedJoin.tla
WRAPPED_BY[TokenGatedJoin]=MCTokenGatedJoin

declare -A EXPECTED_REFUTATION_WRAPPER
declare -A EXPECTED_REFUTATION_INVARIANT
EXPECTED_REFUTATION_WRAPPER[AccountingScopeLifetimeBooleanUnsafe]=AccountingScopeLifetime
EXPECTED_REFUTATION_INVARIANT[AccountingScopeLifetimeBooleanUnsafe]=AccountingScopeReflectsOwners
EXPECTED_REFUTATION_WRAPPER[AtomicCommAccountingIntroductionUnsafe]=MCAtomicCommAccountingIntroductionUnsafe
EXPECTED_REFUTATION_INVARIANT[AtomicCommAccountingIntroductionUnsafe]=ExactCommCost
EXPECTED_REFUTATION_WRAPPER[AuthenticatedSupplySnapshotLiveUnsafe]=AuthenticatedSupplySnapshot
EXPECTED_REFUTATION_INVARIANT[AuthenticatedSupplySnapshotLiveUnsafe]=CandidateMintCannotFundItself
EXPECTED_REFUTATION_WRAPPER[ReplaySupplySnapshotLiveQueryUnsafe]=MCReplaySupplySnapshot
EXPECTED_REFUTATION_INVARIANT[ReplaySupplySnapshotLiveQueryUnsafe]=ExactRecordedReplayTrace
EXPECTED_REFUTATION_WRAPPER[ReplayRootMaterializationEagerUnsafe]=ReplayRootMaterialization
EXPECTED_REFUTATION_INVARIANT[ReplayRootMaterializationEagerUnsafe]=SnapshotReadsMaterializedRoot
EXPECTED_REFUTATION_WRAPPER[ReplayRootMaterializationHistoryUnsafe]=ReplayRootMaterialization
EXPECTED_REFUTATION_INVARIANT[ReplayRootMaterializationHistoryUnsafe]=CompletedValidatorsAgree
EXPECTED_REFUTATION_WRAPPER[ReplayRootMaterializationQueryUnsafe]=ReplayRootMaterialization
EXPECTED_REFUTATION_INVARIANT[ReplayRootMaterializationQueryUnsafe]=SnapshotsUseOrdinaryRuntime
EXPECTED_REFUTATION_WRAPPER[StateBoundAdmissionDriftUnsafe]=MCStateBoundAdmission
EXPECTED_REFUTATION_INVARIANT[StateBoundAdmissionDriftUnsafe]=EvidenceMatchesCommit
EXPECTED_REFUTATION_WRAPPER[StateBoundAdmissionExhaustionUnsafe]=MCStateBoundAdmission
EXPECTED_REFUTATION_INVARIANT[StateBoundAdmissionExhaustionUnsafe]=AdmissionRequiresCompletedProof
EXPECTED_REFUTATION_WRAPPER[StateBoundAdmissionStructuralUnsafe]=MCStateBoundAdmission
EXPECTED_REFUTATION_INVARIANT[StateBoundAdmissionStructuralUnsafe]=EvidenceMatchesCommit
EXPECTED_REFUTATION_WRAPPER[StateBoundAdmissionReplayUnsafe]=MCStateBoundAdmission
EXPECTED_REFUTATION_INVARIANT[StateBoundAdmissionReplayUnsafe]=CommitMatchesReplay
EXPECTED_REFUTATION_WRAPPER[StateBoundAdmissionCheckpointUnsafe]=MCStateBoundAdmission
EXPECTED_REFUTATION_INVARIANT[StateBoundAdmissionCheckpointUnsafe]=PhysicalRejectionCreatesNoCheckpoint
EXPECTED_REFUTATION_WRAPPER[StateBoundValidatorConvergenceContextUnsafe]=MCStateBoundValidatorConvergence
EXPECTED_REFUTATION_INVARIANT[StateBoundValidatorConvergenceContextUnsafe]=AcceptedUsesAuthenticatedContext
EXPECTED_REFUTATION_WRAPPER[StateBoundValidatorConvergenceOrderUnsafe]=MCStateBoundValidatorConvergence
EXPECTED_REFUTATION_INVARIANT[StateBoundValidatorConvergenceOrderUnsafe]=AcceptedUsesCanonicalDeployOrder
EXPECTED_REFUTATION_WRAPPER[StateBoundValidatorConvergenceScheduleUnsafe]=MCStateBoundValidatorConvergence
EXPECTED_REFUTATION_INVARIANT[StateBoundValidatorConvergenceScheduleUnsafe]=AcceptedReproducesCertificate
EXPECTED_REFUTATION_WRAPPER[EndToEndCostConsensusDoubleCreditUnsafe]=MCEndToEndCostConsensus
EXPECTED_REFUTATION_INVARIANT[EndToEndCostConsensusDoubleCreditUnsafe]=SettlementDoesNotReapplyGenesisFunding
EXPECTED_REFUTATION_WRAPPER[EndToEndCostConsensusFundingBypassUnsafe]=MCEndToEndCostConsensus
EXPECTED_REFUTATION_INVARIANT[EndToEndCostConsensusFundingBypassUnsafe]=EveryExecutedDeploymentWasFunded
EXPECTED_REFUTATION_WRAPPER[EndToEndCostConsensusGenesisMismatchUnsafe]=MCEndToEndCostConsensus
EXPECTED_REFUTATION_INVARIANT[EndToEndCostConsensusGenesisMismatchUnsafe]=AdmissionRequiresGenesisAgreement
EXPECTED_REFUTATION_WRAPPER[EndToEndCostConsensusGenesisAuthorityMismatchUnsafe]=MCEndToEndCostConsensus
EXPECTED_REFUTATION_INVARIANT[EndToEndCostConsensusGenesisAuthorityMismatchUnsafe]=GenesisExecutionReplayAuthorityAgree
EXPECTED_REFUTATION_WRAPPER[EndToEndCostConsensusUnsafe]=MCEndToEndCostConsensus
EXPECTED_REFUTATION_INVARIANT[EndToEndCostConsensusUnsafe]=LocalFaultNeverCreatesSlashEvidence
EXPECTED_REFUTATION_WRAPPER[EndToEndCostConsensusOriginBypassUnsafe]=MCEndToEndCostConsensus
EXPECTED_REFUTATION_INVARIANT[EndToEndCostConsensusOriginBypassUnsafe]=ValidationOriginParity
EXPECTED_REFUTATION_WRAPPER[DeployTraceSegmentationRetentionUnsafe]=MCDeployTraceSegmentation
EXPECTED_REFUTATION_INVARIANT[DeployTraceSegmentationRetentionUnsafe]=CheckpointContainsOnlyItsDeploy
EXPECTED_REFUTATION_WRAPPER[MergeAggregateAgreementPrefixUnsafe]=MCMergeAggregateAgreement
EXPECTED_REFUTATION_INVARIANT[MergeAggregateAgreementPrefixUnsafe]=AcceptanceIsPermutationInvariant
EXPECTED_REFUTATION_WRAPPER[LocatedAuthoritySettlementMetadataErasureUnsafe]=MCLocatedAuthoritySettlement
EXPECTED_REFUTATION_INVARIANT[LocatedAuthoritySettlementMetadataErasureUnsafe]=RealizedBackedByReservation
EXPECTED_REFUTATION_WRAPPER[LocatedAuthoritySettlementAmbientPurseUnsafe]=MCLocatedAuthoritySettlement
EXPECTED_REFUTATION_INVARIANT[LocatedAuthoritySettlementAmbientPurseUnsafe]=NoAmbientAuthority
EXPECTED_REFUTATION_WRAPPER[LocatedAuthoritySettlementContinuationRewrapUnsafe]=MCLocatedAuthoritySettlement
EXPECTED_REFUTATION_INVARIANT[LocatedAuthoritySettlementContinuationRewrapUnsafe]=RealizedBackedByReservation
EXPECTED_REFUTATION_WRAPPER[LocatedAuthoritySettlementNonAtomicDebitUnsafe]=MCLocatedAuthoritySettlement
EXPECTED_REFUTATION_INVARIANT[LocatedAuthoritySettlementNonAtomicDebitUnsafe]=NoPartialEventDebit
EXPECTED_REFUTATION_WRAPPER[LocatedAuthoritySettlementReplayOmissionUnsafe]=MCLocatedAuthoritySettlement
EXPECTED_REFUTATION_INVARIANT[LocatedAuthoritySettlementReplayOmissionUnsafe]=ReplayPreservesAuthority
EXPECTED_REFUTATION_WRAPPER[LocatedAuthoritySettlementSlotIdentityUnsafe]=MCLocatedAuthoritySettlement
EXPECTED_REFUTATION_INVARIANT[LocatedAuthoritySettlementSlotIdentityUnsafe]=CrossDeploySlotIdentityStable
EXPECTED_REFUTATION_WRAPPER[AuthorityPresentationMissingUnsafe]=AuthorityPresentation
EXPECTED_REFUTATION_INVARIANT[AuthorityPresentationMissingUnsafe]=IntermediatePartitionAdmitted
EXPECTED_REFUTATION_WRAPPER[AuthorityPresentationWeakeningUnsafe]=AuthorityPresentation
EXPECTED_REFUTATION_INVARIANT[AuthorityPresentationWeakeningUnsafe]=NoWeakening
EXPECTED_REFUTATION_WRAPPER[AuthorityPresentationNonAtomicUnsafe]=AuthorityPresentation
EXPECTED_REFUTATION_INVARIANT[AuthorityPresentationNonAtomicUnsafe]=NoPartialEventDebit
EXPECTED_REFUTATION_WRAPPER[AuthorityPresentationReplayUnsafe]=AuthorityPresentation
EXPECTED_REFUTATION_INVARIANT[AuthorityPresentationReplayUnsafe]=ReplayUsesCertifiedPresentation
EXPECTED_REFUTATION_WRAPPER[AuthorityPresentationCertificateUnsafe]=AuthorityPresentation
EXPECTED_REFUTATION_INVARIANT[AuthorityPresentationCertificateUnsafe]=CertificateBindsPhysicalReservation
EXPECTED_REFUTATION_WRAPPER[ForcedRedexAccountingDedupUnsafe]=ForcedRedexAccounting
EXPECTED_REFUTATION_INVARIANT[ForcedRedexAccountingDedupUnsafe]=EveryForcedRedexConsumesOne
EXPECTED_REFUTATION_WRAPPER[ForcedRedexAccountingReplayUnsafe]=ForcedRedexAccounting
EXPECTED_REFUTATION_INVARIANT[ForcedRedexAccountingReplayUnsafe]=ReplayStaysWithinCertificate
EXPECTED_REFUTATION_WRAPPER[StructuralAuthorityBoundReuseUnsafe]=StructuralAuthorityBound
EXPECTED_REFUTATION_INVARIANT[StructuralAuthorityBoundReuseUnsafe]=RealizedNeverExceedsStructuralDemand
EXPECTED_REFUTATION_WRAPPER[CausalStackOrderHashUnsafe]=CausalStackOrder
EXPECTED_REFUTATION_INVARIANT[CausalStackOrderHashUnsafe]=CausallyFundedTraceIsAccepted
EXPECTED_REFUTATION_WRAPPER[CapacityBoundedTraceFixedCapUnsafe]=MCCapacityBoundedTrace
EXPECTED_REFUTATION_INVARIANT[CapacityBoundedTraceFixedCapUnsafe]=AcceptedTraceIsExact
EXPECTED_REFUTATION_WRAPPER[RuntimeBoundAuthorityPrematureRejectUnsafe]=RuntimeBoundAuthority
EXPECTED_REFUTATION_INVARIANT[RuntimeBoundAuthorityPrematureRejectUnsafe]=BoundAuthorityDeferred
EXPECTED_REFUTATION_WRAPPER[RuntimeBoundAuthorityCandidateMintUnsafe]=RuntimeBoundAuthority
EXPECTED_REFUTATION_INVARIANT[RuntimeBoundAuthorityCandidateMintUnsafe]=CandidateStackCannotFundCreator
EXPECTED_REFUTATION_WRAPPER[RuntimeBoundAuthorityReplayUnsafe]=RuntimeBoundAuthority
EXPECTED_REFUTATION_INVARIANT[RuntimeBoundAuthorityReplayUnsafe]=ReplayPreservesResolvedSlot
EXPECTED_REFUTATION_WRAPPER[PayloadSortPersistenceStorageErasureUnsafe]=PayloadSortPersistence
EXPECTED_REFUTATION_INVARIANT[PayloadSortPersistenceStorageErasureUnsafe]=StoragePreservesCompletePayload
EXPECTED_REFUTATION_WRAPPER[PayloadSortPersistenceMatcherErasureUnsafe]=PayloadSortPersistence
EXPECTED_REFUTATION_INVARIANT[PayloadSortPersistenceMatcherErasureUnsafe]=MatcherCaptureIsExact
EXPECTED_REFUTATION_WRAPPER[PayloadSortPersistenceReplayErasureUnsafe]=PayloadSortPersistence
EXPECTED_REFUTATION_INVARIANT[PayloadSortPersistenceReplayErasureUnsafe]=ReplayPreservesCompletePayload
EXPECTED_REFUTATION_WRAPPER[SettlementMergeVisibilityEventOmissionUnsafe]=SettlementMergeVisibility
EXPECTED_REFUTATION_INVARIANT[SettlementMergeVisibilityEventOmissionUnsafe]=SettlementStateChangeIsIndexed
EXPECTED_REFUTATION_WRAPPER[SettlementMergeVisibilityIdentityCollapseUnsafe]=SettlementMergeVisibility
EXPECTED_REFUTATION_INVARIANT[SettlementMergeVisibilityIdentityCollapseUnsafe]=DistinctInstancesRemainMergeable
EXPECTED_REFUTATION_WRAPPER[SettlementMergeVisibilityReplayOmissionUnsafe]=SettlementMergeVisibility
EXPECTED_REFUTATION_INVARIANT[SettlementMergeVisibilityReplayOmissionUnsafe]=ReplayReproducesRemoval
EXPECTED_REFUTATION_WRAPPER[SettlementMergeVisibilityTraceLossUnsafe]=SettlementMergeVisibility
EXPECTED_REFUTATION_INVARIANT[SettlementMergeVisibilityTraceLossUnsafe]=SoftCheckpointPreservesTracePrefix
EXPECTED_REFUTATION_WRAPPER[LocatedStackConservationDuplicateUnsafe]=LocatedStackConservation
EXPECTED_REFUTATION_INVARIANT[LocatedStackConservationDuplicateUnsafe]=UserStackProductionConserves
EXPECTED_REFUTATION_WRAPPER[LocatedStackConservationPartialUnsafe]=LocatedStackConservation
EXPECTED_REFUTATION_INVARIANT[LocatedStackConservationPartialUnsafe]=UserStackProductionConserves
EXPECTED_REFUTATION_WRAPPER[LocatedStackConservationReplayUnsafe]=LocatedStackConservation
EXPECTED_REFUTATION_INVARIANT[LocatedStackConservationReplayUnsafe]=ReplayMatchesCommittedTransfer
EXPECTED_REFUTATION_WRAPPER[StateBoundFrontierExpansionFixedUnsafe]=MCStateBoundFrontierExpansion
EXPECTED_REFUTATION_INVARIANT[StateBoundFrontierExpansionFixedUnsafe]=CompleteBackedTraceIsAccepted
EXPECTED_REFUTATION_WRAPPER[StateBoundFrontierExpansionUnbackedUnsafe]=MCStateBoundFrontierExpansion
EXPECTED_REFUTATION_INVARIANT[StateBoundFrontierExpansionUnbackedUnsafe]=CapacityUsesOnlyAuthenticatedBacking
EXPECTED_REFUTATION_WRAPPER[StateBoundFrontierExpansionLeakUnsafe]=MCStateBoundFrontierExpansion
EXPECTED_REFUTATION_INVARIANT[StateBoundFrontierExpansionLeakUnsafe]=SpeculativeAttemptsAreEffectFree
EXPECTED_REFUTATION_WRAPPER[StateBoundFrontierExpansionReplayUnsafe]=MCStateBoundFrontierExpansion
EXPECTED_REFUTATION_INVARIANT[StateBoundFrontierExpansionReplayUnsafe]=ReplayUsesTheExpandedBound
EXPECTED_REFUTATION_WRAPPER[ExchangeFlowOneSidedUnsafe]=ExchangeFlow
EXPECTED_REFUTATION_INVARIANT[ExchangeFlowOneSidedUnsafe]=Inv_RequiresBothInputs
EXPECTED_REFUTATION_WRAPPER[VaultBackedCostLifecycleExecuteFirstUnsafe]=MCVaultBackedCostLifecycle
EXPECTED_REFUTATION_INVARIANT[VaultBackedCostLifecycleExecuteFirstUnsafe]=ExecutionRequiresCompleteReservation
EXPECTED_REFUTATION_WRAPPER[VaultBackedCostLifecyclePartialReserveUnsafe]=MCVaultBackedCostLifecycle
EXPECTED_REFUTATION_INVARIANT[VaultBackedCostLifecyclePartialReserveUnsafe]=ReservationMatchesCertificate
EXPECTED_REFUTATION_WRAPPER[VaultBackedCostLifecycleUnauthorizedUnsafe]=MCVaultBackedCostLifecycle
EXPECTED_REFUTATION_INVARIANT[VaultBackedCostLifecycleUnauthorizedUnsafe]=ReservationUsesAuthorizedPayers
EXPECTED_REFUTATION_WRAPPER[VaultBackedCostLifecycleRefundLossUnsafe]=MCVaultBackedCostLifecycle
EXPECTED_REFUTATION_INVARIANT[VaultBackedCostLifecycleRefundLossUnsafe]=CanonicalValueConserved
EXPECTED_REFUTATION_WRAPPER[VaultBackedCostLifecycleReplayOmissionUnsafe]=MCVaultBackedCostLifecycle
EXPECTED_REFUTATION_INVARIANT[VaultBackedCostLifecycleReplayOmissionUnsafe]=ReplayMatchesCommit
EXPECTED_REFUTATION_WRAPPER[VaultBackedCostLifecycleDoubleMintUnsafe]=MCVaultBackedCostLifecycle
EXPECTED_REFUTATION_INVARIANT[VaultBackedCostLifecycleDoubleMintUnsafe]=MintOccursAtMostOnce
EXPECTED_REFUTATION_WRAPPER[VaultBackedCostLifecycleIndependentCreditUnsafe]=MCVaultBackedCostLifecycle
EXPECTED_REFUTATION_INVARIANT[VaultBackedCostLifecycleIndependentCreditUnsafe]=SingleCanonicalLedger
EXPECTED_REFUTATION_WRAPPER[AtomicVaultSettlementRefinementGlobalCellUnsafe]=MCAtomicVaultSettlementRefinement
EXPECTED_REFUTATION_INVARIANT[AtomicVaultSettlementRefinementGlobalCellUnsafe]=NoPersistentReservationState
EXPECTED_REFUTATION_WRAPPER[WalletFundedLollipopFundingCopyUnsafe]=MCWalletFundedLollipop
EXPECTED_REFUTATION_INVARIANT[WalletFundedLollipopFundingCopyUnsafe]=CanonicalCustodyConserved
EXPECTED_REFUTATION_WRAPPER[WalletFundedLollipopCapabilityLeakUnsafe]=MCWalletFundedLollipop
EXPECTED_REFUTATION_INVARIANT[WalletFundedLollipopCapabilityLeakUnsafe]=FundingUsesAddressWithoutDelegatingDraw
EXPECTED_REFUTATION_WRAPPER[WalletFundedLollipopPayerCollapseUnsafe]=MCWalletFundedLollipop
EXPECTED_REFUTATION_INVARIANT[WalletFundedLollipopPayerCollapseUnsafe]=CertifiedPayerIsSlot
EXPECTED_REFUTATION_WRAPPER[WalletFundedLollipopReplayOmissionUnsafe]=MCWalletFundedLollipop
EXPECTED_REFUTATION_INVARIANT[WalletFundedLollipopReplayOmissionUnsafe]=ReplayMatchesCommit
EXPECTED_REFUTATION_WRAPPER[WalletFundedLollipopMissingOuterUnsafe]=MCWalletFundedLollipop
EXPECTED_REFUTATION_INVARIANT[WalletFundedLollipopMissingOuterUnsafe]=ContinuationRequiresOuter
EXPECTED_REFUTATION_WRAPPER[WalletFundedLollipopBoundChargeUnsafe]=MCWalletFundedLollipop
EXPECTED_REFUTATION_INVARIANT[WalletFundedLollipopBoundChargeUnsafe]=UnusedCertifiedBoundIsRefunded
EXPECTED_REFUTATION_WRAPPER[WalletFundedLollipopGatewayAuthBypassUnsafe]=MCWalletFundedLollipop
EXPECTED_REFUTATION_INVARIANT[WalletFundedLollipopGatewayAuthBypassUnsafe]=OnlyGatewayAuthorizesContinuation
EXPECTED_REFUTATION_WRAPPER[NormalizerEnvironmentRefinementEmptyUnsafe]=NormalizerEnvironmentRefinement
EXPECTED_REFUTATION_INVARIANT[NormalizerEnvironmentRefinementEmptyUnsafe]=CertificationExecutionReplayUseSameEnvironment
EXPECTED_REFUTATION_WRAPPER[PhysicalSettlementWorklistRecursiveUnsafe]=PhysicalSettlementWorklist
EXPECTED_REFUTATION_INVARIANT[PhysicalSettlementWorklistRecursiveUnsafe]=NativeStackBound
EXPECTED_REFUTATION_WRAPPER[OslfLocatedTypingContractionUnsafe]=OslfLocatedTyping
EXPECTED_REFUTATION_INVARIANT[OslfLocatedTypingContractionUnsafe]=LinearNoContraction
EXPECTED_REFUTATION_WRAPPER[OslfLocatedTypingWeakeningUnsafe]=OslfLocatedTyping
EXPECTED_REFUTATION_INVARIANT[OslfLocatedTypingWeakeningUnsafe]=LinearNoWeakening
EXPECTED_REFUTATION_WRAPPER[OslfLocatedTypingAliasUnsafe]=OslfLocatedTyping
EXPECTED_REFUTATION_INVARIANT[OslfLocatedTypingAliasUnsafe]=LocationIsolation
EXPECTED_REFUTATION_WRAPPER[OslfLocatedTypingUpperModalUnsafe]=OslfLocatedTyping
EXPECTED_REFUTATION_INVARIANT[OslfLocatedTypingUpperModalUnsafe]=ModalEvidenceSound
EXPECTED_REFUTATION_WRAPPER[OslfLocatedTypingCandidateCreditUnsafe]=OslfLocatedTyping
EXPECTED_REFUTATION_INVARIANT[OslfLocatedTypingCandidateCreditUnsafe]=AuthenticatedFundingOnly

# Collect all .cfg files whose paired .tla module exists.
specs=()
spec_roots=()
for cfg in *.cfg; do
    [[ -e "$cfg" ]] || continue
    base="${cfg%.cfg}"
    if [[ -z "$FILTER" || "$base" == *"$FILTER"* ]]; then
        if [[ "$base" != MC* ]] && [[ -n "${WRAPPED_BY[$base]:-}" ]]; then
            wrapper="${WRAPPED_BY[$base]}"
            if [[ -f "${wrapper}.tla" ]]; then
                specs+=("$base")
                spec_roots+=("${wrapper}.tla")
            fi
        elif [[ -f "${base}.tla" ]]; then
            specs+=("$base")
            spec_roots+=("${base}.tla")
        fi
    fi
done

matching_refutations=0
for base in "${!EXPECTED_REFUTATION_WRAPPER[@]}"; do
    if [[ -z "$FILTER" || "$base" == *"$FILTER"* ]]; then
        matching_refutations=$((matching_refutations + 1))
    fi
done

if [[ ${#specs[@]} -eq 0 && $matching_refutations -eq 0 ]]; then
    echo "No matching specs found" >&2
    exit 2
fi

echo "Running TLC against ${#specs[@]} cost_accounted_rho specs"
echo "Memory envelope: -Xmx${TLC_HEAP}, ${TLC_WORKERS} workers, on-disk metadir, MemoryMax=${TLC_RSS} (MemorySwapMax=0)"
echo

passes=0
failures=0
failed_specs=()
# Per-spec metadirs under an ON-DISK root. NOT mktemp -d, which lands in
# TMPDIR=/tmp — tmpfs (RAM) on this host, so TLC's multi-GB state graph
# would spill into RAM instead of onto the NVMe. Cleared up front (a prior
# SIGKILL'd run leaks its metadir because the EXIT trap never fires) and
# again on exit.
METADIR_ROOT="$TLC_METADIR_ROOT/cost-accounted-gate"
rm -rf "$METADIR_ROOT"
mkdir -p "$METADIR_ROOT"
trap 'rm -rf "$METADIR_ROOT"' EXIT

for i in "${!specs[@]}"; do
    base="${specs[$i]}"
    spec_root="${spec_roots[$i]}"
    printf "  %-40s " "${base} (${spec_root%.tla})"
    metadir="$METADIR_ROOT/$base"
    output=$(tlc_run "$metadir" "${base}.cfg" "$spec_root" -deadlock 2>&1 || true)
    if grep -q "Model checking completed. No error has been found" <<<"$output"; then
        echo "PASS"
        passes=$((passes + 1))
    elif grep -q "Error:" <<<"$output"; then
        echo "FAIL"
        failures=$((failures + 1))
        failed_specs+=("$base")
        echo "$output" | tail -10 | sed 's/^/    /'
    else
        echo "INCONCLUSIVE"
        failures=$((failures + 1))
        failed_specs+=("$base")
        echo "$output" | tail -5 | sed 's/^/    /'
    fi
done

expected_refutations=0
for base in "${!EXPECTED_REFUTATION_WRAPPER[@]}"; do
    if [[ -n "$FILTER" && "$base" != *"$FILTER"* ]]; then
        continue
    fi
    wrapper="${EXPECTED_REFUTATION_WRAPPER[$base]}"
    invariant="${EXPECTED_REFUTATION_INVARIANT[$base]}"
    if [[ ! -f "${base}.cfg" || ! -f "${wrapper}.tla" ]]; then
        printf "  %-40s FAIL\n" "${base} (expected refutation)"
        failures=$((failures + 1))
        failed_specs+=("$base(missing-control)")
        continue
    fi
    expected_refutations=$((expected_refutations + 1))
    printf "  %-40s " "${base} (expected refutation)"
    metadir="$METADIR_ROOT/$base"
    output=$(tlc_run "$metadir" "${base}.cfg" "${wrapper}.tla" -deadlock 2>&1 || true)
    if grep -Fq "Invariant ${invariant} is violated" <<<"$output" \
       || grep -Fq "The invariant of ${invariant} is equal to FALSE" <<<"$output"; then
        echo "PASS (refuted ${invariant})"
        passes=$((passes + 1))
    else
        echo "FAIL (expected ${invariant} counterexample)"
        failures=$((failures + 1))
        failed_specs+=("$base(expected-refutation)")
        echo "$output" | tail -10 | sed 's/^/    /'
    fi
done

# ─────────────────────────────────────────────────────────────────────────
# Validator behavioral contract (Workstream E, stage E5): the arithmetic
# obligations of the built-in validator's contract, discharged DEDUCTIVELY
# by TLAPS (not bounded model-checking) in formal/tlaplus/validator/
# Validator.tla. The state-machine obligations stay TLC-checked above
# (RuntimeBudgetReplay). Local-only, like the rest of this script.
VALIDATOR_TLA_DIR="$REPO_ROOT/formal/tlaplus/validator"
if [[ -z "$FILTER" || "Validator" == *"$FILTER"* ]] \
   && [[ -f "$VALIDATOR_TLA_DIR/Validator.tla" ]]; then
    printf "  %-40s " "Validator (TLAPS contract proofs)"
    # TLAPS and its zenon backend install under ~/.local; make them findable
    # without disturbing the TLC PATH used above (scoped to this subshell).
    VALIDATOR_PATH="$HOME/.local/tlaps/bin:$HOME/.local/bin:/usr/bin:$PATH"
    if PATH="$VALIDATOR_PATH" command -v tlapm >/dev/null 2>&1; then
        # Run TLAPS from the validator dir so its .tlacache lands locally.
        tlaps_out=$( cd "$VALIDATOR_TLA_DIR" \
            && PATH="$VALIDATOR_PATH" tlapm Validator.tla 2>&1 || true )
        # tlapm prints one "All N obligations proved." per module root; the
        # imported TLAPS.tla reports "All 0 obligation proved", so success is
        # a non-zero-obligation "All N obligations proved." for Validator.tla
        # with no "failed"/"omitted" anywhere.
        if grep -Eq "All [1-9][0-9]* obligations? proved\." <<<"$tlaps_out" \
           && ! grep -Eiq "obligation.*(failed|omitted)|[1-9][0-9]* (failed|omitted)" <<<"$tlaps_out"; then
            echo "PASS"
            passes=$((passes + 1))
        else
            echo "FAIL"
            failures=$((failures + 1))
            failed_specs+=("Validator(TLAPS)")
            echo "$tlaps_out" | tail -10 | sed 's/^/    /'
        fi
    else
        echo "SKIP (tlapm not on PATH)"
    fi
fi

echo
echo "Summary: $passes passed, $failures failed ($expected_refutations expected refutations exercised)"
if [[ $failures -gt 0 ]]; then
    echo "Failed specs:"
    printf '  - %s\n' "${failed_specs[@]}"
    exit 1
fi
