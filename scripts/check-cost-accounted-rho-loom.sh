#!/usr/bin/env bash
# Multi-prover cross-witness (LOCAL-ONLY, fail-soft): EXHAUSTIVE loom interleaving
# verification of the cost-accounting concurrency shadow models — the Rust-level
# complement to the TLA+ exhaustive models. Runs the isolated
# `cost-accounting-loom-models` crate (loom-only deps, so it builds under
# `--cfg loom`, unlike the rholang test crate whose tokio-tungstenite dep breaks
# under loom) so loom explores ALL thread interleavings of:
#   - concurrent disjoint-pool admission with no global lock (CA-P-171,
#     ↔ EvalScheduling.tla:DisjointPoolsAdmitConcurrentlyNoGlobalLock),
#   - the N-ary join's atomic combined-token debit (CA-P-052/108,
#     ↔ TokenGatedJoin.tla:Inv_M1_AtomicNoPartialPrefix).
#
# Fail-soft: absent cargo is reported and skipped (exit 0). A loom run that
# explores an interleaving violating an assertion IS a failure.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "Checking cost-accounted rho loom concurrency models (exhaustive --cfg loom)..."

if ! command -v cargo >/dev/null 2>&1; then
  echo "  cargo not found — skipped (fail-soft)."
  exit 0
fi

# `--cfg loom` makes loom explore all interleavings; -C target-cpu=native is
# preserved (RUSTFLAGS overrides .cargo/config.toml, and gxhash needs aes/sse2).
out="$(cd "$ROOT" && RUSTFLAGS="--cfg loom -C target-cpu=native" \
  LOOM_MAX_PREEMPTIONS="${LOOM_MAX_PREEMPTIONS:-3}" \
  timeout 900 cargo test -p cost-accounting-loom-models 2>&1)"
rc=$?

if printf '%s\n' "$out" | grep -qE 'error\[|error: could not compile'; then
  echo "  loom crate failed to build:" >&2
  printf '%s\n' "$out" | grep -E 'error' | tail -15 >&2
  exit 1
fi

fails="$(printf '%s\n' "$out" | grep -oE '[0-9]+ failed' | awk '{s+=$1} END{print s+0}')"
if [ "$rc" -ne 0 ] || [ "${fails:-1}" != "0" ]; then
  echo "  loom reported failures (rc=$rc, failed=$fails):" >&2
  printf '%s\n' "$out" | grep -E 'test result|FAILED|panicked' | tail -20 >&2
  exit 1
fi

passed="$(printf '%s\n' "$out" | grep -oE '[0-9]+ passed' | awk '{s+=$1} END{print s+0}')"
echo "  loom: all interleavings explored, $passed passed / 0 failed (concurrent-admission + atomic-join)."
echo "Loom concurrency cross-witness passed."
exit 0
