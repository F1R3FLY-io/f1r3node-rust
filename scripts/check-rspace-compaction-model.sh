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
evidence_dir="$repo_root/target/verification/rspace-compaction"
mkdir -p "$evidence_dir"
cd "$model_dir"
metadir="$(tlc_metadir rspace-compaction)"
sha256sum RSpaceCompaction.tla RSpaceCompaction*.cfg \
  "$repo_root/scripts/check-rspace-compaction-model.sh"

baseline_log="$evidence_dir/baseline.log"
if ! tlc_run "$metadir" RSpaceCompaction.cfg RSpaceCompaction.tla -cleanup >"$baseline_log" 2>&1; then
  cat "$baseline_log"
  echo "rspace-compaction: baseline failed" >&2
  exit 1
fi
if ! rg -Fq 'Model checking completed. No error has been found.' "$baseline_log"; then
  cat "$baseline_log"
  echo "rspace-compaction: baseline did not complete cleanly" >&2
  exit 1
fi

mutant_log="$evidence_dir/skip-leased-closure-mutant.log"
if tlc_run "$metadir" RSpaceCompactionSkipLeasedClosureMutant.cfg RSpaceCompaction.tla -cleanup >"$mutant_log" 2>&1; then
  cat "$mutant_log"
  echo "rspace-compaction: skip-leased-closure mutant unexpectedly passed" >&2
  exit 1
fi
if ! rg -Fq 'Invariant LeaseSafety is violated' "$mutant_log" ||
   ! rg -Fq 'The behavior up to this point is:' "$mutant_log"; then
  cat "$mutant_log"
  echo "rspace-compaction: mutant failed without the expected lease-safety trace" >&2
  exit 1
fi
if ! awk '
  /^State [0-9]+: <Publish / && !published { published = NR }
  /^State [0-9]+: <AcquireLease / && published && !leased { leased = NR }
  /^State [0-9]+: <DeleteOld / && leased { deleted = NR }
  END { exit !(published && leased > published && deleted > leased) }
' "$mutant_log"; then
  cat "$mutant_log"
  echo "rspace-compaction: mutant trace lacks publication, lease, then deletion" >&2
  exit 1
fi

echo "rspace-compaction: baseline and leased-closure mutant passed; logs in $evidence_dir"
