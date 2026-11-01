#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export TLC_REPO_ROOT="$repo_root"
export TLC_HEAP="${TLC_HEAP:-512m}"
export TLC_WORKERS="${TLC_WORKERS:-2}"
export TLC_MEMORY_HIGH="${TLC_MEMORY_HIGH:-1G}"
export TLC_RSS="${TLC_RSS:-2G}"
export TLC_CPU_QUOTA="${TLC_CPU_QUOTA:-200%}"
export TLC_TASKS_MAX="${TLC_TASKS_MAX:-64}"
export TLC_RUNTIME_MAX_SEC="${TLC_RUNTIME_MAX_SEC:-120}"
# shellcheck source=scripts/lib/tlc-run.sh
source "$repo_root/scripts/lib/tlc-run.sh"

cd "$repo_root/formal/tlaplus/stack_safe_pda"
model=RSpaceDagStateTransfer
metadir="$(tlc_metadir rspace-dag-state-transfer)"

fail() {
  echo "dag-state-transfer: $*" >&2
  exit 1
}

rg -qx 'EXTENDS RSpaceAffineImportLifecycle' "$model.tla" ||
  fail "the model no longer extends RSpaceAffineImportLifecycle"
for inherited in TypeInvariant SealRequiresCompleteClosure PublicationRequiresConsumedSeal \
  CurrentRootIsVerified AtMostOneRacingActivationPublishes ActivationGenerationIsCASProtected \
  PreserveCurrentNeverMovesAuthority
do
  rg -qx "INVARIANT $inherited" "$model.cfg" ||
    fail "the faithful configuration dropped the inherited invariant $inherited"
done
for property in AffineLifecycleIsPreserved FrontierAdvancesByOne; do
  rg -qx "PROPERTY $property" "$model.cfg" ||
    fail "the faithful configuration dropped the property $property"
done

if ! output="$(tlc_run "$metadir" "$model.cfg" "$model.tla" -cleanup 2>&1)"; then
  printf '%s\n' "$output"
  fail "the faithful model failed"
fi
printf '%s\n' "$output"
rg -qF 'Model checking completed. No error has been found.' <<<"$output" ||
  fail "the faithful model did not complete"

for control in \
  "CrossVersionResumeMutant:StreamMatchesSessionIdentity" \
  "SkippedFrontierMutant:FrontierIsContiguous" \
  "MixedSessionMutant:StagingIsSessionBound" \
  "StateVersionOnlyResumeMutant:SessionBindingIsExact" \
  "ResumeWithoutPinMutant:ServingIsPinned" \
  "ReclaimPinnedMutant:PinnedGenerationsAreRetained" \
  "LegacyFallbackMutant:SessionIdentityIsSupported" \
  "OlderAuthorityMutant:StreamMatchesSessionIdentity" \
  "EarlyAdmissionMutant:ColdAdmissionRequiresCompleteClosure" \
  "FrontierBeforeObjectsMutant:FrontierIsContiguous" \
  "AffineIncompleteAdoptionMutant:AdoptionRequiresAdmittedClosure" \
  "AbortAfterAdmissionMutant:NoSessionHoldsNothing" \
  "NeverAdoptsCurrent:NeverAdoptsCurrent" \
  "NeverAdoptsKnown:NeverAdoptsKnown" \
  "NeverRefusesLegacyOnlyPeer:NeverRefusesLegacyOnlyPeer" \
  "NeverRefusesUnsupportedBridge:NeverRefusesUnsupportedBridge" \
  "NeverRefusesProfileMismatch:NeverRefusesProfileMismatch" \
  "NeverResumesAfterDisconnect:NeverResumesAfterDisconnect" \
  "NeverRefusesResumeOnLegacyOnlyPeer:NeverRefusesResumeOnLegacyOnlyPeer" \
  "NeverRefusesResumeOnChangedTransferVersion:NeverRefusesResumeOnChangedTransferVersion" \
  "NeverRefusesResumeOnBridgeChange:NeverRefusesResumeOnBridgeChange" \
  "NeverRefusesResumeOnProfileChange:NeverRefusesResumeOnProfileChange" \
  "NeverRefusesResumeAfterPinLapse:NeverRefusesResumeAfterPinLapse" \
  "NeverResumesAcrossSourcePublication:NeverResumesAcrossSourcePublication" \
  "NeverStagesBeyondFrontier:NeverStagesBeyondFrontier" \
  "NeverAdoptsAfterAbort:NeverAdoptsAfterAbort" \
  "NeverRejectsTransferActivation:NeverRejectsTransferActivation" \
  "NeverReclaimsSupersededGeneration:NeverReclaimsSupersededGeneration"
do
  config="${control%%:*}"
  invariant="${control#*:}"
  if output="$(tlc_run "$metadir" "$model$config.cfg" "$model.tla" -cleanup 2>&1)"; then
    printf '%s\n' "$output"
    fail "expected counterexample was absent: $config"
  fi
  printf '%s\n' "$output"
  rg -qF "Invariant $invariant is violated" <<<"$output" ||
    fail "$config failed without its expected invariant: $invariant"
done

for control in \
  "LifecyclePropertyMutant:Action property line 95, col 17 to line 95, col 29 of module RSpaceAffineImportLifecycle is violated" \
  "FrontierPropertyMutant:Action property FrontierAdvancesByOne is violated"
do
  config="${control%%:*}"
  expected="${control#*:}"
  if output="$(tlc_run "$metadir" "$model$config.cfg" "$model.tla" -cleanup 2>&1)"; then
    printf '%s\n' "$output"
    fail "expected counterexample was absent: $config"
  fi
  printf '%s\n' "$output"
  rg -qF "$expected" <<<"$output" ||
    fail "$config failed without its expected property violation: $expected"
done

echo "dag-state-transfer: faithful model, twelve mutants, two property controls and sixteen reachability witnesses passed"
