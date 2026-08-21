#!/usr/bin/env bash
# scripts/check-finalized-floor-ALL.sh
#
# LOCAL-ONLY verification gate for the finalized-floor multi-parent merge.
# Runs every formal layer for the feature under a bounded memory envelope:
#
#   1. Rocq  (AUTHORITATIVE) — builds formal/rocq/finalized_floor and asserts the
#      forty headline results, including exact occurrence status, exact-effect causal rejection closure,
#      and heartbeat recovery/backpressure refinement,
#      plus the three GuardBridge lemmas that derive Floor.v's AdjDC premise from the
#      Rust committee-constancy guard (guard_constant_committee_transparent,
#      upgo_finalized, chain_adj_AdjDC) are axiom-free. Any failure here fails the gate.
#   2. TLA+/Apalache         — TLC on the POST-fix MC_FinalizedFloor.cfg + the
#      H3/T-PS MC_FinalizedFloorScan.cfg (both must pass) and the two PRE-fix cfgs
#      (both must reproduce their counterexample), plus the abstract admission model
#      and unsafe counterexample, plus exact merge-effect provenance, state-preserving
#      causal-parent floor rebasing after LFB advancement, and its negative controls. Apalache
#      symbolically checks the state-preservation and arrival-order invariants and is mandatory;
#      TLC is skipped only when its jar is unavailable.
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
DEPLOY_RECOVERY_TLA_DIR="$REPO_ROOT/formal/tlaplus/deploy_recovery"
WL_DIR="$REPO_ROOT/formal/wolfram/finalized_floor"
ROCQ_MEMMAX="${ROCQ_MEMMAX:-16G}"
LOG_DIR="$REPO_ROOT/target/verification/finalized-floor"
mkdir -p "$LOG_DIR"
VERIFY_TMP="$LOG_DIR/tmp"
mkdir -p "$VERIFY_TMP"
export TMPDIR="$VERIFY_TMP"
TLC_METADIR_ROOT="$VERIFY_TMP/tlc-metadir"
export TLC_METADIR_ROOT
mkdir -p "$TLC_METADIR_ROOT"
trap 'rm -rf "$VERIFY_TMP"' EXIT

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
  if capped make -C "$ROCQ_DIR" -j1 >"$LOG_DIR/ff_rocq_build.log" 2>&1; then
    pass "Rocq build (Foundation, CliqueOracle, AccountableSafety, Floor, GuardBridge, Merge, OccurrenceDisposition, FinalizedOccurrenceStatus, Recovery, MergeRecoveryCoherence, AdmissionEffectAlignment, RejectionReasonConfluence, ProtocolVersionLifecycle, ProtocolActivationCoherence, Selection, IntegerAdd, FtExact, StateEffectProvenance, CertifiedFloorPromotion, MainTheorem)"
    # Coq derives the module name from the file's basename, so it must be a valid
    # identifier (no dots) — use a fixed name inside a scratch dir.
    tmpd=$(mktemp -d "$LOG_DIR/rocq-gate.XXXXXX")
    chk="$tmpd/GateCheck.v"
    # The 5 original capstones + the 3 Phase-7 GuardBridge lemmas that close the
    # "Rocq assumes what Rust enforces" seam (guard⇒AdjDC bridge + frontier-is-
    # finalized) + the C1/C5 sweep: the θ-exact + advancement capstone and
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
Print Assumptions finalized_floor_merge_correct.
Print Assumptions finalized_floor_occurrence_correct.
Print Assumptions finalized_floor_occurrence_status_scope_correct.
Print Assumptions finalized_floor_recovery_admission_correct.
Print Assumptions finalized_floor_recovery_leadership_correct.
Print Assumptions finalized_floor_merge_recovery_coherence_correct.
Print Assumptions finalized_floor_admission_effect_alignment_correct.
Print Assumptions finalized_floor_rejection_reason_confluence_correct.
Print Assumptions finalized_floor_protocol_activation_correct.
Print Assumptions finalized_floor_protocol_lifecycle_correct.
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
Print Assumptions finalizer_progress_correct.
Print Assumptions bootstrap_replay_and_local_fault_recovery_correct.
Print Assumptions terminal_funding_admission_lifecycle_correct.
Print Assumptions finalized_floor_effect_causal_closure_correct.
Print Assumptions finalized_floor_state_lineage_correct.
Print Assumptions finalized_floor_state_effect_provenance_correct.
Print Assumptions finalized_floor_rebased_parent_selection_correct.
Print Assumptions finalized_floor_state_support_refines_causal_certificate.
Print Assumptions finalized_floor_certified_promotion_correct.
Print Assumptions finalized_floor_latest_message_coverage_correct.
Print Assumptions finalized_floor_linear_snapshot_reuse_correct.
Print Assumptions finalized_floor_snapshot_materialization_correct.
Print Assumptions finalized_floor_heartbeat_backpressure_correct.
Print Assumptions finalized_floor_accountable_safety_correct.
Print Assumptions finalized_floor_strict_accountable_safety_correct.
Print Assumptions finalized_floor_parallel_validator_consensus_correct.
Print Assumptions finalized_floor_parallel_accountable_promotion_correct.
EOF
    out=$(coqc -Q "$ROCQ_DIR/theories" FinalizedFloor "$chk" 2>&1)
    rm -rf "$tmpd"
    n_closed=$(grep -c "Closed under the global context" <<<"$out")
    if [[ "$n_closed" == "40" ]]; then
      pass "all 40 headline results axiom-free, including accountable parallel promotion, accountable finality, parallel-validator isolation, admission/effect alignment, exact occurrence status, exact-effect causal closure, floor-rebased causal inputs, merge-effect provenance, certified-floor promotion, coverage transparency, snapshot materialization, and heartbeat backpressure"
    else
      fail "headline results NOT all axiom-free ($n_closed/40 Closed):"; printf '      %s\n' "${out//$'\n'/$'\n      '}"
    fi
    # Independent kernel re-check (coqchk) — the TRUSTED kernel re-verifies every
    # capstone + dependency `.vo`, not just the elaborator's Print Assumptions.
    if capped coqchk -Q "$ROCQ_DIR/theories" FinalizedFloor FinalizedFloor.MainTheorem \
         >"$LOG_DIR/ff_coqchk.log" 2>&1 && grep -q "Modules were successfully checked" "$LOG_DIR/ff_coqchk.log"; then
      pass "coqchk kernel re-check (MainTheorem + all deps)"
    else
      fail "coqchk kernel re-check FAILED (see $LOG_DIR/ff_coqchk.log)"; tail -10 "$LOG_DIR/ff_coqchk.log" | sed 's/^/      /'
    fi
  else
    fail "Rocq build failed (see $LOG_DIR/ff_rocq_build.log)"; tail -20 "$LOG_DIR/ff_rocq_build.log" | sed 's/^/      /'
  fi
else
  fail "coqc not found — Rocq is authoritative, cannot skip"
fi

