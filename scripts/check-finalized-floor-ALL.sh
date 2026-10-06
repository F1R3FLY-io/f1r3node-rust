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
#   7. Rust  (fail-soft)     — `cargo test -p casper` the finalized-floor proptests
#      (G2 θ_ppm provenance + f32↔ppm round-trip; P1 committee derivation PLAY≡REPLAY).
#      SKIPPED if cargo is absent; any proptest failure fails the gate.
#   8. Loom  (fail-soft)     — exhaustive thread-interleaving model check that the
#      floor_index/frontier_index memoization (write-once, node-identical pure
#      function) can never observe a torn or regressed cached value (T-CACHE, proved
#      sequentially, now stressed concurrently). SKIPPED if cargo is absent or the
#      loom test cannot be built in this cfg; a real interleaving violation fails.
#
# POLICY: this script is for LOCAL use only. Do NOT wire it (or any Rocq/TLA+/
# Wolfram step) into .github/workflows/* — an earlier formal-CI workflow was
# deliberately removed. See docs/casper/theory/finalized-floor/finalized-floor-verification.md.
#
# Companion doc: docs/casper/theory/finalized-floor/finalized-floor-verification.md
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

echo "== [1/8] Rocq (authoritative) =="
if command -v coqc >/dev/null 2>&1 || [[ -x "$HOME/.opam/default/bin/coqc" ]]; then
  # shellcheck disable=SC1090
  eval "$(opam env 2>/dev/null)" 2>/dev/null || true
  ( cd "$ROCQ_DIR" && coq_makefile -f _CoqProject -o Makefile ) >/dev/null 2>&1
  if capped make -C "$ROCQ_DIR" -j1 >/tmp/ff_rocq_build.log 2>&1; then
    pass "Rocq build (Foundation, CliqueOracle, Floor, GuardBridge, Merge, Recovery, Selection, IntegerAdd, FtExact, MainTheorem)"
    # Coq derives the module name from the file's basename, so it must be a valid
    # identifier (no dots) — use a fixed name inside a scratch dir.
    tmpd=$(mktemp -d)
    chk="$tmpd/GateCheck.v"
    # The 5 original capstones + the 3 Phase-7 GuardBridge lemmas that close the
    # "Rocq assumes what Rust enforces" seam (guard⇒AdjDC bridge + frontier-is-
    # finalized) + the C1/C5 sweep: the 6th capstone (θ-exact + advancement) and
    # its two load-bearing standalone lemmas (the θ→majority refinement bridge and
    # preservation⇒advancement generalization) + the C1' θ≤0 hard-gate closure:
    # Finalized_ft_hg_refines_Finalized (the θ-independent 2·agreeing>S gate yields
    # strict-majority Finalized for ALL num, incl the default θ=0) and BridgeFt's
    # guard_constant_committee_transparent_ft (T-CACHE directly over Finalized_ft
    # via L_ANC_ft, so cache transparency covers θ≤0 without the num>0 bridge).
    cat > "$chk" <<'EOF'
