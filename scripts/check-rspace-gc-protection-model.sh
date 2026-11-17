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
sha256sum RSpaceGcProtection.tla "$repo_root/scripts/check-rspace-gc-protection-model.sh"

for baseline in \
  'baseline:RSpaceGcProtection.cfg' \
  'known-baseline:RSpaceGcProtectionKnown.cfg'
do
  label="${baseline%%:*}"
  config="${baseline#*:}"
  baseline_log="$evidence_dir/$label.log"
  if ! tlc_run "$metadir" "$config" RSpaceGcProtection.tla -cleanup >"$baseline_log" 2>&1; then
    cat "$baseline_log"
    echo "gc-protection: $label TLC failed" >&2
    exit 1
  fi
  if ! rg -Fq 'Model checking completed. No error has been found.' "$baseline_log"; then
    cat "$baseline_log"
    echo "gc-protection: $label TLC did not report a completed clean check" >&2
    exit 1
  fi
done

witness_log="$evidence_dir/known-overlap-witness.log"
if tlc_run "$metadir" RSpaceGcProtectionKnownOverlapWitness.cfg RSpaceGcProtection.tla -cleanup >"$witness_log" 2>&1; then
  cat "$witness_log"
  echo "gc-protection: known-overlap lifecycle witness was not reached" >&2
  exit 1
fi
if ! rg -Fq 'Invariant KnownOverlapLifecycleAbsent is violated' "$witness_log"; then
  cat "$witness_log"
  echo "gc-protection: known-overlap witness failed without its expected counterexample" >&2
  exit 1
fi
if ! rg -Fq 'The behavior up to this point is:' "$witness_log"; then
  cat "$witness_log"
  echo "gc-protection: known-overlap witness has no trace" >&2
  exit 1
fi
if ! awk '
  /^State [0-9]+: <Crash / && !crash { crash = NR }
  /^State [0-9]+: <Recover / && crash && !recover { recover = NR }
  /^State [0-9]+: <Reclaim / && recover && !reclaim { reclaim = NR }
  /^State [0-9]+: <AcquireNewPin / && reclaim && !new_pin { new_pin = NR }
  END { exit !(crash && recover > crash && reclaim > recover && new_pin > reclaim) }
' "$witness_log"; then
  cat "$witness_log"
  echo "gc-protection: known-overlap witness lacks crash, recovery, reclaim, then new pin" >&2
  exit 1
fi

for mutant in \
  'OmitCoupledFacetMutant:PublishedClosureProtected' \
  'IgnoreExactPinMutant:ExactOldPinsProtected' \
  'IgnoreDurableExactPinMutant:DurableExactGenerationProtected' \
  'OmitKnownClosureMutant:KnownPublicationClosureProtected' \
  'PrunePastWalStartMutant:WalFrontierProtected' \
  'AdmitPastWalWindowMutant:WalAdmissionBounded' \
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

echo "gc-protection: both baselines, overlap witness, and seven causal mutants passed; logs in $evidence_dir"
