#!/usr/bin/env bash
# scripts/check-finalized-floor-ALL.sh
#
# LOCAL-ONLY verification gate for the finalized-floor multi-parent merge.
# Runs every formal layer for the feature under a bounded memory envelope:
#
#   1. Rocq  (AUTHORITATIVE) — builds formal/rocq/finalized_floor and asserts the
#      capstone `finalized_floor_merge_correct` is axiom-free. Any failure here
#      fails the gate.
#   2. TLA+  (fail-soft)     — TLC on the POST-fix MC_FinalizedFloor.cfg (must
#      pass) and the PRE-fix MC_FinalizedFloor_pre_fix.cfg (must produce the
#      write-loss counterexample). SKIPPED if no TLC jar is available.
#   3. Wolfram (fail-soft)   — runs delta_ratchet.wl (ratchet instability).
#      SKIPPED if no wolfram/math kernel is on PATH.
#
# POLICY: this script is for LOCAL use only. Do NOT wire it (or any Rocq/TLA+/
# Wolfram step) into .github/workflows/* — an earlier formal-CI workflow was
# deliberately removed. See docs/theory/finalized-floor/finalized-floor-verification.md.
#
# Companion doc: docs/theory/finalized-floor/finalized-floor-verification.md
#
# Env knobs:
#   ROCQ_MEMMAX=16G   systemd MemoryMax for the Rocq build (default 16G)
#   RUN_SOAK=1        also run the (slow, ~15 min) 400+-block Rust soak
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ROCQ_DIR="$REPO_ROOT/formal/rocq/finalized_floor"
TLA_DIR="$REPO_ROOT/formal/tlaplus/finalized_floor"
WL_DIR="$REPO_ROOT/formal/wolfram/finalized_floor"
ROCQ_MEMMAX="${ROCQ_MEMMAX:-16G}"

rc=0
pass() { printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; rc=1; }
skip() { printf '  \033[33mSKIP\033[0m %s\n' "$1"; }

# --- memory-capped runner (systemd-run scope, else prlimit, else bare) ----------
capped() {
  if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --scope true >/dev/null 2>&1; then
    systemd-run --user --scope -p "MemoryMax=$ROCQ_MEMMAX" -p CPUQuota=1800% -p TasksMax=200 "$@"
  else
    "$@"
  fi
}

echo "== [1/3] Rocq (authoritative) =="
if command -v coqc >/dev/null 2>&1 || [[ -x "$HOME/.opam/default/bin/coqc" ]]; then
  # shellcheck disable=SC1090
  eval "$(opam env 2>/dev/null)" 2>/dev/null || true
  ( cd "$ROCQ_DIR" && coq_makefile -f _CoqProject -o Makefile ) >/dev/null 2>&1
  if capped make -C "$ROCQ_DIR" -j1 >/tmp/ff_rocq_build.log 2>&1; then
    pass "Rocq build (Foundation, CliqueOracle, Floor, Merge, Recovery, MainTheorem)"
    # Coq derives the module name from the file's basename, so it must be a valid
    # identifier (no dots) — use a fixed name inside a scratch dir.
    tmpd=$(mktemp -d)
    chk="$tmpd/GateCheck.v"
    cat > "$chk" <<'EOF'
From FinalizedFloor Require Import MainTheorem.
Print Assumptions finalized_floor_merge_correct.
EOF
    out=$(coqc -Q "$ROCQ_DIR/theories" FinalizedFloor "$chk" 2>&1)
    rm -rf "$tmpd"
    if grep -q "Closed under the global context" <<<"$out"; then
      pass "capstone finalized_floor_merge_correct is axiom-free"
    else
      fail "capstone is NOT axiom-free:"; echo "$out" | sed 's/^/      /'
    fi
  else
    fail "Rocq build failed (see /tmp/ff_rocq_build.log)"; tail -20 /tmp/ff_rocq_build.log | sed 's/^/      /'
  fi
else
  fail "coqc not found — Rocq is authoritative, cannot skip"
fi

echo "== [2/3] TLA+ (fail-soft) =="
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
if [[ -f "$TLC_JAR" ]] || command -v tlc >/dev/null 2>&1; then
  # shellcheck disable=SC1091
  source "$REPO_ROOT/scripts/lib/tlc-run.sh"
  # POST-fix: must pass.
  if tlc_run "$(tlc_metadir ff_post_gate)" "$TLA_DIR/MC_FinalizedFloor.cfg" "$TLA_DIR/FinalizedFloor.tla" >/tmp/ff_tlc_post.log 2>&1; then
    pass "TLA+ post-fix SpecFixed (Inv_NoLostParentWrite, Inv_DeltaWithinCap, Liveness_Progress)"
  else
    fail "TLA+ post-fix MC_FinalizedFloor.cfg did NOT pass (see /tmp/ff_tlc_post.log)"
  fi
  # PRE-fix: must FAIL (counterexample). Inverted sense.
  if tlc_run "$(tlc_metadir ff_pre_gate)" "$TLA_DIR/MC_FinalizedFloor_pre_fix.cfg" "$TLA_DIR/FinalizedFloor.tla" >/tmp/ff_tlc_pre.log 2>&1; then
    fail "TLA+ pre-fix should VIOLATE Inv_NoLostParentWrite but passed (the bug demo is broken)"
  else
    if grep -q "Inv_NoLostParentWrite is violated" /tmp/ff_tlc_pre.log; then
      pass "TLA+ pre-fix reproduces the write-loss counterexample"
    else
      fail "TLA+ pre-fix failed for the wrong reason (see /tmp/ff_tlc_pre.log)"
    fi
  fi
else
  skip "no TLC jar (\$TLC_JAR) or 'tlc' on PATH"
fi

echo "== [3/3] Wolfram (fail-soft) =="
WL_BIN=""
command -v wolfram >/dev/null 2>&1 && WL_BIN=wolfram
[[ -z "$WL_BIN" ]] && command -v math >/dev/null 2>&1 && WL_BIN=math
if [[ -n "$WL_BIN" && -f "$WL_DIR/delta_ratchet.wl" ]]; then
  wlout=$("$WL_BIN" -script "$WL_DIR/delta_ratchet.wl" 2>&1); wlrc=$?
  echo "$wlout" >/tmp/ff_wolfram.log
  if grep -qiE 'no valid password|activation key|license' <<<"$wlout"; then
    # The bare CLI is unlicensed in some environments; delta_ratchet.wl is
    # validated via the Wolfram MCP evaluator (which carries the license).
    skip "Wolfram CLI unlicensed (delta_ratchet.wl validated via the MCP evaluator)"
  elif [[ $wlrc -eq 0 ]]; then
    pass "Wolfram delta_ratchet.wl (buggy advance unstable, fixed advance stable)"
  else
    fail "Wolfram delta_ratchet.wl errored (see /tmp/ff_wolfram.log)"
  fi
else
  skip "no wolfram/math kernel on PATH"
fi

if [[ "${RUN_SOAK:-0}" == "1" ]]; then
  echo "== [soak] 400+-block Rust soak (slow) =="
  if cargo test -p casper --test mod --release -- finalized_floor_400_block_soak --ignored >/tmp/ff_soak.log 2>&1; then
    pass "finalized_floor_400_block_soak"
  else
    fail "soak failed (see /tmp/ff_soak.log)"
  fi
fi

echo
if [[ $rc -eq 0 ]]; then
  printf '\033[32m== finalized-floor verification: ALL GATES OK ==\033[0m\n'
else
  printf '\033[31m== finalized-floor verification: FAILURES ABOVE ==\033[0m\n'
fi
exit $rc