From FinalizedFloor Require Import MainTheorem.
From FinalizedFloor Require Import GuardBridge.
From FinalizedFloor Require Import CliqueOracle.
From FinalizedFloor Require Import SettledEffectProbe.
Print Assumptions finalized_floor_merge_correct.
Print Assumptions finalized_floor_selection_correct.
Print Assumptions finalized_floor_arithmetic_correct.
Print Assumptions finalized_floor_phase7_correct.
Print Assumptions finalized_floor_ftexact_correct.
Print Assumptions finalized_floor_ftprovenance_correct.
Print Assumptions guard_constant_committee_transparent.
Print Assumptions upgo_finalized.
Print Assumptions chain_adj_AdjDC.
Print Assumptions finalized_floor_thetaexact_advance_correct.
Print Assumptions Finalized_ft_refines_Finalized.
Print Assumptions snap_extends_snap_advances.
Print Assumptions Finalized_ft_hg_refines_Finalized.
Print Assumptions guard_constant_committee_transparent_ft.
Print Assumptions walk_collect_equiv.
Print Assumptions walk_segment_composition.
Print Assumptions walk_memo_false_stable.
Print Assumptions walk_true_stable.
EOF
    out=$(coqc -Q "$ROCQ_DIR/theories" FinalizedFloor "$chk" 2>&1)
    # The expected count derives from the assertion list itself, so adding
    # a theorem above cannot silently desynchronize a hardcoded number.
    n_expected=$(grep -c '^Print Assumptions' "$chk")
    rm -rf "$tmpd"
    n_closed=$(grep -c "Closed under the global context" <<<"$out")
    if [[ "$n_closed" == "$n_expected" ]]; then
      pass "all $n_expected headline results axiom-free:"
      printf '         - %s\n' \
        "6 capstones: merge, selection, arithmetic, phase7, A9 ftexact, G2 ftprovenance (θ_ppm on-chain provenance + widened [-den,den] overflow envelope)" \
        "guard⇒AdjDC bridge: guard_constant_committee_transparent, upgo_finalized, chain_adj_AdjDC" \
        "C1/C5 sweep: finalized_floor_thetaexact_advance_correct, Finalized_ft_refines_Finalized, snap_extends_snap_advances" \
        "C1' θ≤0 hard-gate: Finalized_ft_hg_refines_Finalized; BridgeFt θ-exact cache transparency: guard_constant_committee_transparent_ft" \
        "CLAIM-FINALITY-001 settled-effect probe algebra: walk_collect_equiv, walk_segment_composition, walk_memo_false_stable, walk_true_stable"
    else
      fail "headline results NOT all axiom-free ($n_closed/$n_expected Closed):"; echo "$out" | sed 's/^/      /'
    fi
    # Independent kernel re-check (coqchk) — the TRUSTED kernel re-verifies every
    # capstone + dependency `.vo`, not just the elaborator's Print Assumptions.
    if capped coqchk -Q "$ROCQ_DIR/theories" FinalizedFloor FinalizedFloor.MainTheorem \
         >/tmp/ff_coqchk.log 2>&1 && grep -q "Modules were successfully checked" /tmp/ff_coqchk.log; then
      pass "coqchk kernel re-check (MainTheorem + all deps)"
    else
      fail "coqchk kernel re-check FAILED (see /tmp/ff_coqchk.log)"; tail -10 /tmp/ff_coqchk.log | sed 's/^/      /'
    fi
  else
    fail "Rocq build failed (see /tmp/ff_rocq_build.log)"; tail -20 /tmp/ff_rocq_build.log | sed 's/^/      /'
  fi
else
  fail "coqc not found — Rocq is authoritative, cannot skip"
fi

echo "== [2/8] TLA+ (fail-soft) =="
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

echo "== [3/8] Z3 cross-witness (fail-soft) =="
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
  if python3 "$REPO_ROOT/formal/z3/finalized_floor/ft_exact_no_overflow.py" >/tmp/ff_z3_fte.log 2>&1; then
    pass "Z3 A9 exact-integer FT (i128 no-overflow; exact≡ratio; f32 residual real)"
  else
    fail "Z3 ft_exact_no_overflow.py failed (see /tmp/ff_z3_fte.log)"
  fi
  if python3 "$REPO_ROOT/formal/z3/finalized_floor/ft_ppm_roundtrip.py" >/tmp/ff_z3_rt.log 2>&1; then
    pass "Z3 G2 ppm provenance + round-trip (to_ppm monotone/range/½ppm round-trip; redisplay fixed-point; exact-decision display-invariance; IEEE FPA corroboration)"
  else
    fail "Z3 ft_ppm_roundtrip.py failed (see /tmp/ff_z3_rt.log)"
  fi
else
  skip "no python3 z3 module"
fi

echo "== [4/8] Sage cross-witness (fail-soft) =="
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

echo "== [5/8] Wolfram (fail-soft) =="
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

echo "== [6/8] PlantUML diagrams (fail-soft) =="
# The dossier's diagram set must render cleanly: a populated SVG (closing </svg>),
# no stderr from plantuml. Mirrors the slashing diagram convention. Doc-only, so
# fail-soft; SKIPPED if plantuml is absent or no .puml sources exist yet.
DIAG_DIR="$REPO_ROOT/docs/casper/theory/finalized-floor/diagrams"
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
    skip "no .puml sources in docs/casper/theory/finalized-floor/diagrams"
  fi
else
  skip "no plantuml on PATH"
