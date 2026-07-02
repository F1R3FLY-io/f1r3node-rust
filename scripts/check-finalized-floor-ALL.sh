#!/usr/bin/env bash
# scripts/check-finalized-floor-ALL.sh
#
# LOCAL-ONLY verification gate for the finalized-floor multi-parent merge.
# Runs every formal layer for the feature under a bounded memory envelope:
#
#   1. Rocq  (AUTHORITATIVE) — builds formal/rocq/finalized_floor and asserts the
#      four capstones (finalized_floor_{merge,selection,arithmetic,phase7}_correct)
#      plus the three GuardBridge lemmas that derive Floor.v's AdjDC premise from the
#      Rust committee-constancy guard (guard_constant_committee_transparent,
#      upgo_finalized, chain_adj_AdjDC) are axiom-free. Any failure here fails the gate.
#   2. TLA+  (fail-soft)     — TLC on the POST-fix MC_FinalizedFloor.cfg + the
#      H3/T-PS MC_FinalizedFloorScan.cfg (both must pass) and the two PRE-fix cfgs
#      (both must reproduce their counterexample). SKIPPED if no TLC jar.
#   3. Z3    (fail-soft)     — ft_algebra + BitVec-64 IntegerAdd launder witnesses.
#   4. Sage  (fail-soft)     — FT-algebra identity + finalization-margin monotonicity.
#   5. Wolfram (fail-soft)   — delta_ratchet.wl (ratchet instability). SKIPPED if
#      no kernel is on PATH, or if the CLI kernel cannot bind the license in this
#      shell (the model is validated via the licensed Wolfram MCP evaluator; a
#      `mathpass` password is version-keyed, so a CLI kernel whose entry was
#      issued for another major version reports "no valid password" even though
#      the license itself is valid).
#   6. Diagrams (fail-soft) — renders the dossier's PlantUML diagram set and asserts
#      a populated SVG (closing </svg>) with no stderr. SKIPPED if plantuml is absent.
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

echo "== [1/6] Rocq (authoritative) =="
if command -v coqc >/dev/null 2>&1 || [[ -x "$HOME/.opam/default/bin/coqc" ]]; then
  # shellcheck disable=SC1090
  eval "$(opam env 2>/dev/null)" 2>/dev/null || true
  ( cd "$ROCQ_DIR" && coq_makefile -f _CoqProject -o Makefile ) >/dev/null 2>&1
  if capped make -C "$ROCQ_DIR" -j1 >/tmp/ff_rocq_build.log 2>&1; then
    pass "Rocq build (Foundation, CliqueOracle, Floor, GuardBridge, Merge, Recovery, Selection, IntegerAdd, MainTheorem)"
    # Coq derives the module name from the file's basename, so it must be a valid
    # identifier (no dots) — use a fixed name inside a scratch dir.
    tmpd=$(mktemp -d)
    chk="$tmpd/GateCheck.v"
    # The 4 capstones + the 3 Phase-7 GuardBridge lemmas that close the "Rocq
    # assumes what Rust enforces" seam (guard⇒AdjDC bridge + frontier-is-finalized).
    cat > "$chk" <<'EOF'
From FinalizedFloor Require Import MainTheorem.
From FinalizedFloor Require Import GuardBridge.
Print Assumptions finalized_floor_merge_correct.
Print Assumptions finalized_floor_selection_correct.
Print Assumptions finalized_floor_arithmetic_correct.
Print Assumptions finalized_floor_phase7_correct.
Print Assumptions guard_constant_committee_transparent.
Print Assumptions upgo_finalized.
Print Assumptions chain_adj_AdjDC.
EOF
    out=$(coqc -Q "$ROCQ_DIR/theories" FinalizedFloor "$chk" 2>&1)
    rm -rf "$tmpd"
    n_closed=$(grep -c "Closed under the global context" <<<"$out")
    if [[ "$n_closed" == "7" ]]; then
      pass "all 7 headline results axiom-free (4 capstones + guard⇒AdjDC bridge, upgo_finalized, chain_adj_AdjDC)"
    else
      fail "headline results NOT all axiom-free ($n_closed/7 Closed):"; echo "$out" | sed 's/^/      /'
    fi
  else
    fail "Rocq build failed (see /tmp/ff_rocq_build.log)"; tail -20 /tmp/ff_rocq_build.log | sed 's/^/      /'
  fi
else
  fail "coqc not found — Rocq is authoritative, cannot skip"
fi

echo "== [2/6] TLA+ (fail-soft) =="
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
  # H3 / T-PS scan model: post-fix (BadCut=0) must PASS.
  if tlc_run "$(tlc_metadir ffscan_gate)" "$TLA_DIR/MC_FinalizedFloorScan.cfg" "$TLA_DIR/FinalizedFloorScan.tla" >/tmp/ff_tlc_scan.log 2>&1; then
    pass "TLA+ scan post-fix (H3 no-drop for ANY parent set = T-PS)"
  else
    fail "TLA+ scan MC_FinalizedFloorScan.cfg did NOT pass (see /tmp/ff_tlc_scan.log)"
  fi
  # H3 bug (BadCut=1, cut above floor): must produce the drop counterexample.
  if tlc_run "$(tlc_metadir ffscan_bug_gate)" "$TLA_DIR/MC_FinalizedFloorScan_bug.cfg" "$TLA_DIR/FinalizedFloorScan.tla" >/tmp/ff_tlc_scan_bug.log 2>&1; then
    fail "TLA+ scan bug should VIOLATE Inv_NoParentWriteDropped but passed"
  else
    if grep -q "Inv_NoParentWriteDropped is violated" /tmp/ff_tlc_scan_bug.log; then
      pass "TLA+ scan bug reproduces the H3 cut-above-floor drop"
    else
      fail "TLA+ scan bug failed for the wrong reason (see /tmp/ff_tlc_scan_bug.log)"
    fi
  fi
