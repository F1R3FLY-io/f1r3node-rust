#!/usr/bin/env bash
# scripts/check-fork-choice-ALL.sh
#
# LOCAL-ONLY verification gate for the LMD-GHOST fork-choice ("ghosting") logic.
# Structural clone of scripts/check-finalized-floor-ALL.sh. Runs every formal layer
# for the feature under a bounded memory envelope:
#
#   1. Rocq  (AUTHORITATIVE once theories exist) — builds formal/rocq/fork_choice and
#      asserts the four capstones (fork_choice_{determinism,ghost,bound,bridge}_correct)
#      plus the three seam lemmas the Rust ENFORCES (validation_implies_wf_dag,
#      honest_forkchoice_parents_validate, sort_total_order) are axiom-free. Any failure
#      here fails the gate. SKIPPED only while no theories/*.v exist yet (scaffold phase).
#   2. TLA+  (fail-soft) — TLC on MC_ForkChoice.cfg + MC_ForkChoiceScan.cfg (both PASS)
#      and the two counterexample cfgs (MC_ForkChoice_nontotal.cfg reproduces the S1
#      non-total-sort fork; MC_ForkChoiceScan_bug.cfg reproduces the node-local-top LCA
#      divergence). SKIPPED if no TLC jar.
#   3. Z3    (fail-soft) — tiebreak_total_order + score_supply_cap BitVec witnesses.
#   4. Sage  (fail-soft) — fork-choice algebra (score monoid + argmax uniqueness).
#   5. Wolfram (fail-soft) — ghost_heaviest_subtree.wl (greedy == global heaviest;
#      measure monotone; LCA-drag cap-boundedness). SKIPPED if no kernel is on PATH, or
#      if the CLI kernel cannot bind the license in this shell (the model is validated
#      via the licensed Wolfram MCP evaluator; a `mathpass` password is version-keyed).
#   6. Diagrams (fail-soft) — renders the dossier's PlantUML diagram set and asserts a
#      populated SVG (closing </svg>) with no stderr. SKIPPED if plantuml is absent.
#   7. Rust  (fail-soft) — `cargo test -p casper` the fork-choice verification proptests
#      (C12: the concrete Estimator::filter_deep_parents conforms to GuardBridge.v's
#      within_depth/prop_filter — soundness + main-retention + completeness + exact-set).
#      SKIPPED if cargo is absent; any proptest failure fails the gate.
#
# POLICY: this script is for LOCAL use only. Do NOT wire it (or any Rocq/TLA+/Wolfram
# step) into .github/workflows/* — an earlier formal-CI workflow was deliberately
# removed. See docs/theory/fork-choice/fork-choice-verification.md.
#
# Env knobs:
#   ROCQ_MEMMAX=16G   systemd MemoryMax for the Rocq build (default 16G)
#   RUN_SOAK=1        also run the (slow) multi-writer fork-choice churn Rust soak
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ROCQ_DIR="$REPO_ROOT/formal/rocq/fork_choice"
TLA_DIR="$REPO_ROOT/formal/tlaplus/fork_choice"
Z3_DIR="$REPO_ROOT/formal/z3/fork_choice"
SAGE_DIR="$REPO_ROOT/formal/sage/fork_choice"
WL_DIR="$REPO_ROOT/formal/wolfram/fork_choice"
DIAG_DIR="$REPO_ROOT/docs/theory/fork-choice/diagrams"
ROCQ_MEMMAX="${ROCQ_MEMMAX:-16G}"

rc=0
pass() { printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; rc=1; }
skip() { printf '  \033[33mSKIP\033[0m %s\n' "$1"; }

# --- memory-capped runner (systemd-run scope, else bare) ------------------------
capped() {
  if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --scope true >/dev/null 2>&1; then
    systemd-run --user --scope -p "MemoryMax=$ROCQ_MEMMAX" -p CPUQuota=1800% -p TasksMax=200 "$@"
  else
    "$@"
  fi
}

