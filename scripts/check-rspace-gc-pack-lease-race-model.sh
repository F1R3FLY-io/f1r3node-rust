#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export TLC_REPO_ROOT="$repo_root"
export TLC_HEAP="${TLC_HEAP:-512m}"
export TLC_WORKERS="${TLC_WORKERS:-2}"
export TLC_MEMORY_HIGH="${TLC_MEMORY_HIGH:-1G}"
export TLC_RSS="${TLC_RSS:-4G}"
export TLC_CPU_QUOTA="${TLC_CPU_QUOTA:-200%}"
export TLC_TASKS_MAX="${TLC_TASKS_MAX:-64}"
export TLC_RUNTIME_MAX_SEC="${TLC_RUNTIME_MAX_SEC:-180}"
source "$repo_root/scripts/lib/tlc-run.sh"

model_dir="$repo_root/formal/tlaplus/stack_safe_pda"
evidence_dir="$repo_root/target/verification/rspace-gc-pack-lease-race"
mkdir -p "$evidence_dir"
cd "$model_dir"
metadir="$(tlc_metadir rspace-gc-pack-lease-race)"
sha256sum RSpaceGcPackLeaseRace.tla RSpaceGcPackLeaseRace*.cfg \
  "$repo_root/scripts/check-rspace-gc-pack-lease-race-model.sh"

baseline_log="$evidence_dir/baseline.log"
if ! tlc_run "$metadir" RSpaceGcPackLeaseRace.cfg RSpaceGcPackLeaseRace.tla -cleanup >"$baseline_log" 2>&1; then
  cat "$baseline_log"
  echo "gc-pack-lease-race: baseline failed" >&2
  exit 1
fi
if ! rg -Fq 'Model checking completed. No error has been found.' "$baseline_log"; then
  cat "$baseline_log"
  echo "gc-pack-lease-race: baseline did not complete cleanly" >&2
  exit 1
fi

witness_log="$evidence_dir/older-witness.log"
if tlc_run "$metadir" RSpaceGcPackLeaseRaceOlderWitness.cfg RSpaceGcPackLeaseRace.tla -cleanup >"$witness_log" 2>&1; then
  cat "$witness_log"
  echo "gc-pack-lease-race: older-epoch deferral witness was not reached" >&2
  exit 1
fi
if ! rg -Fq 'Invariant OlderLifecycleAbsent is violated' "$witness_log" ||
   ! rg -Fq 'The behavior up to this point is:' "$witness_log"; then
  cat "$witness_log"
  echo "gc-pack-lease-race: witness failed without the expected trace" >&2
  exit 1
fi
if ! awk '
  /^State [0-9]+: <BlockFirst / && !blocked { blocked = NR }
  /^State [0-9]+: <Release / && blocked && !released { released = NR }
  /^State [0-9]+: <RetireFirst / && released && !retired { retired = NR }
  /^State [0-9]+: <DeleteFirst / && retired { deleted = NR }
  END { exit !(blocked && released > blocked && retired > released && deleted > retired) }
' "$witness_log"; then
  cat "$witness_log"
  echo "gc-pack-lease-race: witness lacks blocked retry and deletion order" >&2
  exit 1
fi

mutant_log="$evidence_dir/exact-key-only-mutant.log"
if tlc_run "$metadir" RSpaceGcPackLeaseRaceExactKeyOnlyMutant.cfg RSpaceGcPackLeaseRace.tla -cleanup >"$mutant_log" 2>&1; then
  cat "$mutant_log"
  echo "gc-pack-lease-race: exact-key-only mutant unexpectedly passed" >&2
  exit 1
fi
if ! rg -Fq 'Invariant OlderEpochPackSafe is violated' "$mutant_log" ||
   ! rg -Fq 'The behavior up to this point is:' "$mutant_log"; then
  cat "$mutant_log"
  echo "gc-pack-lease-race: exact-key-only mutant missed old-epoch deletion" >&2
  exit 1
fi

echo "gc-pack-lease-race: baseline, older-epoch witness, and exact-key-only mutant passed"
