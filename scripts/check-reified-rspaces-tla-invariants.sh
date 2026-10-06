#!/usr/bin/env bash
# scripts/check-reified-rspaces-tla-invariants.sh
#
# TLA+ gate for the Reified RSpaces models (formal/tlaplus/reified_rspaces).
# P1: CheckpointReplay.tla (T1-T4, checkpoint and replay, and T6 of
# decision 36: an honest deploy replays to the failure and the spending of
# play) and TxnLockCoverage.tla (T5, lock coverage of space transactions,
# FR-4).
# P4: HeadOnlyDiscipline.tla (head-only matching of a queue: no pending
# match, a blocked head blocks the rest, FIFO), CounterConflict.tla (a merge
# without a check #1 race is a serial run: M-1 and A2) and NativeHandles.tla
# (handle uniqueness, dangling handles, restarts, handle calls that merge
# like contract calls).
#
# The gate copies the TLA+ directory into a scratch directory and checks it
# there. Legs:
#   1. bounds    - every .cfg starts with a '\* BOUNDS:' header, and every
#                  .cfg belongs to exactly one leg below
#   2. safe      - TLC explores the complete state space of each safe
#                  configuration (breadth-first; no -simulate, -dfid or
#                  -seed). T1-T5 and the auxiliary invariants must hold.
#   3. unsafe    - each Unsafe configuration refutes exactly its invariant:
#                  scripts/verify-reified/tlc-expect-violation.sh must pass,
#                  and the same checker with --complement must pass: TLC
#                  checks the configuration without that invariant, and the
#                  other invariants hold on the full state space
#   4. apalache  - BASE (Init => Inv, --length=0) and STEP (Inv /\ Next =>
#                  Inv', --length=1) for the inductive invariants IndInv
#                  (CheckpointReplay: T3 and T4), LockIndInv
#                  (TxnLockCoverage: T5), HeadIndInv (HeadOnlyDiscipline)
#                  and HandleIndInv (NativeHandles). Each negative control
#                  must refute its inductive invariant in the expected
#                  conjunct.
#
# TLC runs through scripts/lib/tlc-run.sh (tlc_bounded, tlc_metadir and the
# source-identity check) with a heap of at most 4g and an on-disk metadir.
# A missing tool is a FAIL, never a SKIP.
#
# Environment:
#   RRS_SANDBOX=1      the job runs inside the verifier scope: no
#                      systemd-run, logs under $RRS_SCRATCH/logs
#   RRS_SCRATCH        scratch root (default <repo>/target/verification/
#                      reified-rspaces/scratch)
#   RRS_MEMORY_MAX     cgroup memory ceiling (default 6G)
#   RRS_CPU_QUOTA      cgroup CPU quota (default 400%)
#   TLC_JAR            default /usr/share/java/tla2tools.jar
#   TLC_HEAP           JVM heap, at most 4g (default 4g)
#   TLC_WORKERS        TLC workers (default 1, as design §5(b) RF4)
#   APALACHE_TIMEOUT   seconds for each Apalache run (default 1800)
#   RRS_TLA_SRC        the TLA+ directory to check (default formal/tlaplus/
#                      reified_rspaces); negative controls use a copy
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="${RRS_TLA_SRC:-$ROOT/formal/tlaplus/reified_rspaces}"
SCRATCH_ROOT="${RRS_SCRATCH:-$ROOT/target/verification/reified-rspaces/scratch}"
if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
  LOG_DIR="$SCRATCH_ROOT/logs/tla"
else
  LOG_DIR="$ROOT/target/verification/reified-rspaces/logs/tla"
fi
RRS_MEMORY_MAX="${RRS_MEMORY_MAX:-6G}"
RRS_CPU_QUOTA="${RRS_CPU_QUOTA:-400%}"
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
TLC_HEAP="${TLC_HEAP:-4g}"
TLC_WORKERS="${TLC_WORKERS:-1}"
APALACHE_TIMEOUT="${APALACHE_TIMEOUT:-1800}"