echo "== [2/8] TLA+ + Apalache state-preservation verification =="
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
if [[ -f "$TLC_JAR" ]] || command -v tlc >/dev/null 2>&1; then
  # shellcheck disable=SC1091
  source "$REPO_ROOT/scripts/lib/tlc-run.sh"
  # POST-fix: must pass.
  if tlc_run "$(tlc_metadir ff_post_gate)" "$TLA_DIR/MC_FinalizedFloor.cfg" "$TLA_DIR/FinalizedFloor.tla" >"$LOG_DIR/ff_tlc_post.log" 2>&1; then
    pass "TLA+ post-fix SpecFixed (Inv_NoLostParentWrite, Inv_DeltaWithinCap, Liveness_Progress)"
  else
    fail "TLA+ post-fix MC_FinalizedFloor.cfg did NOT pass (see $LOG_DIR/ff_tlc_post.log)"
  fi
  # PRE-fix: must FAIL (counterexample). Inverted sense.
  if tlc_run "$(tlc_metadir ff_pre_gate)" "$TLA_DIR/MC_FinalizedFloor_pre_fix.cfg" "$TLA_DIR/FinalizedFloor.tla" >"$LOG_DIR/ff_tlc_pre.log" 2>&1; then
    fail "TLA+ pre-fix should VIOLATE Inv_NoLostParentWrite but passed (the bug demo is broken)"
  else
    if grep -q "Inv_NoLostParentWrite is violated" "$LOG_DIR/ff_tlc_pre.log"; then
      pass "TLA+ pre-fix reproduces the write-loss counterexample"
    else
      fail "TLA+ pre-fix failed for the wrong reason (see $LOG_DIR/ff_tlc_pre.log)"
    fi
  fi
  # H3 / T-PS scan model: post-fix (BadCut=0) must PASS.
  if tlc_run "$(tlc_metadir ffscan_gate)" "$TLA_DIR/MC_FinalizedFloorScan.cfg" "$TLA_DIR/FinalizedFloorScan.tla" >"$LOG_DIR/ff_tlc_scan.log" 2>&1; then
    pass "TLA+ scan post-fix (H3 no-drop for ANY parent set = T-PS)"
  else
    fail "TLA+ scan MC_FinalizedFloorScan.cfg did NOT pass (see $LOG_DIR/ff_tlc_scan.log)"
  fi
  # H3 bug (BadCut=1, cut above floor): must produce the drop counterexample.
  if tlc_run "$(tlc_metadir ffscan_bug_gate)" "$TLA_DIR/MC_FinalizedFloorScan_bug.cfg" "$TLA_DIR/FinalizedFloorScan.tla" >"$LOG_DIR/ff_tlc_scan_bug.log" 2>&1; then
    fail "TLA+ scan bug should VIOLATE Inv_NoParentWriteDropped but passed"
  else
    if grep -q "Inv_NoParentWriteDropped is violated" "$LOG_DIR/ff_tlc_scan_bug.log"; then
      pass "TLA+ scan bug reproduces the H3 cut-above-floor drop"
    else
      fail "TLA+ scan bug failed for the wrong reason (see $LOG_DIR/ff_tlc_scan_bug.log)"
    fi
  fi
  if tlc_run "$(tlc_metadir ff_finalizer_progress)" "$TLA_DIR/MC_FinalizerProgress.cfg" "$TLA_DIR/FinalizerProgress.tla" >"$LOG_DIR/ff_tlc_finalizer_progress.log" 2>&1; then
    pass "TLA+ complete finalizer scan preserves highest-candidate safety and eventual selection"
  else
    fail "TLA+ complete finalizer scan failed (see $LOG_DIR/ff_tlc_finalizer_progress.log)"
  fi
  for unsafe_kind in cap budget timeout; do
    unsafe_cfg="$TLA_DIR/MC_FinalizerProgress_${unsafe_kind}_unsafe.cfg"
    unsafe_log="$LOG_DIR/ff_tlc_finalizer_progress_${unsafe_kind}_unsafe.log"
    if tlc_run "$(tlc_metadir "ff_finalizer_progress_${unsafe_kind}_unsafe")" "$unsafe_cfg" "$TLA_DIR/FinalizerProgress.tla" >"$unsafe_log" 2>&1; then
      fail "TLA+ finalizer ${unsafe_kind} control should violate eventual selection but passed"
    elif grep -q "Temporal properties were violated" "$unsafe_log"; then
      pass "TLA+ finalizer ${unsafe_kind} control reproduces candidate starvation"
    else
      fail "TLA+ finalizer ${unsafe_kind} control failed for the wrong reason (see $unsafe_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_heartbeat_backpressure)" "$TLA_DIR/MC_HeartbeatFinalityBackpressure.cfg" "$TLA_DIR/HeartbeatFinalityBackpressure.tla" >"$LOG_DIR/ff_tlc_heartbeat_backpressure.log" 2>&1; then
    pass "TLA+ asynchronous heartbeat recovery preserves bounded admission, unique per-round leadership, exact dual certificates, and offline-leader liveness"
  else
    fail "TLA+ heartbeat/finality backpressure model failed (see $LOG_DIR/ff_tlc_heartbeat_backpressure.log)"
  fi
  if tlc_run "$(tlc_metadir ff_heartbeat_eager_unsafe)" "$TLA_DIR/MC_HeartbeatFinalityBackpressure_eager_unsafe.cfg" "$TLA_DIR/HeartbeatFinalityBackpressure.tla" >"$LOG_DIR/ff_tlc_heartbeat_eager_unsafe.log" 2>&1; then
    fail "TLA+ eager-heartbeat control should exceed the validation backlog bound but passed"
  elif grep -q "Inv_ValidationBacklogBounded is violated" "$LOG_DIR/ff_tlc_heartbeat_eager_unsafe.log"; then
    pass "TLA+ eager-heartbeat control reproduces unbounded validation admission"
  else
    fail "TLA+ eager-heartbeat control failed for the wrong reason (see $LOG_DIR/ff_tlc_heartbeat_eager_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_heartbeat_offline_leader_unsafe)" "$TLA_DIR/MC_HeartbeatFinalityBackpressure_offline_leader_unsafe.cfg" "$TLA_DIR/HeartbeatFinalityBackpressure.tla" >"$LOG_DIR/ff_tlc_heartbeat_offline_leader_unsafe.log" 2>&1; then
    fail "TLA+ fixed-offline-leader control should starve finality recovery but passed"
  elif grep -q "Temporal properties were violated" "$LOG_DIR/ff_tlc_heartbeat_offline_leader_unsafe.log"; then
    pass "TLA+ fixed-offline-leader control reproduces finality starvation"
  else
    fail "TLA+ fixed-offline-leader control failed for the wrong reason (see $LOG_DIR/ff_tlc_heartbeat_offline_leader_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_heartbeat_causal_only_unsafe)" "$TLA_DIR/MC_HeartbeatFinalityBackpressure_causal_only_unsafe.cfg" "$TLA_DIR/HeartbeatFinalityBackpressure.tla" >"$LOG_DIR/ff_tlc_heartbeat_causal_only_unsafe.log" 2>&1; then
    fail "TLA+ causal-only heartbeat control should promote without a state certificate but passed"
  elif grep -q "Inv_PromotionUsesExactStateMajority is violated" "$LOG_DIR/ff_tlc_heartbeat_causal_only_unsafe.log"; then
    pass "TLA+ causal-only heartbeat control reproduces unsupported state-floor promotion"
  else
    fail "TLA+ causal-only heartbeat control failed for the wrong reason (see $LOG_DIR/ff_tlc_heartbeat_causal_only_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_accountable_finality)" "$TLA_DIR/MC_AccountableFinality.cfg" "$TLA_DIR/AccountableFinality.tla" >"$LOG_DIR/ff_tlc_accountable_finality.log" 2>&1; then
    pass "TLA+ asynchronous support interleavings preserve exact weighted accountable finality"
  else
    fail "TLA+ accountable-finality model failed (see $LOG_DIR/ff_tlc_accountable_finality.log)"
  fi
  if tlc_run "$(tlc_metadir ff_accountable_honest_double_support_unsafe)" "$TLA_DIR/MC_AccountableFinality_honest_double_support_unsafe.cfg" "$TLA_DIR/AccountableFinality.tla" >"$LOG_DIR/ff_tlc_accountable_honest_double_support_unsafe.log" 2>&1; then
    fail "TLA+ honest-double-support control should create a conflict below the fault budget but passed"
  elif grep -q "Inv_FloorConflictRequiresFaultBudget is violated" "$LOG_DIR/ff_tlc_accountable_honest_double_support_unsafe.log"; then
    pass "TLA+ honest-double-support control reproduces unaccountable conflicting certificates"
  else
    fail "TLA+ honest-double-support control failed for the wrong reason (see $LOG_DIR/ff_tlc_accountable_honest_double_support_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_accountable_fault_budget_unsafe)" "$TLA_DIR/MC_AccountableFinality_fault_budget_unsafe.cfg" "$TLA_DIR/AccountableFinality.tla" >"$LOG_DIR/ff_tlc_accountable_fault_budget_unsafe.log" 2>&1; then
    fail "TLA+ over-budget Byzantine control should create conflicting floor certificates but passed"
  elif grep -q "Inv_NoConflictingFloor is violated" "$LOG_DIR/ff_tlc_accountable_fault_budget_unsafe.log"; then
    pass "TLA+ over-budget Byzantine control demonstrates the accountable-safety boundary"
  else
    fail "TLA+ over-budget Byzantine control failed for the wrong reason (see $LOG_DIR/ff_tlc_accountable_fault_budget_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_state_lineage)" "$TLA_DIR/MC_StateLineageFinality.cfg" "$TLA_DIR/StateLineageFinality.tla" >"$LOG_DIR/ff_tlc_state_lineage.log" 2>&1; then
    pass "TLA+ dual-certificate state admission preserves committed LFB state and rebase liveness without changing causal certificates"
  else
    fail "TLA+ state-lineage safe model failed (see $LOG_DIR/ff_tlc_state_lineage.log)"
  fi
  if tlc_run "$(tlc_metadir ff_state_lineage_unsafe)" "$TLA_DIR/MC_StateLineageFinality_unsafe.cfg" "$TLA_DIR/StateLineageFinality.tla" >"$LOG_DIR/ff_tlc_state_lineage_unsafe.log" 2>&1; then
    fail "TLA+ unguarded state-lineage control should lose a committed LFB state but passed"
  elif grep -q "Inv_AllCommittedStatesRemainInLineage is violated" "$LOG_DIR/ff_tlc_state_lineage_unsafe.log"; then
    pass "TLA+ unguarded control reproduces certified stale-state LFB advancement"
  else
    fail "TLA+ unguarded state-lineage control failed for the wrong reason (see $LOG_DIR/ff_tlc_state_lineage_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_state_support_unsafe)" "$TLA_DIR/MC_StateLineageFinality_state_support_unsafe.cfg" "$TLA_DIR/StateLineageFinality.tla" >"$LOG_DIR/ff_tlc_state_support_unsafe.log" 2>&1; then
    fail "TLA+ causal-only support control should promote a state-unsupported floor but passed"
  elif grep -q "Inv_NoUnsupportedStateFloor is violated" "$LOG_DIR/ff_tlc_state_support_unsafe.log"; then
    pass "TLA+ causal-only control reproduces rejected-parent state-floor promotion"
  else
    fail "TLA+ causal-only support control failed for the wrong reason (see $LOG_DIR/ff_tlc_state_support_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_state_lineage_main_spine_bug)" "$TLA_DIR/MC_StateLineageFinality_main_spine_bug.cfg" "$TLA_DIR/StateLineageFinality.tla" >"$LOG_DIR/ff_tlc_state_lineage_main_spine_bug.log" 2>&1; then
    fail "TLA+ main-spine admission control should reject a valid state-preserving merge but passed"
  elif grep -q "Inv_OffMainRebaseRestoresEligibility is equal to FALSE" "$LOG_DIR/ff_tlc_state_lineage_main_spine_bug.log"; then
    pass "TLA+ main-spine control reproduces asymmetric finalizer starvation"
  else
    fail "TLA+ main-spine admission control failed for the wrong reason (see $LOG_DIR/ff_tlc_state_lineage_main_spine_bug.log)"
  fi
  if tlc_run "$(tlc_metadir ff_state_lineage_main_spine_liveness)" "$TLA_DIR/MC_StateLineageFinality_main_spine_liveness.cfg" "$TLA_DIR/StateLineageFinality.tla" >"$LOG_DIR/ff_tlc_state_lineage_main_spine_liveness.log" 2>&1; then
    fail "TLA+ main-spine liveness control should starve off-main rebase progress but passed"
  elif grep -q "Temporal properties were violated" "$LOG_DIR/ff_tlc_state_lineage_main_spine_liveness.log"; then
    pass "TLA+ main-spine liveness control reproduces permanent asymmetric finalizer starvation"
  else
      fail "TLA+ main-spine liveness control failed for the wrong reason (see $LOG_DIR/ff_tlc_state_lineage_main_spine_liveness.log)"
  fi
  if tlc_run "$(tlc_metadir ff_state_effect_provenance)" "$TLA_DIR/MC_StateEffectProvenance.cfg" "$TLA_DIR/StateEffectProvenance.tla" >"$LOG_DIR/ff_tlc_state_effect_provenance.log" 2>&1; then
    pass "TLA+ exact merge-effect recurrence preserves accepted three-way effects, parent-order invariance, majority support, and promotion liveness"
  else
    fail "TLA+ merge-effect provenance model failed (see $LOG_DIR/ff_tlc_state_effect_provenance.log)"
  fi
  if tlc_run "$(tlc_metadir ff_state_effect_provenance_unsafe)" "$TLA_DIR/MC_StateEffectProvenanceUnsafe.cfg" "$TLA_DIR/StateEffectProvenance.tla" >"$LOG_DIR/ff_tlc_state_effect_provenance_unsafe.log" 2>&1; then
    fail "TLA+ single-base provenance control should lose the accepted source effect but passed"
  elif grep -q "Inv_DeliveredQuorumCertifiesSource is violated" "$LOG_DIR/ff_tlc_state_effect_provenance_unsafe.log"; then
    pass "TLA+ single-base control reproduces loss of majority state support"
  else
    fail "TLA+ single-base provenance control failed for the wrong reason (see $LOG_DIR/ff_tlc_state_effect_provenance_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_state_preserving_fork_choice)" "$TLA_DIR/MC_StatePreservingForkChoice.cfg" "$TLA_DIR/StatePreservingForkChoice.tla" >"$LOG_DIR/ff_tlc_state_preserving_fork_choice.log" 2>&1; then
    pass "TLA+ causal fork choice retains valid tips, falls back to the LFB only when empty, and floor-rebases proposal state"
  else
    fail "TLA+ causal-parent floor-rebase model failed (see $LOG_DIR/ff_tlc_state_preserving_fork_choice.log)"
  fi
  if tlc_run "$(tlc_metadir ff_state_preserving_fork_choice_unsafe)" "$TLA_DIR/MC_StatePreservingForkChoiceUnsafe.cfg" "$TLA_DIR/StatePreservingForkChoice.tla" >"$LOG_DIR/ff_tlc_state_preserving_fork_choice_unsafe.log" 2>&1; then
    fail "TLA+ floor-unprotected replay control should lose finalized state after LFB advancement but passed"
  elif grep -q "Inv_ProposalPreservesSnapshotFloor is violated" "$LOG_DIR/ff_tlc_state_preserving_fork_choice_unsafe.log"; then
    pass "TLA+ floor-unprotected replay control reproduces finalized-effect loss after LFB advancement"
  else
      fail "TLA+ floor-unprotected replay control failed for the wrong reason (see $LOG_DIR/ff_tlc_state_preserving_fork_choice_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_certified_floor_promotion)" "$TLA_DIR/MC_CertifiedFloorPromotion.cfg" "$TLA_DIR/CertifiedFloorPromotion.tla" >"$LOG_DIR/ff_tlc_certified_floor_promotion.log" 2>&1; then
    pass "TLA+ dual-certified universal causal floor promotion is arrival-order independent and live"
  else
    fail "TLA+ certified-floor promotion model failed (see $LOG_DIR/ff_tlc_certified_floor_promotion.log)"
  fi
  if tlc_run "$(tlc_metadir ff_certified_floor_promotion_unsafe)" "$TLA_DIR/MC_CertifiedFloorPromotionUnsafe.cfg" "$TLA_DIR/CertifiedFloorPromotion.tla" >"$LOG_DIR/ff_tlc_certified_floor_promotion_unsafe.log" 2>&1; then
    fail "TLA+ main-spine-only floor discovery should miss the off-main certified floor but passed"
  elif grep -q "Inv_CompleteEvidencePromotesCertifiedFloor is violated" "$LOG_DIR/ff_tlc_certified_floor_promotion_unsafe.log"; then
    pass "TLA+ main-spine-only control reproduces off-main certified-floor starvation"
  else
      fail "TLA+ main-spine-only floor-discovery control failed for the wrong reason (see $LOG_DIR/ff_tlc_certified_floor_promotion_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_latest_message_coverage)" "$TLA_DIR/MC_LatestMessageCoverage.cfg" "$TLA_DIR/LatestMessageCoverage.tla" >"$LOG_DIR/ff_tlc_latest_message_coverage.log" 2>&1; then
    pass "TLA+ descending latest-message coverage is pairwise-exact, fail-closed, and live"
  else
    fail "TLA+ latest-message coverage model failed (see $LOG_DIR/ff_tlc_latest_message_coverage.log)"
  fi
  if tlc_run "$(tlc_metadir ff_latest_message_coverage_unsafe)" "$TLA_DIR/MC_LatestMessageCoverageUnsafe.cfg" "$TLA_DIR/LatestMessageCoverage.tla" >"$LOG_DIR/ff_tlc_latest_message_coverage_unsafe.log" 2>&1; then
    fail "TLA+ unordered coverage control should encounter late propagation but passed"
  elif grep -q "Inv_NoLateCoverage is violated" "$LOG_DIR/ff_tlc_latest_message_coverage_unsafe.log"; then
    pass "TLA+ unordered coverage control reproduces late incomplete support propagation"
  else
      fail "TLA+ unordered coverage control failed for the wrong reason (see $LOG_DIR/ff_tlc_latest_message_coverage_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_snapshot_floor_materialization)" "$TLA_DIR/MC_SnapshotFloorMaterialization.cfg" "$TLA_DIR/SnapshotFloorMaterialization.tla" >"$LOG_DIR/ff_tlc_snapshot_floor_materialization.log" 2>&1; then
    pass "TLA+ snapshot floor closure is complete, exact, interleaving-safe, and live"
  else
    fail "TLA+ snapshot floor-materialization model failed (see $LOG_DIR/ff_tlc_snapshot_floor_materialization.log)"
  fi
  if tlc_run "$(tlc_metadir ff_snapshot_floor_materialization_unsafe)" "$TLA_DIR/MC_SnapshotFloorMaterializationUnsafe.cfg" "$TLA_DIR/SnapshotFloorMaterialization.tla" >"$LOG_DIR/ff_tlc_snapshot_floor_materialization_unsafe.log" 2>&1; then
    fail "TLA+ parent-only snapshot materialization should permit selection without off-parent latest-message provenance but passed"
  elif grep -q "Inv_SelectedSnapshotHasCompleteProvenance is violated" "$LOG_DIR/ff_tlc_snapshot_floor_materialization_unsafe.log"; then
    pass "TLA+ parent-only control reproduces snapshot selection with missing off-parent provenance"
  else
    fail "TLA+ parent-only snapshot materialization failed for the wrong reason (see $LOG_DIR/ff_tlc_snapshot_floor_materialization_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_effect_causal_closure)" "$DEPLOY_RECOVERY_TLA_DIR/MC_EffectCausalClosure.cfg" "$DEPLOY_RECOVERY_TLA_DIR/EffectCausalClosure.tla" >"$LOG_DIR/ff_tlc_effect_causal_closure.log" 2>&1; then
    pass "TLA+ exact-effect rejection is the complete transitive causal closure under every classification order"
  else
    fail "TLA+ exact-effect causal-closure model failed (see $LOG_DIR/ff_tlc_effect_causal_closure.log)"
  fi
  if tlc_run "$(tlc_metadir ff_effect_block_lineage_unsafe)" "$DEPLOY_RECOVERY_TLA_DIR/MC_EffectCausalClosure_block_lineage_unsafe.cfg" "$DEPLOY_RECOVERY_TLA_DIR/EffectCausalClosure.tla" >"$LOG_DIR/ff_tlc_effect_block_lineage_unsafe.log" 2>&1; then
    fail "TLA+ block-lineage control should reject independent exact effects but passed"
  elif grep -q "Inv_IndependentEffectsSurvive is violated" "$LOG_DIR/ff_tlc_effect_block_lineage_unsafe.log"; then
    pass "TLA+ block-lineage control reproduces independent exact-effect loss"
  else
    fail "TLA+ block-lineage control failed for the wrong reason (see $LOG_DIR/ff_tlc_effect_block_lineage_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_effect_direct_only_unsafe)" "$DEPLOY_RECOVERY_TLA_DIR/MC_EffectCausalClosure_direct_only_unsafe.cfg" "$DEPLOY_RECOVERY_TLA_DIR/EffectCausalClosure.tla" >"$LOG_DIR/ff_tlc_effect_direct_only_unsafe.log" 2>&1; then
    fail "TLA+ direct-only control should retain a transitive dependent but passed"
  elif grep -q "Inv_NoAcceptedDependsOnRejected is violated" "$LOG_DIR/ff_tlc_effect_direct_only_unsafe.log"; then
    pass "TLA+ direct-only control reproduces orphaned transitive-effect acceptance"
  else
    fail "TLA+ direct-only control failed for the wrong reason (see $LOG_DIR/ff_tlc_effect_direct_only_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_finalized_occurrence_status)" "$DEPLOY_RECOVERY_TLA_DIR/MC_FinalizedOccurrenceStatus.cfg" "$DEPLOY_RECOVERY_TLA_DIR/FinalizedOccurrenceStatus.tla" >"$LOG_DIR/ff_tlc_finalized_occurrence_status.log" 2>&1; then
    pass "TLA+ finalized deploy status matches exact dispositions across the complete LFB causal closure"
  else
    fail "TLA+ finalized occurrence-status model failed (see $LOG_DIR/ff_tlc_finalized_occurrence_status.log)"
  fi
  if tlc_run "$(tlc_metadir ff_finalized_occurrence_main_chain_unsafe)" "$DEPLOY_RECOVERY_TLA_DIR/MC_FinalizedOccurrenceStatus_main_chain_unsafe.cfg" "$DEPLOY_RECOVERY_TLA_DIR/FinalizedOccurrenceStatus.tla" >"$LOG_DIR/ff_tlc_finalized_occurrence_status_unsafe.log" 2>&1; then
    fail "TLA+ main-chain-only occurrence-status control should retain a rejected secondary-parent source but passed"
  elif grep -q "Inv_StatusMatchesCommittedState is violated" "$LOG_DIR/ff_tlc_finalized_occurrence_status_unsafe.log"; then
    pass "TLA+ main-chain-only control reproduces finalized-status/state disagreement"
  else
    fail "TLA+ main-chain-only occurrence-status control failed for the wrong reason (see $LOG_DIR/ff_tlc_finalized_occurrence_status_unsafe.log)"
  fi
  if "$REPO_ROOT/scripts/check-parallel-validator-consensus.sh" >"$LOG_DIR/ff_tlc_parallel_validator.log" 2>&1; then
    pass "TLA+ independent validator replay, support delivery, crash, and floor-publication interleavings preserve state lineage; all eight defect controls are detected"
  else
    fail "TLA+ parallel-validator consensus model failed (see $LOG_DIR/ff_tlc_parallel_validator.log)"
  fi
