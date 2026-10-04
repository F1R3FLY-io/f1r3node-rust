#!/usr/bin/env bash
# scripts/verify-reified/tlc-expect-violation.sh [--complement] <cfg> <Invariant> [module.tla]
#
# Unsafe-control check for the Reified RSpaces TLA+ models. The script runs
# TLC exhaustively (breadth-first, no -simulate, -dfid or -seed) on the
# module next to <cfg>. It exits 0 only when TLC stops with a safety
# violation (exit code 12) and the violated invariant is exactly
# <Invariant>. Every other outcome exits non-zero:
#   1  TLC found no violation, or it reported another invariant
#   2  usage or input error (missing file, unknown invariant, bad header)
#   3  a required tool is missing
#
# With --complement, the script checks the other half of the control. It
# copies <cfg> into the scratch directory without the line that names
# <Invariant>, and it requires that TLC then completes the full state space
# with no violation (exit code 0). Together the two runs show that the
# Unsafe configuration breaks <Invariant> and no other invariant. The
# invariant must be on its own line under INVARIANTS, or the script exits 2.
#
# The module defaults to the only *.tla file in the directory of <cfg>.
#
# Environment:
#   RRS_SANDBOX=1   the job runs inside the verifier scope: no systemd-run
#   RRS_SCRATCH     scratch root (default <repo>/target/verification/
#                   reified-rspaces/scratch); the run writes nothing else
#   TLC_JAR         default /usr/share/java/tla2tools.jar
#   TLC_HEAP        JVM heap, at most 4g (default 4g)
#   TLC_WORKERS     TLC workers (default 1)
#
# Logs: <repo>/target/verification/reified-rspaces/logs on the host, or
# $RRS_SCRATCH/logs in the sandbox (the source tree is read-only there).
set -uo pipefail

MODE=violation
if [[ "${1:-}" == "--complement" ]]; then
  MODE=complement
  shift