# <module> <config>
SAFE_CONFIGS=(
  "CheckpointReplay MC_CheckpointReplay.cfg"
  "CheckpointReplay MC_CheckpointReplay_TwoBlock.cfg"
  "TxnLockCoverage MC_TxnLockCoverage.cfg"
  "HeadOnlyDiscipline MC_HeadOnlyDiscipline.cfg"
  "CounterConflict MC_CounterConflict.cfg"
  "NativeHandles MC_NativeHandles.cfg"
)
# <module> <config> <the one invariant that it must refute>
UNSAFE_CONFIGS=(
  "CheckpointReplay UuidUnsafe.cfg ReplayPrefixEqual"
  "CheckpointReplay RootIgnoresVolatileUnsafe.cfg SameRootIffSameState"
  "CheckpointReplay DoublePairUnsafe.cfg PairedOnce"
  "CheckpointReplay JoinResidueUnsafe.cfg NoVolatileResidue"
  "CheckpointReplay StagingDivergenceUnsafe.cfg ReplayOutcomeEqual"
  "CheckpointReplay AcceptUnpairedUnsafe.cfg ReplayPrefixEqual"
  "TxnLockCoverage UncoveredChannelUnsafe.cfg Serializable"
  "TxnLockCoverage UnjoinedChannelUnsafe.cfg Serializable"
  "HeadOnlyDiscipline HeadOnlyScanUnsafe.cfg HeadBlocks"
  "CounterConflict CounterDroppedUnsafe.cfg NoMergedOrderFabrication"
  "CounterConflict PeekCounterFreeUnsafe.cfg NoMergedOrderFabrication"
  "NativeHandles JoinDescriptorUnsafe.cfg HandleCallsMergeLikeContracts"
)
# <name>|<module>|<config>|<length>|<expected outcome>|<marker in the violated formula>
APALACHE_CHECKS=(
  "base|CheckpointReplay|MC_CheckpointReplay_IndBase_Apalache.cfg|0|NoError|"
  "step|CheckpointReplay|MC_CheckpointReplay_IndStep_Apalache.cfg|1|NoError|"
  "step-doublepair-unsafe|CheckpointReplay|DoublePairUnsafe_IndStep_Apalache.cfg|1|Error|vStaged["
  "step-joinresidue-unsafe|CheckpointReplay|JoinResidueUnsafe_IndStep_Apalache.cfg|1|Error|Swept\""
  "lock-base|TxnLockCoverage|MC_TxnLockCoverage_IndBase_Apalache.cfg|0|NoError|"
  "lock-step|TxnLockCoverage|MC_TxnLockCoverage_IndStep_Apalache.cfg|1|NoError|"
  "lock-step-uncovered-unsafe|TxnLockCoverage|UncoveredChannelUnsafe_IndStep_Apalache.cfg|1|Error|stagedOne"
  "lock-step-unjoined-unsafe|TxnLockCoverage|UnjoinedChannelUnsafe_IndStep_Apalache.cfg|1|Error|stagedOne"
  "head-base|HeadOnlyDiscipline|MC_HeadOnlyDiscipline_IndBase_Apalache.cfg|0|NoError|"
  "head-step|HeadOnlyDiscipline|MC_HeadOnlyDiscipline_IndStep_Apalache.cfg|1|NoError|"
  "head-step-scan-unsafe|HeadOnlyDiscipline|HeadOnlyScanUnsafe_IndStep_Apalache.cfg|1|Error|[\"fired\"]"
  "handle-base|NativeHandles|MC_NativeHandles_IndBase_Apalache.cfg|0|NoError|"
  "handle-step|NativeHandles|MC_NativeHandles_IndStep_Apalache.cfg|1|NoError|"
  "handle-step-joindescriptor-unsafe|NativeHandles|JoinDescriptorUnsafe_IndStep_Apalache.cfg|1|Error|\"user\""
)

