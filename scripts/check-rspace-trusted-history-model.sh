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
source "$repo_root/scripts/lib/tlc-run.sh"

cd "$repo_root/formal/tlaplus/stack_safe_pda"
metadir="$(tlc_metadir rspace-trusted-history)"
model=RSpaceTrustedIncrementalHistory
tlc_run "$metadir" "$model.cfg" "$model.tla" -cleanup
for control in \
  "RSpaceTrustedIncrementalHistoryAdoptPreparedOnIndeterminateMutant.cfg:HistorySoleAuthority" \
  "RSpaceTrustedIncrementalHistorySuffixReplayMutant.cfg:HistoryAuthorityIsCatalogSelected" \
  "RSpaceTrustedIncrementalHistoryPathnameRecoveryMutant.cfg:HistoryAuthorityIsCatalogSelected" \
  "RSpaceTrustedIncrementalHistoryPathnameRereadMutant.cfg:RunAdoptedSuccessorIsExactCatalog" \
  "RSpaceTrustedIncrementalHistoryStaleWriterRestorationMutant.cfg:HistoryRecoveryRequiredAfterAmbiguity" \
  "RSpaceTrustedIncrementalHistoryAppendWithoutTruncationMutant.cfg:HistoryAppointedPrefixIsCommitted" \
  "RSpaceTrustedIncrementalHistoryStaleCoreRetryMutant.cfg:HistoryCommitsAtMostOnce" \
  "RSpaceTrustedIncrementalHistoryTruncateBelowAppointedMutant.cfg:HistoryPinnedPrefixStable" \
  "RSpaceTrustedIncrementalHistoryTruncateOnRefusalMutant.cfg:HistoryRefusalPreservesFiles" \
  "RSpaceTrustedIncrementalHistoryMisclassifiedCatalogWriteMutant.cfg:RunOutcomeBoundaryIsExact" \
  "RSpaceTrustedIncrementalHistoryNeverRecoveredSuccessorAfterAmbiguity.cfg:NeverRecoveredSuccessorAfterAmbiguity" \
  "RSpaceTrustedIncrementalHistoryNeverRecoveredPredecessorAfterAmbiguity.cfg:NeverRecoveredPredecessorAfterAmbiguity" \
  "RSpaceTrustedIncrementalHistoryNeverRetriedAfterSuffix.cfg:NeverRetriedAfterSuffix" \
  "RSpaceTrustedIncrementalHistoryNeverReaderAcrossFault.cfg:NeverReaderAcrossFault" \
  "RSpaceTrustedIncrementalHistoryNeverRestartedMidPublication.cfg:NeverRestartedMidPublication" \
  "RSpaceTrustedIncrementalHistoryNeverRecoveredUnderDecoy.cfg:NeverRecoveredUnderDecoy" \
  "RSpaceTrustedIncrementalHistoryNeverRefusedWithSuffix.cfg:NeverRefusedWithSuffix" \
  "RSpaceTrustedIncrementalHistoryNeverRepairedForeignCatalog.cfg:NeverRepairedForeignCatalog"
do
  config="${control%%:*}"
  invariant="${control#*:}"
  if output="$(tlc_run "$metadir" "$config" "$model.tla" -cleanup 2>&1)"; then
    printf '%s\n' "$output"
    echo "trusted-history: expected counterexample was absent: $config" >&2
    exit 1
  fi
  printf '%s\n' "$output"
  if ! rg -q "Invariant ${invariant} is violated" <<<"$output"; then
    echo "trusted-history: $config failed without its expected invariant: $invariant" >&2
    exit 1
  fi
done
echo "trusted-history: control, ten causal mutants and eight reachability witnesses passed"
