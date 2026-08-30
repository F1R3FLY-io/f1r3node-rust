#!/usr/bin/env bash
# scripts/check-fork-choice-ALL.sh
#
# LOCAL-ONLY verification gate for the LMD-GHOST fork-choice ("ghosting") logic.
# Structural clone of scripts/check-finalized-floor-ALL.sh. Runs every formal layer
# for the feature under a bounded memory envelope:
#
#   1. Rocq  (AUTHORITATIVE once theories exist) — builds formal/rocq/fork_choice and
#      asserts the six capstones, including certified-context and exact concurrent
#      terminal-frontier correctness,
#      plus the seam lemmas the Rust ENFORCES (validation_implies_wf_dag,
#      validation_implies_single_root [the approved-genesis pin that makes single_root
#      DERIVED from validate.rs::justification_follows, not assumed],
#      honest_forkchoice_parents_validate, sort_total_order) are axiom-free. Any failure
#      here fails the gate. SKIPPED only while no theories/*.v exist yet (scaffold phase).
#   2. TLA+  (fail-soft) — TLC (BOUNDED, explicit-state) on MC_ForkChoice.cfg +
#      MC_ForkChoiceScan.cfg (both PASS) and the two counterexample cfgs
#      (MC_ForkChoice_nontotal.cfg reproduces the S1 non-total-sort fork;
#      MC_ForkChoiceScan_bug.cfg reproduces the node-local-top LCA divergence).
#      Exhaustive only up to MaxId=3/MaxScore=2. SKIPPED if no TLC jar.
#   3. Apalache (fail-soft) — UNBOUNDED symbolic (SMT) inductive-invariant check that
#      COMPLEMENTS the bounded TLC run: proves IndInv == TypeOK /\ Inv_Deterministic /\
#      Inv_HeaviestSubtree is INDUCTIVE (holds on ALL reachable states, no finite
#      horizon) for UNBOUNDED integer scores (score : Int -> Int, not 0..MaxScore), via
#      BASE (Init |= IndInv) + STEP (Next preserves IndInv) on the type-annotated wrapper
#      ForkChoice_apalache.tla. MaxId=6 (2x TLC's 3) is a finite tip-arena bound the
#      Apalache set-encoding requires; only the tip-COUNT is bounded, scores are not.
#      SKIPPED if apalache-mc is absent (mirrors the Wolfram fail-soft tier).
#   4. Z3    (fail-soft) — tiebreak_total_order + score_supply_cap BitVec witnesses.
#   5. Sage  (fail-soft) — fork-choice algebra (score monoid + argmax uniqueness).
#   6. Wolfram (optional, fail-soft) — ghost_heaviest_subtree.wl (greedy head, asynchronous
#      frontier confluence, unsafe global-terminal counterexample, bounded measures).
#      SKIPPED unless RUN_WOLFRAM=1 and a kernel is on PATH;
#      a discovered kernel must bind its configured license and pass the self-test.
#   7. Diagrams (fail-soft) — renders the dossier's PlantUML diagram set and asserts a
#      populated SVG (closing </svg>) with no stderr. SKIPPED if plantuml is absent.
#   8. Rust  (fail-soft) — `cargo test -p casper` the fork-choice verification proptests
#      (C12: the concrete Estimator::filter_deep_parents conforms to GuardBridge.v's
#      within_depth/prop_filter — soundness + main-retention + completeness + exact-set).
#      SKIPPED if cargo is absent; any proptest failure fails the gate.
#
# POLICY: this script is for LOCAL use only. Do NOT wire it (or any Rocq/TLA+/Apalache/
# Wolfram step) into .github/workflows/* — an earlier formal-CI workflow was deliberately
# removed. See docs/casper/theory/fork-choice/fork-choice-verification.md.
#
# Env knobs:
#   ROCQ_MEMMAX=16G   systemd MemoryMax for the Rocq build (default 16G)
#   RUN_SOAK=1        also run the (slow) multi-writer fork-choice churn Rust soak
#   RUN_WOLFRAM=1     opt into the licensed Wolfram cross-witness tier
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ROCQ_DIR="$REPO_ROOT/formal/rocq/fork_choice"
TLA_DIR="$REPO_ROOT/formal/tlaplus/fork_choice"
Z3_DIR="$REPO_ROOT/formal/z3/fork_choice"
SAGE_DIR="$REPO_ROOT/formal/sage/fork_choice"
WL_DIR="$REPO_ROOT/formal/wolfram/fork_choice"
DIAG_DIR="$REPO_ROOT/docs/casper/theory/fork-choice/diagrams"
ROCQ_MEMMAX="${ROCQ_MEMMAX:-16G}"
LOG_DIR="$REPO_ROOT/target/verification/fork-choice"
mkdir -p "$LOG_DIR"
export TLC_WORKERS="${TLC_WORKERS:-1}"

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