PASS=0
FAIL=0
SKIP=0
pass() { PASS=$((PASS + 1)); echo "  PASS $*"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $*"; }

finish() {
  echo "reified-rspaces tla: PASS=$PASS FAIL=$FAIL SKIP=$SKIP"
  if [[ $FAIL -gt 0 ]]; then
    exit 1
  fi
  exit 0
}

if [[ ! "$TLC_HEAP" =~ ^([1-4])g$|^([0-9]{1,4})m$ ]]; then
  echo "error: TLC_HEAP must be at most 4g (got $TLC_HEAP)" >&2
  exit 2
fi
if [[ ! "$TLC_WORKERS" =~ ^[1-9][0-9]*$ ]]; then
  echo "error: TLC_WORKERS must be a positive number (got $TLC_WORKERS)" >&2
  exit 2
fi
if [[ ! "$APALACHE_TIMEOUT" =~ ^[1-9][0-9]*$ ]]; then
  echo "error: APALACHE_TIMEOUT must be a positive number of seconds" >&2
  exit 2
fi

echo "== Reified RSpaces TLA+ gate (CheckpointReplay, TxnLockCoverage, HeadOnlyDiscipline, CounterConflict, NativeHandles)"

# ── Tools ───────────────────────────────────────────────────────────────────
tools_ok=true
for tool in java apalache-mc timeout; do
  if command -v "$tool" >/dev/null 2>&1; then
    pass "tool $tool: $(command -v "$tool")"
  else
    fail "tool $tool is missing"
    tools_ok=false
  fi
done
if [[ -f "$TLC_JAR" ]]; then
  pass "tool TLC: $TLC_JAR"
else
  fail "tool TLC: no jar at $TLC_JAR"
  tools_ok=false
fi
if [[ -x "$ROOT/scripts/verify-reified/tlc-expect-violation.sh" ]]; then
  pass "tool tlc-expect-violation.sh"
else
  fail "tool scripts/verify-reified/tlc-expect-violation.sh is missing or not executable"
  tools_ok=false
fi
if [[ "${RRS_SANDBOX:-}" != "1" ]]; then
  if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --scope --quiet true >/dev/null 2>&1; then
    pass "tool systemd-run: user scopes are available"
  else
    fail "tool systemd-run: no user scope (set RRS_SANDBOX=1 inside a verifier scope)"
    tools_ok=false
  fi
fi
if [[ "$tools_ok" != true ]]; then
  finish
fi

bounded() {
  if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
    "$@"
  else
    systemd-run --user --scope --quiet -p "MemoryMax=$RRS_MEMORY_MAX" -p MemorySwapMax=0 \
      -p "CPUQuota=$RRS_CPU_QUOTA" -- "$@"
  fi
}

# ── Scratch copy ────────────────────────────────────────────────────────────
mkdir -p "$SCRATCH_ROOT" "$LOG_DIR" || { fail "cannot create $SCRATCH_ROOT or $LOG_DIR"; finish; }
WORK="$(mktemp -d "$SCRATCH_ROOT/tla.XXXXXX")" || { fail "cannot create a scratch directory"; finish; }
trap 'rm -rf -- "$WORK"' EXIT
export TMPDIR="$WORK/tmp"
mkdir -p "$TMPDIR" "$WORK/tla" "$WORK/jtmp"
cp -- "$SRC"/*.tla "$SRC"/*.cfg "$WORK/tla/" || { fail "cannot copy $SRC"; finish; }
echo "   scratch: $WORK"

export TLC_REPO_ROOT="$ROOT"
export TLC_METADIR_ROOT="$WORK/metadir"
export TLC_RSS="$RRS_MEMORY_MAX"
export TLC_HEAP TLC_WORKERS TLC_JAR
if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
  export ALLOW_UNBOUNDED_TLC=1
fi
# shellcheck source=/dev/null
source "$ROOT/scripts/lib/tlc-run.sh"

# run_tlc <label> <cfg> <module> <log>: exhaustive breadth-first TLC.
run_tlc() {
  local label="$1" cfg="$2" module="$3" log="$4" before after metadir
  before="$(tlc_recovery_identity "$(tlc_source_hash "$cfg" "$module")")" || return 1
  metadir="$(tlc_metadir "$label")"
  tlc_bounded java "-Xmx$TLC_HEAP" -XX:+UseParallelGC "-Djava.io.tmpdir=$WORK/jtmp" \
    -cp "$TLC_JAR" tlc2.TLC -workers "$TLC_WORKERS" -fp 0 -metadir "$metadir" \
    -config "$cfg" "$module" > "$log" 2>&1
  local rc=$?
  after="$(tlc_recovery_identity "$(tlc_source_hash "$cfg" "$module")")" || return 1
  tlc_require_unchanged_identity "$before" "$after" || return $?
  return "$rc"
}

tlc_summary() {
  local log="$1" states depth
  states="$(grep -oE '[0-9,]+ distinct states found' "$log" | tail -1 | grep -oE '^[0-9,]+')"
  depth="$(grep -oE 'depth of the complete state graph search is [0-9]+' "$log" | grep -oE '[0-9]+$')"
  echo "${states:-?} distinct states, depth ${depth:-?}"
}

# ── 1. Bounds headers and leg coverage ─────────────────────────────────────
for cfg in "$WORK"/tla/*.cfg; do
  if head -n 1 "$cfg" | grep -q '^\\\* BOUNDS:'; then
    pass "bounds $(basename "$cfg"): $(head -n 1 "$cfg" | sed 's/^\\\* BOUNDS: //')"
  else
    fail "bounds $(basename "$cfg"): the first line is not a '\\* BOUNDS:' header"
  fi
done
declare -A IN_LEG=()
for entry in "${SAFE_CONFIGS[@]}" "${UNSAFE_CONFIGS[@]}"; do
  read -r _ cfg _ <<<"$entry"
  IN_LEG[$cfg]=$(( ${IN_LEG[$cfg]:-0} + 1 ))
done
for entry in "${APALACHE_CHECKS[@]}"; do
  IFS='|' read -r _ _ cfg _ _ _ <<<"$entry"
  IN_LEG[$cfg]=$(( ${IN_LEG[$cfg]:-0} + 1 ))
done
for cfg in "$WORK"/tla/*.cfg; do
  name="$(basename "$cfg")"
  case "${IN_LEG[$name]:-0}" in
    1) pass "leg $name: checked by exactly one leg" ;;
    0) fail "leg $name: no leg of the gate checks this configuration" ;;
    *) fail "leg $name: ${IN_LEG[$name]} legs check this configuration" ;;
  esac
done

# ── 2. Safe configurations ──────────────────────────────────────────────────
for entry in "${SAFE_CONFIGS[@]}"; do
  read -r module_name cfg <<<"$entry"
  module="$WORK/tla/$module_name.tla"
  log="$LOG_DIR/${cfg%.cfg}.log"
  if [[ ! -f "$WORK/tla/$cfg" || ! -f "$module" ]]; then
    fail "safe $cfg: the configuration or $module_name.tla is missing"
    continue
  fi
  run_tlc "${cfg%.cfg}" "$WORK/tla/$cfg" "$module" "$log"
  rc=$?
  if [[ $rc -eq 0 ]] && grep -q 'Running breadth-first search Model-Checking' "$log" \
     && grep -q 'Model checking completed. No error has been found.' "$log"; then
    pass "safe $cfg ($module_name): $(tlc_summary "$log"); no invariant violated"
  else
    fail "safe $cfg ($module_name): TLC exit code $rc; see $log"
    grep -E 'Error|violated|Exception' "$log" | head -n 10 | sed 's/^/        /'
  fi
done

# ── 3. Unsafe controls ──────────────────────────────────────────────────────
for entry in "${UNSAFE_CONFIGS[@]}"; do
  read -r module_name cfg invariant <<<"$entry"
  module="$WORK/tla/$module_name.tla"
  if [[ ! -f "$WORK/tla/$cfg" || ! -f "$module" ]]; then
    fail "unsafe $cfg: the configuration or $module_name.tla is missing"
    continue
  fi
  out="$(RRS_SCRATCH="$WORK/expect" RRS_SANDBOX="${RRS_SANDBOX:-}" TLC_JAR="$TLC_JAR" TLC_HEAP="$TLC_HEAP" \
         TLC_WORKERS="$TLC_WORKERS" TLC_RSS="$RRS_MEMORY_MAX" \
         "$ROOT/scripts/verify-reified/tlc-expect-violation.sh" "$WORK/tla/$cfg" "$invariant" "$module" 2>&1)"
  rc=$?
  printf '%s\n' "$out" > "$LOG_DIR/${cfg%.cfg}.expect.log"
  if [[ $rc -eq 0 ]]; then
    pass "unsafe $cfg ($module_name): $(printf '%s\n' "$out" | grep '^PASS:' | sed 's/^PASS: //')"
  else
    fail "unsafe $cfg ($module_name): expected exactly a violation of $invariant; see $LOG_DIR/${cfg%.cfg}.expect.log"
    printf '%s\n' "$out" | tail -n 5 | sed 's/^/        /'
  fi

  # The complement: without the refuted invariant, every other invariant
  # holds on the full state space. The same checker runs in the pgmcp
  # verification recipe.
  out="$(RRS_SCRATCH="$WORK/expect" RRS_SANDBOX="${RRS_SANDBOX:-}" TLC_JAR="$TLC_JAR" TLC_HEAP="$TLC_HEAP" \
         TLC_WORKERS="$TLC_WORKERS" TLC_RSS="$RRS_MEMORY_MAX" \
         "$ROOT/scripts/verify-reified/tlc-expect-violation.sh" --complement "$WORK/tla/$cfg" "$invariant" "$module" 2>&1)"
  rc=$?
  printf '%s\n' "$out" > "$LOG_DIR/${cfg%.cfg}.complement.log"
  if [[ $rc -eq 0 ]]; then
    pass "unsafe $cfg without $invariant: $(printf '%s\n' "$out" | grep '^PASS:' | sed 's/^PASS: //')"
  else
    fail "unsafe $cfg without $invariant: another invariant fails or TLC failed; see $LOG_DIR/${cfg%.cfg}.complement.log"
    printf '%s\n' "$out" | tail -n 5 | sed 's/^/        /'
  fi
done

# ── 4. Apalache inductive checks ────────────────────────────────────────────
# Each check runs through scripts/verify-reified/apalache-expect.sh, the same
# checker that the pgmcp verification recipe runs.
for entry in "${APALACHE_CHECKS[@]}"; do
  IFS='|' read -r name module_name cfg length expected marker <<<"$entry"
  if [[ ! -f "$WORK/tla/$cfg" || ! -f "$WORK/tla/$module_name.tla" ]]; then
    fail "apalache $name: $cfg or $module_name.tla is missing"
    continue
  fi
  log="$LOG_DIR/apalache-$name.log"
  if [[ "$expected" == "NoError" ]]; then
    outcome=(NoError)
  else
    outcome=(Error "$marker")
  fi
  RRS_SCRATCH="$WORK" bounded timeout "$APALACHE_TIMEOUT" \
    "$ROOT/scripts/verify-reified/apalache-expect.sh" "$WORK/tla/$module_name.tla" "$cfg" "$length" "${outcome[@]}" \
    > "$log" 2>&1
  rc=$?
  if [[ $rc -eq 0 ]]; then
    pass "apalache $name ($module_name): $(grep -E '^apalache-expect: PASS' "$log" | tail -1 | sed 's/^apalache-expect: PASS //')"
    violation="$(find "$WORK/apalache-expect" -path "*${cfg%.cfg}*" -name violation1.tla -print -quit 2>/dev/null)"
    [[ -n "$violation" ]] && cp -- "$violation" "$LOG_DIR/apalache-$name.violation.tla"
  else
    fail "apalache $name ($module_name): $cfg --length=$length: see $log"
    [[ $rc -eq 124 ]] && echo "        timed out after ${APALACHE_TIMEOUT}s"
    tail -n 6 "$log" | sed 's/^/        /'
  fi
done

echo "   logs: $LOG_DIR"
finish