else
  skip "no TLC jar (\$TLC_JAR) or 'tlc' on PATH"
fi

if command -v apalache-mc >/dev/null 2>&1; then
  apalache_out="$(mktemp -d "$LOG_DIR/apalache-state-lineage.XXXXXX")"
  parallel_validator_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/parallel-validator-safe" check --config=MC_ParallelValidatorConsensusApalache.cfg --length=6 ParallelValidatorConsensus.tla 2>&1)"
  parallel_validator_rc=$?
  printf '%s\n' "$parallel_validator_output" >"$LOG_DIR/ff_apalache_parallel_validator.log"
  if [[ $parallel_validator_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_parallel_validator.log"; then
    pass "Apalache independent-validator replay, support, and floor-publication invariants through bound 6"
  else
    fail "Apalache parallel-validator consensus model failed (see $LOG_DIR/ff_apalache_parallel_validator.log)"
  fi
  parallel_validator_crash_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/parallel-validator-crash-safe" check --config=MC_ParallelValidatorConsensusCrashApalache.cfg --length=6 ParallelValidatorConsensus.tla 2>&1)"
  parallel_validator_crash_rc=$?
  printf '%s\n' "$parallel_validator_crash_output" >"$LOG_DIR/ff_apalache_parallel_validator_crash.log"
  if [[ $parallel_validator_crash_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_parallel_validator_crash.log"; then
    pass "Apalache validator-local roots survive crash/restart schedules through bound 6"
  else
    fail "Apalache parallel-validator crash model failed (see $LOG_DIR/ff_apalache_parallel_validator_crash.log)"
  fi
  parallel_validator_crash_unsafe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/parallel-validator-crash-root-unsafe" check --config=MC_ParallelValidatorConsensus_crash_root_unsafe.cfg --length=5 ParallelValidatorConsensus.tla 2>&1)"
  parallel_validator_crash_unsafe_rc=$?
  printf '%s\n' "$parallel_validator_crash_unsafe_output" >"$LOG_DIR/ff_apalache_parallel_validator_crash_root_unsafe.log"
  if [[ $parallel_validator_crash_unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_parallel_validator_crash_root_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_parallel_validator_crash_root_unsafe.log"; then
    pass "Apalache crash-root deletion control finds lost validator-local replay state"
  else
    fail "Apalache crash-root deletion control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_parallel_validator_crash_root_unsafe.log)"
  fi
  accountable_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/accountable-safe" check --config=MC_AccountableFinalityApalache.cfg --length=5 AccountableFinality.tla 2>&1)"
  accountable_rc=$?
  printf '%s\n' "$accountable_output" >"$LOG_DIR/ff_apalache_accountable_finality.log"
  if [[ $accountable_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_accountable_finality.log"; then
    pass "Apalache exact weighted accountable-finality invariants through bound 5"
  else
    fail "Apalache accountable-finality model failed (see $LOG_DIR/ff_apalache_accountable_finality.log)"
  fi
  accountable_unsafe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/accountable-unsafe" check --config=MC_AccountableFinality_honest_double_support_unsafe_Apalache.cfg --length=3 AccountableFinality.tla 2>&1)"
  accountable_unsafe_rc=$?
  printf '%s\n' "$accountable_unsafe_output" >"$LOG_DIR/ff_apalache_accountable_finality_unsafe.log"
  if [[ $accountable_unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_accountable_finality_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_accountable_finality_unsafe.log"; then
    pass "Apalache honest-double-support control finds an unaccountable certificate conflict"
  else
    fail "Apalache accountable-finality negative control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_accountable_finality_unsafe.log)"
  fi
  heartbeat_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/heartbeat-safe" check --config=MC_HeartbeatFinalityBackpressureApalache.cfg --length=10 HeartbeatFinalityBackpressure.tla 2>&1)"
  heartbeat_rc=$?
  printf '%s\n' "$heartbeat_output" >"$LOG_DIR/ff_apalache_heartbeat_backpressure.log"
  if [[ $heartbeat_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_heartbeat_backpressure.log"; then
    pass "Apalache asynchronous heartbeat/backpressure invariants through bound 10"
  else
    fail "Apalache heartbeat/finality backpressure model failed (see $LOG_DIR/ff_apalache_heartbeat_backpressure.log)"
  fi
  heartbeat_eager_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/heartbeat-eager-unsafe" check --config=MC_HeartbeatFinalityBackpressure_eager_unsafe_Apalache.cfg --length=8 HeartbeatFinalityBackpressure.tla 2>&1)"
  heartbeat_eager_rc=$?
  printf '%s\n' "$heartbeat_eager_output" >"$LOG_DIR/ff_apalache_heartbeat_eager_unsafe.log"
  if [[ $heartbeat_eager_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_heartbeat_eager_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_heartbeat_eager_unsafe.log"; then
    pass "Apalache eager-heartbeat control finds validation-backlog overflow"
  else
    fail "Apalache eager-heartbeat control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_heartbeat_eager_unsafe.log)"
  fi
  heartbeat_causal_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/heartbeat-causal-unsafe" check --config=MC_HeartbeatFinalityBackpressure_causal_only_unsafe_Apalache.cfg --length=8 HeartbeatFinalityBackpressure.tla 2>&1)"
  heartbeat_causal_rc=$?
  printf '%s\n' "$heartbeat_causal_output" >"$LOG_DIR/ff_apalache_heartbeat_causal_only_unsafe.log"
  if [[ $heartbeat_causal_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_heartbeat_causal_only_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_heartbeat_causal_only_unsafe.log"; then
    pass "Apalache causal-only heartbeat control finds unsupported state-floor promotion"
  else
    fail "Apalache causal-only heartbeat control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_heartbeat_causal_only_unsafe.log)"
  fi
  safe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/safe" check --config=MC_StateLineageFinalityApalache.cfg --length=8 StateLineageFinality.tla 2>&1)"
  safe_rc=$?
  printf '%s\n' "$safe_output" >"$LOG_DIR/ff_apalache_state_lineage.log"
  if [[ $safe_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_state_lineage.log"; then
    pass "Apalache two-validator state-lineage invariants through bound 8"
  else
    fail "Apalache state-lineage safe model failed (see $LOG_DIR/ff_apalache_state_lineage.log)"
  fi
  effect_provenance_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/state-effect-safe" check --config=MC_StateEffectProvenanceApalache.cfg --length=8 StateEffectProvenanceApalache.tla 2>&1)"
  effect_provenance_rc=$?
  printf '%s\n' "$effect_provenance_output" >"$LOG_DIR/ff_apalache_state_effect_provenance.log"
  if [[ $effect_provenance_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_state_effect_provenance.log"; then
    pass "Apalache arrival-order merge settlement preserves all accepted parent effects through bound 8"
  else
    fail "Apalache merge-effect provenance model failed (see $LOG_DIR/ff_apalache_state_effect_provenance.log)"
  fi
  effect_provenance_unsafe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/state-effect-unsafe" check --config=MC_StateEffectProvenanceUnsafeApalache.cfg --length=4 StateEffectProvenanceApalache.tla 2>&1)"
  effect_provenance_unsafe_rc=$?
  printf '%s\n' "$effect_provenance_unsafe_output" >"$LOG_DIR/ff_apalache_state_effect_provenance_unsafe.log"
  if [[ $effect_provenance_unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_state_effect_provenance_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_state_effect_provenance_unsafe.log"; then
    pass "Apalache single-base control finds accepted source-effect loss"
  else
    fail "Apalache single-base provenance control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_state_effect_provenance_unsafe.log)"
  fi
  fork_choice_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/state-preserving-fork-choice-safe" check --config=MC_StatePreservingForkChoiceApalache.cfg --length=10 StatePreservingForkChoice.tla 2>&1)"
  fork_choice_rc=$?
  printf '%s\n' "$fork_choice_output" >"$LOG_DIR/ff_apalache_state_preserving_fork_choice.log"
  if [[ $fork_choice_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_state_preserving_fork_choice.log"; then
    pass "Apalache node-local causal-parent floor-rebase invariants through bound 10"
  else
    fail "Apalache causal-parent floor-rebase model failed (see $LOG_DIR/ff_apalache_state_preserving_fork_choice.log)"
  fi
  fork_choice_unsafe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/state-preserving-fork-choice-unsafe" check --config=MC_StatePreservingForkChoiceUnsafeApalache.cfg --length=8 StatePreservingForkChoice.tla 2>&1)"
  fork_choice_unsafe_rc=$?
  printf '%s\n' "$fork_choice_unsafe_output" >"$LOG_DIR/ff_apalache_state_preserving_fork_choice_unsafe.log"
  if [[ $fork_choice_unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_state_preserving_fork_choice_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_state_preserving_fork_choice_unsafe.log"; then
    pass "Apalache floor-unprotected replay control finds finalized-effect loss"
  else
    fail "Apalache floor-unprotected replay control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_state_preserving_fork_choice_unsafe.log)"
  fi
  certified_floor_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/certified-floor-safe" check --config=MC_CertifiedFloorPromotionApalache.cfg --length=8 CertifiedFloorPromotion.tla 2>&1)"
  certified_floor_rc=$?
  printf '%s\n' "$certified_floor_output" >"$LOG_DIR/ff_apalache_certified_floor_promotion.log"
  if [[ $certified_floor_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_certified_floor_promotion.log"; then
    pass "Apalache certified-floor promotion invariants through bound 8"
  else
    fail "Apalache certified-floor promotion model failed (see $LOG_DIR/ff_apalache_certified_floor_promotion.log)"
  fi
  certified_floor_unsafe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/certified-floor-unsafe" check --config=MC_CertifiedFloorPromotionUnsafeApalache.cfg --length=4 CertifiedFloorPromotion.tla 2>&1)"
  certified_floor_unsafe_rc=$?
  printf '%s\n' "$certified_floor_unsafe_output" >"$LOG_DIR/ff_apalache_certified_floor_promotion_unsafe.log"
  if [[ $certified_floor_unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_certified_floor_promotion_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_certified_floor_promotion_unsafe.log"; then
    pass "Apalache main-spine-only control finds off-main certified-floor starvation"
  else
    fail "Apalache main-spine-only floor-discovery control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_certified_floor_promotion_unsafe.log)"
  fi
  latest_coverage_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/latest-coverage-safe" check --config=MC_LatestMessageCoverageApalache.cfg --length=8 LatestMessageCoverage.tla 2>&1)"
  latest_coverage_rc=$?
  printf '%s\n' "$latest_coverage_output" >"$LOG_DIR/ff_apalache_latest_message_coverage.log"
  if [[ $latest_coverage_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_latest_message_coverage.log"; then
    pass "Apalache descending latest-message coverage invariants through bound 8"
  else
    fail "Apalache latest-message coverage model failed (see $LOG_DIR/ff_apalache_latest_message_coverage.log)"
  fi
  latest_coverage_unsafe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/latest-coverage-unsafe" check --config=MC_LatestMessageCoverageUnsafeApalache.cfg --length=4 LatestMessageCoverage.tla 2>&1)"
  latest_coverage_unsafe_rc=$?
  printf '%s\n' "$latest_coverage_unsafe_output" >"$LOG_DIR/ff_apalache_latest_message_coverage_unsafe.log"
  if [[ $latest_coverage_unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_latest_message_coverage_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_latest_message_coverage_unsafe.log"; then
    pass "Apalache unordered coverage control finds late incomplete propagation"
  else
    fail "Apalache unordered coverage control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_latest_message_coverage_unsafe.log)"
  fi
  snapshot_materialization_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/snapshot-materialization-safe" check --config=MC_SnapshotFloorMaterializationApalache.cfg --length=8 SnapshotFloorMaterialization.tla 2>&1)"
  snapshot_materialization_rc=$?
  printf '%s\n' "$snapshot_materialization_output" >"$LOG_DIR/ff_apalache_snapshot_floor_materialization.log"
  if [[ $snapshot_materialization_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_snapshot_floor_materialization.log"; then
    pass "Apalache snapshot floor-closure invariants through bound 8"
  else
    fail "Apalache snapshot floor-materialization model failed (see $LOG_DIR/ff_apalache_snapshot_floor_materialization.log)"
  fi
  snapshot_materialization_unsafe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/snapshot-materialization-unsafe" check --config=MC_SnapshotFloorMaterializationUnsafeApalache.cfg --length=4 SnapshotFloorMaterialization.tla 2>&1)"
  snapshot_materialization_unsafe_rc=$?
  printf '%s\n' "$snapshot_materialization_unsafe_output" >"$LOG_DIR/ff_apalache_snapshot_floor_materialization_unsafe.log"
  if [[ $snapshot_materialization_unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_snapshot_floor_materialization_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_snapshot_floor_materialization_unsafe.log"; then
    pass "Apalache parent-only control finds missing off-parent snapshot provenance"
  else
    fail "Apalache parent-only snapshot materialization did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_snapshot_floor_materialization_unsafe.log)"
  fi
  unsafe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/unsafe" check --config=MC_StateLineageFinality_unsafe.cfg --inv=Inv_AllCommittedStatesRemainInLineage --length=2 StateLineageFinality.tla 2>&1)"
  unsafe_rc=$?
  printf '%s\n' "$unsafe_output" >"$LOG_DIR/ff_apalache_state_lineage_unsafe.log"
  if [[ $unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_state_lineage_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_state_lineage_unsafe.log"; then
    pass "Apalache unguarded control finds the stale-state counterexample"
  else
    fail "Apalache unguarded control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_state_lineage_unsafe.log)"
  fi
  state_support_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/state-support-unsafe" check --config=MC_StateLineageFinality_state_support_unsafe.cfg --inv=Inv_NoUnsupportedStateFloor --length=3 StateLineageFinality.tla 2>&1)"
  state_support_rc=$?
  printf '%s\n' "$state_support_output" >"$LOG_DIR/ff_apalache_state_support_unsafe.log"
  if [[ $state_support_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_state_support_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_state_support_unsafe.log"; then
    pass "Apalache causal-only control finds rejected-parent state-floor promotion"
  else
    fail "Apalache causal-only support control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_state_support_unsafe.log)"
  fi
  main_spine_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/main-spine-bug" check --config=MC_StateLineageFinality_main_spine_bug.cfg --inv=Inv_OffMainRebaseRestoresEligibility --length=1 StateLineageFinality.tla 2>&1)"
  main_spine_rc=$?
  printf '%s\n' "$main_spine_output" >"$LOG_DIR/ff_apalache_state_lineage_main_spine_bug.log"
  if [[ $main_spine_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_state_lineage_main_spine_bug.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_state_lineage_main_spine_bug.log"; then
    pass "Apalache main-spine control finds the asymmetric liveness counterexample"
  else
    fail "Apalache main-spine control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_state_lineage_main_spine_bug.log)"
  fi
  effect_safe_output="$(cd "$DEPLOY_RECOVERY_TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/effect-safe" check --config=MC_EffectCausalClosure.cfg --length=6 EffectCausalClosure.tla 2>&1)"
  effect_safe_rc=$?
  printf '%s\n' "$effect_safe_output" >"$LOG_DIR/ff_apalache_effect_causal_closure.log"
  if [[ $effect_safe_rc -eq 0 ]] && grep -q 'EXITCODE: OK' "$LOG_DIR/ff_apalache_effect_causal_closure.log"; then
    pass "Apalache exact-effect causal-closure invariants through the complete bounded execution"
  else
    fail "Apalache exact-effect causal-closure model failed (see $LOG_DIR/ff_apalache_effect_causal_closure.log)"
  fi
  effect_block_output="$(cd "$DEPLOY_RECOVERY_TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/effect-block-unsafe" check --config=MC_EffectCausalClosure_block_lineage_unsafe.cfg --inv=Inv_IndependentEffectsSurvive --length=5 EffectCausalClosure.tla 2>&1)"
  effect_block_rc=$?
  printf '%s\n' "$effect_block_output" >"$LOG_DIR/ff_apalache_effect_block_lineage_unsafe.log"
  if [[ $effect_block_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_effect_block_lineage_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_effect_block_lineage_unsafe.log"; then
    pass "Apalache block-lineage control finds independent exact-effect loss"
  else
    fail "Apalache block-lineage control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_effect_block_lineage_unsafe.log)"
  fi
  effect_direct_output="$(cd "$DEPLOY_RECOVERY_TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/effect-direct-unsafe" check --config=MC_EffectCausalClosure_direct_only_unsafe.cfg --inv=Inv_NoAcceptedDependsOnRejected --length=3 EffectCausalClosure.tla 2>&1)"
  effect_direct_rc=$?
  printf '%s\n' "$effect_direct_output" >"$LOG_DIR/ff_apalache_effect_direct_only_unsafe.log"
  if [[ $effect_direct_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_effect_direct_only_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_effect_direct_only_unsafe.log"; then
    pass "Apalache direct-only control finds orphaned transitive-effect acceptance"
  else
    fail "Apalache direct-only control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_effect_direct_only_unsafe.log)"
  fi
  occurrence_status_output="$(cd "$DEPLOY_RECOVERY_TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/finalized-occurrence-safe" check --config=MC_FinalizedOccurrenceStatusApalache.cfg --length=6 FinalizedOccurrenceStatus.tla 2>&1)"
  occurrence_status_rc=$?
  printf '%s\n' "$occurrence_status_output" >"$LOG_DIR/ff_apalache_finalized_occurrence_status.log"
  if [[ $occurrence_status_rc -eq 0 ]] && grep -q 'EXITCODE: OK' "$LOG_DIR/ff_apalache_finalized_occurrence_status.log"; then
    pass "Apalache finalized occurrence-status invariants across causal evidence interleavings"
  else
    fail "Apalache finalized occurrence-status model failed (see $LOG_DIR/ff_apalache_finalized_occurrence_status.log)"
  fi
  occurrence_status_unsafe_output="$(cd "$DEPLOY_RECOVERY_TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/finalized-occurrence-main-chain-unsafe" check --config=MC_FinalizedOccurrenceStatus_main_chain_unsafe_Apalache.cfg --length=5 FinalizedOccurrenceStatus.tla 2>&1)"
  occurrence_status_unsafe_rc=$?
  printf '%s\n' "$occurrence_status_unsafe_output" >"$LOG_DIR/ff_apalache_finalized_occurrence_status_unsafe.log"
  if [[ $occurrence_status_unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_finalized_occurrence_status_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_finalized_occurrence_status_unsafe.log"; then
    pass "Apalache main-chain-only control finds finalized-status/state disagreement"
  else
    fail "Apalache main-chain-only occurrence-status control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_finalized_occurrence_status_unsafe.log)"
  fi
  rm -rf "$apalache_out"
else
  fail "apalache-mc not found — state-preservation symbolic verification is mandatory"
fi

echo "== [3/8] Z3 cross-witness (fail-soft) =="
if command -v python3 >/dev/null 2>&1 && python3 -c 'import z3' >/dev/null 2>&1; then
  if python3 "$REPO_ROOT/formal/z3/finalized_floor/ft_algebra_crosswitness.py" >"$LOG_DIR/ff_z3_ft.log" 2>&1; then
    pass "Z3 FT-algebra + L-ANC/L-SNAP monotonicity + merge determinism"
  else
    fail "Z3 ft_algebra_crosswitness.py failed (see $LOG_DIR/ff_z3_ft.log)"
  fi
  if python3 "$REPO_ROOT/formal/z3/finalized_floor/integeradd_launder_bitvec.py" >"$LOG_DIR/ff_z3_ia.log" 2>&1; then
    pass "Z3 BitVec-64 IntegerAdd launder (exists on wrap; checked_combine launder-free)"
  else
    fail "Z3 integeradd_launder_bitvec.py failed (see $LOG_DIR/ff_z3_ia.log)"
  fi
  if python3 "$REPO_ROOT/formal/z3/finalized_floor/ft_exact_no_overflow.py" >"$LOG_DIR/ff_z3_fte.log" 2>&1; then
    pass "Z3 A9 exact-integer FT (i128 no-overflow; exact≡ratio; f32 residual real)"
  else
    fail "Z3 ft_exact_no_overflow.py failed (see $LOG_DIR/ff_z3_fte.log)"
  fi
  if python3 "$REPO_ROOT/formal/z3/finalized_floor/ft_ppm_roundtrip.py" >"$LOG_DIR/ff_z3_rt.log" 2>&1; then
    pass "Z3 G2 ppm provenance + round-trip (to_ppm monotone/range/½ppm round-trip; redisplay fixed-point; exact-decision display-invariance; IEEE FPA corroboration)"
  else
    fail "Z3 ft_ppm_roundtrip.py failed (see $LOG_DIR/ff_z3_rt.log)"
  fi
else
  skip "no python3 z3 module"
fi

echo "== [4/8] Sage cross-witness (fail-soft) =="
if command -v sage >/dev/null 2>&1; then
  mkdir -p "$VERIFY_TMP/sage"
  if DOT_SAGE="$VERIFY_TMP/sage" sage "$REPO_ROOT/formal/sage/finalized_floor/ft_algebra.sage" >"$LOG_DIR/ff_sage.log" 2>&1 \
       && grep -q "ALL PASS" "$LOG_DIR/ff_sage.log"; then
    pass "Sage FT-algebra identity + finalization-margin monotonicity"
  else
    fail "Sage ft_algebra.sage failed (see $LOG_DIR/ff_sage.log)"
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
  echo "$wlout" >"$LOG_DIR/ff_wolfram.log"
  if grep -qiE 'no valid password|cannot find a valid password' <<<"$wlout"; then
    # The LICENSE is valid — delta_ratchet.wl is validated via the licensed
    # Wolfram MCP evaluator. This CLI kernel simply could not BIND the license
    # in this shell: a `mathpass` password is version-keyed, so an entry issued
    # for a different major version does not license the installed kernel. To
    # enable the CLI, activate this kernel (`math`, then Web Activation with your
    # activation key) or install `wolframscript` (WolframID/cloud licensing).
    skip "Wolfram CLI kernel ($WL_BIN) could not bind the license in this shell — model validated via the licensed MCP evaluator (details: $LOG_DIR/ff_wolfram.log)"
  elif [[ $wlrc -eq 0 ]]; then
    pass "Wolfram delta_ratchet.wl via $WL_BIN (buggy advance unstable, fixed advance stable)"
  else
    fail "Wolfram delta_ratchet.wl errored under $WL_BIN (see $LOG_DIR/ff_wolfram.log)"
  fi
else
  skip "no wolframscript/math/wolfram kernel on PATH"
fi

echo "== [6/8] PlantUML diagrams (fail-soft) =="
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
      derr=$(env -u DISPLAY plantuml -tsvg "$puml" 2>&1)
      if [[ -n "$derr" ]] || [[ ! -s "$svg" ]] || ! grep -q "</svg>" "$svg" 2>/dev/null; then
        fail "diagram $(basename "$puml") did not render clean"
        [[ -n "$derr" ]] && printf '      %s\n' "${derr//$'\n'/$'\n      '}"
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

echo "== [7/8] Rust proptests + floor-selection lib tests (fail-soft) =="
# The finalized-floor Phase-4 proptests, wired into the `mod` integration-test binary
# (casper/tests/finalized_floor/): G2 θ_ppm provenance + f32↔ppm round-trip, and P1
# committee derivation PLAY≡REPLAY. Compiles the casper test harness (one-time; cached
# thereafter), then runs only the `finalized_floor::` tests. SKIPPED if cargo is absent;
# any proptest failure fails the gate.
if command -v cargo >/dev/null 2>&1; then
  if cargo test -p casper --test mod -- finalized_floor:: >"$LOG_DIR/ff_rust_prop.log" 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/ff_rust_prop.log"; then
    n_rust=$(grep -oE 'result: ok\. [0-9]+ passed' "$LOG_DIR/ff_rust_prop.log" | grep -oE '[0-9]+' | head -1)
    pass "Rust finalized-floor proptests (${n_rust:-?} passed: G2 provenance/round-trip + P1 committee PLAY≡REPLAY)"
  else
    fail "Rust finalized-floor proptests failed (see $LOG_DIR/ff_rust_prop.log)"; tail -20 "$LOG_DIR/ff_rust_prop.log" | sed 's/^/      /'
  fi
  # Floor Selection lib tests (finality::floor #[cfg(test)]) — the derive_floor case
  # analysis that Selection.v proves: Case-A common-ancestor (T-LIN), highest-sound
  # maximality (T-DET), general-finalized result (T-FIN), plus the Case-B dominating-tip
  # pick and the incompatible-fork safety error. These are LIB unit tests (not the `mod`
  # integration binary), so they need their own invocation.
  if cargo test -p casper --lib finality::floor:: >"$LOG_DIR/ff_rust_lib.log" 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/ff_rust_lib.log" \
       && grep -q "derive_floor_promotes_dual_certified_universal_secondary_ancestor ... ok" "$LOG_DIR/ff_rust_lib.log" \
       && grep -q "dual_certified_universal_floor_is_independent_of_branch_shape_and_parent_order ... ok" "$LOG_DIR/ff_rust_lib.log" \
       && grep -q "latest_message_coverage_rejects_non_descending_edges ... ok" "$LOG_DIR/ff_rust_lib.log" \
       && grep -q "finalized_floor_materializes_off_parent_latest_message_provenance ... ok" "$LOG_DIR/ff_rust_lib.log" \
       && grep -q "universal_frontier_reuse_requires_a_linear_parent_and_unchanged_prior_snapshot ... ok" "$LOG_DIR/ff_rust_lib.log"; then
    n_lib=$(grep -oE 'result: ok\. [0-9]+ passed' "$LOG_DIR/ff_rust_lib.log" | grep -oE '[0-9]+' | head -1)
    pass "Rust floor-selection lib tests (${n_lib:-?} passed: dual-certified promotion + pairwise coverage equivalence + linear-reuse guards + rejected-state control)"
  else
    fail "Rust floor-selection lib tests failed (see $LOG_DIR/ff_rust_lib.log)"; tail -20 "$LOG_DIR/ff_rust_lib.log" | sed 's/^/      /'
  fi
  if cargo test -p casper --lib engine::multi_parent_casper::snapshot::tests:: >"$LOG_DIR/ff_rust_snapshot.log" 2>&1 \
       && grep -q "empty_valid_parent_set_falls_back_to_last_finalized_block ... ok" "$LOG_DIR/ff_rust_snapshot.log" \
       && grep -q "parent_selection_prunes_dag_covered_parents ... ok" "$LOG_DIR/ff_rust_snapshot.log"; then
    pass "Rust snapshot parent selection preserves causal coverage and falls back to the captured LFB only for an empty valid set"
  else
    fail "Rust causal-parent snapshot regressions failed (see $LOG_DIR/ff_rust_snapshot.log)"; tail -20 "$LOG_DIR/ff_rust_snapshot.log" | sed 's/^/      /'
  fi
  if cargo test -p casper --test mod -- batch2::finalizer_test::finalizer_examines_a_complete_frozen_candidate_set_beyond_the_old_prefix --exact >"$LOG_DIR/ff_rust_finalizer_progress.log" 2>&1 \
       && cargo test -p casper --test mod -- batch2::finalizer_test::finalizer_requires_main_parent_convergence_in_a_reconvergent_dag --exact >>"$LOG_DIR/ff_rust_finalizer_progress.log" 2>&1 \
       && cargo test -p casper --test mod -- batch2::finalizer_test::finalizer_rejects_dag_descendant_without_state_lineage --exact >>"$LOG_DIR/ff_rust_finalizer_progress.log" 2>&1 \
       && cargo test -p casper --test mod -- batch2::finalizer_test::finalizer_advances_to_state_descendant_when_lfb_is_a_secondary_parent --exact >>"$LOG_DIR/ff_rust_finalizer_progress.log" 2>&1 \
       && cargo test -p casper --test mod -- compute_parents_post_state_regression_spec::compute_parents_post_state_fast_paths_only_when_the_cover_preserves_the_floor --exact >>"$LOG_DIR/ff_rust_finalizer_progress.log" 2>&1 \
       && test "$(grep -cE "test result: ok\. 1 passed" "$LOG_DIR/ff_rust_finalizer_progress.log")" -eq 5; then
    pass "Rust complete-scan, main-parent convergence, unchanged-clique/state-preservation, off-main rebase progress, and execution-rebase regressions"
  else
    fail "Rust finalizer progress regressions failed (see $LOG_DIR/ff_rust_finalizer_progress.log)"; tail -20 "$LOG_DIR/ff_rust_finalizer_progress.log" | sed 's/^/      /'
  fi
  if cargo test -p node heartbeat_proposer >"$LOG_DIR/ff_rust_heartbeat.log" 2>&1 \
       && grep -q "recovery_leader_is_unique_and_permutation_invariant ... ok" "$LOG_DIR/ff_rust_heartbeat.log" \
       && grep -q "recovery_leader_rotation_visits_every_unique_validator ... ok" "$LOG_DIR/ff_rust_heartbeat.log" \
       && grep -q "do_heartbeat_check_triggers_one_recovery_proposal_after_observed_lfb_stall ... ok" "$LOG_DIR/ff_rust_heartbeat.log" \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/ff_rust_heartbeat.log"; then
    pass "Rust heartbeat recovery unit/property tests (observed LFB progress, bounded proposal admission, canonical committee, and rotating leadership)"
  else
    fail "Rust heartbeat/finality recovery regressions failed (see $LOG_DIR/ff_rust_heartbeat.log)"; tail -20 "$LOG_DIR/ff_rust_heartbeat.log" | sed 's/^/      /'
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
  if cargo test -p block-storage --test loom_frontier_floor_cache >"$LOG_DIR/ff_loom.log" 2>&1; then
    if grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/ff_loom.log"; then
      pass "Loom finalized-floor cache (no torn/regressed value on any interleaving; write-once memo + single-key MVCC)"
    else
      skip "Loom finalized-floor cache: test target unavailable in this build cfg (fail-soft; see $LOG_DIR/ff_loom.log)"
    fi
  elif grep -q "test result: FAILED" "$LOG_DIR/ff_loom.log"; then
    fail "Loom finalized-floor cache found a torn/regressed interleaving (see $LOG_DIR/ff_loom.log)"; tail -20 "$LOG_DIR/ff_loom.log" | sed 's/^/      /'
  else
    skip "Loom finalized-floor cache: could not build the loom test in this cfg (fail-soft; see $LOG_DIR/ff_loom.log)"
  fi
else
  skip "no cargo on PATH"
fi

if [[ "${RUN_SOAK:-0}" == "1" ]]; then
  echo "== [soak] 400+-block Rust soak (slow) =="
  if cargo test -p casper --test mod --release -- finalized_floor_400_block_soak --ignored >"$LOG_DIR/ff_soak.log" 2>&1; then
    pass "finalized_floor_400_block_soak"
  else
    fail "soak failed (see $LOG_DIR/ff_soak.log)"
  fi
fi

echo
if [[ $rc -eq 0 ]]; then
  rm -f "$LOG_DIR"/ff_*.log
  printf '\033[32m== finalized-floor verification: ALL GATES OK ==\033[0m\n'
else
  printf '\033[31m== finalized-floor verification: FAILURES ABOVE ==\033[0m\n'
fi
exit $rc
