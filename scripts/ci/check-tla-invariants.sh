#!/usr/bin/env bash
# scripts/ci/check-tla-invariants.sh — run TLC against every MC_*.cfg
# under formal/tlaplus/slashing/ and assert clean.
#
# Reference: docs/theory/slashing/design/14-test-plan.md §14.6 / §14.9.
# Invokes the TLA+ model checker (TLC) against each MC instance:
#   • MC_EquivocationDetector{,_safety,_liveness}.tla / .cfg
#   • MC_EquivocationDetectorEager{,_3v}.tla / .cfg
#   • MC_ConcurrentTracker{,_pre_fix}.tla / .cfg
#   • MC_SlashFlow.tla / .cfg
#   • MC_TwoLevelSlashing.tla / .cfg
#
# A non-zero exit code from TLC for any post-fix configuration is a CI
# failure; the pre-fix configurations (e.g. MC_ConcurrentTracker_pre_fix)
# are *expected* to violate their invariants and are skipped here (they
# are the formal-side counter-examples, run manually for validation).

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
    MC_EquivocationDetector
    MC_EquivocationDetector_safety
    MC_EquivocationDetector_liveness
    MC_EquivocationDetectorEager
    MC_EquivocationDetectorEager_3v
    MC_ConcurrentTracker
    MC_SlashFlow
    MC_TwoLevelSlashing
    MC_WithdrawFlow
)

cd "$TLA_DIR"

failed=0
for cfg in "${POST_FIX_CONFIGS[@]}"; do
    if [[ ! -f "$cfg.tla" || ! -f "$cfg.cfg" ]]; then
        echo "SKIP   $cfg (missing $cfg.tla or $cfg.cfg)"
        continue
    fi
    printf "CHECK  %-40s ... " "$cfg"
    if $TLC_CMD -workers auto -config "$cfg.cfg" "$cfg.tla" >"/tmp/tlc-$cfg.log" 2>&1; then
        echo "ok"
    else
        echo "FAIL"
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
