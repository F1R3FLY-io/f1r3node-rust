#!/usr/bin/env bash
# scripts/ci/check-tla-invariants.sh — run TLC against the bounded
# post-fix MC configs under formal/tlaplus/slashing/ and assert clean.
#
# Reference: docs/theory/slashing/design/14-test-plan.md §14.6 / §14.9.
# Invokes the TLA+ model checker (TLC) against each MC instance:
#   • MC_EquivocationDetector_liveness.tla / .cfg
#   • MC_EquivocationDetectorEager.tla / .cfg
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
TLA_DIR="$REPO_ROOT/formal/tlaplus/slashing"

if [[ ! -d "$TLA_DIR" ]]; then
    echo "ERROR: TLA+ slashing directory not found at $TLA_DIR" >&2
    exit 2
fi

# Locate TLC. Common installation paths:
#   • $TLA_TOOLS_JAR pointing at tla2tools.jar (preferred, explicit)
#   • Java + tla2tools.jar in /usr/share/tla / /opt/tlaplus / ~/.tla
#   • `tlc` wrapper script on PATH
TLC_CMD=""
if [[ -n "${TLA_TOOLS_JAR:-}" && -f "$TLA_TOOLS_JAR" ]]; then
    TLC_CMD="java -XX:+UseParallelGC -jar $TLA_TOOLS_JAR"
elif command -v tlc >/dev/null 2>&1; then
    TLC_CMD="tlc"
else
    for candidate in \
        /usr/share/tla/tla2tools.jar \
        /opt/tlaplus/tla2tools.jar \
        "$HOME/.tla/tla2tools.jar"
    do
        if [[ -f "$candidate" ]]; then
            TLC_CMD="java -XX:+UseParallelGC -jar $candidate"
            break
        fi
    done
fi

if [[ -z "$TLC_CMD" ]]; then
    echo "ERROR: TLC not found. Set TLA_TOOLS_JAR=/path/to/tla2tools.jar," >&2
    echo "       install tlaplus, or place the jar at one of: " >&2
    echo "         /usr/share/tla/tla2tools.jar" >&2
    echo "         /opt/tlaplus/tla2tools.jar" >&2
    echo "         ~/.tla/tla2tools.jar" >&2
    exit 3
fi

# Post-fix configs: each must TLC-clean.
POST_FIX_CONFIGS=(
    MC_EquivocationDetector_liveness
    MC_EquivocationDetector_liveness_2v
    MC_EquivocationDetectorEager
    MC_EquivocationDetectorEager_3v2s
    MC_ConcurrentTracker
    MC_SlashFlow
    MC_TwoLevelSlashing
    MC_AuthorizedSlashFlow
    MC_JustificationProjection
    MC_WithdrawFlow
)

if [[ "${RUN_EXHAUSTIVE_TLA:-0}" == "1" ]]; then
    POST_FIX_CONFIGS+=(
        MC_EquivocationDetector
        MC_EquivocationDetectorEager_3v
        MC_EquivocationDetector_safety
    )
fi

cd "$TLA_DIR"

# Per-config wall-clock cap: one wedged or state-exploded config must not
# consume the whole job silently (observed: the first config alone exceeded
# a 60-minute CI job with no output). Timeouts are reported distinctly from
# invariant violations. Override via TLC_PER_CONFIG_TIMEOUT (GNU timeout
# duration syntax); the cap is skipped when `timeout` is unavailable.
TLC_PER_CONFIG_TIMEOUT="${TLC_PER_CONFIG_TIMEOUT:-45m}"
TIMEOUT_CMD=""
if command -v timeout >/dev/null 2>&1; then
    TIMEOUT_CMD="timeout --signal=TERM --kill-after=60 $TLC_PER_CONFIG_TIMEOUT"
fi

failed=0
timeouts=0
violations=0
for cfg in "${POST_FIX_CONFIGS[@]}"; do
    if [[ ! -f "$cfg.tla" || ! -f "$cfg.cfg" ]]; then
        echo "SKIP   $cfg (missing $cfg.tla or $cfg.cfg)"
        continue
    fi
    started_epoch="$(date +%s)"
    echo "CHECK  $cfg (started $(date -u +%H:%M:%SZ), cap $TLC_PER_CONFIG_TIMEOUT)"
    set +e
    $TIMEOUT_CMD $TLC_CMD -workers auto -config "$cfg.cfg" "$cfg.tla" >"/tmp/tlc-$cfg.log" 2>&1
    status=$?
    set -e
    elapsed="$(( $(date +%s) - started_epoch ))s"
    if (( status == 0 )); then
        echo "OK     $cfg ($elapsed)"
    elif (( status == 124 )); then
        echo "TIMEOUT $cfg after $elapsed (cap $TLC_PER_CONFIG_TIMEOUT) — treat as failure; profile or split the config"
        failed=$((failed + 1))
        timeouts=$((timeouts + 1))
    else
        echo "FAIL   $cfg ($elapsed)"
        echo "--- last 40 lines of /tmp/tlc-$cfg.log ---"
        tail -40 "/tmp/tlc-$cfg.log"
        echo "--- end log ---"
        failed=$((failed + 1))
        violations=$((violations + 1))
    fi
done

if (( failed > 0 )); then
    echo "FAILED: $failed config(s) did not verify — $timeouts cap timeout(s), $violations violation-or-error(s)."
    exit 1
fi

echo "All $((${#POST_FIX_CONFIGS[@]})) post-fix TLA+ configurations clean."
