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
TLA_ROOT="$REPO_ROOT/formal/tlaplus"

if [[ ! -d "$TLA_ROOT/slashing" ]]; then
    echo "ERROR: TLA+ slashing directory not found at $TLA_ROOT/slashing" >&2
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

# Post-fix configs: each must TLC-clean. Entries are
# <subdir under formal/tlaplus>/<config basename>.
POST_FIX_CONFIGS=(
    slashing/MC_EquivocationDetector
    slashing/MC_EquivocationDetector_liveness
    slashing/MC_EquivocationDetectorEager
    slashing/MC_EquivocationDetectorEager_3v
    slashing/MC_ConcurrentTracker
    slashing/MC_SlashFlow
    slashing/MC_TwoLevelSlashing
    slashing/MC_AuthorizedSlashFlow
    slashing/MC_JustificationProjection
    slashing/MC_WithdrawFlow
    block_admission/MC_BlockAdmission
)

if [[ "${RUN_EXHAUSTIVE_TLA:-0}" == "1" ]]; then
    POST_FIX_CONFIGS+=(slashing/MC_EquivocationDetector_safety)
fi

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
for entry in "${POST_FIX_CONFIGS[@]}"; do
    dir="$TLA_ROOT/${entry%/*}"
    cfg="${entry##*/}"
    if [[ ! -f "$dir/$cfg.tla" || ! -f "$dir/$cfg.cfg" ]]; then
        echo "SKIP   $entry (missing $cfg.tla or $cfg.cfg in $dir)"
        continue
    fi
    started_epoch="$(date +%s)"
    echo "CHECK  $entry (started $(date -u +%H:%M:%SZ), cap $TLC_PER_CONFIG_TIMEOUT)"
    set +e
    (cd "$dir" && $TIMEOUT_CMD $TLC_CMD -workers auto -config "$cfg.cfg" "$cfg.tla") >"/tmp/tlc-$cfg.log" 2>&1
    status=$?
    set -e
    elapsed="$(( $(date +%s) - started_epoch ))s"
    if (( status == 0 )); then
        echo "OK     $cfg ($elapsed)"
    elif (( status == 124 )); then
        echo "TIMEOUT $cfg after $elapsed (cap $TLC_PER_CONFIG_TIMEOUT) — treat as failure; profile or split the config"
        failed=$((failed + 1))
    else
        echo "FAIL   $cfg ($elapsed)"
        echo "--- last 40 lines of /tmp/tlc-$cfg.log ---"
        tail -40 "/tmp/tlc-$cfg.log"
        echo "--- end log ---"
        failed=$((failed + 1))
    fi
done

if (( failed > 0 )); then
    echo "FAILED: $failed config(s) violated invariants."
    exit 1
fi

echo "All $((${#POST_FIX_CONFIGS[@]})) post-fix TLA+ configurations clean."
