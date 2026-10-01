#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
report_dir="$repo_root/target/verification/pathmap-anchor-loom"
mkdir -p "$report_dir"

command=(cargo test -p pathmap-anchor-loom-models -- --test-threads=1)
if command -v systemd-run >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1; then
  report="$(mktemp "$report_dir/run.XXXXXX.log")"
  if systemd-run --user --scope \
    -p MemoryHigh=1G -p MemoryMax=1536M -p MemorySwapMax=0 -p CPUQuota=100% \
    -- env RUSTFLAGS="--cfg loom -C target-cpu=native" \
    LOOM_MAX_PREEMPTIONS="${LOOM_MAX_PREEMPTIONS:-3}" \
    "${command[@]}" >"$report" 2>&1; then
    printf 'pathmap-anchor-loom: raw log %s\n' "$report"
  else
    status=$?
    tail -n 80 "$report" >&2
    printf 'pathmap-anchor-loom: raw log %s\n' "$report" >&2
    exit "$status"
  fi
else
  echo "pathmap-anchor-loom: systemd user scopes are required for bounded verification" >&2
  exit 3
fi

if grep -qE 'error\[|error: could not compile|FAILED|panicked' "$report"; then
  tail -n 80 "$report" >&2
  printf 'pathmap-anchor-loom: raw log %s\n' "$report" >&2
  exit 1
fi

grep -E 'test result:|pathmap-anchor-loom-models' "$report"
echo "PathMap anchor Loom schedules passed."