fi
if [[ $# -lt 2 || $# -gt 3 ]]; then
  echo "usage: $0 [--complement] <cfg> <Invariant> [module.tla]" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CFG_IN="$1"
INVARIANT="$2"
MODULE_IN="${3:-}"

if [[ ! -f "$CFG_IN" ]]; then
  echo "FAIL: config not found: $CFG_IN" >&2
  exit 2
fi
CFG="$(cd "$(dirname "$CFG_IN")" && pwd)/$(basename "$CFG_IN")"
CFG_DIR="$(dirname "$CFG")"

if [[ -z "$MODULE_IN" ]]; then
  mapfile -t modules < <(find "$CFG_DIR" -maxdepth 1 -type f -name '*.tla' | sort)
  if [[ ${#modules[@]} -ne 1 ]]; then
    echo "FAIL: pass the module explicitly: $CFG_DIR holds ${#modules[@]} .tla files" >&2
    exit 2
  fi
  MODULE="${modules[0]}"
else
  if [[ ! -f "$MODULE_IN" ]]; then
    echo "FAIL: module not found: $MODULE_IN" >&2
    exit 2
  fi
  MODULE="$(cd "$(dirname "$MODULE_IN")" && pwd)/$(basename "$MODULE_IN")"
fi

if ! head -n 1 "$CFG" | grep -q '^\\\* BOUNDS:'; then
  echo "FAIL: $CFG does not start with a '\\* BOUNDS:' header" >&2
  exit 2
fi
if ! grep -Eq "^[[:space:]]*(INVARIANT[S]?[[:space:]]+)?${INVARIANT}[[:space:]]*$" "$CFG"; then
  echo "FAIL: $CFG does not check the invariant $INVARIANT" >&2
  exit 2
fi
if ! grep -Eq "^${INVARIANT}[[:space:]]*==" "$MODULE"; then
  echo "FAIL: $MODULE does not define the invariant $INVARIANT" >&2
  exit 2
fi

TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
TLC_HEAP="${TLC_HEAP:-4g}"
TLC_WORKERS="${TLC_WORKERS:-1}"
if [[ ! "$TLC_HEAP" =~ ^([1-4])g$|^([0-9]{1,4})m$ ]]; then
  echo "FAIL: TLC_HEAP must be at most 4g (got $TLC_HEAP)" >&2
  exit 2
fi
if ! command -v java >/dev/null 2>&1; then
  echo "FAIL: java is missing" >&2
  exit 3
fi
if [[ ! -f "$TLC_JAR" ]]; then
  echo "FAIL: TLC jar is missing: $TLC_JAR" >&2
  exit 3
fi

SCRATCH_ROOT="${RRS_SCRATCH:-$REPO_ROOT/target/verification/reified-rspaces/scratch}"
if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
  LOG_DIR="$SCRATCH_ROOT/logs"
else
  LOG_DIR="$REPO_ROOT/target/verification/reified-rspaces/logs"
fi
mkdir -p "$SCRATCH_ROOT/tlc-expect" "$LOG_DIR" || { echo "FAIL: cannot create $SCRATCH_ROOT or $LOG_DIR" >&2; exit 2; }
WORK="$(mktemp -d "$SCRATCH_ROOT/tlc-expect/$(basename "$CFG" .cfg).XXXXXX")"
mkdir -p "$WORK/jtmp" "$WORK/meta"
export TMPDIR="$WORK/jtmp"
LOG="$LOG_DIR/expect-$(basename "$CFG" .cfg).log"
RUN_CFG="$CFG"
RUN_MODULE="$MODULE"

# The complement copies the modules and drops the one invariant line.
if [[ "$MODE" == "complement" ]]; then
  mkdir -p "$WORK/complement"
  if ! cp -- "$(dirname "$MODULE")"/*.tla "$WORK/complement/"; then
    echo "FAIL: cannot copy the modules next to $MODULE" >&2
    rm -rf -- "$WORK"
    exit 2
  fi
  RUN_MODULE="$WORK/complement/$(basename "$MODULE")"
  RUN_CFG="$WORK/complement/$(basename "$CFG")"
  grep -vE "^[[:space:]]*${INVARIANT}[[:space:]]*$" "$CFG" > "$RUN_CFG"
  if grep -qE "^[[:space:]]*(INVARIANT[S]?[[:space:]]+)?${INVARIANT}[[:space:]]*$" "$RUN_CFG" \
     || cmp -s "$CFG" "$RUN_CFG"; then
    echo "FAIL: cannot remove $INVARIANT from $(basename "$CFG"); list it on its own line under INVARIANTS" >&2
    rm -rf -- "$WORK"
    exit 2
  fi
  LOG="$LOG_DIR/complement-$(basename "$CFG" .cfg).log"
fi

# TLC_RSS caps the cgroup; RRS_SANDBOX lets the verifier scope bound the job.
export TLC_REPO_ROOT="$REPO_ROOT"
export TLC_RSS="${TLC_RSS:-6G}"
if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
  export ALLOW_UNBOUNDED_TLC=1
fi
# shellcheck source=/dev/null
source "$REPO_ROOT/scripts/lib/tlc-run.sh"

identity_before="$(tlc_recovery_identity "$(tlc_source_hash "$CFG" "$MODULE")")"
tlc_bounded java "-Xmx$TLC_HEAP" -XX:+UseParallelGC "-Djava.io.tmpdir=$WORK/jtmp" \
  -cp "$TLC_JAR" tlc2.TLC -workers "$TLC_WORKERS" -fp 0 -metadir "$WORK/meta" \
  -config "$RUN_CFG" "$RUN_MODULE" > "$LOG" 2>&1
rc=$?
rm -rf -- "$WORK"
identity_after="$(tlc_recovery_identity "$(tlc_source_hash "$CFG" "$MODULE")")"
if ! tlc_require_unchanged_identity "$identity_before" "$identity_after"; then
  echo "FAIL: the model or the configuration changed during the TLC run; see $LOG" >&2
  exit 1
fi

if ! grep -q 'Running breadth-first search Model-Checking' "$LOG"; then
  echo "FAIL: TLC did not run a breadth-first (exhaustive) search; see $LOG" >&2
  exit 1
fi
mapfile -t violated < <(grep -oE '^Error: Invariant [A-Za-z0-9_]+ is violated' "$LOG" \
  | sed -E 's/^Error: Invariant ([A-Za-z0-9_]+) is violated/\1/')
states="$(grep -oE '[0-9,]+ distinct states found' "$LOG" | tail -1)"
depth="$(grep -oE 'depth of the complete state graph search is [0-9]+' "$LOG" | grep -oE '[0-9]+$' || true)"
if [[ "$MODE" == "complement" ]]; then
  if [[ $rc -ne 0 || ${#violated[@]} -ne 0 ]] \
     || ! grep -q 'Model checking completed. No error has been found.' "$LOG"; then
    echo "FAIL: $(basename "$CFG") without $INVARIANT: TLC exit code $rc, violated [${violated[*]:-none}]; see $LOG" >&2
    exit 1
  fi
  echo "PASS: $(basename "$CFG") without $INVARIANT violates no other invariant (${states:-? distinct states found}, depth ${depth:-?})"
  exit 0
fi
if [[ $rc -ne 12 ]]; then
  echo "FAIL: TLC exit code $rc (expected 12, a safety violation) for $(basename "$CFG"); see $LOG" >&2
  exit 1
fi
if [[ ${#violated[@]} -ne 1 || "${violated[0]}" != "$INVARIANT" ]]; then
  echo "FAIL: $(basename "$CFG") violated [${violated[*]:-none}], expected exactly $INVARIANT; see $LOG" >&2
  exit 1
fi
echo "PASS: $(basename "$CFG") refutes exactly $INVARIANT (${states:-? distinct states found}, depth ${depth:-?})"
exit 0
