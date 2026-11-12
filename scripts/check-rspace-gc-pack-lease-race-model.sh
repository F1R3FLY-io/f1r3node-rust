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

for invariant in PinnedReadReturnsAdmittedBytes DeadProcessHoldsNothing; do
  if ! rg -q "^[[:space:]]+${invariant}\$" RSpaceGcPackLeaseRace.cfg; then
    echo "gc-pack-lease-race: baseline does not check $invariant" >&2
    exit 1
  fi
done

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

# A foreign unlink: one process closes its owner while a read handle stays,
# another process opens the store, retires the range in its own registry and
# removes an entry that the handle still covers.
foreign_unlink_order() {
  awk '
    /^State [0-9]+: <CloseOwner / && !closed { closed = NR }
    /^State [0-9]+: <OpenStore / && closed && !opened { opened = NR }
    /^State [0-9]+: <RetireFirst / && opened && !retired { retired = NR }
    /^State [0-9]+: <DeleteFirst / && retired { deleted = NR }
    END { exit !(closed && opened > closed && retired > opened && deleted > retired) }
  ' "$1"
}

reopen_log="$evidence_dir/path-reopen-mutant.log"
if tlc_run "$metadir" RSpaceGcPackLeaseRacePathReopenMutant.cfg RSpaceGcPackLeaseRace.tla -cleanup >"$reopen_log" 2>&1; then
  cat "$reopen_log"
  echo "gc-pack-lease-race: path-reopen mutant unexpectedly passed" >&2
  exit 1
fi
if ! rg -Fq 'Invariant PinnedReadReturnsAdmittedBytes is violated' "$reopen_log" ||
   ! rg -Fq 'The behavior up to this point is:' "$reopen_log" ||
   ! foreign_unlink_order "$reopen_log" ||
   rg -q '^/\\ rewritten = \{[^}]' "$reopen_log"; then
  cat "$reopen_log"
  echo "gc-pack-lease-race: path-reopen mutant missed the read by name after a foreign unlink" >&2
  exit 1
fi

rewrite_log="$evidence_dir/rewrite-in-place-mutant.log"
if tlc_run "$metadir" RSpaceGcPackLeaseRaceRewriteInPlaceMutant.cfg RSpaceGcPackLeaseRace.tla -cleanup >"$rewrite_log" 2>&1; then
  cat "$rewrite_log"
  echo "gc-pack-lease-race: rewrite-in-place mutant unexpectedly passed" >&2
  exit 1
fi
if ! rg -Fq 'Invariant PinnedReadReturnsAdmittedBytes is violated' "$rewrite_log" ||
   ! rg -Fq 'The behavior up to this point is:' "$rewrite_log" ||
   ! foreign_unlink_order "$rewrite_log" ||
   ! rg -q '^/\\ rewritten = \{[^}]' "$rewrite_log"; then
  cat "$rewrite_log"
  echo "gc-pack-lease-race: rewrite-in-place mutant missed the read of rewritten bytes" >&2
  exit 1
fi

echo "gc-pack-lease-race: baseline, older-epoch witness, exact-key-only mutant, path-reopen mutant, and rewrite-in-place mutant passed"