echo "== [1/7] Rocq (authoritative) =="
if ! ls "$ROCQ_DIR"/theories/*.v >/dev/null 2>&1; then
  skip "no Rocq theories yet (scaffold phase) — becomes AUTHORITATIVE once modules land"
elif command -v coqc >/dev/null 2>&1 || [[ -x "$HOME/.opam/default/bin/coqc" ]]; then
  # shellcheck disable=SC1090
  eval "$(opam env 2>/dev/null)" 2>/dev/null || true
  ( cd "$ROCQ_DIR" && coq_makefile -f _CoqProject -o Makefile ) >/dev/null 2>&1
  if capped make -C "$ROCQ_DIR" -j1 >/tmp/fc_rocq_build.log 2>&1; then
    pass "Rocq build (Foundation, Score, Filter, TieBreak, Lca, Rank, Bound, GuardBridge, MainTheorem)"
    tmpd=$(mktemp -d)
    chk="$tmpd/GateCheck.v"
    # The 4 capstones + the 3 seam lemmas the Rust ENFORCES (bridge, not assume) +
    # lca_is_lowest + the C2/C4 derived LCA results (maximality + descends-from-root).
    cat > "$chk" <<'EOF'
From ForkChoice Require Import MainTheorem.
From ForkChoice Require Import GuardBridge.
From ForkChoice Require Import TieBreak.
From ForkChoice Require Import Lca.
Print Assumptions fork_choice_determinism_correct.
Print Assumptions fork_choice_ghost_correct.
Print Assumptions fork_choice_bound_correct.
Print Assumptions fork_choice_bridge_correct.
Print Assumptions validation_implies_wf_dag.
Print Assumptions honest_forkchoice_parents_validate.
Print Assumptions sort_total_order.
Print Assumptions reduce_converges.
Print Assumptions lca_is_lowest.
Print Assumptions lcua_many_is_max.
Print Assumptions descends_from_root.
Print Assumptions common_ancestor_root.
EOF
    out=$(coqc -Q "$ROCQ_DIR/theories" ForkChoice "$chk" 2>&1)
    rm -rf "$tmpd"
    n_closed=$(grep -c "Closed under the global context" <<<"$out")
    if [[ "$n_closed" == "12" ]]; then
      pass "all 12 headline results axiom-free (4 capstones + validation⇒wf_dag, honest-parents-validate, sort_total_order, reduce_converges, lca_is_lowest, lcua_many_is_max [C2], descends_from_root+common_ancestor_root [C4])"
    else
      fail "headline results NOT all axiom-free ($n_closed/12 Closed):"; echo "$out" | sed 's/^/      /'
    fi
    # Independent kernel re-check (coqchk) — the TRUSTED kernel re-verifies every
    # capstone + dependency `.vo`, not just the elaborator's Print Assumptions.
    if capped coqchk -Q "$ROCQ_DIR/theories" ForkChoice ForkChoice.MainTheorem \
         >/tmp/fc_coqchk.log 2>&1 && grep -q "Modules were successfully checked" /tmp/fc_coqchk.log; then
      pass "coqchk kernel re-check (MainTheorem + all deps)"
    else
      fail "coqchk kernel re-check FAILED (see /tmp/fc_coqchk.log)"; tail -10 /tmp/fc_coqchk.log | sed 's/^/      /'
    fi
  else
    fail "Rocq build failed (see /tmp/fc_rocq_build.log)"; tail -20 /tmp/fc_rocq_build.log | sed 's/^/      /'
  fi
else
  fail "coqc not found — Rocq is authoritative, cannot skip"
fi

echo "== [2/7] TLA+ (fail-soft) =="
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
if ! ls "$TLA_DIR"/*.tla >/dev/null 2>&1; then
  skip "no TLA+ modules yet"
elif [[ -f "$TLC_JAR" ]] || command -v tlc >/dev/null 2>&1; then
  # shellcheck disable=SC1091
  source "$REPO_ROOT/scripts/lib/tlc-run.sh"
  if tlc_run "$(tlc_metadir fc_det)" "$TLA_DIR/MC_ForkChoice.cfg" "$TLA_DIR/ForkChoice.tla" >/tmp/fc_tlc_det.log 2>&1; then
    pass "TLA+ ForkChoice (Inv_Deterministic, Inv_HeaviestSubtree, ...)"
  else
    fail "TLA+ MC_ForkChoice.cfg did NOT pass (see /tmp/fc_tlc_det.log)"
  fi
  if tlc_run "$(tlc_metadir fc_nontotal)" "$TLA_DIR/MC_ForkChoice_nontotal.cfg" "$TLA_DIR/ForkChoice.tla" >/tmp/fc_tlc_nontotal.log 2>&1; then
    fail "TLA+ non-total tie-break should VIOLATE Inv_Deterministic but passed"
  elif grep -q "Inv_Deterministic is violated" /tmp/fc_tlc_nontotal.log; then
    pass "TLA+ non-total tie-break reproduces the S1 fork counterexample"
  else
    fail "TLA+ non-total cfg failed for the wrong reason (see /tmp/fc_tlc_nontotal.log)"
  fi
  if tlc_run "$(tlc_metadir fc_scan)" "$TLA_DIR/MC_ForkChoiceScan.cfg" "$TLA_DIR/ForkChoiceScan.tla" >/tmp/fc_tlc_scan.log 2>&1; then
    pass "TLA+ scan (Inv_LcaDeterministic for ANY latest-message set)"
  else
    fail "TLA+ MC_ForkChoiceScan.cfg did NOT pass (see /tmp/fc_tlc_scan.log)"
  fi
  if tlc_run "$(tlc_metadir fc_scan_bug)" "$TLA_DIR/MC_ForkChoiceScan_bug.cfg" "$TLA_DIR/ForkChoiceScan.tla" >/tmp/fc_tlc_scan_bug.log 2>&1; then
    fail "TLA+ scan bug should VIOLATE Inv_LcaDeterministic but passed"
  elif grep -q "Inv_LcaDeterministic is violated" /tmp/fc_tlc_scan_bug.log; then
    pass "TLA+ scan bug reproduces the node-local-top LCA divergence"
  else
    fail "TLA+ scan bug failed for the wrong reason (see /tmp/fc_tlc_scan_bug.log)"
  fi
else
  skip "no TLC jar (\$TLC_JAR) or 'tlc' on PATH"
fi

echo "== [3/7] Z3 cross-witness (fail-soft) =="
if ! ls "$Z3_DIR"/*.py >/dev/null 2>&1; then
  skip "no Z3 scripts yet"
elif command -v python3 >/dev/null 2>&1 && python3 -c 'import z3' >/dev/null 2>&1; then
  if python3 "$Z3_DIR/tiebreak_total_order.py" >/tmp/fc_z3_tb.log 2>&1; then
    pass "Z3 tie-break total order + argmax uniqueness (no fork)"
  else
    fail "Z3 tiebreak_total_order.py failed (see /tmp/fc_z3_tb.log)"
  fi
  if python3 "$Z3_DIR/score_supply_cap_bitvec.py" >/tmp/fc_z3_sc.log 2>&1; then
    pass "Z3 BitVec-64 score accumulation (assoc/comm; no overflow under supply cap)"
  else
    fail "Z3 score_supply_cap_bitvec.py failed (see /tmp/fc_z3_sc.log)"
  fi
else
  skip "no python3 z3 module"
fi

echo "== [4/7] Sage cross-witness (fail-soft) =="
if ! ls "$SAGE_DIR"/*.sage >/dev/null 2>&1; then
  skip "no Sage scripts yet"
elif command -v sage >/dev/null 2>&1; then
  if sage "$SAGE_DIR/forkchoice_algebra.sage" >/tmp/fc_sage.log 2>&1 && grep -q "ALL PASS" /tmp/fc_sage.log; then
    pass "Sage fork-choice algebra (score monoid + heaviest-subtree argmax)"
  else
    fail "Sage forkchoice_algebra.sage failed (see /tmp/fc_sage.log)"
  fi
else
  skip "no sage on PATH"
fi

echo "== [5/7] Wolfram (fail-soft) =="
WL_BIN=""; WL_RUN=()
if command -v wolframscript >/dev/null 2>&1; then WL_BIN=wolframscript; WL_RUN=(wolframscript -file)
elif command -v math >/dev/null 2>&1;       then WL_BIN=math;          WL_RUN=(math -script)
elif command -v wolfram >/dev/null 2>&1;    then WL_BIN=wolfram;       WL_RUN=(wolfram -script)
fi
if [[ -z "$WL_BIN" || ! -f "$WL_DIR/ghost_heaviest_subtree.wl" ]]; then
  skip "no wolframscript/math/wolfram kernel on PATH, or no ghost_heaviest_subtree.wl yet"
else
  wlout=$("${WL_RUN[@]}" "$WL_DIR/ghost_heaviest_subtree.wl" 2>&1); wlrc=$?
  echo "$wlout" >/tmp/fc_wolfram.log
  if grep -qiE 'no valid password|cannot find a valid password' <<<"$wlout"; then
    skip "Wolfram CLI kernel ($WL_BIN) could not bind the license in this shell — model validated via the licensed MCP evaluator (details: /tmp/fc_wolfram.log)"
  elif [[ $wlrc -eq 0 ]]; then
    pass "Wolfram ghost_heaviest_subtree.wl via $WL_BIN (greedy == global heaviest; measure monotone)"
  else
    fail "Wolfram ghost_heaviest_subtree.wl errored under $WL_BIN (see /tmp/fc_wolfram.log)"
  fi
fi

echo "== [6/7] PlantUML diagrams (fail-soft) =="
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
    skip "no .puml sources in docs/theory/fork-choice/diagrams"
  fi
else
  skip "no plantuml on PATH"
fi

echo "== [7/7] Rust proptests (fail-soft) =="
# C12: the fork-choice verification proptests wired into the `mod` integration-test
# binary (casper/tests/fork_choice/): prop_filter_deep_parents asserts the concrete
# `Estimator::filter_deep_parents` (estimator.rs:133-182) conforms to GuardBridge.v's
# within_depth/prop_filter model — every RETAINED secondary parent is within depth
# (soundness), the main parent is ALWAYS retained first, nothing within depth is dropped
# (completeness), and the retained set equals {main} ∪ prop_filter(secondaries). Compiles
# the casper test harness (cached thereafter), then runs only the module's tests. SKIPPED
# if cargo is absent; any proptest failure fails the gate.
if command -v cargo >/dev/null 2>&1; then
  if cargo test -p casper --test mod -- fork_choice::prop_filter_deep_parents >/tmp/fc_rust_prop.log 2>&1 \
       && grep -q "test result: ok" /tmp/fc_rust_prop.log; then
    n_rust=$(grep -oE 'result: ok\. [0-9]+ passed' /tmp/fc_rust_prop.log | grep -oE '[0-9]+' | head -1)
    pass "Rust fork-choice proptests (${n_rust:-?} passed: filter_deep_parents ⊨ within_depth/prop_filter — soundness + main-kept + completeness + exact-set)"
  else
    fail "Rust fork-choice proptests failed (see /tmp/fc_rust_prop.log)"; tail -20 /tmp/fc_rust_prop.log | sed 's/^/      /'
  fi
else
  skip "no cargo on PATH"
fi

if [[ "${RUN_SOAK:-0}" == "1" ]]; then
  echo "== [soak] multi-writer fork-choice churn Rust soak (slow) =="
  if cargo test -p casper --test mod --release -- fork_choice_churn_soak --ignored >/tmp/fc_soak.log 2>&1; then
    pass "fork_choice_churn_soak"
  else
    fail "soak failed (see /tmp/fc_soak.log)"
  fi
fi

echo
if [[ $rc -eq 0 ]]; then
  printf '\033[32m== fork-choice verification: ALL GATES OK ==\033[0m\n'
else
  printf '\033[31m== fork-choice verification: FAILURES ABOVE ==\033[0m\n'
fi
exit $rc
