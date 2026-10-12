#!/usr/bin/env bash
# scripts/check-reified-rspaces-ALL.sh
#
# The aggregate gate of Reified RSpaces (plan recipe family RF12; P10 design
# v2, section 2.1). It runs every leg that holds for the whole campaign and
# prints one line "reified-rspaces ALL: PASS=n FAIL=m SKIP=0". It exits 0 only
# when every leg passes. A missing tool is a FAIL, never a SKIP.
#
# Legs:
#   1. proofs    - scripts/check-reified-rspaces-proofs.sh (hygiene, the
#                  standalone compiles, the headlines, rocqchk, the project
#                  build, the negative controls and the axiom control)
#   2. tla       - scripts/check-reified-rspaces-tla-invariants.sh (TLC, the
#                  Unsafe controls and Apalache)
#   3. z3        - the three scripts of formal/z3/reified_rspaces
#   4. sage      - formal/sage/reified_rspaces/similarity_metrics.sage in a
#                  scratch copy (sage writes a preparsed file next to its
#                  input). Without RRS_SAGE_WRITE the script compares the
#                  regenerated reference vectors with
#                  test-vectors/numeric-reference-v1.json byte for byte.
#                  Then scripts/verify-reified/numeric_reference_receipt.py.
#   5. checkers  - each command of scripts/verify-reified/checker-commands.txt:
#                  the checker tests, the closure, the consensus checklist,
#                  the completeness phases, the footprint, the model and
#                  invariant maps, the test-port ledger, the patch gate and
#                  doc_lint with every rule (the render check and the
#                  citation rules included)
#   6. examples  - cargo test --release -p rholang --test reified_examples_execute
#   7. loom      - cargo test --release -p rholang --test loom_reified
#   8. benches   - cargo bench -p rholang --bench B -- --test, once for each
#                  Reified bench, with RRS_BENCH_MAX_SIZE=1000
#
# The cargo suites of the per-phase gate sets (G2 to G17) are not legs.
#
# Environment:
#   RRS_SANDBOX=1       the job runs inside the verifier scope: no nested
#                       systemd-run, logs and scratch under $RRS_SCRATCH, and
#                       no write to the tree
#   RRS_SCRATCH         scratch root (default <repo>/target/verification/
#                       reified-rspaces/scratch)
#   RRS_MEMORY_MAX      cgroup memory ceiling of a cargo leg (default 24G)
#   RRS_CPU_QUOTA       cgroup CPU quota of a cargo leg (default 800%)
#   RRS_REDACTION_HOME  home directory of the original design ledger, for the
#                       ledger-copy checks (default $HOME)
#   RRS_ALL_LEGS        space-separated subset of the leg names (default: all)
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRATCH_ROOT="${RRS_SCRATCH:-$ROOT/target/verification/reified-rspaces/scratch}"
if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
  LOG_DIR="$SCRATCH_ROOT/logs/all"
else
  LOG_DIR="$ROOT/target/verification/reified-rspaces/all"
fi
RRS_MEMORY_MAX="${RRS_MEMORY_MAX:-24G}"
RRS_CPU_QUOTA="${RRS_CPU_QUOTA:-800%}"
export RRS_REDACTION_HOME="${RRS_REDACTION_HOME:-$HOME}"
ALL_LEGS=(proofs tla z3 sage checkers examples loom benches)
read -r -a LEGS <<< "${RRS_ALL_LEGS:-${ALL_LEGS[*]}}"
BENCHES=(reified_spaces reified_persistence)
Z3_SCRIPTS=(squared_form_cosine topk_tiebreak_total_order metric_algebra)

PASS=0
FAIL=0
SKIP=0
pass() { PASS=$((PASS + 1)); echo "  PASS $*"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $*"; }

finish() {
  echo "reified-rspaces ALL: PASS=$PASS FAIL=$FAIL SKIP=$SKIP"
  if [[ $FAIL -gt 0 ]]; then
    exit 1
  fi
  exit 0
}

for leg in "${LEGS[@]}"; do
  case " ${ALL_LEGS[*]} " in
    *" $leg "*) ;;
    *) echo "error: unknown leg $leg (legs: ${ALL_LEGS[*]})" >&2; exit 2 ;;
  esac
done

wants() {
  case " ${LEGS[*]} " in
    *" $1 "*) return 0 ;;
    *) return 1 ;;
  esac
}

echo "== Reified RSpaces aggregate gate (legs: ${LEGS[*]})"

# ── Tools ───────────────────────────────────────────────────────────────────
tools_ok=true
need_tool() {
  if command -v "$1" >/dev/null 2>&1; then
    pass "tool $1: $(command -v "$1")"
  else
    fail "tool $1 is missing"
    tools_ok=false
  fi
}
need_tool bash
need_tool python3
if wants examples || wants loom || wants benches; then
  need_tool cargo
