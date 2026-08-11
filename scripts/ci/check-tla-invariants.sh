#!/usr/bin/env bash
# scripts/ci/check-tla-invariants.sh — run TLC against the bounded
# post-fix MC configs under formal/tlaplus/ and assert clean.
#
# Reference: docs/theory/slashing/design/14-test-plan.md §14.6 / §14.9.
# Invokes the TLA+ model checker (TLC) against each MC instance:
#   • slashing/MC_EquivocationDetector_liveness{,_2v}.tla / .cfg
#   • slashing/MC_EquivocationDetectorEager{,_3v2s}.tla / .cfg
#   • slashing/MC_ConcurrentTracker{,_pre_fix}.tla / .cfg
#   • slashing/MC_SlashFlow.tla / .cfg   (exhaustive tier — see below)
#   • slashing/MC_SlashFlowRedeem.tla / .cfg
#   • slashing/MC_TwoLevelSlashing.tla / .cfg
#   • slashing/MC_AuthorizedSlashFlow.tla / .cfg
#   • slashing/MC_JustificationProjection.tla / .cfg
#   • slashing/MC_WithdrawFlow.tla / .cfg
#   • block_admission/MC_BlockAdmission.tla / .cfg
#   • deploy_occurrence/MC_DeployOccurrence.tla / .cfg
#
# A non-zero exit code from TLC for any post-fix configuration is a CI
# failure; the pre-fix configurations (e.g. MC_ConcurrentTracker_pre_fix)
# are *expected* to violate their invariants and are skipped here (they
# are the formal-side counter-examples, run manually for validation).
#
# The exhaustive tier (RUN_EXHAUSTIVE_TLA=1) holds the configs whose state
# spaces exceed the per-config wall-clock cap: MC_EquivocationDetector and
# MC_EquivocationDetectorEager_3v hit the 45m cap on every nightly since
# the schedule began (2026-07-25) — their interleaved liveness passes go
# superlinear past ~100M states — alongside MC_EquivocationDetector_safety,
# which can run for many hours. The nightly tier therefore gates on the
# fast configs only; the heavy pair runs opt-in until it gets the
# liveness/safety split that rescued MC_EquivocationDetector_liveness.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TLA_ROOT="$REPO_ROOT/formal/tlaplus"

if [[ ! -d "$TLA_ROOT/slashing" ]]; then
    echo "ERROR: TLA+ slashing directory not found at $TLA_ROOT/slashing" >&2
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

# Post-fix configs: each must TLC-clean. Entries are
# <subdir under formal/tlaplus>/<config basename>.
POST_FIX_CONFIGS=(
    slashing/MC_EquivocationDetector_liveness
    slashing/MC_EquivocationDetector_liveness_2v
    slashing/MC_EquivocationDetectorEager
    slashing/MC_EquivocationDetectorEager_3v2s
    slashing/MC_ConcurrentTracker
    slashing/MC_SlashFlowRedeem
    slashing/MC_TwoLevelSlashing
    slashing/MC_AuthorizedSlashFlow
    slashing/MC_JustificationProjection
    slashing/MC_WithdrawFlow
    block_admission/MC_BlockAdmission
    deploy_occurrence/MC_DeployOccurrence
)

if [[ "${RUN_EXHAUSTIVE_TLA:-0}" == "1" ]]; then
    POST_FIX_CONFIGS+=(
        slashing/MC_EquivocationDetector
        slashing/MC_EquivocationDetectorEager_3v
        slashing/MC_EquivocationDetector_safety
        # Stage-C MC_SlashFlow outgrew the 45m nightly cap (2026-08-08: 45m
        # TIMEOUT niced/4-workers; a dedicated 24g/12-worker run explored
        # ~200M states to depth 22 with ZERO violations and the frontier
        # still growing — the full space at these bounds is not nightly
        # material). The nightly gate keeps MC_SlashFlowRedeem (seconds) as
        # the bounded TLC cross-check and the TLAPS SlashFlowProofs section
        # below as the DEDUCTIVE authority for the safety invariant (that
        # section's own comment already documents this division of labor:
        # the full SlashFlow space "is too large to exhaustively enumerate").
        slashing/MC_SlashFlow
    )
fi

# Per-config wall-clock cap: one wedged or state-exploded config must not
# consume the whole job silently (observed: the first config alone exceeded
# a 60-minute CI job with no output). Timeouts are reported distinctly from
# invariant violations. The cap is enforced INSIDE tlc_run (TLC_WALL_TIMEOUT
# in scripts/lib/tlc-run.sh) so it composes with the memory bound — the
# `timeout` sits between systemd-run/prlimit and the JVM, TERM reaches java
# directly, and exit 124 propagates. Override via TLC_PER_CONFIG_TIMEOUT
# (GNU timeout duration syntax); skipped when `timeout` is unavailable.
TLC_PER_CONFIG_TIMEOUT="${TLC_PER_CONFIG_TIMEOUT:-45m}"

