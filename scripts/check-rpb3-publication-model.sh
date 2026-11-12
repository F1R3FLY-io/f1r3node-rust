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
export TLC_RUNTIME_MAX_SEC="${TLC_RUNTIME_MAX_SEC:-180}"
source "$repo_root/scripts/lib/tlc-run.sh"

model=RSpaceRPB3CanonicalAdmissionPublication
model_dir="$repo_root/formal/tlaplus/stack_safe_pda"
evidence_dir="$repo_root/target/verification/rpb3-publication"
mkdir -p "$evidence_dir"
cd "$model_dir"
metadir="$(tlc_metadir rpb3-publication)"
sha256sum "$model.tla" "$model.cfg" \
  "${model}MalformedMutant.cfg" "${model}UnsealedMutant.cfg" "${model}ObservationMutant.cfg" \
  "${model}PartialClosureMutant.cfg" \
  "$repo_root/scripts/check-rpb3-publication-model.sh"

baseline_invariants=(
  TypeInvariant
  MalformedInputCannotBeAdmitted
  MalformedRefusalPreservesEveryNamespace
  UnpublishedImportHasNoRootAuthority
  UnpublishedCheckpointPreservesEveryNamespace
  PublicationRequiresAdmissionAndSeal
  PublicationMatchesTypedOracle
  EveryKnownRootHasCompleteClosureAndExactObservation
  CurrentRootIsKnown
  GenerationAdvancesOnlyOnActivation
)
for invariant in "${baseline_invariants[@]}"; do
  if ! rg -Fxq "INVARIANT $invariant" "$model.cfg"; then
    echo "rpb3-publication: the baseline configuration does not check $invariant" >&2
    exit 1
  fi
done
configured="$(rg -c '^INVARIANT ' "$model.cfg" || true)"
if [[ "$configured" != "${#baseline_invariants[@]}" ]]; then
  echo "rpb3-publication: the baseline configuration checks ${configured:-0} invariants, expected ${#baseline_invariants[@]}" >&2
  exit 1
fi
if ! rg -Fxq 'SPECIFICATION Spec' "$model.cfg"; then
  echo "rpb3-publication: the baseline configuration does not check Spec" >&2
  exit 1
fi

baseline_log="$evidence_dir/baseline.log"
if ! tlc_run "$metadir" "$model.cfg" "$model.tla" -cleanup >"$baseline_log" 2>&1; then
  cat "$baseline_log"
  echo "rpb3-publication: baseline failed" >&2
  exit 1
fi
if ! rg -Fq 'Model checking completed. No error has been found.' "$baseline_log"; then
  cat "$baseline_log"
  echo "rpb3-publication: baseline did not complete cleanly" >&2
  exit 1
fi
states="$(rg -o '[0-9]+ states generated, [0-9]+ distinct states found, 0 states left on queue' "$baseline_log" | tail -n 1 || true)"
if [[ -z "$states" ]]; then
  cat "$baseline_log"
  echo "rpb3-publication: baseline reports no exhausted state count" >&2
  exit 1
fi

for mutant in \
  'Malformed:MalformedAdmissionMutantSpec:MalformedInputCannotBeAdmitted:AdmitMalformedMutant' \
  'Unsealed:UnsealedImportMutantSpec:PublicationRequiresAdmissionAndSeal:PublishUnsealedImportMutant' \
  'Observation:WrongObservationMutantSpec:PublicationMatchesTypedOracle:PublishWrongObservationMutant' \
  'PartialClosure:PartialClosureMutantSpec:EveryKnownRootHasCompleteClosureAndExactObservation:PublishPartialClosureMutant'
do
  IFS=: read -r variant specification invariant action <<<"$mutant"
  config="${model}${variant}Mutant.cfg"
  log="$evidence_dir/$variant-mutant.log"
  if ! rg -Fxq "SPECIFICATION $specification" "$config" ||
     ! rg -Fxq "INVARIANT $invariant" "$config"; then
    echo "rpb3-publication: $config does not check $invariant under $specification" >&2
    exit 1
  fi
  if tlc_run "$metadir" "$config" "$model.tla" -cleanup >"$log" 2>&1; then
    cat "$log"
    echo "rpb3-publication: $variant mutant unexpectedly passed" >&2
    exit 1
  fi
  if ! rg -Fq "Invariant $invariant is violated" "$log" ||
     ! rg -Fq 'The behavior up to this point is:' "$log"; then
    cat "$log"
    echo "rpb3-publication: $variant mutant failed without a trace violating $invariant" >&2
    exit 1
  fi
  if ! rg -q "^State [0-9]+: <$action " "$log"; then
    cat "$log"
    echo "rpb3-publication: $variant mutant trace does not take $action" >&2
    exit 1
  fi
done

echo "rpb3-publication: baseline passed ($states) and four mutants violated their invariants; logs in $evidence_dir"
