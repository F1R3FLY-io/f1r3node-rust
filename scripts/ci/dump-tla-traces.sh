#!/usr/bin/env bash
# scripts/ci/dump-tla-traces.sh — regenerate TLA+ trace files used by
# the Rust trace-replay tests.
#
# Reference: docs/theory/slashing/design/14-test-plan.md §14.6
# (Item 4 / Track 7 of the principled-resolution session).
#
# The trace JSONs at `casper/tests/slashing/tla_traces/*.json` are
# checked into the repo as canonical schedules for each TLA+ MC
# config. They are hand-authored to exercise the invariants of
# interest and are kept stable so the Rust trace-replay tests
# regress deterministically.
#
# When a TLA+ spec changes (action signature, variable shape) the
# corresponding trace JSON must be updated. This script documents
# how to obtain a TLC-emitted trace by deliberately violating an
# invariant: TLC dumps the counter-example trace, which can then be
# adapted to the JSON schema. Use this WORKFLOW (no automatic
# regeneration; traces remain hand-curated):
#
#   1. Add a deliberately-false invariant to the .cfg, e.g.
#      `INVARIANT FALSE_PROPERTY` where FALSE_PROPERTY := FALSE.
#   2. Run TLC: $TLC_CMD -workers auto -config <name>.cfg <name>.tla
#   3. TLC emits a trace; copy the action sequence into the JSON.
#   4. Restore the .cfg.
#   5. Re-run the Rust test to verify the trace is well-formed.
#
# This script does NOT perform the regeneration automatically — it
# is documentation of the workflow and a sanity-check that TLC is
# available.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TLA_DIR="$REPO_ROOT/formal/tlaplus/slashing"
TRACE_DIR="$REPO_ROOT/casper/tests/slashing/tla_traces"

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
    echo "ERROR: TLC not found." >&2
    exit 3
fi

# The 5 specs whose schedules drive the Rust replay tests.
declare -A SPEC_TO_TRACE=(
    ["MC_EquivocationDetector"]="mc_equivocation_detector.json"
    ["MC_ConcurrentTracker"]="mc_concurrent_tracker.json"
    ["MC_SlashFlow"]="mc_slash_flow.json"
    ["MC_TwoLevelSlashing"]="mc_two_level_slashing.json"
    ["MC_WithdrawFlow"]="mc_withdraw_flow.json"
)

echo "=== TLA+ trace-replay status ==="
for spec in "${!SPEC_TO_TRACE[@]}"; do
    trace="${SPEC_TO_TRACE[$spec]}"
    spec_path="$TLA_DIR/$spec.tla"
    cfg_path="$TLA_DIR/$spec.cfg"
    trace_path="$TRACE_DIR/$trace"

    printf "  %-35s " "$spec"
    if [[ ! -f "$spec_path" || ! -f "$cfg_path" ]]; then
        echo "MISSING SPEC or CFG"
        continue
    fi
    if [[ ! -f "$trace_path" ]]; then
        echo "MISSING TRACE: $trace"
        continue
    fi
    echo "OK ($trace)"
done

echo ""
echo "Trace files are hand-authored and checked into the repo."
echo "Regeneration workflow is documented at the top of this script."
echo ""
echo "To run the Rust trace-replay tests:"
echo "  cargo test -p casper --test mod -- slashing::tla_trace_replay"