# Registered entries are hand-maintained above: a malformed entry or a
# missing config file is a broken registration, not a skippable condition —
# a silent SKIP here would let a renamed or deleted model quietly leave CI.
failed=0
timeouts=0
violations=0
for entry in "${POST_FIX_CONFIGS[@]}"; do
    if [[ "$entry" != */* ]]; then
        echo "ERROR: malformed POST_FIX_CONFIGS entry '$entry' (expected <subdir>/<config>)" >&2
        exit 2
    fi
    dir="$TLA_ROOT/${entry%/*}"
    cfg="${entry##*/}"
    log="$LOG_DIR/tlc-${entry//\//-}.log"
    if [[ ! -f "$dir/$cfg.tla" || ! -f "$dir/$cfg.cfg" ]]; then
        echo "FAIL   $entry (missing $cfg.tla or $cfg.cfg in $dir — registered config not found)"
        failed=$((failed + 1))
        violations=$((violations + 1))
        continue
    fi
    started_epoch="$(date +%s)"
    echo "CHECK  $entry (started $(date -u +%H:%M:%SZ), cap $TLC_PER_CONFIG_TIMEOUT)"
    set +e
    (cd "$dir" && TLC_WALL_TIMEOUT="$TLC_PER_CONFIG_TIMEOUT" \
        tlc_run "$(tlc_metadir "slashing-${entry//\//-}")" "$cfg.cfg" "$cfg.tla") >"$log" 2>&1
    status=$?
    set -e
    elapsed="$(( $(date +%s) - started_epoch ))s"
    if (( status == 0 )); then
        echo "OK     $entry ($elapsed)"
    elif (( status == 124 )); then
        echo "TIMEOUT $entry after $elapsed (cap $TLC_PER_CONFIG_TIMEOUT) — treat as failure; profile or split the config"
        failed=$((failed + 1))
        timeouts=$((timeouts + 1))
    else
        echo "FAIL   $entry ($elapsed)"
        echo "--- last 40 lines of $log ---"
        tail -40 "$log"
        echo "--- end log ---"
        failed=$((failed + 1))
        violations=$((violations + 1))
    fi
    # Reclaim this model's on-disk state graph immediately (MC_SlashFlow
    # alone reaches ~11 GB); without this the per-model metadirs accumulate
    # on the NVMe until the EXIT trap. The log under $LOG_DIR is retained.
    \rm -rf "$TLC_METADIR_ROOT/slashing-${entry//\//-}" 2>/dev/null || true
done

# Redemption-un-halt invariant (Inv_RedeemedValidatorUnhalted): the full
# MC_SlashFlow state space is too large to exhaustively enumerate (its TLC run
# above checks it only as far as it gets), so this safety invariant is
# established DEDUCTIVELY by TLAPS — no state search (cannot OOM) and proved for
# ALL parameter values — via the inductive invariant in SlashFlowProofs.tla
# (IndInv = TypeOK /\ active=>bonded /\ active=>not-halted). This mirrors how the
# cost-accounting gate discharges the validator contract with `tlapm`. The tiny
# MC_SlashFlowRedeem above is the bounded TLC cross-check. Local-only.
SLASHING_TLAPS_PATH="$HOME/.local/tlaps/bin:$HOME/.local/bin:/usr/bin:$PATH"
if PATH="$SLASHING_TLAPS_PATH" command -v tlapm >/dev/null 2>&1; then
    printf "PROVE  %-40s ... " "SlashFlowProofs (TLAPS)"
    tlaps_out=$( cd "$TLA_ROOT/slashing" && PATH="$SLASHING_TLAPS_PATH" tlapm SlashFlowProofs.tla 2>&1 || true )
    if echo "$tlaps_out" | grep -Eq "All [1-9][0-9]* obligations? proved\." \
       && ! echo "$tlaps_out" | grep -Eiq "obligation.*(failed|omitted)|[1-9][0-9]* (failed|omitted)"; then
        echo "ok"
    else
        echo "FAIL"
        echo "$tlaps_out" | tail -20 | sed 's/^/    /'
        failed=$((failed + 1))
    fi
else
    echo "SKIP   SlashFlowProofs (tlapm not on PATH)"
fi

if (( failed > 0 )); then
    echo "FAILED: $failed config(s) did not verify — $timeouts cap timeout(s), $violations violation-or-error(s)."
    exit 1
fi

echo "All $((${#POST_FIX_CONFIGS[@]})) post-fix TLA+ configurations clean (+ SlashFlowProofs TLAPS)."