fi
if wants sage; then
  need_tool sage
fi
if wants z3; then
  if python3 -c 'import z3' >/dev/null 2>&1; then
    pass "tool z3: the python3 module imports"
  else
    fail "tool z3: the python3 module z3 does not import"
    tools_ok=false
  fi
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

# A cargo leg runs under a memory and CPU ceiling unless the verifier scope bounds it.
bounded() {
  if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
    "$@"
  else
    systemd-run --user --scope --quiet -p "MemoryMax=$RRS_MEMORY_MAX" -p MemorySwapMax=0 \
      -p "CPUQuota=$RRS_CPU_QUOTA" -- "$@"
  fi
}

mkdir -p "$SCRATCH_ROOT" "$LOG_DIR" || { fail "cannot create $SCRATCH_ROOT or $LOG_DIR"; finish; }
WORK="$(mktemp -d "$SCRATCH_ROOT/all.XXXXXX")" || { fail "cannot create a scratch directory"; finish; }
trap 'rm -rf -- "$WORK"' EXIT
export TMPDIR="$WORK/tmp"
mkdir -p "$TMPDIR"
cd "$ROOT" || { fail "cannot enter $ROOT"; finish; }

# run_leg NAME LOG COMMAND... runs one command, keeps its output in LOG, and
# reports the last line that matches the summary pattern of the leg.
run_leg() {
  local name=$1 log=$2
  shift 2
  local start status
  start=$(date +%s)
  "$@" > "$log" 2>&1
  status=$?
  local seconds=$(( $(date +%s) - start ))
  if [[ $status -eq 0 ]]; then
    pass "$name (${seconds} s): $(tail -n 1 "$log" | cut -c1-160)"
  else
    fail "$name (exit $status, ${seconds} s): see $log"
  fi
}

# ── 1. proofs ───────────────────────────────────────────────────────────────
if wants proofs; then
  run_leg "proofs" "$LOG_DIR/proofs.log" bash scripts/check-reified-rspaces-proofs.sh
fi

# ── 2. tla ──────────────────────────────────────────────────────────────────
if wants tla; then
  run_leg "tla" "$LOG_DIR/tla.log" bash scripts/check-reified-rspaces-tla-invariants.sh
fi

# ── 3. z3 ───────────────────────────────────────────────────────────────────
if wants z3; then
  for script in "${Z3_SCRIPTS[@]}"; do
    run_leg "z3 $script" "$LOG_DIR/z3-$script.log" python3 "formal/z3/reified_rspaces/$script.py"
  done
fi

# ── 4. sage and the receipt ─────────────────────────────────────────────────
if wants sage; then
  mkdir -p "$WORK/sage"
  cp formal/sage/reified_rspaces/similarity_metrics.sage "$WORK/sage/"
  run_leg "sage" "$LOG_DIR/sage.log" \
    env -u RRS_SAGE_WRITE RRS_REPO_ROOT="$ROOT" bash -c 'cd "$1" && sage similarity_metrics.sage' _ "$WORK/sage"
  run_leg "receipt" "$LOG_DIR/receipt.log" python3 scripts/verify-reified/numeric_reference_receipt.py
fi

# ── 5. checkers ─────────────────────────────────────────────────────────────
if wants checkers; then
  list=scripts/verify-reified/checker-commands.txt
  if [[ ! -f "$list" ]]; then
    fail "checkers: $list is missing"
  else
    index=0
    while IFS= read -r command || [[ -n "$command" ]]; do
      [[ -z "${command//[[:space:]]/}" || "$command" == \#* ]] && continue
      index=$((index + 1))
      run_leg "checker $index: $command" "$LOG_DIR/checker-$index.log" bash -c "$command"
    done < "$list"
    if [[ $index -eq 0 ]]; then
      fail "checkers: $list lists no command"
    fi
  fi
fi

# ── 6. examples ─────────────────────────────────────────────────────────────
if wants examples; then
  run_leg "examples" "$LOG_DIR/examples.log" \
    bounded cargo test --release -p rholang --test reified_examples_execute
fi

# ── 7. loom ─────────────────────────────────────────────────────────────────
if wants loom; then
  run_leg "loom" "$LOG_DIR/loom.log" \
    bounded cargo test --release -p rholang --test loom_reified -- --nocapture
fi

# ── 8. benches ──────────────────────────────────────────────────────────────
if wants benches; then
  export RRS_BENCH_MAX_SIZE=1000
  for bench in "${BENCHES[@]}"; do
    run_leg "bench $bench" "$LOG_DIR/bench-$bench.log" \
      bounded cargo bench -p rholang --bench "$bench" -- --test
  done
fi

finish