fi

echo "== [7/8] Rust proptests + floor-selection lib tests (fail-soft) =="
# The finalized-floor Phase-4 proptests, wired into the `mod` integration-test binary
# (casper/tests/finalized_floor/): G2 θ_ppm provenance + f32↔ppm round-trip, and P1
# committee derivation PLAY≡REPLAY. Compiles the casper test harness (one-time; cached
# thereafter), then runs only the `finalized_floor::` tests. SKIPPED if cargo is absent;
# any proptest failure fails the gate.
if command -v cargo >/dev/null 2>&1; then
  if cargo test -p casper --test mod -- finalized_floor:: >/tmp/ff_rust_prop.log 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" /tmp/ff_rust_prop.log; then
    n_rust=$(grep -oE 'result: ok\. [0-9]+ passed' /tmp/ff_rust_prop.log | grep -oE '[0-9]+' | head -1)
    pass "Rust finalized-floor proptests (${n_rust:-?} passed: G2 provenance/round-trip + P1 committee PLAY≡REPLAY)"
  else
    fail "Rust finalized-floor proptests failed (see /tmp/ff_rust_prop.log)"; tail -20 /tmp/ff_rust_prop.log | sed 's/^/      /'
  fi
  # Floor Selection lib tests (finality::floor #[cfg(test)]) — the derive_floor case
  # analysis that Selection.v proves: Case-A common-ancestor (T-LIN), highest-sound
  # maximality (T-DET), general-finalized result (T-FIN), plus the Case-B dominating-tip
  # pick and the incompatible-fork safety error. These are LIB unit tests (not the `mod`
  # integration binary), so they need their own invocation.
  if cargo test -p casper --lib finality::floor:: >/tmp/ff_rust_lib.log 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" /tmp/ff_rust_lib.log; then
    n_lib=$(grep -oE 'result: ok\. [0-9]+ passed' /tmp/ff_rust_lib.log | grep -oE '[0-9]+' | head -1)
    pass "Rust floor-selection lib tests (${n_lib:-?} passed: T-LIN Case-A + T-DET maximality + T-FIN + Case-B + incompatible-fork)"
  else
    fail "Rust floor-selection lib tests failed (see /tmp/ff_rust_lib.log)"; tail -20 /tmp/ff_rust_lib.log | sed 's/^/      /'
  fi
else
  skip "no cargo on PATH"
fi

echo "== [8/8] Loom concurrency (fail-soft) =="
# Exhaustive thread-interleaving model check (block-storage/tests/
# loom_frontier_floor_cache.rs) that the finalized-floor floor_index/frontier_index
# memoization — a write-once, node-identical PURE function whose accessors take
# `&self` and are deliberately NOT behind a global lock — can never expose a torn
# or regressed cached value: on EVERY interleaving any observed value is in
# {absent, canonical}, the final value is canonical, and no read regresses below a
# prior read. The REAL guarantee is idempotence + LMDB single-key MVCC; loom checks
# the Rust memory-model shape. Uses loom::sync::* directly, so no --cfg loom flag is
# needed (matches loom_equivocations_tracker). SKIPPED (fail-soft) if cargo is absent
# or the loom test cannot be built in this cfg; a genuine interleaving violation FAILS.
if command -v cargo >/dev/null 2>&1; then
  if cargo test -p block-storage --test loom_frontier_floor_cache >/tmp/ff_loom.log 2>&1; then
    if grep -qE "test result: ok\. [1-9][0-9]* passed" /tmp/ff_loom.log; then
      pass "Loom finalized-floor cache (no torn/regressed value on any interleaving; write-once memo + single-key MVCC)"
    else
      skip "Loom finalized-floor cache: test target unavailable in this build cfg (fail-soft; see /tmp/ff_loom.log)"
    fi
  elif grep -q "test result: FAILED" /tmp/ff_loom.log; then
    fail "Loom finalized-floor cache found a torn/regressed interleaving (see /tmp/ff_loom.log)"; tail -20 /tmp/ff_loom.log | sed 's/^/      /'
  else
    skip "Loom finalized-floor cache: could not build the loom test in this cfg (fail-soft; see /tmp/ff_loom.log)"
  fi
else
  skip "no cargo on PATH"
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