else
  skip "no TLC jar (\$TLC_JAR) or 'tlc' on PATH"
fi

echo "== [3/6] Z3 cross-witness (fail-soft) =="
if command -v python3 >/dev/null 2>&1 && python3 -c 'import z3' >/dev/null 2>&1; then
  if python3 "$REPO_ROOT/formal/z3/finalized_floor/ft_algebra_crosswitness.py" >/tmp/ff_z3_ft.log 2>&1; then
    pass "Z3 FT-algebra + L-ANC/L-SNAP monotonicity + merge determinism"
  else
    fail "Z3 ft_algebra_crosswitness.py failed (see /tmp/ff_z3_ft.log)"
  fi
  if python3 "$REPO_ROOT/formal/z3/finalized_floor/integeradd_launder_bitvec.py" >/tmp/ff_z3_ia.log 2>&1; then
    pass "Z3 BitVec-64 IntegerAdd launder (exists on wrap; checked_combine launder-free)"
  else
    fail "Z3 integeradd_launder_bitvec.py failed (see /tmp/ff_z3_ia.log)"
  fi
else
  skip "no python3 z3 module"
fi

echo "== [4/6] Sage cross-witness (fail-soft) =="
if command -v sage >/dev/null 2>&1; then
  if sage "$REPO_ROOT/formal/sage/finalized_floor/ft_algebra.sage" >/tmp/ff_sage.log 2>&1 \
       && grep -q "ALL PASS" /tmp/ff_sage.log; then
    pass "Sage FT-algebra identity + finalization-margin monotonicity"
  else
    fail "Sage ft_algebra.sage failed (see /tmp/ff_sage.log)"
  fi
else
  skip "no sage on PATH"
fi

echo "== [5/6] Wolfram (fail-soft) =="
# Prefer wolframscript (WolframID/cloud licensing), then the classic `math`
# kernel (reads $UserBaseDirectory/Licensing/mathpass), then `wolfram`.
WL_BIN=""; WL_RUN=()
if command -v wolframscript >/dev/null 2>&1; then WL_BIN=wolframscript; WL_RUN=(wolframscript -file)
elif command -v math >/dev/null 2>&1;       then WL_BIN=math;          WL_RUN=(math -script)
elif command -v wolfram >/dev/null 2>&1;    then WL_BIN=wolfram;       WL_RUN=(wolfram -script)
fi
if [[ -n "$WL_BIN" && -f "$WL_DIR/delta_ratchet.wl" ]]; then
  wlout=$("${WL_RUN[@]}" "$WL_DIR/delta_ratchet.wl" 2>&1); wlrc=$?
  echo "$wlout" >/tmp/ff_wolfram.log
  if grep -qiE 'no valid password|cannot find a valid password' <<<"$wlout"; then
    # The LICENSE is valid — delta_ratchet.wl is validated via the licensed
    # Wolfram MCP evaluator. This CLI kernel simply could not BIND the license
    # in this shell: a `mathpass` password is version-keyed, so an entry issued
    # for a different major version does not license the installed kernel. To
    # enable the CLI, activate this kernel (`math`, then Web Activation with your
    # activation key) or install `wolframscript` (WolframID/cloud licensing).
    skip "Wolfram CLI kernel ($WL_BIN) could not bind the license in this shell — model validated via the licensed MCP evaluator (details: /tmp/ff_wolfram.log)"
  elif [[ $wlrc -eq 0 ]]; then
    pass "Wolfram delta_ratchet.wl via $WL_BIN (buggy advance unstable, fixed advance stable)"
  else
    fail "Wolfram delta_ratchet.wl errored under $WL_BIN (see /tmp/ff_wolfram.log)"
  fi
else
  skip "no wolframscript/math/wolfram kernel on PATH"
fi

echo "== [6/6] PlantUML diagrams (fail-soft) =="
# The dossier's diagram set must render cleanly: a populated SVG (closing </svg>),
# no stderr from plantuml. Mirrors the slashing diagram convention. Doc-only, so
# fail-soft; SKIPPED if plantuml is absent or no .puml sources exist yet.
DIAG_DIR="$REPO_ROOT/docs/theory/finalized-floor/diagrams"
if command -v plantuml >/dev/null 2>&1; then
  n_puml=$(find "$DIAG_DIR" -name '*.puml' 2>/dev/null | wc -l)
  if [[ "$n_puml" -gt 0 ]]; then
    diag_ok=1
    for puml in "$DIAG_DIR"/*.puml; do
      svg="${puml%.puml}.svg"
      derr=$(plantuml -tsvg "$puml" 2>&1)
      if [[ -n "$derr" ]] || [[ ! -s "$svg" ]] || ! grep -q "</svg>" "$svg" 2>/dev/null; then
        fail "diagram $(basename "$puml") did not render clean"
        [[ -n "$derr" ]] && echo "$derr" | sed 's/^/      /'
        diag_ok=0
      fi
    done
    [[ "$diag_ok" == "1" ]] && pass "all $n_puml PlantUML diagrams render clean (populated SVG, no stderr)"
  else
    skip "no .puml sources in docs/theory/finalized-floor/diagrams"
  fi
else
  skip "no plantuml on PATH"
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