echo "== [1/8] Rocq (authoritative) =="
if ! ls "$ROCQ_DIR"/theories/*.v >/dev/null 2>&1; then
  skip "no Rocq theories yet (scaffold phase) — becomes AUTHORITATIVE once modules land"
elif command -v coqc >/dev/null 2>&1 || [[ -x "$HOME/.opam/default/bin/coqc" ]]; then
  # shellcheck disable=SC1090
  eval "$(opam env 2>/dev/null)" 2>/dev/null || true
  ( cd "$ROCQ_DIR" && coq_makefile -f _CoqProject -o Makefile ) >/dev/null 2>&1
  if capped make -C "$ROCQ_DIR" -j1 >"$LOG_DIR/fc_rocq_build.log" 2>&1; then
    pass "Rocq build (Foundation, Score, Filter, CertifiedContext, TieBreak, Lca, Rank, TerminalFrontier, Bound, ParentAntichain, GuardBridge, MainTheorem)"
    tmpd=$(mktemp -d "$LOG_DIR/gate-check.XXXXXX")
    chk="$tmpd/GateCheck.v"
    # The capstones + seam lemmas the Rust ENFORCES (bridge, not assume) +
    # lca_is_lowest + the C2/C4 derived LCA results (maximality + descends-from-root).
    # validation_implies_single_root is the approved-genesis-pin bridge that makes
    # single_root DERIVED (from validate.rs::justification_follows) rather than an
    # assumed premise of the ghost capstone.
    cat > "$chk" <<'EOF'
From ForkChoice Require Import MainTheorem.
From ForkChoice Require Import GuardBridge.
From ForkChoice Require Import TieBreak.
From ForkChoice Require Import Lca.
From ForkChoice Require Import CertifiedContext.
From ForkChoice Require Import TerminalFrontier.
From ForkChoice Require Import ParentAntichain.
Print Assumptions fork_choice_determinism_correct.
Print Assumptions fork_choice_certified_context_correct.
Print Assumptions fork_choice_parent_antichain_correct.
Print Assumptions fork_choice_ghost_correct.
Print Assumptions fork_choice_terminal_frontier_correct.
Print Assumptions fork_choice_bound_correct.
Print Assumptions fork_choice_bridge_correct.
Print Assumptions validation_implies_wf_dag.
Print Assumptions validation_implies_single_root.
Print Assumptions honest_forkchoice_parents_validate.
Print Assumptions sort_total_order.
Print Assumptions reduce_converges.
Print Assumptions lca_is_lowest.
Print Assumptions lcua_many_is_max.
Print Assumptions descends_from_root.
Print Assumptions common_ancestor_root.
Print Assumptions complete_slots_sound.
Print Assumptions floor_projection_sound.
Print Assumptions outside_floor_excluded.
Print Assumptions incomplete_slots_fail_closed.
Print Assumptions receiver_state_noninterference.
Print Assumptions depth_filter_preserves_head.
Print Assumptions honest_forkchoice_parents_validate.
Print Assumptions capped_parents_validate.
Print Assumptions terminal_frontier_exact.
Print Assumptions terminal_frontier_nodup.
Print Assumptions ghost_head_in_terminal_frontier.
Print Assumptions terminal_frontier_confluent.
Print Assumptions ranked_ghost_frontier_correct.
EOF
    out=$(coqc -Q "$ROCQ_DIR/theories" ForkChoice "$chk" 2>&1)
    rm -rf "$tmpd"
    n_closed=$(grep -c "Closed under the global context" <<<"$out")
    if [[ "$n_closed" == "29" ]]; then
      pass "all 29 headline results axiom-free (7 capstones + certified-context, terminal-frontier, LCA, validation, antichain, and parent-bound seams)"
    else
      fail "headline results NOT all axiom-free ($n_closed/29 Closed):"; printf '      %s\n' "${out//$'\n'/$'\n      '}"
    fi
    # Independent kernel re-check (coqchk) — the TRUSTED kernel re-verifies every
    # capstone + dependency `.vo`, not just the elaborator's Print Assumptions.
    if capped coqchk -Q "$ROCQ_DIR/theories" ForkChoice ForkChoice.MainTheorem \
         >"$LOG_DIR/fc_coqchk.log" 2>&1 && grep -q "Modules were successfully checked" "$LOG_DIR/fc_coqchk.log"; then
      pass "coqchk kernel re-check (MainTheorem + all deps)"
    else
      fail "coqchk kernel re-check FAILED (see $LOG_DIR/fc_coqchk.log)"; tail -10 "$LOG_DIR/fc_coqchk.log" | sed 's/^/      /'
    fi
  else
    fail "Rocq build failed (see $LOG_DIR/fc_rocq_build.log)"; tail -20 "$LOG_DIR/fc_rocq_build.log" | sed 's/^/      /'
  fi
else
  fail "coqc not found — Rocq is authoritative, cannot skip"
fi

echo "== [2/8] TLA+ TLC bounded (fail-soft) =="
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
if ! ls "$TLA_DIR"/*.tla >/dev/null 2>&1; then
  skip "no TLA+ modules yet"
elif [[ -f "$TLC_JAR" ]] || command -v tlc >/dev/null 2>&1; then
  # shellcheck disable=SC1091
  source "$REPO_ROOT/scripts/lib/tlc-run.sh"
  if tlc_run "$(tlc_metadir fc_det)" "$TLA_DIR/MC_ForkChoice.cfg" "$TLA_DIR/ForkChoice.tla" >"$LOG_DIR/fc_tlc_det.log" 2>&1; then
    pass "TLA+ ForkChoice (Inv_Deterministic, Inv_HeaviestSubtree, ...)"
  else
    fail "TLA+ MC_ForkChoice.cfg did NOT pass (see $LOG_DIR/fc_tlc_det.log)"
  fi
  if tlc_run "$(tlc_metadir fc_nontotal)" "$TLA_DIR/MC_ForkChoice_nontotal.cfg" "$TLA_DIR/ForkChoice.tla" >"$LOG_DIR/fc_tlc_nontotal.log" 2>&1; then
    fail "TLA+ non-total tie-break should VIOLATE Inv_Deterministic but passed"
  elif grep -q "Inv_Deterministic is violated" "$LOG_DIR/fc_tlc_nontotal.log"; then
    pass "TLA+ non-total tie-break reproduces the S1 fork counterexample"
  else
    fail "TLA+ non-total cfg failed for the wrong reason (see $LOG_DIR/fc_tlc_nontotal.log)"
  fi
  if tlc_run "$(tlc_metadir fc_scan)" "$TLA_DIR/MC_ForkChoiceScan.cfg" "$TLA_DIR/ForkChoiceScan.tla" >"$LOG_DIR/fc_tlc_scan.log" 2>&1; then
    pass "TLA+ scan (complete certified messages retained; receiver-local top cannot change LCA)"
  else
    fail "TLA+ MC_ForkChoiceScan.cfg did NOT pass (see $LOG_DIR/fc_tlc_scan.log)"
  fi
  if tlc_run "$(tlc_metadir fc_scan_bug)" "$TLA_DIR/MC_ForkChoiceScan_bug.cfg" "$TLA_DIR/ForkChoiceScan.tla" >"$LOG_DIR/fc_tlc_scan_bug.log" 2>&1; then
    fail "TLA+ scan bug should VIOLATE Inv_LcaDeterministic but passed"
  elif grep -q "Inv_LcaDeterministic is violated" "$LOG_DIR/fc_tlc_scan_bug.log"; then
    pass "TLA+ legacy local-top projection reproduces receiver-dependent LCA divergence"
  else
      fail "TLA+ scan bug failed for the wrong reason (see $LOG_DIR/fc_tlc_scan_bug.log)"
  fi
  if tlc_run "$(tlc_metadir fc_terminal_frontier)" "$TLA_DIR/MC_GhostTerminalFrontier.cfg" "$TLA_DIR/GhostTerminalFrontier.tla" >"$LOG_DIR/fc_tlc_terminal_frontier.log" 2>&1; then
    pass "TLA+ asynchronous terminal-frontier expansion converges exactly and retains the greedy GHOST head across every expansion order"
  else
    fail "TLA+ MC_GhostTerminalFrontier.cfg did NOT pass (see $LOG_DIR/fc_tlc_terminal_frontier.log)"
  fi
  if tlc_run "$(tlc_metadir fc_global_leaf_unsafe)" "$TLA_DIR/MC_GhostTerminalFrontier_global_leaf_unsafe.cfg" "$TLA_DIR/GhostTerminalFrontier.tla" >"$LOG_DIR/fc_tlc_global_leaf_unsafe.log" 2>&1; then
    fail "TLA+ global-terminal-leaf rule should violate Inv_HeadIsGreedyGhost but passed"
  elif grep -Eq "The invariant of Inv_HeadIsGreedyGhost is equal to FALSE|Invariant Inv_HeadIsGreedyGhost is violated" "$LOG_DIR/fc_tlc_global_leaf_unsafe.log"; then
    pass "TLA+ pinned 60 -> 30/30 versus 40 counterexample rejects global terminal-leaf selection"
  else
    fail "TLA+ global-terminal-leaf control failed for the wrong reason (see $LOG_DIR/fc_tlc_global_leaf_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir fc_parent_depth)" "$TLA_DIR/MC_ParentDepthBounds.cfg" "$TLA_DIR/ParentDepthBounds.tla" >"$LOG_DIR/fc_tlc_parent_depth.log" 2>&1; then
    pass "TLA+ parent bounds preserve the selected head, bound every tail, and satisfy the buffered receiver predicate"
  else
    fail "TLA+ MC_ParentDepthBounds.cfg did NOT pass (see $LOG_DIR/fc_tlc_parent_depth.log)"
  fi
  if tlc_run "$(tlc_metadir fc_parent_depth_head_drop)" "$TLA_DIR/MC_ParentDepthBounds_head_drop_unsafe.cfg" "$TLA_DIR/ParentDepthBounds.tla" >"$LOG_DIR/fc_tlc_parent_depth_head_drop.log" 2>&1; then
    fail "TLA+ all-entry depth filtering should violate Inv_HeadPreserved but passed"
  elif grep -Eq "The invariant of Inv_HeadPreserved is equal to FALSE|Invariant Inv_HeadPreserved is violated" "$LOG_DIR/fc_tlc_parent_depth_head_drop.log"; then
    pass "TLA+ all-entry filter reproduces selected-head loss when a secondary is taller"
  else
    fail "TLA+ head-drop control failed for the wrong reason (see $LOG_DIR/fc_tlc_parent_depth_head_drop.log)"
  fi
  for parent_config_control in zero_cap negative_depth negative_buffer; do
    parent_config_log="$LOG_DIR/fc_tlc_parent_depth_${parent_config_control}.log"
    if tlc_run "$(tlc_metadir "fc_parent_depth_${parent_config_control}")" "$TLA_DIR/MC_ParentDepthBounds_${parent_config_control}_unsafe.cfg" "$TLA_DIR/ParentDepthBounds.tla" >"$parent_config_log" 2>&1; then
      fail "TLA+ ${parent_config_control} parent-bound config should violate Inv_ConfigAdmissible but passed"
    elif grep -Eq "The invariant of Inv_ConfigAdmissible is equal to FALSE|Invariant Inv_ConfigAdmissible is violated" "$parent_config_log"; then
      pass "TLA+ ${parent_config_control} parent-bound config is rejected"
    else
      fail "TLA+ ${parent_config_control} parent-bound control failed for the wrong reason (see $parent_config_log)"
    fi
  done
  if tlc_run "$(tlc_metadir fc_parent_frontier_capacity)" "$TLA_DIR/MC_ParentFrontierCapacity.cfg" "$TLA_DIR/ParentFrontierCapacity.tla" >"$LOG_DIR/fc_tlc_parent_frontier_capacity.log" 2>&1; then
    pass "TLA+ exact live frontier fits despite a larger configured validator maximum, and parallel evaluators preserve the complete frontier"
  else
    fail "TLA+ MC_ParentFrontierCapacity.cfg did NOT pass (see $LOG_DIR/fc_tlc_parent_frontier_capacity.log)"
  fi
  if tlc_run "$(tlc_metadir fc_parent_frontier_over_cap)" "$TLA_DIR/MC_ParentFrontierCapacity_over_cap.cfg" "$TLA_DIR/ParentFrontierCapacity.tla" >"$LOG_DIR/fc_tlc_parent_frontier_over_cap.log" 2>&1; then
    pass "TLA+ an exact over-cap frontier defers on every evaluator without publishing a truncated parent list"
  else
    fail "TLA+ MC_ParentFrontierCapacity_over_cap.cfg did NOT pass (see $LOG_DIR/fc_tlc_parent_frontier_over_cap.log)"
  fi
  if tlc_run "$(tlc_metadir fc_parent_frontier_static_unsafe)" "$TLA_DIR/MC_ParentFrontierCapacity_static_maximum_unsafe.cfg" "$TLA_DIR/ParentFrontierCapacity.tla" >"$LOG_DIR/fc_tlc_parent_frontier_static_unsafe.log" 2>&1; then
    fail "TLA+ static maximum gate should violate Inv_ExactFitIsAdmitted but passed"
  elif grep -Eq "The invariant of Inv_ExactFitIsAdmitted is equal to FALSE|Invariant Inv_ExactFitIsAdmitted is violated" "$LOG_DIR/fc_tlc_parent_frontier_static_unsafe.log"; then
    pass "TLA+ static configured-maximum gate reproduces false deferral of an exact frontier that fits"
  else
    fail "TLA+ static configured-maximum control failed for the wrong reason (see $LOG_DIR/fc_tlc_parent_frontier_static_unsafe.log)"
  fi
else
  skip "no TLC jar (\$TLC_JAR) or 'tlc' on PATH"
fi

echo "== [3/8] Apalache unbounded symbolic (fail-soft) =="
# UNBOUNDED inductive-invariant check COMPLEMENTING the bounded TLC run above. On the
# type-annotated wrapper ForkChoice_apalache.tla (the TLC base module + MC_*.cfg are
# left intact), Apalache proves IndInv == TypeOK /\ Inv_Deterministic /\
# Inv_HeaviestSubtree is INDUCTIVE — holds on ALL reachable states, NO finite horizon —
# for UNBOUNDED integer scores (score : Int -> Int, not 0..MaxScore):
#   BASE: --init=Init  --inv=IndInv --length=0   (every Init state |= IndInv)
#   STEP: --init=IndInv --inv=IndInv --length=1   (Next preserves IndInv)
# PASSES iff BOTH report "The outcome is: NoError". SKIPPED if apalache-mc is absent
# (mirrors the Wolfram fail-soft tier). MaxId=6 (2x TLC's 3) is a finite tip-arena bound
# the Apalache set-encoding requires — only the tip-COUNT is bounded; the scores are
# genuinely unbounded, strictly stronger than the bounded MaxScore=2 TLC run. Runs under
# the shared memory cap (`capped`); SMT scratch lands on-disk under target/ (NVMe).
APALACHE_WRAP="$TLA_DIR/ForkChoice_apalache.tla"
if ! command -v apalache-mc >/dev/null 2>&1; then
  skip "no apalache-mc on PATH — unbounded symbolic IndInv is defense-in-depth beyond the bounded TLC above"
elif [[ ! -f "$APALACHE_WRAP" ]]; then
  skip "no ForkChoice_apalache.tla wrapper present"
else
  aout="$REPO_ROOT/target/apalache-fork-choice"; rm -rf "$aout" 2>/dev/null || true; mkdir -p "$aout"
  a_base=0; a_step=0
  if capped apalache-mc check --init=Init --inv=IndInv --length=0 --cinit=CInit \
       --out-dir="$aout" "$APALACHE_WRAP" >"$LOG_DIR/fc_apalache_base.log" 2>&1 \
       && grep -qE "The outcome is: NoError|No error found" "$LOG_DIR/fc_apalache_base.log"; then
    a_base=1
  fi
  if capped apalache-mc check --init=IndInv --inv=IndInv --length=1 --cinit=CInit \
       --out-dir="$aout" "$APALACHE_WRAP" >"$LOG_DIR/fc_apalache_step.log" 2>&1 \
       && grep -qE "The outcome is: NoError|No error found" "$LOG_DIR/fc_apalache_step.log"; then
    a_step=1
  fi
  if [[ "$a_base" == "1" && "$a_step" == "1" ]]; then
    pass "Apalache UNBOUNDED IndInv inductive — BASE+STEP clean: Inv_Deterministic + Inv_HeaviestSubtree hold on ALL reachable states (unbounded Int scores; MaxId=6 > TLC's 3)"
  else
    [[ "$a_base" == "1" ]] || fail "Apalache BASE (Init |= IndInv) did NOT report NoError (see $LOG_DIR/fc_apalache_base.log)"
    [[ "$a_step" == "1" ]] || fail "Apalache STEP (Next preserves IndInv) did NOT report NoError (see $LOG_DIR/fc_apalache_step.log)"
  fi
fi

CAPACITY_APALACHE_OUT="$REPO_ROOT/target/apalache-parent-frontier-capacity"
if command -v apalache-mc >/dev/null 2>&1; then
  rm -rf "$CAPACITY_APALACHE_OUT" 2>/dev/null || true
  mkdir -p "$CAPACITY_APALACHE_OUT"
  if capped apalache-mc check --config="$TLA_DIR/MC_ParentFrontierCapacityApalache.cfg" --length=2 \
       --out-dir="$CAPACITY_APALACHE_OUT/safe" "$TLA_DIR/ParentFrontierCapacity.tla" >"$LOG_DIR/fc_apalache_parent_frontier_capacity.log" 2>&1 \
       && grep -qE "The outcome is: NoError|No error found|EXITCODE: OK" "$LOG_DIR/fc_apalache_parent_frontier_capacity.log"; then
    pass "Apalache exact-frontier admission and non-signing deferral invariants hold through both parallel evaluations"
  else
    fail "Apalache exact-frontier capacity model did NOT report NoError (see $LOG_DIR/fc_apalache_parent_frontier_capacity.log)"
  fi
  if capped apalache-mc check --config="$TLA_DIR/MC_ParentFrontierCapacityStaticMaximumUnsafeApalache.cfg" --length=1 \
       --out-dir="$CAPACITY_APALACHE_OUT/static-unsafe" "$TLA_DIR/ParentFrontierCapacity.tla" >"$LOG_DIR/fc_apalache_parent_frontier_static_unsafe.log" 2>&1; then
    fail "Apalache static configured-maximum gate should violate Inv_ExactFitIsAdmitted but passed"
  elif grep -qE "state invariant [0-9]+ violated|Invariant Inv_ExactFitIsAdmitted is violated" "$LOG_DIR/fc_apalache_parent_frontier_static_unsafe.log"; then
    pass "Apalache independently reproduces false deferral from the static configured-maximum gate"
  else
    fail "Apalache static configured-maximum control failed for the wrong reason (see $LOG_DIR/fc_apalache_parent_frontier_static_unsafe.log)"
  fi
fi

echo "== [4/8] Z3 cross-witness (fail-soft) =="
if ! ls "$Z3_DIR"/*.py >/dev/null 2>&1; then
  skip "no Z3 scripts yet"
elif command -v python3 >/dev/null 2>&1 && python3 -c 'import z3' >/dev/null 2>&1; then
  if python3 "$Z3_DIR/tiebreak_total_order.py" >"$LOG_DIR/fc_z3_tb.log" 2>&1; then
    pass "Z3 tie-break total order + argmax uniqueness (no fork)"
  else
    fail "Z3 tiebreak_total_order.py failed (see $LOG_DIR/fc_z3_tb.log)"
  fi
  if python3 "$Z3_DIR/score_supply_cap_bitvec.py" >"$LOG_DIR/fc_z3_sc.log" 2>&1; then
    pass "Z3 BitVec-64 score accumulation (assoc/comm; no overflow under supply cap)"
  else
    fail "Z3 score_supply_cap_bitvec.py failed (see $LOG_DIR/fc_z3_sc.log)"
  fi
else
  skip "no python3 z3 module"
fi

echo "== [5/8] Sage cross-witness (fail-soft) =="
if ! ls "$SAGE_DIR"/*.sage >/dev/null 2>&1; then
  skip "no Sage scripts yet"
elif command -v sage >/dev/null 2>&1; then
  if env DOT_SAGE="$LOG_DIR/sage" sage "$SAGE_DIR/forkchoice_algebra.sage" >"$LOG_DIR/fc_sage.log" 2>&1 && grep -q "ALL PASS" "$LOG_DIR/fc_sage.log"; then
    pass "Sage fork-choice algebra (score monoid + heaviest-subtree argmax)"
  else
    fail "Sage forkchoice_algebra.sage failed (see $LOG_DIR/fc_sage.log)"
  fi
else
  skip "no sage on PATH"
fi

echo "== [6/8] Wolfram (optional, fail-soft) =="
WL_BIN=""; WL_RUN=()
if command -v wolframscript >/dev/null 2>&1; then WL_BIN=wolframscript; WL_RUN=(wolframscript -file)
elif command -v wolfram >/dev/null 2>&1;    then WL_BIN=wolfram;       WL_RUN=(wolfram -script)
elif command -v math >/dev/null 2>&1;       then WL_BIN=math;          WL_RUN=(math -script)
fi
if [[ "${RUN_WOLFRAM:-0}" != "1" ]]; then
  skip "licensed Wolfram cross-witness tier is opt-in; set RUN_WOLFRAM=1 to run it"
elif [[ -z "$WL_BIN" || ! -f "$WL_DIR/ghost_heaviest_subtree.wl" || ! -f "$WL_DIR/parent_frontier_capacity.wl" ]]; then
  skip "no wolframscript/math/wolfram kernel on PATH, or a fork-choice Wolfram witness is absent"
else
  wolfram_ok=1
  wolfram_license_unavailable=0
  : >"$LOG_DIR/fc_wolfram.log"
  for wolfram_witness in ghost_heaviest_subtree parent_frontier_capacity; do
    wlout=$(env \
      WOLFRAM_BASE="${WOLFRAM_BASE:-/usr/share/Wolfram}" \
      WOLFRAM_LOCALBASE="${WOLFRAM_LOCALBASE:-${HOME}/.Wolfram/Objects}" \
      WOLFRAM_USERBASE="${WOLFRAM_USERBASE:-${HOME}/.Wolfram}" \
      "${WL_RUN[@]}" "$WL_DIR/${wolfram_witness}.wl" 2>&1); wlrc=$?
    printf '== %s ==\n%s\n' "$wolfram_witness" "$wlout" >>"$LOG_DIR/fc_wolfram.log"
    if grep -qiE 'no valid password|cannot find a valid password' <<<"$wlout"; then
      wolfram_license_unavailable=1
      break
    elif [[ $wlrc -ne 0 ]] || ! grep -q "SELF-TEST: PASS" <<<"$wlout"; then
      wolfram_ok=0
      break
    fi
  done
  if [[ "$wolfram_license_unavailable" == "1" ]]; then
    skip "Wolfram CLI kernel ($WL_BIN) license is currently unavailable (details: $LOG_DIR/fc_wolfram.log)"
  elif [[ "$wolfram_ok" == "1" ]]; then
    pass "Wolfram fork-choice witnesses via $WL_BIN (greedy GHOST, asynchronous terminal frontier, and 245700 exact-capacity decisions)"
  else
    fail "Wolfram fork-choice witness errored or omitted its PASS marker under $WL_BIN (see $LOG_DIR/fc_wolfram.log)"
  fi
fi

echo "== [7/8] PlantUML diagrams (fail-soft) =="
if command -v plantuml >/dev/null 2>&1; then
  n_puml=$(find "$DIAG_DIR" -name '*.puml' 2>/dev/null | wc -l)
  if [[ "$n_puml" -gt 0 ]]; then
    diag_ok=1
    for puml in "$DIAG_DIR"/*.puml; do
      svg="${puml%.puml}.svg"
      derr=$(env -u DISPLAY plantuml -tsvg "$puml" 2>&1)
      if [[ -n "$derr" ]] || [[ ! -s "$svg" ]] || ! grep -q "</svg>" "$svg" 2>/dev/null; then
        fail "diagram $(basename "$puml") did not render clean"
        [[ -n "$derr" ]] && printf '      %s\n' "${derr//$'\n'/$'\n      '}"
        diag_ok=0
      fi
    done
    [[ "$diag_ok" == "1" ]] && pass "all $n_puml PlantUML diagrams render clean (populated SVG, no stderr)"
  else
    skip "no .puml sources in docs/casper/theory/fork-choice/diagrams"
  fi
else
  skip "no plantuml on PATH"
fi

echo "== [8/8] Rust proptests (fail-soft) =="
# C12: the fork-choice verification proptests wired into the `mod` integration-test
# binary (casper/tests/fork_choice/): prop_filter_deep_parents asserts the concrete
# `Estimator::filter_deep_parents` conforms to GuardBridge.v's
# within_depth/prop_filter model — every RETAINED secondary parent is within depth
# (soundness), the main parent is ALWAYS retained first, nothing within depth is dropped
# (completeness), and the retained set equals {main} ∪ prop_filter(secondaries). Compiles
# the casper test harness (cached thereafter), then runs only the module's tests. SKIPPED
# if cargo is absent; any proptest failure fails the gate.
if command -v cargo >/dev/null 2>&1; then
  # The `fork_choice::` filter picks up EVERY module in casper/tests/fork_choice/:
  #   prop_filter_deep_parents (C12), prop_estimator_determinism (determinism +
  #   score-monoid + certified-context locality/frozen-authority checks),
  #   prop_bound (B2/B3/B4 sentinel/overflow/empty seams).
  if cargo test -p casper --test mod -- fork_choice:: >"$LOG_DIR/fc_rust_prop.log" 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/fc_rust_prop.log"; then
    n_rust=$(grep -oE 'result: ok\. [0-9]+ passed' "$LOG_DIR/fc_rust_prop.log" | grep -oE '[0-9]+' | head -1)
    pass "Rust fork-choice proptests (${n_rust:-?} passed: certified-context completeness, floor ancestry, frozen authority, receiver-state independence, deterministic GHOST/LCA, and parent bounds)"
  else
    fail "Rust fork-choice proptests failed (see $LOG_DIR/fc_rust_prop.log)"; tail -20 "$LOG_DIR/fc_rust_prop.log" | sed 's/^/      /'
  fi
  # The tie-break total-order proptests live in the `shared` crate (list_ops), the
  # realization of TieBreak.v `sort_total_order` the estimator's ranking depends on.
  if cargo test -p shared list_ops >"$LOG_DIR/fc_rust_listops.log" 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/fc_rust_listops.log"; then
    n_lo=$(grep -oE 'result: ok\. [0-9]+ passed' "$LOG_DIR/fc_rust_listops.log" | grep -oE '[0-9]+' | head -1)
    pass "Rust tie-break proptests (${n_lo:-?} passed: sort_by_with_decreasing_order — perm-invariant + is-permutation + argmax-unique)"
  else
    fail "Rust tie-break (shared list_ops) proptests failed (see $LOG_DIR/fc_rust_listops.log)"; tail -20 "$LOG_DIR/fc_rust_listops.log" | sed 's/^/      /'
  fi
  # T-MP and causal-antichain discharge live in snapshot.rs's in-module tests.
  if cargo test -p casper --lib -- snapshot::tests >"$LOG_DIR/fc_rust_snapshot.log" 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/fc_rust_snapshot.log"; then
    n_sn=$(grep -oE 'result: ok\. [0-9]+ passed' "$LOG_DIR/fc_rust_snapshot.log" | grep -oE '[0-9]+' | head -1)
    pass "Rust proposal-parent properties (${n_sn:-?} passed: GHOST head preserved under input permutations; reachability-maximal compaction covers every causal tip; floor-aware recovery narrowing; deterministic depth expiry)"
  else
    fail "Rust T-MP main-parent proptests failed (see $LOG_DIR/fc_rust_snapshot.log)"; tail -20 "$LOG_DIR/fc_rust_snapshot.log" | sed 's/^/      /'
  fi
  if cargo test -p casper --lib -- estimator::tests >"$LOG_DIR/fc_rust_estimator.log" 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/fc_rust_estimator.log"; then
    n_est=$(grep -oE 'result: ok\. [0-9]+ passed' "$LOG_DIR/fc_rust_estimator.log" | grep -oE '[0-9]+' | head -1)
    pass "Rust production depth-bound proptests (${n_est:-?} passed: selected head preserved; global-height tail exactness; count composition)"
  else
    fail "Rust production depth-bound proptests failed (see $LOG_DIR/fc_rust_estimator.log)"; tail -20 "$LOG_DIR/fc_rust_estimator.log" | sed 's/^/      /'
  fi
  # C12 receive-side mirror: Validate::parents enforces the SAME depth horizon on the
  # receiving side that filter_deep_parents applies proposer-side — an honest within-horizon
  # parent accepts, a too-deep parent is InvalidParents, and depth_buffer extends the
  # horizon. Extends the abstract GuardBridge bridge to the real validator predicate.
  # Integration test in the `mod` binary (casper/tests/batch2/validate_test.rs).
  if cargo test -p casper --test mod -- parent_validation_enforces_max_parent_depth_horizon >"$LOG_DIR/fc_rust_parents.log" 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/fc_rust_parents.log"; then
    pass "Rust Validate::parents depth-horizon (C12 receive-side: accept within / reject beyond / buffer extends)"
  else
    fail "Rust Validate::parents depth-horizon test failed (see $LOG_DIR/fc_rust_parents.log)"; tail -20 "$LOG_DIR/fc_rust_parents.log" | sed 's/^/      /'
  fi
else
  skip "no cargo on PATH"
fi

if [[ "${RUN_SOAK:-0}" == "1" ]]; then
  echo "== [soak] multi-writer fork-choice churn Rust soak (slow) =="
  if cargo test -p casper --test mod --release -- fork_choice_churn_soak --ignored >"$LOG_DIR/fc_soak.log" 2>&1; then
    pass "fork_choice_churn_soak"
  else
    fail "soak failed (see $LOG_DIR/fc_soak.log)"
  fi
fi

echo
if [[ $rc -eq 0 ]]; then
  printf '\033[32m== fork-choice verification: ALL GATES OK ==\033[0m\n'
else
  printf '\033[31m== fork-choice verification: FAILURES ABOVE ==\033[0m\n'
fi
exit $rc
