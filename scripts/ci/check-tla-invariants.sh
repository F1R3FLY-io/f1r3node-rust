#!/usr/bin/env bash
# scripts/ci/check-tla-invariants.sh — run TLC against the bounded
# post-fix MC configs under formal/tlaplus/slashing/ and assert clean.
#
# Reference: docs/theory/slashing/design/14-test-plan.md §14.6 / §14.9.
# Invokes the TLA+ model checker (TLC) against each MC instance:
#   • MC_EquivocationDetector{,_liveness}.tla / .cfg
#   • MC_EquivocationDetectorEager{,_3v}.tla / .cfg
#   • MC_ConcurrentTracker{,_pre_fix}.tla / .cfg
#   • MC_SlashFlow.tla / .cfg
#   • MC_TwoLevelSlashing.tla / .cfg
#   • MC_AuthorizedSlashFlow.tla / .cfg
#   • MC_JustificationProjection.tla / .cfg
#   • MC_WithdrawFlow.tla / .cfg
#
# A non-zero exit code from TLC for any post-fix configuration is a CI
# failure; the pre-fix configurations (e.g. MC_ConcurrentTracker_pre_fix)
# are *expected* to violate their invariants and are skipped here (they
# are the formal-side counter-examples, run manually for validation).
#
# MC_EquivocationDetector_safety is the exhaustive detector safety check.
# It is intentionally opt-in because it can run for many hours; run it
# with RUN_EXHAUSTIVE_TLA=1 after the shorter frontier has stabilized.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TLA_DIR="$REPO_ROOT/formal/tlaplus/slashing"

if [[ ! -d "$TLA_DIR" ]]; then
    echo "ERROR: TLA+ slashing directory not found at $TLA_DIR" >&2
    exit 2
fi

# Shared memory-bounded TLC launcher: on-disk metadir (NOT tmpfs), capped
# -Xmx heap, capped workers (this script previously used `-workers auto`,
# = 64 threads on this host, which multiplied TLC's peak frontier and —
# with a tmpfs metadir — OOM'd the machine on MC_SlashFlow), and a hard
# systemd MemoryMax ceiling. The slashing models are heavier than the
# cost_accounted_rho ones: the exhaustive MC_EquivocationDetector's
# fingerprint set and MC_SlashFlow's liveness graph want > 8 GB, so default
# to a roomier-but-still-bounded 16g heap / 24G ceiling / 8 workers (all
# overridable via TLC_HEAP / TLC_RSS / TLC_WORKERS). Anything that overflows
# the heap spills to the on-disk metadir (DiskFPSet) rather than RAM, and
# the 24G cgroup cap kills a runaway cleanly instead of OOM-ing the host.
# The helper also resolves TLC (jar or `tlc` wrapper), erroring if absent.
: "${TLC_HEAP:=16g}"
: "${TLC_RSS:=24G}"
: "${TLC_WORKERS:=8}"
export TLC_REPO_ROOT="$REPO_ROOT"
source "$REPO_ROOT/scripts/lib/tlc-run.sh"

# On-disk (NVMe) home for the per-config TLC logs — NOT /tmp, which is
# tmpfs (RAM) on this host.
LOG_DIR="$REPO_ROOT/target/slashing-tla-logs"
mkdir -p "$LOG_DIR"

# Clear this runner's on-disk metadirs up front (a SIGKILL'd prior run leaks
# them, since the EXIT trap can't fire on SIGKILL) and again on exit, so
# TLC's multi-GB state graphs don't accumulate on the NVMe across runs.
\rm -rf "$TLC_METADIR_ROOT"/slashing-* 2>/dev/null || true
trap '\rm -rf "$TLC_METADIR_ROOT"/slashing-* 2>/dev/null || true' EXIT

# Post-fix configs: each must TLC-clean.
POST_FIX_CONFIGS=(
    MC_EquivocationDetector
    MC_EquivocationDetector_liveness
    MC_EquivocationDetectorEager
    MC_EquivocationDetectorEager_3v
    MC_ConcurrentTracker
    MC_SlashFlow
    MC_TwoLevelSlashing
    MC_AuthorizedSlashFlow
    MC_JustificationProjection
    MC_WithdrawFlow
)

if [[ "${RUN_EXHAUSTIVE_TLA:-0}" == "1" ]]; then
    POST_FIX_CONFIGS+=(MC_EquivocationDetector_safety)
fi

cd "$TLA_DIR"

failed=0
for cfg in "${POST_FIX_CONFIGS[@]}"; do
    if [[ ! -f "$cfg.tla" || ! -f "$cfg.cfg" ]]; then
        echo "SKIP   $cfg (missing $cfg.tla or $cfg.cfg)"
        continue
    fi
    printf "CHECK  %-40s ... " "$cfg"
    if tlc_run "$(tlc_metadir "slashing-$cfg")" "$cfg.cfg" "$cfg.tla" >"$LOG_DIR/tlc-$cfg.log" 2>&1; then
        echo "ok"
    else
        echo "FAIL"
        echo "--- last 40 lines of $LOG_DIR/tlc-$cfg.log ---"
        tail -40 "$LOG_DIR/tlc-$cfg.log"
        echo "--- end log ---"
        failed=$((failed + 1))
    fi
    # Reclaim this model's on-disk state graph immediately (MC_SlashFlow
    # alone reaches ~11 GB); without this the per-model metadirs accumulate
    # on the NVMe until the EXIT trap. The log under $LOG_DIR is retained.
    \rm -rf "$TLC_METADIR_ROOT/slashing-$cfg" 2>/dev/null || true
done

if (( failed > 0 )); then
    echo "FAILED: $failed config(s) violated invariants."
    exit 1
fi

echo "All $((${#POST_FIX_CONFIGS[@]})) post-fix TLA+ configurations clean."
