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

model_dir="$repo_root/formal/tlaplus/stack_safe_pda"
evidence_dir="$repo_root/target/verification/rspace-gc-protection"
mkdir -p "$evidence_dir"
cd "$model_dir"
metadir="$(tlc_metadir rspace-gc-protection)"

baseline_log="$evidence_dir/baseline.log"
if ! tlc_run "$metadir" RSpaceGcProtection.cfg RSpaceGcProtection.tla -cleanup >"$baseline_log" 2>&1; then
  cat "$baseline_log"
  echo "gc-protection: baseline TLC failed" >&2
  exit 1
fi
if ! rg -Fq 'Model checking completed. No error has been found.' "$baseline_log"; then
  cat "$baseline_log"
  echo "gc-protection: baseline TLC did not report a completed clean check" >&2
  exit 1
fi

for mutant in \
  'OmitCoupledFacetMutant:PublishedClosureProtected' \
  'IgnoreExactPinMutant:ExactOldPinsProtected' \
  'IgnoreDurableExactPinMutant:DurableExactGenerationProtected' \
  'PrunePastWalStartMutant:WalFrontierProtected' \
  'BlockWalOnOldPackMutant:ObjectOnlyPinDoesNotBlockWal'
do
  variant="${mutant%%:*}"
  invariant="${mutant#*:}"
  log="$evidence_dir/$variant.log"
  if tlc_run "$metadir" "RSpaceGcProtection$variant.cfg" RSpaceGcProtection.tla -cleanup >"$log" 2>&1; then
    cat "$log"
    echo "gc-protection: $variant unexpectedly passed" >&2
    exit 1
  fi
  if ! rg -Fq "Invariant $invariant is violated" "$log"; then
    cat "$log"
    echo "gc-protection: $variant failed without expected invariant $invariant" >&2
    exit 1
  fi
  if ! rg -Fq 'The behavior up to this point is:' "$log"; then
    cat "$log"
    echo "gc-protection: $variant has no counterexample trace" >&2
    exit 1
  fi
done

echo "gc-protection: baseline and five causal mutants passed; logs in $evidence_dir"
