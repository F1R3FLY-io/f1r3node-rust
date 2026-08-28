#!/usr/bin/env bash
# scripts/check-finalized-floor-ALL.sh
#
# LOCAL-ONLY verification gate for the finalized-floor multi-parent merge.
# Runs every formal layer for the feature under a bounded memory envelope:
#
#   1. Rocq  (AUTHORITATIVE) — builds formal/rocq/finalized_floor and asserts the
#      every cataloged headline result, including exact occurrence status, exact-effect causal rejection closure,
#      heartbeat recovery/backpressure refinement, and its included proposal
#      reservation/outcome/pending-work scheduler contract,
#      plus the three GuardBridge lemmas that derive Floor.v's AdjDC premise from the
#      Rust committee-constancy guard (guard_constant_committee_transparent,
#      upgo_finalized, chain_adj_AdjDC) are axiom-free. Any failure here fails the gate.
#   2. TLA+/Apalache         — TLC on the POST-fix MC_FinalizedFloor.cfg + the
#      H3/T-PS MC_FinalizedFloorScan.cfg (both must pass) and the two PRE-fix cfgs
#      (both must reproduce their counterexample), plus the abstract admission model
#      and unsafe counterexample, plus exact merge-effect provenance, state-preserving
#      causal-parent floor rebasing after LFB advancement, pending-deploy/recovery
#      composition, proposal-admission coalescing, and their negative controls.
#      Apalache symbolically checks the state-preservation, arrival-order, and
#      proposal-scheduling invariants and is mandatory; TLC is skipped only when
#      its jar is unavailable.
#   3. Z3    (fail-soft)     — ft_algebra + BitVec-64 IntegerAdd launder witnesses.
#   4. Sage  (fail-soft)     — FT-algebra identity + finalization-margin monotonicity.
#   5. Wolfram (optional, fail-soft) — service-rate stability, exact weighted-
#      quorum regions, and pre-benchmark repair-family optimization. SKIPPED
#      unless RUN_WOLFRAM=1; the default gate never starts a kernel or acquires
#      a license.
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
#   RUN_SOAK=1        also run the long-running 400+-block Rust soak
#   RUN_WOLFRAM=1     opt into the licensed Wolfram exploration tier
#   PENDING_HEARTBEAT_APALACHE_SAFE_LENGTH=6
#   PENDING_HEARTBEAT_APALACHE_UNSAFE_LENGTH=6
#   PENDING_HEARTBEAT_APALACHE_TYPEOK_LENGTH=2
#   RECOVERY_COMMITTEE_APALACHE_SAFE_LENGTH=6
#   RECOVERY_COMMITTEE_APALACHE_UNSAFE_LENGTH=4
#   OBJECTIVE_EQUIVOCATION_APALACHE_SAFE_LENGTH=8
#   OBJECTIVE_EQUIVOCATION_APALACHE_UNSAFE_LENGTH=8
#   OBJECTIVE_AUTHORIZATION_APALACHE_SAFE_LENGTH=10
#   OBJECTIVE_AUTHORIZATION_APALACHE_UNSAFE_LENGTH=8
#   BOND_GENERATION_APALACHE_SAFE_LENGTH=10
#   BOND_GENERATION_APALACHE_UNSAFE_LENGTH=10
#   BOND_GENERATION_APALACHE_TIMEOUT=1800
#   CAUSAL_FINALITY_APALACHE_SAFE_LENGTH=4
#   CAUSAL_FINALITY_APALACHE_UNSAFE_LENGTH=4
#   CERTIFIED_OBJECTIVE_APALACHE_SAFE_LENGTH=8
#   CERTIFIED_OBJECTIVE_APALACHE_UNSAFE_LENGTH=8
#   CERTIFIED_OBJECTIVE_APALACHE_NONCANONICAL_LENGTH=12
#   CERTIFIED_OBJECTIVE_APALACHE_NONCANONICAL_TIMEOUT=600
#   CERTIFIED_CAUSAL_ADMISSION_APALACHE_SAFE_LENGTH=5
#   CERTIFIED_CAUSAL_ADMISSION_APALACHE_UNSAFE_LENGTH=5
#   ADMISSION_DISPOSITION_APALACHE_SAFE_LENGTH=6
#   ADMISSION_DISPOSITION_APALACHE_UNSAFE_LENGTH=2
#   CERTIFIED_CONTEXT_APALACHE_SAFE_LENGTH=10
#   CERTIFIED_CONTEXT_APALACHE_UNSAFE_LENGTH=1
#   CERTIFIED_CONTEXT_APALACHE_STALE_LENGTH=10
#   CERTIFIED_FLOOR_APALACHE_SAFE_LENGTH=8
#   CERTIFIED_FLOOR_APALACHE_UNSAFE_LENGTH=6
#   CERTIFIED_FLOOR_APALACHE_CONTEXT_UNSAFE_LENGTH=8
#   CERTIFICATE_RETRIEVAL_APALACHE_SAFE_LENGTH=12
#   CERTIFICATE_RETRIEVAL_APALACHE_UNSAFE_LENGTH=6
#   DEPENDENCY_MAINTENANCE_APALACHE_SAFE_LENGTH=8
#   DEPENDENCY_MAINTENANCE_APALACHE_UNSAFE_LENGTH=3
#   CERTIFIED_SNAPSHOT_APALACHE_SAFE_LENGTH=6
#   CERTIFIED_SNAPSHOT_APALACHE_UNSAFE_LENGTH=4
#   WITNESS_CARRIER_APALACHE_SAFE_LENGTH=5
#   WITNESS_CARRIER_APALACHE_UNSAFE_LENGTH=3
#   PROTOCOL_V5_APALACHE_SAFE_LENGTH=5
#   PROPOSER_COALESCING_APALACHE_SAFE_LENGTH=6
#   PROPOSER_COALESCING_APALACHE_UNSAFE_LENGTH=6
#   LIVE_RECOVERY_APALACHE_SAFE_LENGTH=6
#   LIVE_RECOVERY_APALACHE_UNSAFE_LENGTH=5
#   FINALIZER_MATERIALIZATION_APALACHE_SAFE_LENGTH=8
#   FINALIZER_MATERIALIZATION_APALACHE_UNSAFE_LENGTH=6
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
if command -v flock >/dev/null 2>&1; then
  exec 9>"$REPO_ROOT/target/verification/finalized-floor-heavy.lock"
  flock 9 || exit 1
fi
VERIFY_TMP="$LOG_DIR/tmp"
mkdir -p "$VERIFY_TMP"
export TMPDIR="$VERIFY_TMP"
TLC_METADIR_ROOT="$VERIFY_TMP/tlc-metadir"
export TLC_METADIR_ROOT
mkdir -p "$TLC_METADIR_ROOT"
export TLC_WORKERS=1
export CARGO_BUILD_JOBS=1
export RUST_TEST_THREADS=1
trap 'rm -rf "$VERIFY_TMP"' EXIT

rc=0
pass() { printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; rc=1; }
skip() { printf '  \033[33mSKIP\033[0m %s\n' "$1"; }
all_markers_present() {
  local log="$1" marker
  shift
  for marker in "$@"; do
    if ! grep -Fq -- "$marker" "$log"; then
      printf 'missing required marker: %s\n' "$marker" >>"$log"
      return 1
    fi
  done
}

if "$REPO_ROOT/scripts/check-tlc-source-binding.sh" >"$LOG_DIR/ff_tlc_source_binding.log" 2>&1; then
  pass "TLC checkpoint recovery is bound to source, checker, fingerprint, seed, and worker identity"
else
  fail "TLC source-binding regression failed (see $LOG_DIR/ff_tlc_source_binding.log)"
fi

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
    pass "Rocq build (Foundation, CliqueOracle, AccountableSafety, Floor, GuardBridge, Merge, OccurrenceDisposition, FinalizedOccurrenceStatus, Recovery, MergeRecoveryCoherence, AdmissionEffectAlignment, RejectionReasonConfluence, ProtocolVersionLifecycle, ProtocolActivationCoherence, Selection, IntegerAdd, FtExact, FinalityThresholdAlignment, StateEffectProvenance, CertifiedFloorPromotion, CommitteeTransition, ObjectiveEquivocation, ProposalFloorReadiness, FinalizerFloorMaterialization, MainTheorem)"
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
From FinalizedFloor Require Import BondGenerationLifecycle.
From FinalizedFloor Require Import CausalFinalityProjection.
From FinalizedFloor Require Import CertifiedObjectiveEquivocation.
From FinalizedFloor Require Import CertifiedCausalAdmission.
From FinalizedFloor Require Import ProposalFloorReadiness.
From FinalizedFloor Require Import FinalizerFloorMaterialization.
From FinalizedFloor Require Import GenesisApprovalTrust.
From FinalizedFloor Require Import FinalizationCertificateRetrieval.
From FinalizedFloor Require Import WitnessEquivalentCarrier.
From FinalizedFloor Require Import DependencyMaintenanceRound.
Print Assumptions finalized_floor_merge_correct.
Print Assumptions finalized_floor_candidate_scope_rehome_correct.
Print Assumptions finalized_floor_objective_evidence_sequence_boundary_correct.
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
Print Assumptions committee_transition_correct.
Print Assumptions objective_equivocation_correct.
Print Assumptions finalized_floor_arithmetic_correct.
Print Assumptions finalized_floor_phase7_correct.
Print Assumptions finalized_floor_ftexact_correct.
Print Assumptions finalized_floor_threshold_alignment_correct.
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
Print Assumptions finalized_floor_node_local_product_lifting_correct.
Print Assumptions finalized_floor_node_local_temporal_lifting_correct.
Print Assumptions finalized_floor_atomic_commit_correct.
Print Assumptions finalized_floor_worker_retry_correct.
Print Assumptions finalized_floor_proposal_readiness_correct.
Print Assumptions finalized_floor_recovery_cursors_correct.
Print Assumptions finalized_floor_genesis_approval_trust_correct.
Print Assumptions lifecycle_generation_monotone.
Print Assumptions exhausted_generation_rejects_fresh_bond.
Print Assumptions lifecycle_step_preserves_value.
Print Assumptions lifecycle_well_formed_preserved.
Print Assumptions vindication_restores_exact_pre_slash_phase.
Print Assumptions partial_penalty_restores_exact_pre_slash_phase.
Print Assumptions guilty_resolution_is_strictly_partial.
Print Assumptions causally_equivocating_incarnation_cannot_vote.
Print Assumptions certified_projection_binding_and_evidence_roots_correct.
Print Assumptions candidate_delta_does_not_affect_own_floor.
Print Assumptions equivalent_receivers_derive_identical_consensus.
Print Assumptions sender_certificate_generation_is_parent_derived.
Print Assumptions mismatched_header_generation_cannot_be_certified.
Print Assumptions repaired_evidence_is_derived_from_sound_metadata.
Print Assumptions certified_causal_admission_correct.
Print Assumptions certified_outcome_rejects_any_identity_tamper.
Print Assumptions authenticated_objective_invalidity_is_certified.
Print Assumptions declared_hash_mismatch_cannot_frame_signer.
Print Assumptions local_validation_fault_has_no_consensus_effect.
Print Assumptions typed_admission_classification_total.
Print Assumptions typed_admission_evidence_requires_certified_objective_invalidity.
Print Assumptions finalized_floor_certified_causal_admission_correct.
Print Assumptions finalized_floor_live_minor_fork_recovery_correct.
Print Assumptions validated_materialization_is_exact_and_dual_certified.
Print Assumptions finalizer_discovery_matches_pairwise_certificate.
Print Assumptions highest_exact_candidate_is_unique.
Print Assumptions finalized_floor_materialization_target_alignment_correct.
Print Assumptions finalized_floor_target_deploy_wait_correct.
Print Assumptions finalized_floor_stale_sibling_recovery_correct.
Print Assumptions finalized_floor_certificate_retrieval_correct.
Print Assumptions finalized_floor_dependency_maintenance_correct.
Print Assumptions finalized_floor_witness_equivalent_carrier_correct.
EOF
    expected_closed=$(grep -c '^Print Assumptions ' "$chk")
    out=$(coqc -Q "$ROCQ_DIR/theories" FinalizedFloor "$chk" 2>&1)
    rm -rf "$tmpd"
    n_closed=$(grep -c "Closed under the global context" <<<"$out")
    if [[ "$n_closed" == "$expected_closed" ]]; then
      pass "all $expected_closed headline results axiom-free, including exact target-bound dual certification, all-parent finalizer discovery equivalence, unique highest exact selection, materialization-target alignment, strict candidate/finalizer threshold alignment, candidate-scope deploy rehome, signed-sequence evidence eligibility, typed proposal readiness, outcome-aware finalization retry, typed admission disposition, framing resistance, local-fault isolation, atomic finalization, crash-recovery cursors, local-ledger identity separation, live minority-fork recovery, certified causal admission, exact projection identity binding, durable floor/latest evidence roots, certified validator incarnations, bounded monotonic bond generations, exact quarantine restoration, causal finality projection, objective equivocation, accountable parallel promotion, admission/effect alignment, exact-effect causal closure, merge-effect provenance, committee transition, snapshot materialization, and heartbeat backpressure"
    else
      fail "headline results NOT all axiom-free ($n_closed/$expected_closed Closed):"; printf '      %s\n' "${out//$'\n'/$'\n      '}"
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
if "$REPO_ROOT/scripts/check-finalization-atomicity.sh" >"$LOG_DIR/ff_finalization_atomicity.log" 2>&1; then
  pass "atomic parallel finalization, crash recovery, contiguous projection/effect cursors, and all negative controls"
else
  fail "finalization atomicity/recovery verification failed (see $LOG_DIR/ff_finalization_atomicity.log)"
fi
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
  for recovery_model in \
      'divergent_histories:DivergentFinalizationHistories:MC_DivergentFinalizationHistories.cfg' \
      'live_minority:LiveMinorityForkRecovery:MC_LiveMinorityForkRecovery.cfg'; do
    IFS=: read -r recovery_name recovery_module recovery_cfg <<<"$recovery_model"
    recovery_log="$LOG_DIR/ff_tlc_${recovery_name}_recovery.log"
    if tlc_run "$(tlc_metadir "ff_${recovery_name}_recovery")" "$TLA_DIR/$recovery_cfg" "$TLA_DIR/$recovery_module.tla" >"$recovery_log" 2>&1; then
      pass "TLA+ ${recovery_name} recovery preserves local finality authority, dependency closure, and validator-local concurrency"
    else
      fail "TLA+ ${recovery_name} recovery model failed (see $recovery_log)"
    fi
  done
  for recovery_control in \
      'divergent_remote_ledger:DivergentFinalizationHistories:MC_DivergentFinalizationHistories_remote_ledger_unsafe.cfg' \
      'live_remote_head:LiveMinorityForkRecovery:MC_LiveMinorityForkRecovery_remote_head_unsafe.cfg' \
      'live_dependencies:LiveMinorityForkRecovery:MC_LiveMinorityForkRecovery_dependencies_unsafe.cfg' \
      'live_global_pause:LiveMinorityForkRecovery:MC_LiveMinorityForkRecovery_global_pause_unsafe.cfg'; do
    IFS=: read -r control_name control_module control_cfg <<<"$recovery_control"
    control_log="$LOG_DIR/ff_tlc_${control_name}.log"
    if tlc_run "$(tlc_metadir "ff_${control_name}")" "$TLA_DIR/$control_cfg" "$TLA_DIR/$control_module.tla" >"$control_log" 2>&1; then
      fail "TLA+ ${control_name} negative control should violate Safety but passed"
    elif grep -Fq 'Invariant Safety is violated' "$control_log"; then
      pass "TLA+ ${control_name} negative control reproduces unsafe remote authority or recovery behavior"
    else
      fail "TLA+ ${control_name} negative control failed for the wrong reason (see $control_log)"
    fi
  done
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
    pass "TLA+ local-round heartbeat recovery builds explicit mutually visible block support while preserving bounded admission, exact dual certificates, and offline-leader liveness"
  else
    fail "TLA+ heartbeat/finality backpressure model failed (see $LOG_DIR/ff_tlc_heartbeat_backpressure.log)"
  fi
  if tlc_run "$(tlc_metadir ff_heartbeat_async)" "$TLA_DIR/MC_HeartbeatFinalityBackpressure_async.cfg" "$TLA_DIR/HeartbeatFinalityBackpressure.tla" >"$LOG_DIR/ff_tlc_heartbeat_async.log" 2>&1; then
    pass "TLA+ independently advancing local recovery rounds preserve heartbeat safety without assuming bounded delivery"
  else
    fail "TLA+ asynchronous heartbeat safety model failed (see $LOG_DIR/ff_tlc_heartbeat_async.log)"
  fi
  if tlc_run "$(tlc_metadir ff_heartbeat_existing_candidate)" "$TLA_DIR/MC_HeartbeatFinalityBackpressure_existing_candidate.cfg" "$TLA_DIR/HeartbeatFinalityBackpressure.tla" >"$LOG_DIR/ff_tlc_heartbeat_existing_candidate.log" 2>&1; then
    pass "TLA+ existing-candidate recovery preserves safety and reaches promotion past the offline first leader"
  else
    fail "TLA+ existing-candidate heartbeat model failed (see $LOG_DIR/ff_tlc_heartbeat_existing_candidate.log)"
  fi
  if tlc_run "$(tlc_metadir ff_heartbeat_asymmetric)" "$TLA_DIR/MC_HeartbeatFinalityBackpressure_asymmetric.cfg" "$TLA_DIR/HeartbeatFinalityBackpressure.tla" >"$LOG_DIR/ff_tlc_heartbeat_asymmetric.log" 2>&1; then
    pass "TLA+ asymmetric 1/4/5 stake requires the online 4+5 dual clique and preserves recovery liveness"
  else
    fail "TLA+ asymmetric-stake heartbeat model failed (see $LOG_DIR/ff_tlc_heartbeat_asymmetric.log)"
  fi
  if tlc_run "$(tlc_metadir ff_heartbeat_promotion_witness_unsafe)" "$TLA_DIR/MC_HeartbeatFinalityBackpressure_promotion_witness_unsafe.cfg" "$TLA_DIR/HeartbeatFinalityBackpressure.tla" >"$LOG_DIR/ff_tlc_heartbeat_promotion_witness_unsafe.log" 2>&1; then
    fail "TLA+ promotion-witness control should violate Inv_NoPromotion but passed"
  elif grep -q "Invariant Inv_NoPromotion is violated" "$LOG_DIR/ff_tlc_heartbeat_promotion_witness_unsafe.log"; then
    pass "TLA+ promotion-witness control reaches exact mutual causal/state promotion"
  else
    fail "TLA+ promotion-witness control failed for the wrong reason (see $LOG_DIR/ff_tlc_heartbeat_promotion_witness_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_heartbeat_cadence)" "$TLA_DIR/MC_HeartbeatRecoveryCadence.cfg" "$TLA_DIR/HeartbeatRecoveryCadence.tla" >"$LOG_DIR/ff_tlc_heartbeat_cadence.log" 2>&1; then
    pass "TLA+ parallel local clocks preserve the one-time stall timeout and post-stall check-interval cadence"
  else
    fail "TLA+ heartbeat recovery-cadence model failed (see $LOG_DIR/ff_tlc_heartbeat_cadence.log)"
  fi
  if tlc_run "$(tlc_metadir ff_target_deploy_terminality)" "$TLA_DIR/MC_TargetDeployTerminality.cfg" "$TLA_DIR/TargetDeployTerminality.tla" >"$LOG_DIR/ff_tlc_target_deploy_terminality.log" 2>&1; then
    pass "TLA+ parallel target-deploy observer preserves exact success, strict-height progress renewal, stall detection, and the absolute bound"
  else
    fail "TLA+ target-deploy terminality model failed (see $LOG_DIR/ff_tlc_target_deploy_terminality.log)"
  fi
  for target_control in \
      'fixed_timeout_unsafe:Inv_WithinProgressBudgetRemainsLive:a fixed deadline rejecting a live intermediate-floor trace' \
      'history_anomaly_unsafe:Inv_HistoryAnomalyDetected:a finalized-history revision being silently accepted' \
      'inexact_success_unsafe:Inv_SuccessRequiresExactFinalizedStatus:an intermediate LFB advance masquerading as target terminality' \
      'late_terminal_unsafe:Inv_TerminalOutcomeWithinBudget:a deadline-consuming terminal response bypassing the expired observation budget' \
      'baseline_renewal_unsafe:Inv_FirstObservationDoesNotRenew:a delayed first LFB sample falsely renewing the stall budget'; do
    IFS=: read -r target_suffix target_invariant target_description <<<"$target_control"
    target_log="$LOG_DIR/ff_tlc_target_deploy_${target_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_target_deploy_${target_suffix}")" "$TLA_DIR/MC_TargetDeployTerminality_${target_suffix}.cfg" "$TLA_DIR/TargetDeployTerminality.tla" >"$target_log" 2>&1; then
      fail "TLA+ target-deploy control should reproduce ${target_description} but passed"
    elif grep -Fq "Invariant ${target_invariant} is violated" "$target_log"; then
      pass "TLA+ target-deploy control reproduces ${target_description}"
    else
      fail "TLA+ target-deploy control failed for the wrong reason (see $target_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_heartbeat_collapsed_cadence_unsafe)" "$TLA_DIR/MC_HeartbeatRecoveryCadence_collapsed_unsafe.cfg" "$TLA_DIR/HeartbeatRecoveryCadence.tla" >"$LOG_DIR/ff_tlc_heartbeat_collapsed_cadence_unsafe.log" 2>&1; then
    fail "TLA+ collapsed-timeout cadence control should delay later recovery rounds but passed"
  elif grep -q "Inv_CadenceMatchesContract is violated" "$LOG_DIR/ff_tlc_heartbeat_collapsed_cadence_unsafe.log"; then
    pass "TLA+ collapsed-timeout control reproduces the delayed recovery-round defect"
  else
    fail "TLA+ collapsed-timeout cadence control failed for the wrong reason (see $LOG_DIR/ff_tlc_heartbeat_collapsed_cadence_unsafe.log)"
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
  elif grep -q "Temporal properties were violated" "$LOG_DIR/ff_tlc_heartbeat_offline_leader_unsafe.log" \
       && grep -q '^PROPERTY Live_RecoveryRotatesPastOfflineLeader$' "$TLA_DIR/MC_HeartbeatFinalityBackpressure_offline_leader_unsafe.cfg" \
       && [[ "$(grep -c '^PROPERTY ' "$TLA_DIR/MC_HeartbeatFinalityBackpressure_offline_leader_unsafe.cfg")" -eq 1 ]] \
       && ! grep -q "Invariant .* is violated" "$LOG_DIR/ff_tlc_heartbeat_offline_leader_unsafe.log"; then
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
  if tlc_run "$(tlc_metadir ff_pending_heartbeat)" "$TLA_DIR/MC_PendingDeployHeartbeatComposition.cfg" "$TLA_DIR/PendingDeployHeartbeatComposition.tla" >"$LOG_DIR/ff_tlc_pending_heartbeat.log" 2>&1; then
    pass "TLA+ pending deploys compose with selected recovery while preserving exact terminal disposition, bounded admission, and finality liveness"
  else
    fail "TLA+ pending-deploy/heartbeat composition model failed (see $LOG_DIR/ff_tlc_pending_heartbeat.log)"
  fi
  if tlc_run "$(tlc_metadir ff_pending_heartbeat_ingress)" "$TLA_DIR/MC_PendingDeployHeartbeatComposition_ingress_safety.cfg" "$TLA_DIR/PendingDeployHeartbeatComposition.tla" >"$LOG_DIR/ff_tlc_pending_heartbeat_ingress.log" 2>&1; then
    pass "TLA+ concurrent ingress preserves queue, attempt, occurrence, and terminal-evidence bounds"
  else
    fail "TLA+ pending-deploy ingress-safety model failed (see $LOG_DIR/ff_tlc_pending_heartbeat_ingress.log)"
  fi
  for pending_control in \
      'attempt_closes_round_unsafe:Inv_RetryableOutcomeDoesNotCompleteRound:retryable proposal outcomes closing recovery rounds' \
      'clear_on_start_unsafe:Inv_PoolRemovalRequiresTerminalEvidence:pending work cleared before terminal evidence' \
      'no_recovery_reservation_unsafe:Inv_RecoveryReservationHonored:recovery attempted without an owned reservation' \
      'head_committee_unsafe:Inv_AtMostOneSelectedRecoveryPerRound:divergent parent committees authorizing multiple recovery validators for one floor round' \
      'disjoint_head_eligibility_unsafe:Inv_SelectedRecoveryEligible:a finalized-floor leader rejected by a disjoint parent committee' \
      'head_filtered_justification_unsafe:Inv_QueuedRecoveryHasValidationContext:a finalized-floor leader losing its creator justification and sequence number under parent filtering' \
      'unbounded_duplicate_admission_unsafe:Inv_DuplicateOccurrencesBounded:duplicate admission exceeding the occurrence bound'; do
    IFS=: read -r pending_suffix pending_invariant pending_description <<<"$pending_control"
    pending_log="$LOG_DIR/ff_tlc_pending_heartbeat_${pending_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_pending_heartbeat_${pending_suffix}")" "$TLA_DIR/MC_PendingDeployHeartbeatComposition_${pending_suffix}.cfg" "$TLA_DIR/PendingDeployHeartbeatComposition.tla" >"$pending_log" 2>&1; then
      fail "TLA+ pending-deploy control should reproduce ${pending_description} but passed"
    elif grep -Fq "Invariant ${pending_invariant} is violated" "$pending_log"; then
      pass "TLA+ pending-deploy control reproduces ${pending_description}"
    else
      fail "TLA+ pending-deploy control failed for the wrong reason (see $pending_log)"
    fi
  done
  for pending_temporal_control in \
      'pending_masks_recovery_unsafe:pending work masking selected recovery' \
      'fixed_offline_leader_unsafe:a fixed offline leader starving recovery'; do
    IFS=: read -r pending_suffix pending_description <<<"$pending_temporal_control"
    pending_log="$LOG_DIR/ff_tlc_pending_heartbeat_${pending_suffix}.log"
    pending_cfg="$TLA_DIR/MC_PendingDeployHeartbeatComposition_${pending_suffix}.cfg"
    if tlc_run "$(tlc_metadir "ff_pending_heartbeat_${pending_suffix}")" "$pending_cfg" "$TLA_DIR/PendingDeployHeartbeatComposition.tla" >"$pending_log" 2>&1; then
      fail "TLA+ pending-deploy control should reproduce ${pending_description} but passed"
    elif grep -Fq 'Temporal properties were violated' "$pending_log" \
         && [[ "$(grep -c '^PROPERTY ' "$pending_cfg")" -eq 1 ]] \
         && ! grep -q 'Invariant .* is violated' "$pending_log"; then
      pass "TLA+ pending-deploy control reproduces ${pending_description}"
    else
      fail "TLA+ pending-deploy temporal control failed for the wrong reason (see $pending_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_proposer_coalescing)" "$TLA_DIR/MC_ProposerAdmissionCoalescing.cfg" "$TLA_DIR/ProposerAdmissionCoalescing.tla" >"$LOG_DIR/ff_tlc_proposer_coalescing.log" 2>&1; then
    pass "TLA+ proposal admission coalesces each dirty epoch into one non-empty follow-up and rejects stale recovery permits"
  else
    fail "TLA+ proposer-admission coalescing model failed (see $LOG_DIR/ff_tlc_proposer_coalescing.log)"
  fi
  for proposer_control in \
      'ambient_async_empty_unsafe:Inv_EmptyAuthorityIsRecoveryOnly:ambient asynchronous authority producing an empty block' \
      'lost_pending_wake_unsafe:Inv_PendingWakeLatched:a pending-deploy wake lost during active proposal work' \
      'stale_recovery_permit_unsafe:Inv_StaleRecoveryPermitRejected:a stale recovery permit surviving LFB advancement'; do
    IFS=: read -r proposer_suffix proposer_invariant proposer_description <<<"$proposer_control"
    proposer_log="$LOG_DIR/ff_tlc_proposer_coalescing_${proposer_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_proposer_coalescing_${proposer_suffix}")" "$TLA_DIR/MC_ProposerAdmissionCoalescing_${proposer_suffix}.cfg" "$TLA_DIR/ProposerAdmissionCoalescing.tla" >"$proposer_log" 2>&1; then
      fail "TLA+ proposer-admission control should reproduce ${proposer_description} but passed"
    elif grep -Fq "Invariant ${proposer_invariant} is violated" "$proposer_log"; then
      pass "TLA+ proposer-admission control reproduces ${proposer_description}"
    else
      fail "TLA+ proposer-admission control failed for the wrong reason (see $proposer_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_recovery_committee_transition)" "$TLA_DIR/MC_RecoveryCommitteeTransition.cfg" "$TLA_DIR/RecoveryCommitteeTransition.tla" >"$LOG_DIR/ff_tlc_recovery_committee_transition.log" 2>&1; then
    pass "TLA+ canonical root/key/LMM admission and accepted positive post-state registration preserve finalized-floor recovery authority"
  else
    fail "TLA+ recovery committee-transition model failed (see $LOG_DIR/ff_tlc_recovery_committee_transition.log)"
  fi
  for transition_control in \
      'post_auth_unsafe:Inv_ProspectiveAuthorizationDeferred:same-block post-state bonds authorizing their own block' \
      'head_justifications_unsafe:Inv_QueuedRecoveryHasExactContext:head-filtered justifications removing a recovery creator sequence' \
      'premature_promotion_unsafe:Inv_FloorValidatorsRegistered:committee promotion preceding validator registration' \
      'head_weights_unsafe:Inv_SynchronyAdmissionMatchesFloor:head weights changing finalized-floor synchrony admission' \
      'mismatched_cache_unsafe:Inv_SerializedBondsArePostStateCache:a serialized bond cache disagreeing with replayed post-state bonds' \
      'filtered_sequence_unsafe:Inv_PackagedSequenceUsesUnfilteredLmm:valid-only sequence metadata disagreeing with the exact unfiltered creator justification' \
      'invalid_registration_unsafe:Inv_InvalidPostStateDoesNotRegister:an invalid block registering arbitrary validator bonds' \
      'root_admission_unsafe:Inv_ApprovedGenesisIsSoleRoot:an ordinary or counterfeit parentless block entering the root set' \
      'sender_key_unsafe:Inv_JustificationKeysMatchCitedSenders:a non-genesis justification key disagreeing with the cited sender' \
      'registration_genesis_unsafe:Inv_RegisteredSlotsUseCanonicalGenesis:local invalid height-zero junk selecting a validator slot genesis' \
      'unregistered_lmm_unsafe:Inv_InvalidUnregisteredSendersHaveNoLmmSlot:an invalid unregistered sender creating an LMM slot' \
      'nonpositive_slot_unsafe:Inv_OnlyPositivePostStateBondsCreateSlots:a non-positive post-state bond creating a validator slot' \
      'invalid_finality_lmm_unsafe:Inv_InvalidLmmDoesNotContributeToFinality:an invalid LMM slot contributing to a finality certificate' \
      'legacy_backfill_unsafe:Inv_DuplicateApprovedBackfillsLegacyIndex:a duplicate approved genesis failing to backfill a legacy empty canonical index'; do
    IFS=: read -r transition_suffix transition_invariant transition_description <<<"$transition_control"
    transition_log="$LOG_DIR/ff_tlc_recovery_committee_transition_${transition_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_recovery_committee_transition_${transition_suffix}")" "$TLA_DIR/MC_RecoveryCommitteeTransition_${transition_suffix}.cfg" "$TLA_DIR/RecoveryCommitteeTransition.tla" >"$transition_log" 2>&1; then
      fail "TLA+ recovery committee-transition control should reproduce ${transition_description} but passed"
    elif grep -Fq "Invariant ${transition_invariant} is violated" "$transition_log"; then
      pass "TLA+ recovery committee-transition control reproduces ${transition_description}"
    else
      fail "TLA+ recovery committee-transition control failed for the wrong reason (see $transition_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_objective_equivocation)" "$TLA_DIR/MC_ObjectiveEquivocation.cfg" "$TLA_DIR/ObjectiveEquivocation.tla" >"$LOG_DIR/ff_tlc_objective_equivocation.log" 2>&1; then
    pass "TLA+ opposite-order replicas converge on canonical objective-equivocation evidence, dependencies, and voting exclusion"
  else
    fail "TLA+ objective-equivocation model failed (see $LOG_DIR/ff_tlc_objective_equivocation.log)"
  fi
  for objective_control in \
      'unary_evidence_unsafe:Inv_GroupByIncarnationBeforeCanonicalization:unary arrival-order evidence replacing the incarnation-grouped canonical hash pair' \
      'local_invalid_unsafe:Inv_GroupByIncarnationBeforeCanonicalization:local invalid flags changing objective evidence acceptance' \
      'unary_dependency_unsafe:Inv_BothHashesAreDependencies:dependency closure retaining only one sibling hash' \
      'equivocator_votes_unsafe:Inv_ActiveIncarnationEquivocatorCannotVote:an active-incarnation objective equivocator remaining in finality voters' \
      'unary_fallback_unsafe:Inv_CrossIncarnationPairIsConsistentlyNonSlashable:a cross-incarnation pair falling back to arrival-local unary slash evidence' \
      'volatile_restart_unsafe:Inv_RestartPreservesObjectiveEvidence:restart losing durable objective evidence' \
      'permanent_raw_key_unsafe:Inv_IncarnationTransitionRestoresRawKey:raw-key exclusion persisting across bond incarnations' \
      'first_two_before_incarnation_unsafe:Inv_GroupByIncarnationBeforeCanonicalization:canonicalizing the first two hashes before bond-incarnation grouping' \
      'overbroad_unary_suppression_unsafe:Inv_IndependentUnaryFaultAtOtherSequenceRemainsEligible:objective evidence suppressing an independent unary fault at another sequence' \
      'block_epoch_incarnation_unsafe:Inv_AdversarialBlockEpochDoesNotDefineBondIncarnation:attacker-authored block epochs substituting for immutable bond incarnation' \
      'first_observed_unary_unsafe:Inv_UnaryEvidenceUsesDeterministicMinimum:first-observed unary evidence replacing the deterministic minimum' \
      'post_state_authority_unsafe:Inv_SameBlockUnbondUsesCanonicalPreStateAuthority:post-state bonds and local flags diverging same-block-unbond verdicts' \
      'duplicate_retry_no_repair_unsafe:Inv_DuplicateRetryRepairsEvidenceIndex:duplicate retry leaving the durable evidence index missing' \
      'unfiltered_finality_votes_unsafe:Inv_ExactJustificationsUseFilteredFinalityVotes:invalid exact-justification LMMs contributing to finality votes'; do
    IFS=: read -r objective_suffix objective_invariant objective_description <<<"$objective_control"
    objective_log="$LOG_DIR/ff_tlc_objective_equivocation_${objective_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_objective_equivocation_${objective_suffix}")" "$TLA_DIR/MC_ObjectiveEquivocation_${objective_suffix}.cfg" "$TLA_DIR/ObjectiveEquivocation.tla" >"$objective_log" 2>&1; then
      fail "TLA+ objective-equivocation control should reproduce ${objective_description} but passed"
    elif grep -Fq "Invariant ${objective_invariant} is violated" "$objective_log"; then
      pass "TLA+ objective-equivocation control reproduces ${objective_description}"
    else
      fail "TLA+ objective-equivocation control failed for the wrong reason (see $objective_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_objective_authorization)" "$TLA_DIR/MC_ObjectiveEvidenceAuthorization.cfg" "$TLA_DIR/ObjectiveEvidenceAuthorization.tla" >"$LOG_DIR/ff_tlc_objective_authorization.log" 2>&1; then
    pass "TLA+ objective evidence authorization groups by generation and epoch before canonicalization under one pre-state authority"
  else
    fail "TLA+ objective evidence authorization model failed (see $LOG_DIR/ff_tlc_objective_authorization.log)"
  fi
  for objective_authorization_control in \
      'epoch_after_min_unsafe:Inv_EpochGroupingPrecedesCanonicalization:canonicalization before activation-epoch grouping' \
      'cross_epoch_unsafe:Inv_CrossEpochPairCannotAuthorize:cross-epoch objective authorization' \
      'snapshot_generation_unsafe:Inv_CanonicalAuthorityRoot:stale snapshot bond-generation authority' \
      'snapshot_bond_unsafe:Inv_CanonicalAuthorityRoot:stale snapshot bond authority' \
      'offender_wide_suppression_unsafe:Inv_IndependentUnaryPreserved:offender-wide suppression of an independent unary fault' \
      'pair_only_disabled_unsafe:Inv_EpochGroupingPrecedesCanonicalization:failure to activate on pair-only objective evidence' \
      'predicate_drift_unsafe:Inv_ProposerReceiverParity:proposer and receiver authorization drift'; do
    IFS=: read -r authorization_suffix authorization_invariant authorization_description <<<"$objective_authorization_control"
    authorization_log="$LOG_DIR/ff_tlc_objective_authorization_${authorization_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_objective_authorization_${authorization_suffix}")" "$TLA_DIR/MC_ObjectiveEvidenceAuthorization_${authorization_suffix}.cfg" "$TLA_DIR/ObjectiveEvidenceAuthorization.tla" >"$authorization_log" 2>&1; then
      fail "TLA+ objective-authorization control should reproduce ${authorization_description} but passed"
    elif grep -Fq "Invariant ${authorization_invariant} is violated" "$authorization_log"; then
      pass "TLA+ objective-authorization control reproduces ${authorization_description}"
    else
      fail "TLA+ objective-authorization control failed for the wrong reason (see $authorization_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_bond_generation_lifecycle)" "$TLA_DIR/MC_BondGenerationLifecycle.cfg" "$TLA_DIR/BondGenerationLifecycle.tla" >"$LOG_DIR/ff_tlc_bond_generation_lifecycle.log" 2>&1; then
    pass "TLA+ two-validator bond generations advance only on completed fresh bonds and preserve exact slashing/withdrawal lifecycles"
  else
    fail "TLA+ bond-generation lifecycle model failed (see $LOG_DIR/ff_tlc_bond_generation_lifecycle.log)"
  fi
  for generation_control in \
      'generation_transition_unsafe:Inv_GenerationEqualsSuccessfulBondCount:generation changes outside a completed fresh bond' \
      'rebond_live_unsafe:Inv_AtMostOneLiveGenerationPerKey:rebonding while a live incarnation remains' \
      'current_bond_only_slash_unsafe:Inv_CurrentLockedSlashApplies:failure to slash pending or withdrawing stake' \
      'stale_slash_unsafe:Inv_StaleSlashIsNoninterfering:stale-generation evidence mutating the current incarnation' \
      'burn_rebond_unsafe:Inv_BurnedGenerationCannotRebond:rebonding a terminally burned validator key' \
      'restore_bonded_unsafe:Inv_RedemptionRestoresExactPreSlashPhase:collapsing every redemption origin to Bonded' \
      'full_guilty_unsafe:Inv_GuiltyPenaltyIsStrictlyPartial:using Guilty for total confiscation' \
      'wrap_generation_unsafe:Inv_GenerationEqualsSuccessfulBondCount:wrapping the bounded generation counter'; do
    IFS=: read -r generation_suffix generation_invariant generation_description <<<"$generation_control"
    generation_log="$LOG_DIR/ff_tlc_bond_generation_${generation_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_bond_generation_${generation_suffix}")" "$TLA_DIR/MC_BondGenerationLifecycle_${generation_suffix}.cfg" "$TLA_DIR/BondGenerationLifecycle.tla" >"$generation_log" 2>&1; then
      fail "TLA+ bond-generation control should reproduce ${generation_description} but passed"
    elif grep -Fq "Invariant ${generation_invariant} is violated" "$generation_log"; then
      pass "TLA+ bond-generation control reproduces ${generation_description}"
    else
      fail "TLA+ bond-generation control failed for the wrong reason (see $generation_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_causal_finality_projection)" "$TLA_DIR/MC_CausalFinalityProjection.cfg" "$TLA_DIR/CausalFinalityProjection.tla" >"$LOG_DIR/ff_tlc_causal_finality_projection.log" 2>&1; then
    pass "TLA+ parent-derived causal evidence freezes finality votes without mutating exact wire justifications"
  else
    fail "TLA+ causal-finality projection model failed (see $LOG_DIR/ff_tlc_causal_finality_projection.log)"
  fi
  for projection_control in \
      'ambient_unsafe:Inv_AmbientEvidenceCannotChangeCertifiedResult:ambient-store evidence changing a certified result' \
      'delta_cycle_unsafe:Inv_CandidateDeltaCannotAffectOwnProjection:a candidate using its own evidence delta' \
      'invalid_propagates_unsafe:Inv_InvalidBlocksDoNotPropagateEvidence:an invalid block propagating causal evidence' \
      'missing_dependency_unsafe:Inv_DeltaCarriesBothCertifiedDependencies:an objective pair omitting one dependency' \
      'mutate_exact_unsafe:Inv_ExactJustificationsPreserved:filtered votes replacing exact signed justifications' \
      'unfiltered_votes_unsafe:Inv_InvalidAndCausallyEquivocatingVotesExcluded:invalid or equivocating votes entering finality'; do
    IFS=: read -r projection_suffix projection_invariant projection_description <<<"$projection_control"
    projection_log="$LOG_DIR/ff_tlc_causal_projection_${projection_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_causal_projection_${projection_suffix}")" "$TLA_DIR/MC_CausalFinalityProjection_${projection_suffix}.cfg" "$TLA_DIR/CausalFinalityProjection.tla" >"$projection_log" 2>&1; then
      fail "TLA+ causal-projection control should reproduce ${projection_description} but passed"
    elif grep -Fq "Invariant ${projection_invariant} is violated" "$projection_log"; then
      pass "TLA+ causal-projection control reproduces ${projection_description}"
    else
      fail "TLA+ causal-projection control failed for the wrong reason (see $projection_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_certified_objective_equivocation)" "$TLA_DIR/MC_CertifiedObjectiveEquivocation.cfg" "$TLA_DIR/CertifiedObjectiveEquivocation.tla" >"$LOG_DIR/ff_tlc_certified_objective_equivocation.log" 2>&1; then
    pass "TLA+ exact-parent authority certificates and durable reconciliation converge on certified objective evidence"
  else
    fail "TLA+ certified-objective-equivocation model failed (see $LOG_DIR/ff_tlc_certified_objective_equivocation.log)"
  fi
  if tlc_run "$(tlc_metadir ff_certified_objective_sequence_boundary)" "$TLA_DIR/MC_CertifiedObjectiveEquivocation_sequence_boundary.cfg" "$TLA_DIR/CertifiedObjectiveEquivocation.tla" >"$LOG_DIR/ff_tlc_certified_objective_sequence_boundary.log" 2>&1; then
    pass "TLA+ attributable negative-sequence rejections persist without entering objective evidence"
  else
    fail "TLA+ certified-objective signed-sequence boundary failed (see $LOG_DIR/ff_tlc_certified_objective_sequence_boundary.log)"
  fi
  for certified_control in \
      'header_trusted_unsafe:Inv_MetadataCertificatesUseExactParentAuthority:trusting an unverified header generation' \
      'post_state_unsafe:Inv_MetadataCertificatesUseExactParentAuthority:using same-block post-state authority' \
      'duplicate_no_repair_unsafe:Inv_DuplicateRetryRepairsEvidence:failing to repair evidence on duplicate insertion' \
      'negative_sequence_unsafe:Inv_IneligibleSequenceNeverBecomesEvidence:indexing an ineligible negative sequence as objective evidence' \
      'local_invalid_gate_unsafe:Inv_ReconciledSiblingEvidenceIsComplete:gating objective evidence on a local invalid flag' \
      'noncanonical_lmm_unsafe:Inv_EquivalentDurableViewsConverge:arrival-dependent latest-message selection'; do
    IFS=: read -r certified_suffix certified_invariant certified_description <<<"$certified_control"
    certified_log="$LOG_DIR/ff_tlc_certified_objective_${certified_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_certified_objective_${certified_suffix}")" "$TLA_DIR/MC_CertifiedObjectiveEquivocation_${certified_suffix}.cfg" "$TLA_DIR/CertifiedObjectiveEquivocation.tla" >"$certified_log" 2>&1; then
      fail "TLA+ certified-objective control should reproduce ${certified_description} but passed"
    elif grep -Fq "Invariant ${certified_invariant} is violated" "$certified_log"; then
      pass "TLA+ certified-objective control reproduces ${certified_description}"
    else
      fail "TLA+ certified-objective control failed for the wrong reason (see $certified_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_certified_causal_admission)" "$TLA_DIR/MC_CertifiedCausalAdmission.cfg" "$TLA_DIR/CertifiedCausalAdmission.tla" >"$LOG_DIR/ff_tlc_certified_causal_admission.log" 2>&1; then
    pass "TLA+ opposite-order replicas certify identical causal admission contexts with rejected-wrapper traversal, accepted-only propagation, proof-leaf isolation, and one proof per validator incarnation"
  else
    fail "TLA+ certified causal-admission model failed (see $LOG_DIR/ff_tlc_certified_causal_admission.log)"
  fi
  for causal_admission_control in \
      'rejected_barrier_unsafe:Inv_RejectedWrapperTraversed:stopping causal traversal at a rejected wrapper' \
      'rejected_delta_unsafe:Inv_RejectedDeltaIgnored:propagating a rejected block evidence delta' \
      'proof_context_unsafe:Inv_ProofRootsAreLeafFacts:recursively importing context through proof roots' \
      'per_sequence_unbounded_unsafe:Inv_CanonicalIncarnationBound:retaining one proof per sequence instead of one per validator incarnation' \
      'ambient_tracker_unsafe:Inv_CertifiedContextExact:receiver-local tracker state changing a certified admission context' \
      'partial_dependencies_unsafe:Inv_FullyKnownCandidatesAccepted:certifying before the complete causal evidence dependency closure is available'; do
    IFS=: read -r causal_admission_suffix causal_admission_invariant causal_admission_description <<<"$causal_admission_control"
    causal_admission_log="$LOG_DIR/ff_tlc_certified_causal_admission_${causal_admission_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_certified_causal_admission_${causal_admission_suffix}")" "$TLA_DIR/MC_CertifiedCausalAdmission_${causal_admission_suffix}.cfg" "$TLA_DIR/CertifiedCausalAdmission.tla" >"$causal_admission_log" 2>&1; then
      fail "TLA+ certified causal-admission control should reproduce ${causal_admission_description} but passed"
    elif grep -Eq "The invariant of ${causal_admission_invariant} is equal to FALSE|Invariant ${causal_admission_invariant} is violated" "$causal_admission_log"; then
      pass "TLA+ certified causal-admission control reproduces ${causal_admission_description}"
    else
      fail "TLA+ certified causal-admission control failed for the wrong reason (see $causal_admission_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_certified_admission_disposition)" "$TLA_DIR/MC_CertifiedAdmissionDisposition.cfg" "$TLA_DIR/CertifiedAdmissionDisposition.tla" >"$LOG_DIR/ff_tlc_certified_admission_disposition.log" 2>&1; then
    pass "TLA+ parallel validators preserve typed accepted, certified-objective, unattributable, and retryable-local-fault admission outcomes"
  else
    fail "TLA+ certified admission-disposition model failed (see $LOG_DIR/ff_tlc_certified_admission_disposition.log)"
  fi
  for disposition_control in \
      'summary_unsafe:Inv_AuthenticatedObjectiveCertified:evaluating signed objective invalidity before authority certification' \
      'hash_unsafe:Inv_HashMismatchUnattributable:attributing a relay-mutated body to the signer of its declared hash' \
      'local_fault_unsafe:Inv_LocalFaultHasNoDurableEffects:converting a node-local replay fault into slash evidence'; do
    IFS=: read -r disposition_suffix disposition_invariant disposition_description <<<"$disposition_control"
    disposition_log="$LOG_DIR/ff_tlc_certified_admission_disposition_${disposition_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_certified_admission_disposition_${disposition_suffix}")" "$TLA_DIR/MC_CertifiedAdmissionDisposition_${disposition_suffix}.cfg" "$TLA_DIR/CertifiedAdmissionDisposition.tla" >"$disposition_log" 2>&1; then
      fail "TLA+ admission-disposition control should reproduce ${disposition_description} but passed"
    elif grep -Eq "The invariant of ${disposition_invariant} is equal to FALSE|Invariant ${disposition_invariant} is violated" "$disposition_log"; then
      pass "TLA+ admission-disposition control reproduces ${disposition_description}"
    else
      fail "TLA+ admission-disposition control failed for the wrong reason (see $disposition_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_certified_consensus_context)" "$TLA_DIR/MC_CertifiedConsensusContext.cfg" "$TLA_DIR/CertifiedConsensusContext.tla" >"$LOG_DIR/ff_tlc_certified_consensus_context.log" 2>&1; then
    pass "TLA+ closure-equivalent replicas share one weighted, generation-aware consensus context while finalization workers evaluate optimistically in parallel"
  else
    fail "TLA+ certified-consensus-context model failed (see $LOG_DIR/ff_tlc_certified_consensus_context.log)"
  fi
  for context_control in \
      'local_lmm_unsafe:AdmissionClosureAgreement:receiver-local latest messages changing admission' \
      'local_tracker_unsafe:AdmissionClosureAgreement:receiver-local tracker contents changing admission' \
      'local_finalized_unsafe:AdmissionClosureAgreement:receiver-local finalized flags changing admission' \
      'parent_order_unsafe:AdmissionClosureAgreement:parent iteration order changing admission' \
      'candidate_prestate_unsafe:CandidatePrestateAuthorityNoninterference:an unfinalized candidate pre-state changing authority' \
      'snapshot_prefilter_unsafe:ConsensusContextExtensional:local prefiltering changing the certified context' \
      'estimator_refilter_unsafe:EstimatorConsumesOneProjection:the estimator filtering an already certified vote projection' \
      'head_weight_unsafe:EstimatorUsesFrozenAuthority:mutable head weights replacing frozen-floor authority' \
      'local_top_unsafe:EstimatorLcaContextExtensional:receiver-local DAG height changing the estimator LCA' \
      'outside_floor_vote_unsafe:EligibleVotesDescendFromFloor:a vote outside the certified floor entering fork choice' \
      'incomplete_slots_unsafe:CompleteLatestMessageSlots:fork choice proceeding without one slot per active validator' \
      'finalizer_reprojection_unsafe:FinalizerConsumesOneProjection:the finalizer independently reprojecting certified votes' \
      'generation_blind_lmm_unsafe:GenerationScopedVotes:an old or future validator incarnation occupying the wrong vote slot' \
      'stale_finalizer_unsafe:StaleFinalizerCannotCommit:a stale concurrent finalizer appending after its expected head'; do
    IFS=: read -r context_suffix context_invariant context_description <<<"$context_control"
    context_log="$LOG_DIR/ff_tlc_certified_context_${context_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_certified_context_${context_suffix}")" "$TLA_DIR/MC_CertifiedConsensusContext_${context_suffix}.cfg" "$TLA_DIR/CertifiedConsensusContext.tla" >"$context_log" 2>&1; then
      fail "TLA+ certified-context control should reproduce ${context_description} but passed"
    elif grep -Eq "The invariant of ${context_invariant} is equal to FALSE|Invariant ${context_invariant} is violated" "$context_log"; then
      pass "TLA+ certified-context control reproduces ${context_description}"
    else
      fail "TLA+ certified-context control failed for the wrong reason (see $context_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_certified_floor_commitment)" "$TLA_DIR/MC_CertifiedFloorCommitment.cfg" "$TLA_DIR/CertifiedFloorCommitment.tla" >"$LOG_DIR/ff_tlc_certified_floor_commitment.log" 2>&1; then
    pass "TLA+ certified-floor commitments preserve every durable parent floor, bind candidate authority, remain cache-transparent, and converge after dependency fetch"
  else
    fail "TLA+ certified-floor commitment model failed (see $LOG_DIR/ff_tlc_certified_floor_commitment.log)"
  fi
  for floor_commitment_control in \
      'no_verification_unsafe:AcceptedCertifiedRebasesHaveEvidence:accepting an unverified finalization certificate' \
      'cached_use_unsafe:AcceptedCandidatesPreserveEveryParentFloor:reusing a verified certificate without candidate-specific parent-floor admission' \
      'parent_floor_unsafe:AcceptedCandidatesPreserveEveryParentFloor:admitting a historical certificate over a newer parent floor' \
      'context_unsafe:AcceptedCandidatesBindAuthorityContext:omitting the signed candidate authority-context binding' \
      'receiver_lfb_unsafe:ReceiverLocalFloorDoesNotChangeCompatibility:using a receiver-local LFB in deterministic candidate admission'; do
    IFS=: read -r floor_commitment_suffix floor_commitment_invariant floor_commitment_description <<<"$floor_commitment_control"
    floor_commitment_log="$LOG_DIR/ff_tlc_certified_floor_commitment_${floor_commitment_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_certified_floor_commitment_${floor_commitment_suffix}")" "$TLA_DIR/MC_CertifiedFloorCommitment_${floor_commitment_suffix}.cfg" "$TLA_DIR/CertifiedFloorCommitment.tla" >"$floor_commitment_log" 2>&1; then
      fail "TLA+ certified-floor control should reproduce ${floor_commitment_description} but passed"
    elif grep -Eq "The invariant of ${floor_commitment_invariant} is equal to FALSE|Invariant ${floor_commitment_invariant} is violated" "$floor_commitment_log"; then
      pass "TLA+ certified-floor control reproduces ${floor_commitment_description}"
    else
      fail "TLA+ certified-floor control failed for the wrong reason (see $floor_commitment_log)"
    fi
  done
  for floor_liveness_control in \
      'no_commitment_unsafe:proposal floor not committed on the wire' \
      'no_fetch_unsafe:missing certificate dependency never fetched'; do
    IFS=: read -r floor_liveness_suffix floor_liveness_description <<<"$floor_liveness_control"
    floor_liveness_log="$LOG_DIR/ff_tlc_certified_floor_commitment_${floor_liveness_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_certified_floor_commitment_${floor_liveness_suffix}")" "$TLA_DIR/MC_CertifiedFloorCommitment_${floor_liveness_suffix}.cfg" "$TLA_DIR/CertifiedFloorCommitment.tla" >"$floor_liveness_log" 2>&1; then
      fail "TLA+ certified-floor liveness control should reproduce ${floor_liveness_description} but passed"
    elif grep -Eq 'Temporal properties were violated|Property .* is violated' "$floor_liveness_log"; then
      pass "TLA+ certified-floor liveness control reproduces ${floor_liveness_description}"
    else
      fail "TLA+ certified-floor liveness control failed for the wrong reason (see $floor_liveness_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_finalization_certificate_retrieval)" "$TLA_DIR/MC_FinalizationCertificateRetrieval.cfg" "$TLA_DIR/FinalizationCertificateRetrieval.tla" >"$LOG_DIR/ff_tlc_finalization_certificate_retrieval.log" 2>&1; then
    pass "TLA+ typed finalization-certificate retrieval is bounded, restart-stable, duplicate-safe, and eventually wakes every detached block"
  else
    fail "TLA+ finalization-certificate retrieval model failed (see $LOG_DIR/ff_tlc_finalization_certificate_retrieval.log)"
  fi
  for certificate_retrieval_control in \
      'untyped_unsafe:TypedDependencyNamespaceIsDisjoint:block hashes satisfying certificate dependencies' \
      'validation_unsafe:OnlyValidResponsesPersist:invalid or digest-mismatched certificate persistence' \
      'unsolicited_unsafe:UnsolicitedResponsesDoNotMutate:unsolicited certificate responses mutating durable state' \
      'failed_send_unsafe:FailedSendsRetainObligations:transport failure dropping a live proof obligation' \
      'restart_unsafe:RestartNeverStrandsPersistentObligations:restart losing a persistent detached-block obligation' \
      'duplicate_wake_unsafe:EveryBlockIsQueuedAtMostOnce:duplicate responses enqueueing one block more than once'; do
    IFS=: read -r certificate_retrieval_suffix certificate_retrieval_invariant certificate_retrieval_description <<<"$certificate_retrieval_control"
    certificate_retrieval_log="$LOG_DIR/ff_tlc_finalization_certificate_retrieval_${certificate_retrieval_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_finalization_certificate_retrieval_${certificate_retrieval_suffix}")" "$TLA_DIR/MC_FinalizationCertificateRetrieval_${certificate_retrieval_suffix}.cfg" "$TLA_DIR/FinalizationCertificateRetrieval.tla" >"$certificate_retrieval_log" 2>&1; then
      fail "TLA+ certificate-retrieval control should reproduce ${certificate_retrieval_description} but passed"
    elif grep -Eq "The invariant of ${certificate_retrieval_invariant} is equal to FALSE|Invariant ${certificate_retrieval_invariant} is violated" "$certificate_retrieval_log"; then
      pass "TLA+ certificate-retrieval control reproduces ${certificate_retrieval_description}"
    else
      fail "TLA+ certificate-retrieval control failed for the wrong reason (see $certificate_retrieval_log)"
    fi
  done
  if tlc_run "$(tlc_metadir ff_dependency_maintenance_round)" "$TLA_DIR/MC_DependencyMaintenanceRound.cfg" "$TLA_DIR/DependencyMaintenanceRound.tla" >"$LOG_DIR/ff_tlc_dependency_maintenance_round.log" 2>&1; then
    pass "TLA+ mixed block/certificate maintenance attempts the full round snapshot before returning its first dispatch error"
  else
    fail "TLA+ dependency-maintenance round failed (see $LOG_DIR/ff_tlc_dependency_maintenance_round.log)"
  fi
  if tlc_run "$(tlc_metadir ff_dependency_maintenance_round_abort_unsafe)" "$TLA_DIR/MC_DependencyMaintenanceRound_abort_unsafe.cfg" "$TLA_DIR/DependencyMaintenanceRound.tla" >"$LOG_DIR/ff_tlc_dependency_maintenance_round_abort_unsafe.log" 2>&1; then
    fail "TLA+ abort-on-first-failure maintenance control should discard an unattempted obligation but passed"
  elif grep -Eq 'The invariant of FailureNeverDiscardsUnattemptedObligations is equal to FALSE|Invariant FailureNeverDiscardsUnattemptedObligations is violated' "$LOG_DIR/ff_tlc_dependency_maintenance_round_abort_unsafe.log"; then
    pass "TLA+ abort-on-first-failure control reproduces caller-level dependency starvation"
  else
    fail "TLA+ abort-on-first-failure maintenance control failed for the wrong reason (see $LOG_DIR/ff_tlc_dependency_maintenance_round_abort_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_certified_snapshot_capture)" "$TLA_DIR/MC_CertifiedSnapshotCapture.cfg" "$TLA_DIR/CertifiedSnapshotCapture.tla" >"$LOG_DIR/ff_tlc_certified_snapshot_capture.log" 2>&1; then
    pass "TLA+ concurrent proposers capture a single durable DAG/floor/certificate revision or retry"
  else
    fail "TLA+ certified snapshot-capture model failed (see $LOG_DIR/ff_tlc_certified_snapshot_capture.log)"
  fi
  if tlc_run "$(tlc_metadir ff_certified_snapshot_capture_torn_unsafe)" "$TLA_DIR/MC_CertifiedSnapshotCapture_torn_unsafe.cfg" "$TLA_DIR/CertifiedSnapshotCapture.tla" >"$LOG_DIR/ff_tlc_certified_snapshot_capture_torn_unsafe.log" 2>&1; then
    fail "TLA+ torn snapshot control should violate revision coherence but passed"
  elif grep -Fq 'Invariant CompletedSnapshotsBindOneRevision is violated' "$LOG_DIR/ff_tlc_certified_snapshot_capture_torn_unsafe.log"; then
    pass "TLA+ torn snapshot control reproduces mixed durable DAG/floor/certificate revisions"
  else
    fail "TLA+ torn snapshot control failed for the wrong reason (see $LOG_DIR/ff_tlc_certified_snapshot_capture_torn_unsafe.log)"
  fi
  if tla2sany "$TLA_DIR/ProtocolV5EndToEnd.tla" >"$LOG_DIR/ff_sany_protocol_v5_end_to_end.log" 2>&1; then
    pass "TLA+ composed protocol-v5 refinement is well formed; exhaustive component models and bounded symbolic composition provide its executable evidence"
  else
    fail "TLA+ composed protocol-v5 refinement is malformed (see $LOG_DIR/ff_sany_protocol_v5_end_to_end.log)"
  fi
  for protocol_v5_control in \
      'post_state_certificate_unsafe:CertificatesUseExactProposalPreState:post-state certification replacing exact proposal pre-state authority' \
      'intrinsic_admission_unsafe:IntrinsicAdmissionOnly:an intrinsically invalid block entering admitted state' \
      'order_dependent_evidence_unsafe:StableEvidenceIsGenerationAwareAndOrderIndependent:sibling delivery order changing durable evidence' \
      'generation_blind_evidence_unsafe:StableEvidenceIsGenerationAwareAndOrderIndependent:cross-incarnation siblings creating evidence' \
      'head_committee_unsafe:FinalizationUsesFrozenFloorCommittee:mutable head bonds replacing the frozen floor committee' \
      'unfiltered_finality_unsafe:ObjectiveEquivocatorsDoNotContributeFinalityVotes:an objective equivocator contributing to finality' \
      'retry_without_repair_unsafe:CompletedRetryRepairsDurableEvidence:duplicate retry completing without durable-index repair' \
      'generation_blind_slash_unsafe:SlashTargetsCurrentBondGeneration:stale evidence slashing a rebonded incarnation' \
      'restore_bonded_unsafe:RedemptionRestoresExactLifecycle:redemption collapsing a withdrawing origin to Bonded' \
      'lost_receipt_unsafe:ResolutionRetriesAreIdempotent:a lost custody receipt applying a resolution twice' \
      'replay_drift_unsafe:ReplayMatchesCanonicalCost:replica-local replay cost drift' \
      'split_settlement_unsafe:SettlementChargesExactlyReplayCost:a split settlement crediting a fee different from replay cost'; do
    IFS=: read -r protocol_v5_suffix protocol_v5_invariant protocol_v5_description <<<"$protocol_v5_control"
    protocol_v5_log="$LOG_DIR/ff_tlc_protocol_v5_${protocol_v5_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_protocol_v5_${protocol_v5_suffix}")" "$TLA_DIR/MC_ProtocolV5EndToEnd_${protocol_v5_suffix}.cfg" "$TLA_DIR/ProtocolV5EndToEnd.tla" >"$protocol_v5_log" 2>&1; then
      fail "TLA+ protocol-v5 control should reproduce ${protocol_v5_description} but passed"
    elif grep -Fq "Invariant ${protocol_v5_invariant} is violated" "$protocol_v5_log"; then
      pass "TLA+ protocol-v5 control reproduces ${protocol_v5_description}"
    else
      fail "TLA+ protocol-v5 control failed for the wrong reason (see $protocol_v5_log)"
    fi
  done
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
    pass "TLA+ causal fork choice retains valid tips, inserts the LFB when no causal tip descends from it, and floor-rebases proposal state"
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
  if tlc_run "$(tlc_metadir ff_state_preserving_fork_choice_vote_parent_unsafe)" "$TLA_DIR/MC_StatePreservingForkChoice_parent_uses_votes_unsafe.cfg" "$TLA_DIR/StatePreservingForkChoice.tla" >"$LOG_DIR/ff_tlc_state_preserving_fork_choice_vote_parent_unsafe.log" 2>&1; then
    fail "TLA+ vote-projection parent control should drop an accepted stale sibling but passed"
  elif grep -q "Inv_AllValidLatestTipsRemainCausalInputs is violated" "$LOG_DIR/ff_tlc_state_preserving_fork_choice_vote_parent_unsafe.log"; then
    pass "TLA+ vote-projection parent control reproduces accepted stale-sibling loss"
  else
    fail "TLA+ vote-projection parent control failed for the wrong reason (see $LOG_DIR/ff_tlc_state_preserving_fork_choice_vote_parent_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_state_preserving_fork_choice_invalid_stale_unsafe)" "$TLA_DIR/MC_StatePreservingForkChoice_invalid_stale_unsafe.cfg" "$TLA_DIR/StatePreservingForkChoice.tla" >"$LOG_DIR/ff_tlc_state_preserving_fork_choice_invalid_stale_unsafe.log" 2>&1; then
    fail "TLA+ stale-only parent control should admit an intrinsically invalid tip but passed"
  elif grep -q "Inv_IntrinsicallyInvalidTipIsNeverCausal is violated" "$LOG_DIR/ff_tlc_state_preserving_fork_choice_invalid_stale_unsafe.log"; then
    pass "TLA+ stale-only parent control reproduces multiply-invalid tip admission"
  else
    fail "TLA+ stale-only parent control failed for the wrong reason (see $LOG_DIR/ff_tlc_state_preserving_fork_choice_invalid_stale_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_state_preserving_fork_choice_depth_expiry)" "$TLA_DIR/MC_StatePreservingForkChoice_depth_expiry.cfg" "$TLA_DIR/StatePreservingForkChoice.tla" >"$LOG_DIR/ff_tlc_state_preserving_fork_choice_depth_expiry.log" 2>&1; then
    pass "TLA+ zero-depth causal expiry preserves floor state and restores proposal liveness"
  else
    fail "TLA+ zero-depth causal-expiry model failed (see $LOG_DIR/ff_tlc_state_preserving_fork_choice_depth_expiry.log)"
  fi
  while IFS='|' read -r suffix invariant label; do
    control_log="$LOG_DIR/ff_tlc_state_preserving_fork_choice_${suffix}.log"
    if tlc_run "$(tlc_metadir "ff_state_preserving_fork_choice_${suffix}")" "$TLA_DIR/MC_StatePreservingForkChoice_${suffix}.cfg" "$TLA_DIR/StatePreservingForkChoice.tla" >"$control_log" 2>&1; then
      fail "TLA+ $label control should violate $invariant but passed"
    elif grep -q "$invariant is violated" "$control_log"; then
      pass "TLA+ $label control reproduces the designated safety violation"
    else
      fail "TLA+ $label control failed for the wrong reason (see $control_log)"
    fi
  done <<'EOF'
deploy_promotion_unsafe|Inv_GhostHeadIsMainParent|deploy promotion
omit_floor_evidence_unsafe|Inv_EvidenceRootsIncludeSnapshotFloor|omitted floor evidence root
skip_antichain_unsafe|Inv_ParentsFormReachabilityAntichain|uncompacted causal parents
recovery_floor_unsafe|Inv_RecoveryNarrowingRequiresCoverageAndFloorAncestry|floor-blind recovery narrowing
EOF
  while IFS='|' read -r suffix label; do
    liveness_log="$LOG_DIR/ff_tlc_state_preserving_fork_choice_${suffix}.log"
    if tlc_run "$(tlc_metadir "ff_state_preserving_fork_choice_${suffix}")" "$TLA_DIR/MC_StatePreservingForkChoice_${suffix}.cfg" "$TLA_DIR/StatePreservingForkChoice.tla" >"$liveness_log" 2>&1; then
      fail "TLA+ $label control should violate proposal liveness but passed"
    elif grep -q "Temporal properties were violated" "$liveness_log"; then
      pass "TLA+ $label control reproduces permanent proposal starvation"
    else
      fail "TLA+ $label control failed for the wrong reason (see $liveness_log)"
    fi
  done <<'EOF'
parent_cap_liveness_unsafe|undersized parent capacity
parent_depth_liveness_unsafe|depth bound without causal expiry
EOF
  if tlc_run "$(tlc_metadir ff_stale_sibling_recovery)" "$TLA_DIR/MC_StaleSiblingRecovery.cfg" "$TLA_DIR/StaleSiblingRecovery.tla" >"$LOG_DIR/ff_tlc_stale_sibling_recovery.log" 2>&1; then
    pass "TLA+ asynchronous stale-sibling settlement preserves the floor, emits an exact source tombstone, buffers the rejected occurrence, and elects one recovery owner"
  else
    fail "TLA+ stale-sibling recovery lifecycle failed (see $LOG_DIR/ff_tlc_stale_sibling_recovery.log)"
  fi
  while IFS='|' read -r suffix invariant label; do
    control_log="$LOG_DIR/ff_tlc_stale_sibling_recovery_${suffix}.log"
    if tlc_run "$(tlc_metadir "ff_stale_sibling_recovery_${suffix}")" "$TLA_DIR/MC_StaleSiblingRecovery_${suffix}.cfg" "$TLA_DIR/StaleSiblingRecovery.tla" >"$control_log" 2>&1; then
      fail "TLA+ $label control should violate $invariant but passed"
    elif grep -q "$invariant is violated" "$control_log"; then
      pass "TLA+ $label control reproduces the designated stale-sibling lifecycle violation"
    else
      fail "TLA+ $label control failed for the wrong reason (see $control_log)"
    fi
  done <<'EOF'
drop_stale_unsafe|Inv_AcceptedStaleRemainsCausal|premature stale-sibling removal
signature_tombstone_unsafe|Inv_TombstoneNamesExactSource|signature-only tombstone
missing_buffer_unsafe|Inv_ObservedRejectionIsBuffered|non-atomic rejected-occurrence buffering
suppress_recovery_unsafe|Inv_SelectedRecoveryIsNotSelfChainSuppressed|self-chain recovery suppression
truncated_frontier_unsafe|Inv_SettlementUsesCompleteFrontier|truncated settlement frontier
floor_regression_unsafe|Inv_FinalizedEffectNeverRegresses|finalized-effect regression
nonleader_unsafe|Inv_OnlyCommittedViewLeaderRetries|nonleader recovery ownership
EOF
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
  if tlc_run "$(tlc_metadir ff_finalizer_floor_materialization)" "$TLA_DIR/MC_FinalizerFloorMaterialization.cfg" "$TLA_DIR/FinalizerFloorMaterialization.tla" >"$LOG_DIR/ff_tlc_finalizer_floor_materialization.log" 2>&1; then
    pass "TLA+ parallel proposal deferral materializes the exact dual-certified secondary target under all-parent coverage"
  else
    fail "TLA+ finalizer-floor materialization model failed (see $LOG_DIR/ff_tlc_finalizer_floor_materialization.log)"
  fi
  if tlc_run "$(tlc_metadir ff_finalizer_floor_materialization_main_parent_unsafe)" "$TLA_DIR/MC_FinalizerFloorMaterialization_main_parent_unsafe.cfg" "$TLA_DIR/FinalizerFloorMaterialization.tla" >"$LOG_DIR/ff_tlc_finalizer_floor_materialization_main_parent_unsafe.log" 2>&1; then
    fail "TLA+ main-parent-only finalizer discovery should starve the certified secondary target but passed"
  elif grep -q "Inv_FinalizerDiscoversCandidate is violated" "$LOG_DIR/ff_tlc_finalizer_floor_materialization_main_parent_unsafe.log"; then
    pass "TLA+ main-parent-only finalizer control reproduces permanent secondary-target starvation"
  else
    fail "TLA+ main-parent-only finalizer control failed for the wrong reason (see $LOG_DIR/ff_tlc_finalizer_floor_materialization_main_parent_unsafe.log)"
  fi
  if tlc_run "$(tlc_metadir ff_finalizer_floor_materialization_causal_only_unsafe)" "$TLA_DIR/MC_FinalizerFloorMaterialization_causal_only_unsafe.cfg" "$TLA_DIR/FinalizerFloorMaterialization.tla" >"$LOG_DIR/ff_tlc_finalizer_floor_materialization_causal_only_unsafe.log" 2>&1; then
    fail "TLA+ causal-only target substitution should violate exact target binding but passed"
  elif grep -q "Inv_SelectedTargetBindsRequestedCertificate is violated" "$LOG_DIR/ff_tlc_finalizer_floor_materialization_causal_only_unsafe.log"; then
    pass "TLA+ causal-only control reproduces rejected-sibling target substitution"
  else
    fail "TLA+ causal-only target-substitution control failed for the wrong reason (see $LOG_DIR/ff_tlc_finalizer_floor_materialization_causal_only_unsafe.log)"
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
  if tlc_run "$(tlc_metadir ff_witness_equivalent_carrier)" "$TLA_DIR/MC_WitnessEquivalentCarrier.cfg" "$TLA_DIR/WitnessEquivalentCarrier.tla" >"$LOG_DIR/ff_tlc_witness_equivalent_carrier.log" 2>&1; then
    pass "TLA+ semantic predecessor carriers preserve exact state and block/digest pairing across divergent local witnesses"
  else
    fail "TLA+ witness-equivalent carrier model failed (see $LOG_DIR/ff_tlc_witness_equivalent_carrier.log)"
  fi
  for witness_carrier_control in \
      'exact_digest_unsafe:SemanticCarrierCannotRemainParked:exact local witness identity parks an honest node' \
      'floor_only_unsafe:SelectedCarrierHasExactSemanticState:floor-only matching accepts a different replay state' \
      'copy_digest_unsafe:SelectedCarrierDigestIsPaired:local digest copying splices two proof identities' \
      'wake_unsafe:SemanticCarrierCannotRemainParked:semantic carrier admission fails to wake a parked finalizer'; do
    IFS=: read -r witness_carrier_suffix witness_carrier_invariant witness_carrier_description <<<"$witness_carrier_control"
    witness_carrier_log="$LOG_DIR/ff_tlc_witness_equivalent_carrier_${witness_carrier_suffix}.log"
    if tlc_run "$(tlc_metadir "ff_witness_equivalent_carrier_${witness_carrier_suffix}")" "$TLA_DIR/MC_WitnessEquivalentCarrier_${witness_carrier_suffix}.cfg" "$TLA_DIR/WitnessEquivalentCarrier.tla" >"$witness_carrier_log" 2>&1; then
      fail "TLA+ witness-carrier control should reproduce ${witness_carrier_description} but passed"
    elif grep -Fq "Invariant ${witness_carrier_invariant} is violated" "$witness_carrier_log"; then
      pass "TLA+ witness-carrier control reproduces ${witness_carrier_description}"
    else
      fail "TLA+ witness-carrier control failed for the wrong reason (see $witness_carrier_log)"
    fi
  done
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
  PENDING_HEARTBEAT_APALACHE_SAFE_LENGTH="${PENDING_HEARTBEAT_APALACHE_SAFE_LENGTH:-6}"
  PENDING_HEARTBEAT_APALACHE_UNSAFE_LENGTH="${PENDING_HEARTBEAT_APALACHE_UNSAFE_LENGTH:-6}"
  PENDING_HEARTBEAT_APALACHE_TYPEOK_LENGTH="${PENDING_HEARTBEAT_APALACHE_TYPEOK_LENGTH:-2}"
  RECOVERY_COMMITTEE_APALACHE_SAFE_LENGTH="${RECOVERY_COMMITTEE_APALACHE_SAFE_LENGTH:-6}"
  RECOVERY_COMMITTEE_APALACHE_UNSAFE_LENGTH="${RECOVERY_COMMITTEE_APALACHE_UNSAFE_LENGTH:-4}"
  OBJECTIVE_EQUIVOCATION_APALACHE_SAFE_LENGTH="${OBJECTIVE_EQUIVOCATION_APALACHE_SAFE_LENGTH:-8}"
  OBJECTIVE_EQUIVOCATION_APALACHE_UNSAFE_LENGTH="${OBJECTIVE_EQUIVOCATION_APALACHE_UNSAFE_LENGTH:-8}"
  OBJECTIVE_AUTHORIZATION_APALACHE_SAFE_LENGTH="${OBJECTIVE_AUTHORIZATION_APALACHE_SAFE_LENGTH:-10}"
  OBJECTIVE_AUTHORIZATION_APALACHE_UNSAFE_LENGTH="${OBJECTIVE_AUTHORIZATION_APALACHE_UNSAFE_LENGTH:-8}"
  BOND_GENERATION_APALACHE_SAFE_LENGTH="${BOND_GENERATION_APALACHE_SAFE_LENGTH:-10}"
  BOND_GENERATION_APALACHE_UNSAFE_LENGTH="${BOND_GENERATION_APALACHE_UNSAFE_LENGTH:-10}"
  BOND_GENERATION_APALACHE_TIMEOUT="${BOND_GENERATION_APALACHE_TIMEOUT:-1800}"
  CAUSAL_FINALITY_APALACHE_SAFE_LENGTH="${CAUSAL_FINALITY_APALACHE_SAFE_LENGTH:-4}"
  CAUSAL_FINALITY_APALACHE_UNSAFE_LENGTH="${CAUSAL_FINALITY_APALACHE_UNSAFE_LENGTH:-4}"
  CERTIFIED_OBJECTIVE_APALACHE_SAFE_LENGTH="${CERTIFIED_OBJECTIVE_APALACHE_SAFE_LENGTH:-8}"
  CERTIFIED_OBJECTIVE_APALACHE_UNSAFE_LENGTH="${CERTIFIED_OBJECTIVE_APALACHE_UNSAFE_LENGTH:-8}"
  CERTIFIED_OBJECTIVE_APALACHE_NONCANONICAL_LENGTH="${CERTIFIED_OBJECTIVE_APALACHE_NONCANONICAL_LENGTH:-12}"
  CERTIFIED_OBJECTIVE_APALACHE_NONCANONICAL_TIMEOUT="${CERTIFIED_OBJECTIVE_APALACHE_NONCANONICAL_TIMEOUT:-600}"
  CERTIFIED_CAUSAL_ADMISSION_APALACHE_SAFE_LENGTH="${CERTIFIED_CAUSAL_ADMISSION_APALACHE_SAFE_LENGTH:-5}"
  CERTIFIED_CAUSAL_ADMISSION_APALACHE_UNSAFE_LENGTH="${CERTIFIED_CAUSAL_ADMISSION_APALACHE_UNSAFE_LENGTH:-5}"
  ADMISSION_DISPOSITION_APALACHE_SAFE_LENGTH="${ADMISSION_DISPOSITION_APALACHE_SAFE_LENGTH:-6}"
  ADMISSION_DISPOSITION_APALACHE_UNSAFE_LENGTH="${ADMISSION_DISPOSITION_APALACHE_UNSAFE_LENGTH:-2}"
  CERTIFIED_CONTEXT_APALACHE_SAFE_LENGTH="${CERTIFIED_CONTEXT_APALACHE_SAFE_LENGTH:-10}"
  CERTIFIED_CONTEXT_APALACHE_UNSAFE_LENGTH="${CERTIFIED_CONTEXT_APALACHE_UNSAFE_LENGTH:-1}"
  CERTIFIED_CONTEXT_APALACHE_STALE_LENGTH="${CERTIFIED_CONTEXT_APALACHE_STALE_LENGTH:-10}"
  CERTIFIED_FLOOR_APALACHE_SAFE_LENGTH="${CERTIFIED_FLOOR_APALACHE_SAFE_LENGTH:-8}"
  CERTIFIED_FLOOR_APALACHE_UNSAFE_LENGTH="${CERTIFIED_FLOOR_APALACHE_UNSAFE_LENGTH:-6}"
  CERTIFIED_FLOOR_APALACHE_CONTEXT_UNSAFE_LENGTH="${CERTIFIED_FLOOR_APALACHE_CONTEXT_UNSAFE_LENGTH:-8}"
  CERTIFICATE_RETRIEVAL_APALACHE_SAFE_LENGTH="${CERTIFICATE_RETRIEVAL_APALACHE_SAFE_LENGTH:-12}"
  CERTIFICATE_RETRIEVAL_APALACHE_UNSAFE_LENGTH="${CERTIFICATE_RETRIEVAL_APALACHE_UNSAFE_LENGTH:-6}"
  DEPENDENCY_MAINTENANCE_APALACHE_SAFE_LENGTH="${DEPENDENCY_MAINTENANCE_APALACHE_SAFE_LENGTH:-8}"
  DEPENDENCY_MAINTENANCE_APALACHE_UNSAFE_LENGTH="${DEPENDENCY_MAINTENANCE_APALACHE_UNSAFE_LENGTH:-3}"
  CERTIFIED_SNAPSHOT_APALACHE_SAFE_LENGTH="${CERTIFIED_SNAPSHOT_APALACHE_SAFE_LENGTH:-6}"
  CERTIFIED_SNAPSHOT_APALACHE_UNSAFE_LENGTH="${CERTIFIED_SNAPSHOT_APALACHE_UNSAFE_LENGTH:-4}"
  WITNESS_CARRIER_APALACHE_SAFE_LENGTH="${WITNESS_CARRIER_APALACHE_SAFE_LENGTH:-5}"
  WITNESS_CARRIER_APALACHE_UNSAFE_LENGTH="${WITNESS_CARRIER_APALACHE_UNSAFE_LENGTH:-3}"
  PROTOCOL_V5_APALACHE_SAFE_LENGTH="${PROTOCOL_V5_APALACHE_SAFE_LENGTH:-5}"
  PROPOSER_COALESCING_APALACHE_SAFE_LENGTH="${PROPOSER_COALESCING_APALACHE_SAFE_LENGTH:-6}"
  PROPOSER_COALESCING_APALACHE_UNSAFE_LENGTH="${PROPOSER_COALESCING_APALACHE_UNSAFE_LENGTH:-6}"
  LIVE_RECOVERY_APALACHE_SAFE_LENGTH="${LIVE_RECOVERY_APALACHE_SAFE_LENGTH:-6}"
  LIVE_RECOVERY_APALACHE_UNSAFE_LENGTH="${LIVE_RECOVERY_APALACHE_UNSAFE_LENGTH:-5}"
  FINALIZER_MATERIALIZATION_APALACHE_SAFE_LENGTH="${FINALIZER_MATERIALIZATION_APALACHE_SAFE_LENGTH:-8}"
  FINALIZER_MATERIALIZATION_APALACHE_UNSAFE_LENGTH="${FINALIZER_MATERIALIZATION_APALACHE_UNSAFE_LENGTH:-6}"
  STALE_SIBLING_APALACHE_SAFE_LENGTH="${STALE_SIBLING_APALACHE_SAFE_LENGTH:-14}"
  STALE_SIBLING_APALACHE_UNSAFE_LENGTH="${STALE_SIBLING_APALACHE_UNSAFE_LENGTH:-14}"
  stale_sibling_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/stale-sibling-safe" check --config=StaleSiblingRecoveryApalache.cfg --length="$STALE_SIBLING_APALACHE_SAFE_LENGTH" --no-deadlock StaleSiblingRecovery.tla 2>&1)"
  stale_sibling_rc=$?
  printf '%s\n' "$stale_sibling_output" >"$LOG_DIR/ff_apalache_stale_sibling_recovery.log"
  if [[ $stale_sibling_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_stale_sibling_recovery.log"; then
    pass "Apalache asynchronous stale-sibling recovery invariants through bound $STALE_SIBLING_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache stale-sibling recovery model failed (see $LOG_DIR/ff_apalache_stale_sibling_recovery.log)"
  fi
  for stale_sibling_control in \
      'drop-stale:StaleSiblingRecoveryDropStaleUnsafeApalache.cfg' \
      'signature-tombstone:StaleSiblingRecoverySignatureUnsafeApalache.cfg' \
      'missing-buffer:StaleSiblingRecoveryBufferUnsafeApalache.cfg' \
      'suppress-recovery:StaleSiblingRecoverySuppressionUnsafeApalache.cfg' \
      'truncated-frontier:StaleSiblingRecoveryFrontierUnsafeApalache.cfg' \
      'floor-regression:StaleSiblingRecoveryFloorUnsafeApalache.cfg' \
      'nonleader:StaleSiblingRecoveryLeaderUnsafeApalache.cfg'; do
    IFS=: read -r stale_sibling_name stale_sibling_cfg <<<"$stale_sibling_control"
    stale_sibling_control_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/stale-sibling-$stale_sibling_name" check --config="$stale_sibling_cfg" --length="$STALE_SIBLING_APALACHE_UNSAFE_LENGTH" --no-deadlock StaleSiblingRecovery.tla 2>&1)"
    stale_sibling_control_rc=$?
    stale_sibling_control_log="$LOG_DIR/ff_apalache_stale_sibling_${stale_sibling_name}.log"
    printf '%s\n' "$stale_sibling_control_output" >"$stale_sibling_control_log"
    if [[ $stale_sibling_control_rc -ne 0 ]] \
         && grep -qE 'state invariant [0-9]+ violated' "$stale_sibling_control_log" \
         && grep -q 'The outcome is: Error' "$stale_sibling_control_log"; then
      pass "Apalache $stale_sibling_name control reproduces the stale-sibling lifecycle defect"
    else
      fail "Apalache $stale_sibling_name control did not reproduce the expected counterexample (see $stale_sibling_control_log)"
    fi
  done
  divergent_history_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/divergent-history-safe" check --config=MC_DivergentFinalizationHistoriesApalache.cfg --length=5 --no-deadlock DivergentFinalizationHistories.tla 2>&1)"
  divergent_history_rc=$?
  printf '%s\n' "$divergent_history_output" >"$LOG_DIR/ff_apalache_divergent_history.log"
  if [[ $divergent_history_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_divergent_history.log"; then
    pass "Apalache permits same-target validators with distinct local ledger histories through bound 5"
  else
    fail "Apalache divergent-local-history model failed (see $LOG_DIR/ff_apalache_divergent_history.log)"
  fi
  live_recovery_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/live-minority-recovery-safe" check --config=MC_LiveMinorityForkRecoveryApalache.cfg --length="$LIVE_RECOVERY_APALACHE_SAFE_LENGTH" --no-deadlock LiveMinorityForkRecovery.tla 2>&1)"
  live_recovery_rc=$?
  printf '%s\n' "$live_recovery_output" >"$LOG_DIR/ff_apalache_live_recovery.log"
  if [[ $live_recovery_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_live_recovery.log"; then
    pass "Apalache live minority-fork recovery preserves local publication, dependency closure, and validator/shard concurrency through bound $LIVE_RECOVERY_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache live minority-fork recovery model failed (see $LOG_DIR/ff_apalache_live_recovery.log)"
  fi
  for recovery_control in \
      'divergent-remote-ledger:MC_DivergentFinalizationHistories_remote_ledger_unsafe_Apalache.cfg:5:DivergentFinalizationHistories.tla' \
      "live-remote-head:MC_LiveMinorityForkRecovery_remote_head_unsafe_Apalache.cfg:$LIVE_RECOVERY_APALACHE_UNSAFE_LENGTH:LiveMinorityForkRecovery.tla" \
      'live-dependencies:MC_LiveMinorityForkRecovery_dependencies_unsafe_Apalache.cfg:3:LiveMinorityForkRecovery.tla' \
      'live-global-pause:MC_LiveMinorityForkRecovery_global_pause_unsafe_Apalache.cfg:2:LiveMinorityForkRecovery.tla'; do
    IFS=: read -r recovery_name recovery_cfg recovery_length recovery_module <<<"$recovery_control"
    recovery_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/$recovery_name" check --config="$recovery_cfg" --length="$recovery_length" --no-deadlock "$recovery_module" 2>&1)"
    recovery_rc=$?
    recovery_log="$LOG_DIR/ff_apalache_${recovery_name}.log"
    printf '%s\n' "$recovery_output" >"$recovery_log"
    if [[ $recovery_rc -ne 0 ]] \
         && grep -qE 'state invariant [0-9]+ violated' "$recovery_log" \
         && grep -q 'The outcome is: Error' "$recovery_log"; then
      pass "Apalache $recovery_name control reproduces unsafe remote authority or recovery behavior"
    else
      fail "Apalache $recovery_name control did not reproduce the expected counterexample (see $recovery_log)"
    fi
  done
  parallel_validator_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/parallel-validator-safe" check --config=MC_ParallelValidatorConsensusApalache.cfg --length=6 ParallelValidatorConsensus.tla 2>&1)"
  parallel_validator_rc=$?
  printf '%s\n' "$parallel_validator_output" >"$LOG_DIR/ff_apalache_parallel_validator.log"
  if [[ $parallel_validator_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_parallel_validator.log"; then
    pass "Apalache independent-validator replay, support, and floor-publication invariants through bound 6"
  else
    fail "Apalache parallel-validator consensus model failed (see $LOG_DIR/ff_apalache_parallel_validator.log)"
  fi
  parallel_validator_stale_window_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/parallel-validator-stale-window-safe" check --config=MC_ParallelValidatorConsensusStaleWindowApalache.cfg --length=2 ParallelValidatorConsensus.tla 2>&1)"
  parallel_validator_stale_window_rc=$?
  printf '%s\n' "$parallel_validator_stale_window_output" >"$LOG_DIR/ff_apalache_parallel_validator_stale_window.log"
  if [[ $parallel_validator_stale_window_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_parallel_validator_stale_window.log"; then
    pass "Apalache rejects stale promotion from a concurrently accepted predecessor through bound 2"
  else
    fail "Apalache parallel-validator stale-window model failed (see $LOG_DIR/ff_apalache_parallel_validator_stale_window.log)"
  fi
  parallel_validator_stale_window_unsafe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/parallel-validator-stale-window-unsafe" check --config=MC_ParallelValidatorConsensus_stale_floor_unsafe.cfg --length=1 ParallelValidatorConsensus.tla 2>&1)"
  parallel_validator_stale_window_unsafe_rc=$?
  printf '%s\n' "$parallel_validator_stale_window_unsafe_output" >"$LOG_DIR/ff_apalache_parallel_validator_stale_window_unsafe.log"
  if [[ $parallel_validator_stale_window_unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_parallel_validator_stale_window_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_parallel_validator_stale_window_unsafe.log"; then
    pass "Apalache stale-promotion control finds loss of already committed effects"
  else
    fail "Apalache stale-promotion control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_parallel_validator_stale_window_unsafe.log)"
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
  heartbeat_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/heartbeat-safe" check --config=MC_HeartbeatFinalityBackpressureApalache.cfg --length=5 HeartbeatFinalityBackpressure.tla 2>&1)"
  heartbeat_rc=$?
  printf '%s\n' "$heartbeat_output" >"$LOG_DIR/ff_apalache_heartbeat_backpressure.log"
  if [[ $heartbeat_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_heartbeat_backpressure.log"; then
    pass "Apalache explicit block/view heartbeat/backpressure invariants through bound 5"
  else
    fail "Apalache heartbeat/finality backpressure model failed (see $LOG_DIR/ff_apalache_heartbeat_backpressure.log)"
  fi
  heartbeat_async_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/heartbeat-async-safe" check --config=MC_HeartbeatFinalityBackpressure_async_Apalache.cfg --length=4 HeartbeatFinalityBackpressure.tla 2>&1)"
  heartbeat_async_rc=$?
  printf '%s\n' "$heartbeat_async_output" >"$LOG_DIR/ff_apalache_heartbeat_async.log"
  if [[ $heartbeat_async_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_heartbeat_async.log"; then
    pass "Apalache independently advancing local recovery rounds preserve heartbeat safety through bound 4"
  else
    fail "Apalache asynchronous heartbeat safety model failed (see $LOG_DIR/ff_apalache_heartbeat_async.log)"
  fi
  heartbeat_promotion_witness_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/heartbeat-promotion-witness-unsafe" check --config=MC_HeartbeatFinalityBackpressure_promotion_witness_unsafe_Apalache.cfg --inv=Inv_NoPromotion --length=2 HeartbeatFinalityBackpressure.tla 2>&1)"
  heartbeat_promotion_witness_rc=$?
  printf '%s\n' "$heartbeat_promotion_witness_output" >"$LOG_DIR/ff_apalache_heartbeat_promotion_witness_unsafe.log"
  if [[ $heartbeat_promotion_witness_rc -ne 0 ]] \
       && grep -qE 'Using inv predicate\(s\) Inv_NoPromotion|using Inv_NoPromotion' "$LOG_DIR/ff_apalache_heartbeat_promotion_witness_unsafe.log" \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_heartbeat_promotion_witness_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_heartbeat_promotion_witness_unsafe.log"; then
    pass "Apalache promotion-witness control reaches exact mutual causal/state promotion"
  else
    fail "Apalache promotion-witness control did not violate Inv_NoPromotion (see $LOG_DIR/ff_apalache_heartbeat_promotion_witness_unsafe.log)"
  fi
  heartbeat_cadence_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/heartbeat-cadence-safe" check --config=HeartbeatRecoveryCadenceApalache.cfg --length=10 HeartbeatRecoveryCadence.tla 2>&1)"
  heartbeat_cadence_rc=$?
  printf '%s\n' "$heartbeat_cadence_output" >"$LOG_DIR/ff_apalache_heartbeat_cadence.log"
  if [[ $heartbeat_cadence_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_heartbeat_cadence.log"; then
    pass "Apalache parallel heartbeat clocks preserve separated stall and recovery cadence through bound 10"
  else
    fail "Apalache heartbeat recovery-cadence model failed (see $LOG_DIR/ff_apalache_heartbeat_cadence.log)"
  fi
  target_terminality_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/target-deploy-terminality-safe" check --config=TargetDeployTerminalityApalache.cfg --length=8 --no-deadlock TargetDeployTerminality.tla 2>&1)"
  target_terminality_rc=$?
  printf '%s\n' "$target_terminality_output" >"$LOG_DIR/ff_apalache_target_deploy_terminality.log"
  if [[ $target_terminality_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_target_deploy_terminality.log"; then
    pass "Apalache target-deploy observer preserves exact terminality and both deadline bounds through bound 8"
  else
    fail "Apalache target-deploy terminality model failed (see $LOG_DIR/ff_apalache_target_deploy_terminality.log)"
  fi
  for target_control in \
      'fixed:TargetDeployTerminalityFixedUnsafeApalache.cfg:Inv_WithinProgressBudgetRemainsLive' \
      'history:TargetDeployTerminalityHistoryUnsafeApalache.cfg:Inv_HistoryAnomalyDetected' \
      'inexact:TargetDeployTerminalityInexactUnsafeApalache.cfg:Inv_SuccessRequiresExactFinalizedStatus' \
      'late:TargetDeployTerminalityLateTerminalUnsafeApalache.cfg:Inv_TerminalOutcomeWithinBudget' \
      'baseline:TargetDeployTerminalityBaselineUnsafeApalache.cfg:Inv_FirstObservationDoesNotRenew'; do
    IFS=: read -r target_name target_cfg target_invariant <<<"$target_control"
    target_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/target-deploy-${target_name}-unsafe" check --config="$target_cfg" --inv="$target_invariant" --length=8 --no-deadlock TargetDeployTerminality.tla 2>&1)"
    target_rc=$?
    target_log="$LOG_DIR/ff_apalache_target_deploy_${target_name}_unsafe.log"
    printf '%s\n' "$target_output" >"$target_log"
    if [[ $target_rc -ne 0 ]] \
         && grep -qE 'state invariant [0-9]+ violated' "$target_log" \
         && grep -q 'The outcome is: Error' "$target_log"; then
      pass "Apalache target-deploy ${target_name} control reproduces its observer-contract violation"
    else
      fail "Apalache target-deploy ${target_name} control did not reproduce the expected counterexample (see $target_log)"
    fi
  done
  heartbeat_collapsed_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/heartbeat-cadence-collapsed-unsafe" check --config=HeartbeatRecoveryCadenceCollapsedUnsafeApalache.cfg --inv=Inv_CadenceMatchesContract --length=2 HeartbeatRecoveryCadence.tla 2>&1)"
  heartbeat_collapsed_rc=$?
  printf '%s\n' "$heartbeat_collapsed_output" >"$LOG_DIR/ff_apalache_heartbeat_collapsed_cadence_unsafe.log"
  if [[ $heartbeat_collapsed_rc -ne 0 ]] \
       && grep -qE 'Using inv predicate\(s\) Inv_CadenceMatchesContract|using Inv_CadenceMatchesContract' "$LOG_DIR/ff_apalache_heartbeat_collapsed_cadence_unsafe.log" \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_heartbeat_collapsed_cadence_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_heartbeat_collapsed_cadence_unsafe.log"; then
    pass "Apalache collapsed-timeout cadence control finds delayed post-stall recovery"
  else
    fail "Apalache collapsed-timeout cadence control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_heartbeat_collapsed_cadence_unsafe.log)"
  fi
  heartbeat_eager_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/heartbeat-eager-unsafe" check --config=MC_HeartbeatFinalityBackpressure_eager_unsafe_Apalache.cfg --inv=Inv_ValidationBacklogBounded --length=5 HeartbeatFinalityBackpressure.tla 2>&1)"
  heartbeat_eager_rc=$?
  printf '%s\n' "$heartbeat_eager_output" >"$LOG_DIR/ff_apalache_heartbeat_eager_unsafe.log"
  if [[ $heartbeat_eager_rc -ne 0 ]] \
       && grep -qE 'Using inv predicate\(s\) Inv_ValidationBacklogBounded|using Inv_ValidationBacklogBounded' "$LOG_DIR/ff_apalache_heartbeat_eager_unsafe.log" \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_heartbeat_eager_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_heartbeat_eager_unsafe.log"; then
    pass "Apalache eager-heartbeat control finds validation-backlog overflow"
  else
    fail "Apalache eager-heartbeat control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_heartbeat_eager_unsafe.log)"
  fi
  heartbeat_causal_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/heartbeat-causal-unsafe" check --config=MC_HeartbeatFinalityBackpressure_causal_only_unsafe_Apalache.cfg --inv=Inv_PromotionUsesExactStateMajority --length=2 HeartbeatFinalityBackpressure.tla 2>&1)"
  heartbeat_causal_rc=$?
  printf '%s\n' "$heartbeat_causal_output" >"$LOG_DIR/ff_apalache_heartbeat_causal_only_unsafe.log"
  if [[ $heartbeat_causal_rc -ne 0 ]] \
       && grep -qE 'Using inv predicate\(s\) Inv_PromotionUsesExactStateMajority|using Inv_PromotionUsesExactStateMajority' "$LOG_DIR/ff_apalache_heartbeat_causal_only_unsafe.log" \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_heartbeat_causal_only_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_heartbeat_causal_only_unsafe.log"; then
    pass "Apalache causal-only heartbeat control finds unsupported state-floor promotion"
  else
    fail "Apalache causal-only heartbeat control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_heartbeat_causal_only_unsafe.log)"
  fi
  pending_heartbeat_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/pending-heartbeat-safe" check --config=MC_PendingDeployHeartbeatCompositionApalache.cfg --length="$PENDING_HEARTBEAT_APALACHE_SAFE_LENGTH" PendingDeployHeartbeatComposition.tla 2>&1)"
  pending_heartbeat_rc=$?
  printf '%s\n' "$pending_heartbeat_output" >"$LOG_DIR/ff_apalache_pending_heartbeat.log"
  if [[ $pending_heartbeat_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_pending_heartbeat.log"; then
    pass "Apalache pending-deploy scheduler and occurrence invariants through bound $PENDING_HEARTBEAT_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache pending-deploy scheduler model failed (see $LOG_DIR/ff_apalache_pending_heartbeat.log)"
  fi
  pending_recovery_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/pending-heartbeat-recovery-safe" check --config=MC_PendingDeployHeartbeatCompositionRecoveryApalache.cfg --length="$PENDING_HEARTBEAT_APALACHE_SAFE_LENGTH" PendingDeployHeartbeatComposition.tla 2>&1)"
  pending_recovery_rc=$?
  printf '%s\n' "$pending_recovery_output" >"$LOG_DIR/ff_apalache_pending_heartbeat_recovery.log"
  if [[ $pending_recovery_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_pending_heartbeat_recovery.log"; then
    pass "Apalache pending-deploy finalized-floor recovery invariants through bound $PENDING_HEARTBEAT_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache pending-deploy finalized-floor recovery model failed (see $LOG_DIR/ff_apalache_pending_heartbeat_recovery.log)"
  fi
  pending_typeok_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/pending-heartbeat-typeok" check --config=MC_PendingDeployHeartbeatCompositionTypeOKApalache.cfg --length="$PENDING_HEARTBEAT_APALACHE_TYPEOK_LENGTH" PendingDeployHeartbeatComposition.tla 2>&1)"
  pending_typeok_rc=$?
  printf '%s\n' "$pending_typeok_output" >"$LOG_DIR/ff_apalache_pending_heartbeat_typeok.log"
  if [[ $pending_typeok_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_pending_heartbeat_typeok.log"; then
    pass "Apalache pending-deploy TypeOK obligation through bound $PENDING_HEARTBEAT_APALACHE_TYPEOK_LENGTH"
  else
    fail "Apalache pending-deploy TypeOK model failed (see $LOG_DIR/ff_apalache_pending_heartbeat_typeok.log)"
  fi
  for pending_apalache_control in \
      'attempt_closes_round_unsafe:Inv_RetryableOutcomeDoesNotCompleteRound:retryable outcome closing a recovery round' \
      'clear_on_start_unsafe:Inv_PoolRemovalRequiresTerminalEvidence:premature pending-work removal' \
      'no_recovery_reservation_unsafe:Inv_RecoveryReservationHonored:unreserved recovery execution' \
      'head_committee_unsafe:Inv_AtMostOneSelectedRecoveryPerRound:divergent parent committees authorizing multiple recovery validators for one floor round' \
      'disjoint_head_eligibility_unsafe:Inv_SelectedRecoveryEligible:a finalized-floor leader rejected by a disjoint parent committee' \
      'head_filtered_justification_unsafe:Inv_QueuedRecoveryHasValidationContext:a finalized-floor leader losing its creator justification and sequence number under parent filtering'; do
    IFS=: read -r pending_suffix pending_invariant pending_description <<<"$pending_apalache_control"
    pending_log="$LOG_DIR/ff_apalache_pending_heartbeat_${pending_suffix}.log"
    pending_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/pending-heartbeat-${pending_suffix}" check --config="MC_PendingDeployHeartbeatComposition_${pending_suffix}_Apalache.cfg" --length="$PENDING_HEARTBEAT_APALACHE_UNSAFE_LENGTH" PendingDeployHeartbeatComposition.tla 2>&1)"
    pending_rc=$?
    printf '%s\n' "$pending_output" >"$pending_log"
    if [[ $pending_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${pending_invariant}" "$pending_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$pending_log" \
         && grep -q 'The outcome is: Error' "$pending_log"; then
      pass "Apalache pending-deploy control finds ${pending_description} by bound $PENDING_HEARTBEAT_APALACHE_UNSAFE_LENGTH"
    else
      fail "Apalache pending-deploy control did not reproduce ${pending_description} (see $pending_log)"
    fi
  done
  recovery_committee_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/recovery-committee-transition-safe" check --config=MC_RecoveryCommitteeTransitionApalache.cfg --length="$RECOVERY_COMMITTEE_APALACHE_SAFE_LENGTH" RecoveryCommitteeTransition.tla 2>&1)"
  recovery_committee_rc=$?
  printf '%s\n' "$recovery_committee_output" >"$LOG_DIR/ff_apalache_recovery_committee_transition.log"
  if [[ $recovery_committee_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_recovery_committee_transition.log"; then
    pass "Apalache recovery committee-transition invariants through bound $RECOVERY_COMMITTEE_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache recovery committee-transition model failed (see $LOG_DIR/ff_apalache_recovery_committee_transition.log)"
  fi
  for transition_apalache_control in \
      'post_auth_unsafe:Inv_ProspectiveAuthorizationDeferred:same-block post-state authorization' \
      'head_justifications_unsafe:Inv_QueuedRecoveryHasExactContext:head-filtered creator justification' \
      'premature_promotion_unsafe:Inv_FloorValidatorsRegistered:promotion before registration' \
      'head_weights_unsafe:Inv_SynchronyAdmissionMatchesFloor:head-weight synchrony drift' \
      'mismatched_cache_unsafe:Inv_SerializedBondsArePostStateCache:a serialized/replayed bond-cache mismatch' \
      'filtered_sequence_unsafe:Inv_PackagedSequenceUsesUnfilteredLmm:valid-only next-sequence derivation after invalid latest-message evidence' \
      'invalid_registration_unsafe:Inv_InvalidPostStateDoesNotRegister:invalid-block validator registration' \
      'root_admission_unsafe:Inv_ApprovedGenesisIsSoleRoot:parentless ordinary or counterfeit root admission' \
      'sender_key_unsafe:Inv_JustificationKeysMatchCitedSenders:a sender/key mismatch outside the genesis placeholder' \
      'registration_genesis_unsafe:Inv_RegisteredSlotsUseCanonicalGenesis:order-dependent validator genesis seeding' \
      'unregistered_lmm_unsafe:Inv_InvalidUnregisteredSendersHaveNoLmmSlot:unregistered invalid-sender LMM allocation' \
      'nonpositive_slot_unsafe:Inv_OnlyPositivePostStateBondsCreateSlots:non-positive bond slot allocation' \
      'invalid_finality_lmm_unsafe:Inv_InvalidLmmDoesNotContributeToFinality:invalid-LMM certificate contribution' \
      'legacy_backfill_unsafe:Inv_DuplicateApprovedBackfillsLegacyIndex:missing duplicate-approved legacy-index backfill'; do
    IFS=: read -r transition_suffix transition_invariant transition_description <<<"$transition_apalache_control"
    transition_log="$LOG_DIR/ff_apalache_recovery_committee_transition_${transition_suffix}.log"
    transition_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/recovery-committee-transition-${transition_suffix}" check --config="MC_RecoveryCommitteeTransition_${transition_suffix}_Apalache.cfg" --length="$RECOVERY_COMMITTEE_APALACHE_UNSAFE_LENGTH" RecoveryCommitteeTransition.tla 2>&1)"
    transition_rc=$?
    printf '%s\n' "$transition_output" >"$transition_log"
    if [[ $transition_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${transition_invariant}" "$transition_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$transition_log" \
         && grep -q 'The outcome is: Error' "$transition_log"; then
      pass "Apalache recovery committee-transition control finds ${transition_description} by bound $RECOVERY_COMMITTEE_APALACHE_UNSAFE_LENGTH"
    else
      fail "Apalache recovery committee-transition control did not reproduce ${transition_description} (see $transition_log)"
    fi
  done
  objective_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/objective-equivocation-safe" check --config=MC_ObjectiveEquivocationApalache.cfg --length="$OBJECTIVE_EQUIVOCATION_APALACHE_SAFE_LENGTH" ObjectiveEquivocation.tla 2>&1)"
  objective_rc=$?
  printf '%s\n' "$objective_output" >"$LOG_DIR/ff_apalache_objective_equivocation.log"
  if [[ $objective_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_objective_equivocation.log"; then
    pass "Apalache objective-equivocation invariants through bound $OBJECTIVE_EQUIVOCATION_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache objective-equivocation model failed (see $LOG_DIR/ff_apalache_objective_equivocation.log)"
  fi
  for objective_apalache_control in \
      'unary_evidence_unsafe:Inv_GroupByIncarnationBeforeCanonicalization:unary non-canonical evidence' \
      'local_invalid_unsafe:Inv_GroupByIncarnationBeforeCanonicalization:local-invalid-dependent acceptance' \
      'unary_dependency_unsafe:Inv_BothHashesAreDependencies:one-hash dependency closure' \
      'equivocator_votes_unsafe:Inv_ActiveIncarnationEquivocatorCannotVote:active-incarnation equivocator voting eligibility' \
      'unary_fallback_unsafe:Inv_CrossIncarnationPairIsConsistentlyNonSlashable:cross-incarnation unary slash fallback' \
      'volatile_restart_unsafe:Inv_RestartPreservesObjectiveEvidence:volatile evidence loss' \
      'permanent_raw_key_unsafe:Inv_IncarnationTransitionRestoresRawKey:permanent cross-incarnation raw-key exclusion' \
      'first_two_before_incarnation_unsafe:Inv_GroupByIncarnationBeforeCanonicalization:first-two selection before bond-incarnation grouping' \
      'overbroad_unary_suppression_unsafe:Inv_IndependentUnaryFaultAtOtherSequenceRemainsEligible:overbroad raw-offender unary suppression' \
      'block_epoch_incarnation_unsafe:Inv_AdversarialBlockEpochDoesNotDefineBondIncarnation:block-epoch-as-incarnation substitution' \
      'first_observed_unary_unsafe:Inv_UnaryEvidenceUsesDeterministicMinimum:first-observed unary evidence selection' \
      'post_state_authority_unsafe:Inv_SameBlockUnbondUsesCanonicalPreStateAuthority:post-state bond authority divergence' \
      'duplicate_retry_no_repair_unsafe:Inv_DuplicateRetryRepairsEvidenceIndex:missing duplicate-retry evidence repair' \
      'unfiltered_finality_votes_unsafe:Inv_ExactJustificationsUseFilteredFinalityVotes:unfiltered invalid finality votes'; do
    IFS=: read -r objective_suffix objective_invariant objective_description <<<"$objective_apalache_control"
    objective_log="$LOG_DIR/ff_apalache_objective_equivocation_${objective_suffix}.log"
    objective_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/objective-equivocation-${objective_suffix}" check --config="MC_ObjectiveEquivocation_${objective_suffix}_Apalache.cfg" --length="$OBJECTIVE_EQUIVOCATION_APALACHE_UNSAFE_LENGTH" ObjectiveEquivocation.tla 2>&1)"
    objective_rc=$?
    printf '%s\n' "$objective_output" >"$objective_log"
    if [[ $objective_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${objective_invariant}" "$objective_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$objective_log" \
         && grep -q 'The outcome is: Error' "$objective_log"; then
      pass "Apalache objective-equivocation control finds ${objective_description} by bound $OBJECTIVE_EQUIVOCATION_APALACHE_UNSAFE_LENGTH"
    else
      fail "Apalache objective-equivocation control did not reproduce ${objective_description} (see $objective_log)"
    fi
  done
  objective_authorization_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/objective-authorization-safe" check --config=MC_ObjectiveEvidenceAuthorizationApalache.cfg --length="$OBJECTIVE_AUTHORIZATION_APALACHE_SAFE_LENGTH" ObjectiveEvidenceAuthorization.tla 2>&1)"
  objective_authorization_rc=$?
  printf '%s\n' "$objective_authorization_output" >"$LOG_DIR/ff_apalache_objective_authorization.log"
  if [[ $objective_authorization_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_objective_authorization.log"; then
    pass "Apalache objective evidence authorization invariants through bound $OBJECTIVE_AUTHORIZATION_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache objective evidence authorization model failed (see $LOG_DIR/ff_apalache_objective_authorization.log)"
  fi
  for objective_authorization_control in \
      'epoch_after_min_unsafe:Inv_EpochGroupingPrecedesCanonicalization:canonicalization before activation-epoch grouping' \
      'cross_epoch_unsafe:Inv_CrossEpochPairCannotAuthorize:cross-epoch objective authorization' \
      'snapshot_generation_unsafe:Inv_CanonicalAuthorityRoot:stale snapshot bond-generation authority' \
      'snapshot_bond_unsafe:Inv_CanonicalAuthorityRoot:stale snapshot bond authority' \
      'offender_wide_suppression_unsafe:Inv_IndependentUnaryPreserved:offender-wide suppression of an independent unary fault' \
      'pair_only_disabled_unsafe:Inv_EpochGroupingPrecedesCanonicalization:failure to activate on pair-only objective evidence' \
      'predicate_drift_unsafe:Inv_ProposerReceiverParity:proposer and receiver authorization drift'; do
    IFS=: read -r authorization_suffix authorization_invariant authorization_description <<<"$objective_authorization_control"
    authorization_log="$LOG_DIR/ff_apalache_objective_authorization_${authorization_suffix}.log"
    objective_authorization_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/objective-authorization-${authorization_suffix}" check --config="MC_ObjectiveEvidenceAuthorization_${authorization_suffix}_Apalache.cfg" --length="$OBJECTIVE_AUTHORIZATION_APALACHE_UNSAFE_LENGTH" ObjectiveEvidenceAuthorization.tla 2>&1)"
    objective_authorization_rc=$?
    printf '%s\n' "$objective_authorization_output" >"$authorization_log"
    if [[ $objective_authorization_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${authorization_invariant}" "$authorization_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$authorization_log" \
         && grep -q 'The outcome is: Error' "$authorization_log"; then
      pass "Apalache objective-authorization control finds ${authorization_description} by bound $OBJECTIVE_AUTHORIZATION_APALACHE_UNSAFE_LENGTH"
    else
      fail "Apalache objective-authorization control did not reproduce ${authorization_description} (see $authorization_log)"
    fi
  done
  bond_generation_output="$(cd "$TLA_DIR" && timeout "$BOND_GENERATION_APALACHE_TIMEOUT" apalache-mc --out-dir="$apalache_out/bond-generation-safe" check --config=MC_BondGenerationLifecycleApalache.cfg --length="$BOND_GENERATION_APALACHE_SAFE_LENGTH" BondGenerationLifecycle.tla 2>&1)"
  bond_generation_rc=$?
  printf '%s\n' "$bond_generation_output" >"$LOG_DIR/ff_apalache_bond_generation_lifecycle.log"
  if [[ $bond_generation_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_bond_generation_lifecycle.log"; then
    pass "Apalache bond-generation lifecycle invariants through bound $BOND_GENERATION_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache bond-generation lifecycle model failed (see $LOG_DIR/ff_apalache_bond_generation_lifecycle.log)"
  fi
  for generation_apalache_control in \
      'generation_transition_unsafe:Inv_GenerationEqualsSuccessfulBondCount:generation mutation outside a completed fresh bond' \
      'rebond_live_unsafe:Inv_AtMostOneLiveGenerationPerKey:multiple live generations for one validator key' \
      'current_bond_only_slash_unsafe:Inv_CurrentLockedSlashApplies:pending or withdrawing stake escaping a current-generation slash' \
      'stale_slash_unsafe:Inv_StaleSlashIsNoninterfering:stale-generation evidence mutating the current incarnation' \
      'burn_rebond_unsafe:Inv_BurnedGenerationCannotRebond:rebonding a terminally burned validator key' \
      'restore_bonded_unsafe:Inv_RedemptionRestoresExactPreSlashPhase:quarantine redemption collapsing its origin to Bonded' \
      'full_guilty_unsafe:Inv_GuiltyPenaltyIsStrictlyPartial:Guilty applying total confiscation' \
      'wrap_generation_unsafe:Inv_GenerationEqualsSuccessfulBondCount:bounded generation wraparound'; do
    IFS=: read -r generation_suffix generation_invariant generation_description <<<"$generation_apalache_control"
    generation_log="$LOG_DIR/ff_apalache_bond_generation_${generation_suffix}.log"
    generation_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/bond-generation-${generation_suffix}" check --config="MC_BondGenerationLifecycle_${generation_suffix}_Apalache.cfg" --length="$BOND_GENERATION_APALACHE_UNSAFE_LENGTH" BondGenerationLifecycle.tla 2>&1)"
    generation_rc=$?
    printf '%s\n' "$generation_output" >"$generation_log"
    if [[ $generation_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${generation_invariant}" "$generation_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$generation_log" \
         && grep -q 'The outcome is: Error' "$generation_log"; then
      pass "Apalache bond-generation control finds ${generation_description} by bound $BOND_GENERATION_APALACHE_UNSAFE_LENGTH"
    else
      fail "Apalache bond-generation control did not reproduce ${generation_description} (see $generation_log)"
    fi
  done
  causal_projection_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/causal-finality-safe" check --config=MC_CausalFinalityProjectionApalache.cfg --length="$CAUSAL_FINALITY_APALACHE_SAFE_LENGTH" CausalFinalityProjection.tla 2>&1)"
  causal_projection_rc=$?
  printf '%s\n' "$causal_projection_output" >"$LOG_DIR/ff_apalache_causal_finality_projection.log"
  if [[ $causal_projection_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_causal_finality_projection.log"; then
    pass "Apalache causal-finality projection invariants through bound $CAUSAL_FINALITY_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache causal-finality projection model failed (see $LOG_DIR/ff_apalache_causal_finality_projection.log)"
  fi
  for projection_apalache_control in \
      'ambient_unsafe:Inv_AmbientEvidenceCannotChangeCertifiedResult:ambient evidence changing a certified result' \
      'delta_cycle_unsafe:Inv_CandidateDeltaCannotAffectOwnProjection:a candidate consuming its own outgoing evidence delta' \
      'invalid_propagates_unsafe:Inv_InvalidBlocksDoNotPropagateEvidence:invalid context propagating causal evidence' \
      'missing_dependency_unsafe:Inv_DeltaCarriesBothCertifiedDependencies:objective evidence omitting a sibling dependency' \
      'mutate_exact_unsafe:Inv_ExactJustificationsPreserved:filtered votes replacing exact wire justifications' \
      'unfiltered_votes_unsafe:Inv_InvalidAndCausallyEquivocatingVotesExcluded:invalid or causally equivocating latest messages contributing votes'; do
    IFS=: read -r projection_suffix projection_invariant projection_description <<<"$projection_apalache_control"
    projection_log="$LOG_DIR/ff_apalache_causal_projection_${projection_suffix}.log"
    projection_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/causal-finality-${projection_suffix}" check --config="MC_CausalFinalityProjection_${projection_suffix}_Apalache.cfg" --length="$CAUSAL_FINALITY_APALACHE_UNSAFE_LENGTH" CausalFinalityProjection.tla 2>&1)"
    projection_rc=$?
    printf '%s\n' "$projection_output" >"$projection_log"
    if [[ $projection_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${projection_invariant}" "$projection_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$projection_log" \
         && grep -q 'The outcome is: Error' "$projection_log"; then
      pass "Apalache causal-finality control finds ${projection_description} by bound $CAUSAL_FINALITY_APALACHE_UNSAFE_LENGTH"
    else
      fail "Apalache causal-finality control did not reproduce ${projection_description} (see $projection_log)"
    fi
  done
  certified_objective_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/certified-objective-safe" check --config=MC_CertifiedObjectiveEquivocationApalache.cfg --length="$CERTIFIED_OBJECTIVE_APALACHE_SAFE_LENGTH" CertifiedObjectiveEquivocation.tla 2>&1)"
  certified_objective_rc=$?
  printf '%s\n' "$certified_objective_output" >"$LOG_DIR/ff_apalache_certified_objective_equivocation.log"
  if [[ $certified_objective_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_certified_objective_equivocation.log"; then
    pass "Apalache certified objective-equivocation invariants through bound $CERTIFIED_OBJECTIVE_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache certified objective-equivocation model failed (see $LOG_DIR/ff_apalache_certified_objective_equivocation.log)"
  fi
  certified_sequence_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/certified-objective-sequence-boundary" check --config=MC_CertifiedObjectiveEquivocation_sequence_boundary_Apalache.cfg --length="$CERTIFIED_OBJECTIVE_APALACHE_SAFE_LENGTH" CertifiedObjectiveEquivocation.tla 2>&1)"
  certified_sequence_rc=$?
  printf '%s\n' "$certified_sequence_output" >"$LOG_DIR/ff_apalache_certified_objective_sequence_boundary.log"
  if [[ $certified_sequence_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_certified_objective_sequence_boundary.log"; then
    pass "Apalache certified objective-evidence signed-sequence boundary through bound $CERTIFIED_OBJECTIVE_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache certified objective-evidence signed-sequence boundary failed (see $LOG_DIR/ff_apalache_certified_objective_sequence_boundary.log)"
  fi
  for certified_apalache_control in \
      'header_trusted_unsafe:Inv_MetadataCertificatesUseExactParentAuthority:unverified header generation defining evidence identity' \
      'post_state_unsafe:Inv_MetadataCertificatesUseExactParentAuthority:same-block post-state authority defining evidence identity' \
      'duplicate_no_repair_unsafe:Inv_DuplicateRetryRepairsEvidence:duplicate insertion failing to repair durable evidence' \
      'negative_sequence_unsafe:Inv_IneligibleSequenceNeverBecomesEvidence:indexing an ineligible negative sequence as objective evidence' \
      'local_invalid_gate_unsafe:Inv_ReconciledSiblingEvidenceIsComplete:local invalid flags gating objective evidence' \
      'noncanonical_lmm_unsafe:Inv_EquivalentDurableViewsConverge:arrival-dependent equal-sequence latest-message selection'; do
    IFS=: read -r certified_suffix certified_invariant certified_description <<<"$certified_apalache_control"
    certified_log="$LOG_DIR/ff_apalache_certified_objective_${certified_suffix}.log"
    certified_bound="$CERTIFIED_OBJECTIVE_APALACHE_UNSAFE_LENGTH"
    certified_timeout=300
    if [[ "$certified_suffix" == "noncanonical_lmm_unsafe" ]]; then
      certified_bound="$CERTIFIED_OBJECTIVE_APALACHE_NONCANONICAL_LENGTH"
      certified_timeout="$CERTIFIED_OBJECTIVE_APALACHE_NONCANONICAL_TIMEOUT"
    fi
    certified_output="$(cd "$TLA_DIR" && timeout "$certified_timeout" apalache-mc --out-dir="$apalache_out/certified-objective-${certified_suffix}" check --config="MC_CertifiedObjectiveEquivocation_${certified_suffix}_Apalache.cfg" --length="$certified_bound" CertifiedObjectiveEquivocation.tla 2>&1)"
    certified_rc=$?
    printf '%s\n' "$certified_output" >"$certified_log"
    if [[ $certified_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${certified_invariant}" "$certified_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$certified_log" \
         && grep -q 'The outcome is: Error' "$certified_log"; then
      pass "Apalache certified-objective control finds ${certified_description} by bound $certified_bound"
    else
      fail "Apalache certified-objective control did not reproduce ${certified_description} (see $certified_log)"
    fi
  done
  certified_causal_admission_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/certified-causal-admission-safe" check --config=MC_CertifiedCausalAdmissionApalache.cfg --length="$CERTIFIED_CAUSAL_ADMISSION_APALACHE_SAFE_LENGTH" CertifiedCausalAdmission.tla 2>&1)"
  certified_causal_admission_rc=$?
  printf '%s\n' "$certified_causal_admission_output" >"$LOG_DIR/ff_apalache_certified_causal_admission.log"
  if [[ $certified_causal_admission_rc -eq 0 ]] \
       && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_certified_causal_admission.log"; then
    pass "Apalache certified causal-admission invariants through bound $CERTIFIED_CAUSAL_ADMISSION_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache certified causal-admission model failed (see $LOG_DIR/ff_apalache_certified_causal_admission.log)"
  fi
  for causal_admission_control in \
      'rejected_barrier_unsafe:Inv_RejectedWrapperTraversed:stopping causal traversal at a rejected wrapper' \
      'rejected_delta_unsafe:Inv_RejectedDeltaIgnored:propagating a rejected block evidence delta' \
      'proof_context_unsafe:Inv_ProofRootsAreLeafFacts:recursively importing context through proof roots' \
      'per_sequence_unbounded_unsafe:Inv_CanonicalIncarnationBound:retaining one proof per sequence instead of one per validator incarnation' \
      'ambient_tracker_unsafe:Inv_CertifiedContextExact:receiver-local tracker state changing a certified admission context' \
      'partial_dependencies_unsafe:Inv_FullyKnownCandidatesAccepted:certifying before the complete causal evidence dependency closure is available'; do
    IFS=: read -r causal_admission_suffix causal_admission_invariant causal_admission_description <<<"$causal_admission_control"
    causal_admission_log="$LOG_DIR/ff_apalache_certified_causal_admission_${causal_admission_suffix}.log"
    causal_admission_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/certified-causal-admission-${causal_admission_suffix}" check --config="MC_CertifiedCausalAdmission_${causal_admission_suffix}_Apalache.cfg" --length="$CERTIFIED_CAUSAL_ADMISSION_APALACHE_UNSAFE_LENGTH" CertifiedCausalAdmission.tla 2>&1)"
    causal_admission_rc=$?
    printf '%s\n' "$causal_admission_output" >"$causal_admission_log"
    if [[ $causal_admission_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${causal_admission_invariant}" "$causal_admission_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$causal_admission_log" \
         && grep -q 'The outcome is: Error' "$causal_admission_log"; then
      pass "Apalache certified causal-admission control finds ${causal_admission_description} by bound $CERTIFIED_CAUSAL_ADMISSION_APALACHE_UNSAFE_LENGTH"
    else
      fail "Apalache certified causal-admission control did not reproduce ${causal_admission_description} (see $causal_admission_log)"
    fi
  done
  admission_disposition_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/certified-admission-disposition-safe" check --config=MC_CertifiedAdmissionDispositionApalache.cfg --length="$ADMISSION_DISPOSITION_APALACHE_SAFE_LENGTH" CertifiedAdmissionDisposition.tla 2>&1)"
  admission_disposition_rc=$?
  printf '%s\n' "$admission_disposition_output" >"$LOG_DIR/ff_apalache_certified_admission_disposition.log"
  if [[ $admission_disposition_rc -eq 0 ]] \
       && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_certified_admission_disposition.log"; then
    pass "Apalache typed admission-disposition invariants through bound $ADMISSION_DISPOSITION_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache certified admission-disposition model failed (see $LOG_DIR/ff_apalache_certified_admission_disposition.log)"
  fi
  for disposition_control in \
      'summary_unsafe:Inv_AuthenticatedObjectiveCertified:signed objective invalidity losing its authority certificate' \
      'hash_unsafe:Inv_HashMismatchUnattributable:a relay-mutated body framing the signer' \
      'local_fault_unsafe:Inv_LocalFaultHasNoDurableEffects:a local replay fault creating durable evidence'; do
    IFS=: read -r disposition_suffix disposition_invariant disposition_description <<<"$disposition_control"
    disposition_log="$LOG_DIR/ff_apalache_certified_admission_disposition_${disposition_suffix}.log"
    disposition_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/certified-admission-disposition-${disposition_suffix}" check --config="MC_CertifiedAdmissionDisposition_${disposition_suffix}_Apalache.cfg" --length="$ADMISSION_DISPOSITION_APALACHE_UNSAFE_LENGTH" CertifiedAdmissionDisposition.tla 2>&1)"
    disposition_rc=$?
    printf '%s\n' "$disposition_output" >"$disposition_log"
    if [[ $disposition_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${disposition_invariant}" "$disposition_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$disposition_log" \
         && grep -q 'The outcome is: Error' "$disposition_log"; then
      pass "Apalache admission-disposition control finds ${disposition_description} by bound $ADMISSION_DISPOSITION_APALACHE_UNSAFE_LENGTH"
    else
      fail "Apalache admission-disposition control did not reproduce ${disposition_description} (see $disposition_log)"
    fi
  done
  certified_context_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/certified-context-safe" check --config=MC_CertifiedConsensusContextApalache.cfg --length="$CERTIFIED_CONTEXT_APALACHE_SAFE_LENGTH" CertifiedConsensusContext.tla 2>&1)"
  certified_context_rc=$?
  printf '%s\n' "$certified_context_output" >"$LOG_DIR/ff_apalache_certified_context.log"
  if [[ $certified_context_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_certified_context.log"; then
    pass "Apalache certified-context invariants through bound $CERTIFIED_CONTEXT_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache certified-consensus-context refinement failed (see $LOG_DIR/ff_apalache_certified_context.log)"
  fi
  for context_apalache_control in \
      'local_lmm_unsafe:AdmissionClosureAgreement:receiver-local latest messages changing admission' \
      'local_tracker_unsafe:AdmissionClosureAgreement:receiver-local tracker contents changing admission' \
      'local_finalized_unsafe:AdmissionClosureAgreement:receiver-local finalized flags changing admission' \
      'parent_order_unsafe:AdmissionClosureAgreement:parent iteration order changing admission' \
      'candidate_prestate_unsafe:CandidatePrestateAuthorityNoninterference:an unfinalized candidate pre-state changing authority' \
      'snapshot_prefilter_unsafe:ConsensusContextExtensional:local prefiltering changing the certified context' \
      'estimator_refilter_unsafe:EstimatorConsumesOneProjection:the estimator filtering an already certified vote projection' \
      'head_weight_unsafe:EstimatorUsesFrozenAuthority:mutable head weights replacing frozen-floor authority' \
      'local_top_unsafe:EstimatorLcaContextExtensional:receiver-local DAG height changing the estimator LCA' \
      'outside_floor_vote_unsafe:EligibleVotesDescendFromFloor:a vote outside the certified floor entering fork choice' \
      'incomplete_slots_unsafe:CompleteLatestMessageSlots:fork choice proceeding without one slot per active validator' \
      'finalizer_reprojection_unsafe:FinalizerConsumesOneProjection:the finalizer independently reprojecting certified votes' \
      'generation_blind_lmm_unsafe:GenerationScopedVotes:an old or future validator incarnation occupying the wrong vote slot' \
      'stale_finalizer_unsafe:StaleFinalizerCannotCommit:a stale concurrent finalizer appending after its expected head'; do
    IFS=: read -r context_suffix context_invariant context_description <<<"$context_apalache_control"
    context_bound="$CERTIFIED_CONTEXT_APALACHE_UNSAFE_LENGTH"
    if [[ "$context_suffix" == "stale_finalizer_unsafe" ]]; then
      context_bound="$CERTIFIED_CONTEXT_APALACHE_STALE_LENGTH"
    fi
    context_log="$LOG_DIR/ff_apalache_certified_context_${context_suffix}.log"
    context_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/certified-context-${context_suffix}" check --config="MC_CertifiedConsensusContext_${context_suffix}_Apalache.cfg" --length="$context_bound" CertifiedConsensusContext.tla 2>&1)"
    context_rc=$?
    printf '%s\n' "$context_output" >"$context_log"
    if [[ $context_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${context_invariant}" "$context_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$context_log" \
         && grep -q 'The outcome is: Error' "$context_log"; then
      pass "Apalache certified-context control finds ${context_description} by bound $context_bound"
    else
      fail "Apalache certified-context control did not reproduce ${context_description} (see $context_log)"
    fi
  done
  certified_floor_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/certified-floor-safe" check --config=MC_CertifiedFloorCommitmentApalache.cfg --length="$CERTIFIED_FLOOR_APALACHE_SAFE_LENGTH" CertifiedFloorCommitment.tla 2>&1)"
  certified_floor_rc=$?
  printf '%s\n' "$certified_floor_output" >"$LOG_DIR/ff_apalache_certified_floor_commitment.log"
  if [[ $certified_floor_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_certified_floor_commitment.log"; then
    pass "Apalache certified-floor commitment invariants through bound $CERTIFIED_FLOOR_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache certified-floor commitment refinement failed (see $LOG_DIR/ff_apalache_certified_floor_commitment.log)"
  fi
  for certified_floor_apalache_control in \
      'cached_use_unsafe:AcceptedCandidatesPreserveEveryParentFloor:cached certificate bypass of candidate-specific admission' \
      'parent_floor_unsafe:AcceptedCandidatesPreserveEveryParentFloor:historical certificate reuse over a newer parent floor' \
      'context_unsafe:AcceptedCandidatesBindAuthorityContext:missing signed candidate authority context' \
      'receiver_lfb_unsafe:ReceiverLocalFloorDoesNotChangeCompatibility:receiver-local LFB admission'; do
    IFS=: read -r certified_floor_suffix certified_floor_invariant certified_floor_description <<<"$certified_floor_apalache_control"
    certified_floor_log="$LOG_DIR/ff_apalache_certified_floor_${certified_floor_suffix}.log"
    certified_floor_control_length="$CERTIFIED_FLOOR_APALACHE_UNSAFE_LENGTH"
    if [[ "$certified_floor_suffix" == context_unsafe ]]; then
      certified_floor_control_length="$CERTIFIED_FLOOR_APALACHE_CONTEXT_UNSAFE_LENGTH"
    fi
    certified_floor_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/certified-floor-${certified_floor_suffix}" check --config="MC_CertifiedFloorCommitment_${certified_floor_suffix}_Apalache.cfg" --length="$certified_floor_control_length" CertifiedFloorCommitment.tla 2>&1)"
    certified_floor_rc=$?
    printf '%s\n' "$certified_floor_output" >"$certified_floor_log"
    if [[ $certified_floor_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${certified_floor_invariant}" "$certified_floor_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$certified_floor_log" \
         && grep -q 'The outcome is: Error' "$certified_floor_log"; then
      pass "Apalache certified-floor control finds ${certified_floor_description} by bound $certified_floor_control_length"
    else
      fail "Apalache certified-floor control did not reproduce ${certified_floor_description} (see $certified_floor_log)"
    fi
  done
  witness_carrier_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/witness-equivalent-carrier-safe" check --config=MC_WitnessEquivalentCarrierApalache.cfg --length="$WITNESS_CARRIER_APALACHE_SAFE_LENGTH" --no-deadlock WitnessEquivalentCarrier.tla 2>&1)"
  witness_carrier_rc=$?
  printf '%s\n' "$witness_carrier_output" >"$LOG_DIR/ff_apalache_witness_equivalent_carrier.log"
  if [[ $witness_carrier_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_witness_equivalent_carrier.log"; then
    pass "Apalache semantic witness-carrier invariants through bound $WITNESS_CARRIER_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache witness-equivalent carrier refinement failed (see $LOG_DIR/ff_apalache_witness_equivalent_carrier.log)"
  fi
  for witness_carrier_apalache_control in \
      'exact_digest_unsafe:SemanticCarrierCannotRemainParked:exact local witness identity parking' \
      'floor_only_unsafe:SelectedCarrierHasExactSemanticState:floor-only state substitution' \
      'copy_digest_unsafe:SelectedCarrierDigestIsPaired:block/digest proof splicing' \
      'wake_unsafe:SemanticCarrierCannotRemainParked:missed semantic carrier wakeup'; do
    IFS=: read -r witness_carrier_suffix witness_carrier_invariant witness_carrier_description <<<"$witness_carrier_apalache_control"
    witness_carrier_log="$LOG_DIR/ff_apalache_witness_equivalent_carrier_${witness_carrier_suffix}.log"
    witness_carrier_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/witness-equivalent-carrier-${witness_carrier_suffix}" check --config="MC_WitnessEquivalentCarrier_${witness_carrier_suffix}.cfg" --inv="$witness_carrier_invariant" --length="$WITNESS_CARRIER_APALACHE_UNSAFE_LENGTH" --no-deadlock WitnessEquivalentCarrier.tla 2>&1)"
    witness_carrier_rc=$?
    printf '%s\n' "$witness_carrier_output" >"$witness_carrier_log"
    if [[ $witness_carrier_rc -ne 0 ]] \
         && grep -Fq "Producing verification conditions from the invariant ${witness_carrier_invariant}" "$witness_carrier_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$witness_carrier_log" \
         && grep -q 'The outcome is: Error' "$witness_carrier_log"; then
      pass "Apalache witness-carrier control finds ${witness_carrier_description} by bound $WITNESS_CARRIER_APALACHE_UNSAFE_LENGTH"
    else
      fail "Apalache witness-carrier control did not reproduce ${witness_carrier_description} (see $witness_carrier_log)"
    fi
  done
  certificate_retrieval_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/certificate-retrieval-safe" check --config=MC_FinalizationCertificateRetrievalApalache.cfg --length="$CERTIFICATE_RETRIEVAL_APALACHE_SAFE_LENGTH" --no-deadlock FinalizationCertificateRetrieval.tla 2>&1)"
  certificate_retrieval_rc=$?
  printf '%s\n' "$certificate_retrieval_output" >"$LOG_DIR/ff_apalache_finalization_certificate_retrieval.log"
  if [[ $certificate_retrieval_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_finalization_certificate_retrieval.log"; then
    pass "Apalache typed certificate-retrieval invariants through bound $CERTIFICATE_RETRIEVAL_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache finalization-certificate retrieval refinement failed (see $LOG_DIR/ff_apalache_finalization_certificate_retrieval.log)"
  fi
  for certificate_retrieval_apalache_control in \
      'untyped_unsafe:TypedDependencyNamespaceIsDisjoint:block/certificate namespace aliasing' \
      'validation_unsafe:OnlyValidResponsesPersist:invalid response persistence' \
      'unsolicited_unsafe:UnsolicitedResponsesDoNotMutate:unsolicited response mutation' \
      'failed_send_unsafe:FailedSendsRetainObligations:failed-send obligation loss' \
      'restart_unsafe:RestartNeverStrandsPersistentObligations:restart obligation loss' \
      'duplicate_wake_unsafe:EveryBlockIsQueuedAtMostOnce:duplicate queue wakeup'; do
    IFS=: read -r certificate_retrieval_suffix certificate_retrieval_invariant certificate_retrieval_description <<<"$certificate_retrieval_apalache_control"
    certificate_retrieval_log="$LOG_DIR/ff_apalache_finalization_certificate_retrieval_${certificate_retrieval_suffix}.log"
    certificate_retrieval_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/certificate-retrieval-${certificate_retrieval_suffix}" check --config="MC_FinalizationCertificateRetrieval_${certificate_retrieval_suffix}.cfg" --length="$CERTIFICATE_RETRIEVAL_APALACHE_UNSAFE_LENGTH" --no-deadlock FinalizationCertificateRetrieval.tla 2>&1)"
    certificate_retrieval_rc=$?
    printf '%s\n' "$certificate_retrieval_output" >"$certificate_retrieval_log"
    if [[ $certificate_retrieval_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${certificate_retrieval_invariant}" "$certificate_retrieval_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$certificate_retrieval_log" \
         && grep -q 'The outcome is: Error' "$certificate_retrieval_log"; then
      pass "Apalache certificate-retrieval control finds ${certificate_retrieval_description} by bound $CERTIFICATE_RETRIEVAL_APALACHE_UNSAFE_LENGTH"
    else
      fail "Apalache certificate-retrieval control did not reproduce ${certificate_retrieval_description} (see $certificate_retrieval_log)"
    fi
  done
  dependency_maintenance_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/dependency-maintenance-safe" check --config=MC_DependencyMaintenanceRoundApalache.cfg --length="$DEPENDENCY_MAINTENANCE_APALACHE_SAFE_LENGTH" --no-deadlock DependencyMaintenanceRound.tla 2>&1)"
  dependency_maintenance_rc=$?
  printf '%s\n' "$dependency_maintenance_output" >"$LOG_DIR/ff_apalache_dependency_maintenance_round.log"
  if [[ $dependency_maintenance_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_dependency_maintenance_round.log"; then
    pass "Apalache mixed dependency-maintenance invariants through bound $DEPENDENCY_MAINTENANCE_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache dependency-maintenance refinement failed (see $LOG_DIR/ff_apalache_dependency_maintenance_round.log)"
  fi
  dependency_maintenance_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/dependency-maintenance-abort-unsafe" check --config=MC_DependencyMaintenanceRound_abort_unsafe.cfg --length="$DEPENDENCY_MAINTENANCE_APALACHE_UNSAFE_LENGTH" --no-deadlock DependencyMaintenanceRound.tla 2>&1)"
  dependency_maintenance_rc=$?
  printf '%s\n' "$dependency_maintenance_output" >"$LOG_DIR/ff_apalache_dependency_maintenance_round_abort_unsafe.log"
  if [[ $dependency_maintenance_rc -ne 0 ]] \
       && grep -Fq 'Using inv predicate(s) FailureNeverDiscardsUnattemptedObligations' "$LOG_DIR/ff_apalache_dependency_maintenance_round_abort_unsafe.log" \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_dependency_maintenance_round_abort_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_dependency_maintenance_round_abort_unsafe.log"; then
    pass "Apalache abort-on-first-failure control finds caller-level dependency starvation by bound $DEPENDENCY_MAINTENANCE_APALACHE_UNSAFE_LENGTH"
  else
    fail "Apalache abort-on-first-failure maintenance control did not reproduce dependency starvation (see $LOG_DIR/ff_apalache_dependency_maintenance_round_abort_unsafe.log)"
  fi
  certified_snapshot_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/certified-snapshot-safe" check --config=MC_CertifiedSnapshotCaptureApalache.cfg --length="$CERTIFIED_SNAPSHOT_APALACHE_SAFE_LENGTH" CertifiedSnapshotCapture.tla 2>&1)"
  certified_snapshot_rc=$?
  printf '%s\n' "$certified_snapshot_output" >"$LOG_DIR/ff_apalache_certified_snapshot_capture.log"
  if [[ $certified_snapshot_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_certified_snapshot_capture.log"; then
    pass "Apalache coherent concurrent snapshot capture through bound $CERTIFIED_SNAPSHOT_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache certified snapshot-capture refinement failed (see $LOG_DIR/ff_apalache_certified_snapshot_capture.log)"
  fi
  certified_snapshot_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/certified-snapshot-torn-unsafe" check --config=MC_CertifiedSnapshotCapture_torn_unsafe_Apalache.cfg --length="$CERTIFIED_SNAPSHOT_APALACHE_UNSAFE_LENGTH" CertifiedSnapshotCapture.tla 2>&1)"
  certified_snapshot_rc=$?
  printf '%s\n' "$certified_snapshot_output" >"$LOG_DIR/ff_apalache_certified_snapshot_capture_torn_unsafe.log"
  if [[ $certified_snapshot_rc -ne 0 ]] \
       && grep -Fq 'Using inv predicate(s) CompletedSnapshotsBindOneRevision' "$LOG_DIR/ff_apalache_certified_snapshot_capture_torn_unsafe.log" \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_certified_snapshot_capture_torn_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_certified_snapshot_capture_torn_unsafe.log"; then
    pass "Apalache torn snapshot control finds a mixed durable DAG/floor/certificate revision by bound $CERTIFIED_SNAPSHOT_APALACHE_UNSAFE_LENGTH"
  else
    fail "Apalache torn snapshot control did not reproduce revision incoherence (see $LOG_DIR/ff_apalache_certified_snapshot_capture_torn_unsafe.log)"
  fi
  protocol_v5_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/protocol-v5-safe" check --config=MC_ProtocolV5EndToEndApalache.cfg --length="$PROTOCOL_V5_APALACHE_SAFE_LENGTH" ProtocolV5EndToEnd.tla 2>&1)"
  protocol_v5_rc=$?
  printf '%s\n' "$protocol_v5_output" >"$LOG_DIR/ff_apalache_protocol_v5_end_to_end.log"
  if [[ $protocol_v5_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_protocol_v5_end_to_end.log"; then
    pass "Apalache composed protocol-v5 invariants through bound $PROTOCOL_V5_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache composed protocol-v5 refinement failed (see $LOG_DIR/ff_apalache_protocol_v5_end_to_end.log)"
  fi
  for protocol_v5_apalache_control in \
      'post_state_certificate_unsafe:CertificatesUseExactProposalPreState:6:post-state certification' \
      'intrinsic_admission_unsafe:IntrinsicAdmissionOnly:3:intrinsic-admission bypass' \
      'order_dependent_evidence_unsafe:StableEvidenceIsGenerationAwareAndOrderIndependent:7:arrival-order evidence' \
      'generation_blind_evidence_unsafe:StableEvidenceIsGenerationAwareAndOrderIndependent:10:cross-incarnation evidence' \
      'head_committee_unsafe:FinalizationUsesFrozenFloorCommittee:13:mutable-head finality authority' \
      'unfiltered_finality_unsafe:ObjectiveEquivocatorsDoNotContributeFinalityVotes:18:unfiltered objective-equivocator voting' \
      'retry_without_repair_unsafe:CompletedRetryRepairsDurableEvidence:9:duplicate retry without index repair' \
      'generation_blind_slash_unsafe:SlashTargetsCurrentBondGeneration:11:stale-generation slashing' \
      'restore_bonded_unsafe:RedemptionRestoresExactLifecycle:10:lifecycle-collapsing redemption' \
      'lost_receipt_unsafe:ResolutionRetriesAreIdempotent:10:lost resolution receipt' \
      'replay_drift_unsafe:ReplayMatchesCanonicalCost:4:replica-local replay drift' \
      'split_settlement_unsafe:SettlementChargesExactlyReplayCost:6:split replay/settlement accounting'; do
    IFS=: read -r protocol_v5_suffix protocol_v5_invariant protocol_v5_bound protocol_v5_description <<<"$protocol_v5_apalache_control"
    protocol_v5_log="$LOG_DIR/ff_apalache_protocol_v5_${protocol_v5_suffix}.log"
    protocol_v5_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/protocol-v5-${protocol_v5_suffix}" check --config="MC_ProtocolV5EndToEnd_${protocol_v5_suffix}_Apalache.cfg" --length="$protocol_v5_bound" ProtocolV5EndToEnd.tla 2>&1)"
    protocol_v5_rc=$?
    printf '%s\n' "$protocol_v5_output" >"$protocol_v5_log"
    if [[ $protocol_v5_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${protocol_v5_invariant}" "$protocol_v5_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$protocol_v5_log" \
         && grep -q 'The outcome is: Error' "$protocol_v5_log"; then
      pass "Apalache protocol-v5 control finds ${protocol_v5_description} by bound $protocol_v5_bound"
    else
      fail "Apalache protocol-v5 control did not reproduce ${protocol_v5_description} (see $protocol_v5_log)"
    fi
  done
  proposer_coalescing_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/proposer-coalescing-safe" check --config=MC_ProposerAdmissionCoalescingApalache.cfg --length="$PROPOSER_COALESCING_APALACHE_SAFE_LENGTH" ProposerAdmissionCoalescing.tla 2>&1)"
  proposer_coalescing_rc=$?
  printf '%s\n' "$proposer_coalescing_output" >"$LOG_DIR/ff_apalache_proposer_coalescing.log"
  if [[ $proposer_coalescing_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_proposer_coalescing.log"; then
    pass "Apalache proposer-admission coalescing invariants through bound $PROPOSER_COALESCING_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache proposer-admission coalescing model failed (see $LOG_DIR/ff_apalache_proposer_coalescing.log)"
  fi
  for proposer_apalache_control in \
      'ambient_async_empty_unsafe:Inv_EmptyAuthorityIsRecoveryOnly:ambient empty-block authority' \
      'lost_pending_wake_unsafe:Inv_PendingWakeLatched:a lost pending-deploy wake' \
      'stale_recovery_permit_unsafe:Inv_StaleRecoveryPermitRejected:a stale recovery permit'; do
    IFS=: read -r proposer_suffix proposer_invariant proposer_description <<<"$proposer_apalache_control"
    proposer_log="$LOG_DIR/ff_apalache_proposer_coalescing_${proposer_suffix}.log"
    proposer_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/proposer-coalescing-${proposer_suffix}" check --config="MC_ProposerAdmissionCoalescing_${proposer_suffix}_Apalache.cfg" --length="$PROPOSER_COALESCING_APALACHE_UNSAFE_LENGTH" ProposerAdmissionCoalescing.tla 2>&1)"
    proposer_rc=$?
    printf '%s\n' "$proposer_output" >"$proposer_log"
    if [[ $proposer_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${proposer_invariant}" "$proposer_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$proposer_log" \
         && grep -q 'The outcome is: Error' "$proposer_log"; then
      pass "Apalache proposer-admission control finds ${proposer_description} by bound $PROPOSER_COALESCING_APALACHE_UNSAFE_LENGTH"
    else
      fail "Apalache proposer-admission control did not reproduce ${proposer_description} (see $proposer_log)"
    fi
  done
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
  fork_choice_types_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/state-preserving-fork-choice-types" check --config=MC_StatePreservingForkChoice_types_Apalache.cfg --length=3 StatePreservingForkChoice.tla 2>&1)"
  fork_choice_types_rc=$?
  printf '%s\n' "$fork_choice_types_output" >"$LOG_DIR/ff_apalache_state_preserving_fork_choice_types.log"
  if [[ $fork_choice_types_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_state_preserving_fork_choice_types.log"; then
    pass "Apalache node-local causal-parent representation invariants through bound 3"
  else
    fail "Apalache causal-parent representation model failed (see $LOG_DIR/ff_apalache_state_preserving_fork_choice_types.log)"
  fi
  fork_choice_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/state-preserving-fork-choice-safe" check --config=MC_StatePreservingForkChoiceApalache.cfg --length=4 StatePreservingForkChoice.tla 2>&1)"
  fork_choice_rc=$?
  printf '%s\n' "$fork_choice_output" >"$LOG_DIR/ff_apalache_state_preserving_fork_choice.log"
  if [[ $fork_choice_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_state_preserving_fork_choice.log"; then
    pass "Apalache node-local causal-parent projection invariants through a four-transition proposal projection"
  else
    fail "Apalache causal-parent projection model failed (see $LOG_DIR/ff_apalache_state_preserving_fork_choice.log)"
  fi
  fork_choice_evidence_output="$(cd "$TLA_DIR" && timeout 600 apalache-mc --out-dir="$apalache_out/state-preserving-fork-choice-evidence" check --config=MC_StatePreservingForkChoice_evidence_Apalache.cfg --length=4 StatePreservingForkChoice.tla 2>&1)"
  fork_choice_evidence_rc=$?
  printf '%s\n' "$fork_choice_evidence_output" >"$LOG_DIR/ff_apalache_state_preserving_fork_choice_evidence.log"
  if [[ $fork_choice_evidence_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_state_preserving_fork_choice_evidence.log"; then
    pass "Apalache evidence-root, floor-rebase, funding, and recovery invariants through a four-transition proposal projection"
  else
    fail "Apalache fork-choice evidence/state model failed (see $LOG_DIR/ff_apalache_state_preserving_fork_choice_evidence.log)"
  fi
  fork_choice_unsafe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/state-preserving-fork-choice-unsafe" check --config=MC_StatePreservingForkChoiceUnsafeApalache.cfg --length=4 StatePreservingForkChoice.tla 2>&1)"
  fork_choice_unsafe_rc=$?
  printf '%s\n' "$fork_choice_unsafe_output" >"$LOG_DIR/ff_apalache_state_preserving_fork_choice_unsafe.log"
  if [[ $fork_choice_unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_state_preserving_fork_choice_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_state_preserving_fork_choice_unsafe.log"; then
    pass "Apalache floor-unprotected replay control finds finalized-effect loss"
  else
    fail "Apalache floor-unprotected replay control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_state_preserving_fork_choice_unsafe.log)"
  fi
  fork_choice_vote_parent_unsafe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/state-preserving-fork-choice-vote-parent-unsafe" check --config=MC_StatePreservingForkChoice_parent_uses_votes_unsafe_Apalache.cfg --length=4 StatePreservingForkChoice.tla 2>&1)"
  fork_choice_vote_parent_unsafe_rc=$?
  printf '%s\n' "$fork_choice_vote_parent_unsafe_output" >"$LOG_DIR/ff_apalache_state_preserving_fork_choice_vote_parent_unsafe.log"
  if [[ $fork_choice_vote_parent_unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_state_preserving_fork_choice_vote_parent_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_state_preserving_fork_choice_vote_parent_unsafe.log"; then
    pass "Apalache vote-projection parent control finds accepted stale-sibling loss"
  else
    fail "Apalache vote-projection parent control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_state_preserving_fork_choice_vote_parent_unsafe.log)"
  fi
  fork_choice_invalid_stale_unsafe_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/state-preserving-fork-choice-invalid-stale-unsafe" check --config=MC_StatePreservingForkChoice_invalid_stale_unsafe_Apalache.cfg --length=4 StatePreservingForkChoice.tla 2>&1)"
  fork_choice_invalid_stale_unsafe_rc=$?
  printf '%s\n' "$fork_choice_invalid_stale_unsafe_output" >"$LOG_DIR/ff_apalache_state_preserving_fork_choice_invalid_stale_unsafe.log"
  if [[ $fork_choice_invalid_stale_unsafe_rc -ne 0 ]] \
       && grep -qE 'state invariant [0-9]+ violated' "$LOG_DIR/ff_apalache_state_preserving_fork_choice_invalid_stale_unsafe.log" \
       && grep -q 'The outcome is: Error' "$LOG_DIR/ff_apalache_state_preserving_fork_choice_invalid_stale_unsafe.log"; then
    pass "Apalache stale-only parent control finds multiply-invalid tip admission"
  else
    fail "Apalache stale-only parent control did not reproduce the expected counterexample (see $LOG_DIR/ff_apalache_state_preserving_fork_choice_invalid_stale_unsafe.log)"
  fi
  fork_choice_depth_expiry_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/state-preserving-fork-choice-depth-expiry" check --config=MC_StatePreservingForkChoice_depth_expiry_Apalache.cfg --length=4 StatePreservingForkChoice.tla 2>&1)"
  fork_choice_depth_expiry_rc=$?
  printf '%s\n' "$fork_choice_depth_expiry_output" >"$LOG_DIR/ff_apalache_state_preserving_fork_choice_depth_expiry.log"
  if [[ $fork_choice_depth_expiry_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_state_preserving_fork_choice_depth_expiry.log"; then
    pass "Apalache zero-depth causal-expiry safety invariants through a four-transition proposal projection"
  else
    fail "Apalache zero-depth causal-expiry model failed (see $LOG_DIR/ff_apalache_state_preserving_fork_choice_depth_expiry.log)"
  fi
  while IFS='|' read -r suffix label; do
    control_log="$LOG_DIR/ff_apalache_state_preserving_fork_choice_${suffix}.log"
    control_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/state-preserving-fork-choice-${suffix}" check --config="MC_StatePreservingForkChoice_${suffix}_Apalache.cfg" --length=4 StatePreservingForkChoice.tla 2>&1)"
    control_rc=$?
    printf '%s\n' "$control_output" >"$control_log"
    if [[ $control_rc -ne 0 ]] \
         && grep -qE 'state invariant [0-9]+ violated' "$control_log" \
         && grep -q 'The outcome is: Error' "$control_log"; then
      pass "Apalache $label control reproduces the designated safety violation"
    else
      fail "Apalache $label control did not reproduce the expected counterexample (see $control_log)"
    fi
  done <<'EOF'
deploy_promotion_unsafe|deploy promotion
omit_floor_evidence_unsafe|omitted floor evidence root
skip_antichain_unsafe|uncompacted causal parents
recovery_floor_unsafe|floor-blind recovery narrowing
EOF
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
  finalizer_materialization_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/finalizer-materialization-safe" check --config=MC_FinalizerFloorMaterializationApalache.cfg --length="$FINALIZER_MATERIALIZATION_APALACHE_SAFE_LENGTH" FinalizerFloorMaterialization.tla 2>&1)"
  finalizer_materialization_rc=$?
  printf '%s\n' "$finalizer_materialization_output" >"$LOG_DIR/ff_apalache_finalizer_floor_materialization.log"
  if [[ $finalizer_materialization_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/ff_apalache_finalizer_floor_materialization.log"; then
    pass "Apalache exact finalizer-target materialization invariants through bound $FINALIZER_MATERIALIZATION_APALACHE_SAFE_LENGTH"
  else
    fail "Apalache finalizer-floor materialization model failed (see $LOG_DIR/ff_apalache_finalizer_floor_materialization.log)"
  fi
  for finalizer_materialization_control in \
      'main_parent_unsafe:Inv_FinalizerDiscoversCandidate:main-parent-only secondary-target starvation' \
      'causal_only_unsafe:Inv_SelectedTargetBindsRequestedCertificate:causal-only rejected-sibling target substitution'; do
    IFS=: read -r materialization_suffix materialization_invariant materialization_description <<<"$finalizer_materialization_control"
    materialization_log="$LOG_DIR/ff_apalache_finalizer_floor_materialization_${materialization_suffix}.log"
    finalizer_materialization_output="$(cd "$TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/finalizer-materialization-${materialization_suffix}" check --config="MC_FinalizerFloorMaterialization_${materialization_suffix}_Apalache.cfg" --length="$FINALIZER_MATERIALIZATION_APALACHE_UNSAFE_LENGTH" FinalizerFloorMaterialization.tla 2>&1)"
    finalizer_materialization_rc=$?
    printf '%s\n' "$finalizer_materialization_output" >"$materialization_log"
    if [[ $finalizer_materialization_rc -ne 0 ]] \
         && grep -Fq "Using inv predicate(s) ${materialization_invariant}" "$materialization_log" \
         && grep -qE 'state invariant [0-9]+ violated' "$materialization_log" \
         && grep -q 'The outcome is: Error' "$materialization_log"; then
      pass "Apalache finalizer-materialization control finds ${materialization_description} by bound $FINALIZER_MATERIALIZATION_APALACHE_UNSAFE_LENGTH"
    else
      fail "Apalache finalizer-materialization control did not reproduce ${materialization_description} (see $materialization_log)"
    fi
  done
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

echo "== [5/8] Wolfram (optional, fail-soft) =="
if [[ "${RUN_WOLFRAM:-0}" != "1" ]]; then
  skip "licensed Wolfram exploration tier is opt-in; set RUN_WOLFRAM=1 to run it"
else
  WL_BIN=""; WL_RUN=()
  if command -v wolframscript >/dev/null 2>&1; then WL_BIN=wolframscript; WL_RUN=(wolframscript -file)
  elif command -v math >/dev/null 2>&1;       then WL_BIN=math;          WL_RUN=(math -script)
  elif command -v wolfram >/dev/null 2>&1;    then WL_BIN=wolfram;       WL_RUN=(wolfram -script)
  fi
  if [[ -z "$WL_BIN" ]]; then
    skip "no wolframscript/math/wolfram kernel on PATH"
  elif [[ ! -f "$WL_DIR/delta_ratchet.wl" || ! -f "$WL_DIR/weighted_quorum_regions.wl" || ! -f "$WL_DIR/repair_design_regions.wl" ]]; then
    fail "selected Wolfram exploration tier is missing a required model"
  else
    wolfram_ok=1
    : >"$LOG_DIR/ff_wolfram.log"
    for wolfram_model in delta_ratchet weighted_quorum_regions repair_design_regions; do
      wlout=$(env \
        WOLFRAM_BASE="${WOLFRAM_BASE:-/usr/share/Wolfram}" \
        WOLFRAM_LOCALBASE="${WOLFRAM_LOCALBASE:-${HOME}/.Wolfram/Objects}" \
        WOLFRAM_USERBASE="${WOLFRAM_USERBASE:-${HOME}/.Wolfram}" \
        "${WL_RUN[@]}" "$WL_DIR/${wolfram_model}.wl" 2>&1); wlrc=$?
      printf '%s\n' "$wlout" >>"$LOG_DIR/ff_wolfram.log"
      if grep -qiE 'no valid password|cannot find a valid password' <<<"$wlout"; then
        fail "Wolfram CLI kernel ($WL_BIN) could not bind its configured license (details: $LOG_DIR/ff_wolfram.log)"
        wolfram_ok=0
        break
      elif [[ $wlrc -ne 0 ]] || ! grep -Fq "[$wolfram_model] SELF-TEST: PASS" <<<"$wlout"; then
        fail "Wolfram ${wolfram_model}.wl errored or omitted its PASS marker under $WL_BIN (see $LOG_DIR/ff_wolfram.log)"
        wolfram_ok=0
      fi
    done
    if [[ $wolfram_ok -eq 1 ]]; then
      pass "Wolfram service-rate, exact weighted-quorum, and repair-design exploration via $WL_BIN"
    fi
  fi
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
  if cargo test -p block-storage --lib finalization_ledger >"$LOG_DIR/ff_rust_finalization_ledger.log" 2>&1 \
       && cargo test -p casper --lib finalization_schedule >>"$LOG_DIR/ff_rust_finalization_ledger.log" 2>&1 \
       && cargo test -p casper --lib finalizer_parallelism_rejects_zero_workers >>"$LOG_DIR/ff_rust_finalization_ledger.log" 2>&1 \
       && cargo test -p casper --lib approved_block_ >>"$LOG_DIR/ff_rust_finalization_ledger.log" 2>&1 \
       && test "$(grep -cE 'test result: ok\. [1-9][0-9]* passed' "$LOG_DIR/ff_rust_finalization_ledger.log")" -eq 4; then
    pass "Rust atomic finalization ledger, local-witness validation, divergent local-history identity, genesis-only approval, crash recovery, arbitrary completion order, bounded parallel scheduler, and fail-closed configuration regressions"
  else
    fail "Rust finalization atomicity/recovery regressions failed (see $LOG_DIR/ff_rust_finalization_ledger.log)"; tail -20 "$LOG_DIR/ff_rust_finalization_ledger.log" | sed 's/^/      /'
  fi
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
       && grep -q "dual_certified_universal_floor_is_independent_of_branch_parent_and_validator_order ... ok" "$LOG_DIR/ff_rust_lib.log" \
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
       && cargo test -p casper --test mod -- batch2::finalizer_test::finalizer_recognizes_all_parent_convergence_in_a_reconvergent_dag --exact >>"$LOG_DIR/ff_rust_finalizer_progress.log" 2>&1 \
       && cargo test -p casper --test mod -- batch2::finalizer_test::finalizer_rejects_dag_descendant_without_state_lineage --exact >>"$LOG_DIR/ff_rust_finalizer_progress.log" 2>&1 \
       && cargo test -p casper --test mod -- batch2::finalizer_test::finalizer_advances_to_state_descendant_when_lfb_is_a_secondary_parent --exact >>"$LOG_DIR/ff_rust_finalizer_progress.log" 2>&1 \
       && cargo test -p casper --test mod -- compute_parents_post_state_regression_spec::compute_parents_post_state_fast_paths_only_when_the_cover_preserves_the_floor --exact >>"$LOG_DIR/ff_rust_finalizer_progress.log" 2>&1 \
       && test "$(grep -cE "test result: ok\. 1 passed" "$LOG_DIR/ff_rust_finalizer_progress.log")" -eq 5; then
    pass "Rust complete-scan, all-parent convergence, unchanged-clique/state-preservation, off-main rebase progress, and execution-rebase regressions"
  else
    fail "Rust finalizer progress regressions failed (see $LOG_DIR/ff_rust_finalizer_progress.log)"; tail -20 "$LOG_DIR/ff_rust_finalizer_progress.log" | sed 's/^/      /'
  fi
  proposal_intent_markers=(
    "manual_and_pending_requests_never_authorize_empty_blocks ... ok"
    "recovery_requires_both_fresh_authorization_and_heartbeat_capability ... ok"
    "recovery_leader_fails_closed_without_valid_height_or_committee ... ok"
    "recovery_permit_requires_exact_floor_round_committee_and_local_leader ... ok"
    "recovery_permit_uses_floor_committee_when_head_committee_diverges ... ok"
  )
  if cargo test -p casper proposal_intent_tests --lib >"$LOG_DIR/ff_rust_proposal_intent.log" 2>&1 \
       && all_markers_present "$LOG_DIR/ff_rust_proposal_intent.log" "${proposal_intent_markers[@]}" \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/ff_rust_proposal_intent.log"; then
    pass "Rust proposal-intent markers (empty-block authority, fresh floor permit, floor-bound committee, and fail-closed leader selection)"
  else
    fail "Rust proposal-intent regressions failed (see $LOG_DIR/ff_rust_proposal_intent.log)"; tail -20 "$LOG_DIR/ff_rust_proposal_intent.log" | sed 's/^/      /'
  fi
  recovery_committee_markers=(
    "recovery_validators_ignore_divergent_proposal_committee ... ok"
    "recovery_leader_is_invariant_under_head_committee_drift ... ok"
  )
  if cargo test -p casper protocol_version_tests::recovery_ --lib >"$LOG_DIR/ff_rust_recovery_committee.log" 2>&1 \
       && all_markers_present "$LOG_DIR/ff_rust_recovery_committee.log" "${recovery_committee_markers[@]}" \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/ff_rust_recovery_committee.log"; then
    pass "Rust recovery-committee example/property markers (head-view divergence cannot change floor-bound leadership)"
  else
    fail "Rust recovery-committee regressions failed (see $LOG_DIR/ff_rust_recovery_committee.log)"; tail -20 "$LOG_DIR/ff_rust_recovery_committee.log" | sed 's/^/      /'
  fi
  live_recovery_markers=(
    "stale_validator_should_stay_running_and_request_tips_and_local_finalization ... ok"
    "fresh_validator_should_stay_running_without_approved_block_request ... ok"
  )
  if cargo test -p casper --test mod validator_should_stay_running >"$LOG_DIR/ff_rust_live_recovery.log" 2>&1 \
       && cargo test -p casper --test mod -- batch1::minority_fork_recovery_spec::validator_on_minority_fork_recovers_through_normal_dag_admission --exact >>"$LOG_DIR/ff_rust_live_recovery.log" 2>&1 \
       && all_markers_present "$LOG_DIR/ff_rust_live_recovery.log" "${live_recovery_markers[@]}" \
       && grep -Fq "test batch1::minority_fork_recovery_spec::validator_on_minority_fork_recovers_through_normal_dag_admission ..." "$LOG_DIR/ff_rust_live_recovery.log" \
       && grep -qE "test result: ok\. 1 passed; 0 failed;" "$LOG_DIR/ff_rust_live_recovery.log"; then
    pass "Rust live recovery markers (Running continuity, ordinary multi-peer tip admission, local finalization, and state-preserving proposal resumption)"
  else
    fail "Rust live minority-fork recovery regressions failed (see $LOG_DIR/ff_rust_live_recovery.log)"; tail -20 "$LOG_DIR/ff_rust_live_recovery.log" | sed 's/^/      /'
  fi
  if cargo test -p casper --test mod -- batch2::map_cell_convergence_spec::resolved_asymmetric_frontier_rehomes_excluded_local_deploy --exact >"$LOG_DIR/ff_rust_stale_sibling_recovery.log" 2>&1 \
       && grep -Fq "test batch2::map_cell_convergence_spec::resolved_asymmetric_frontier_rehomes_excluded_local_deploy ... ok" "$LOG_DIR/ff_rust_stale_sibling_recovery.log" \
       && grep -qE "test result: ok\. 1 passed; 0 failed;" "$LOG_DIR/ff_rust_stale_sibling_recovery.log"; then
    pass "Rust exact-frontier stale-sibling lifecycle regression (source tombstone, rejected buffer, elected rehome, and converged final state)"
  else
    fail "Rust stale-sibling lifecycle regression failed (see $LOG_DIR/ff_rust_stale_sibling_recovery.log)"; tail -20 "$LOG_DIR/ff_rust_stale_sibling_recovery.log" | sed 's/^/      /'
  fi
  witness_carrier_markers=(
    "semantically_equivalent_witness_carriers_preserve_the_selected_proof_pair ... ok"
    "carrier_selection_is_permutation_invariant_and_preserves_digest_pairing ... ok"
  )
  if cargo test -p casper carrier --lib >"$LOG_DIR/ff_rust_witness_equivalent_carrier.log" 2>&1 \
       && all_markers_present "$LOG_DIR/ff_rust_witness_equivalent_carrier.log" "${witness_carrier_markers[@]}"; then
    pass "Rust semantic witness-carrier example/property regressions (proof equivalence, deterministic selection, and exact pair binding)"
  else
    fail "Rust witness-equivalent carrier regressions failed (see $LOG_DIR/ff_rust_witness_equivalent_carrier.log)"; tail -20 "$LOG_DIR/ff_rust_witness_equivalent_carrier.log" | sed 's/^/      /'
  fi
  heartbeat_markers=(
    "finality_progress_opens_each_recovery_round_once_and_resets_on_progress ... ok"
    "finality_progress_rejects_out_of_order_completion ... ok"
    "recovery_leader_is_unique_and_permutation_invariant ... ok"
    "recovery_leader_rotation_visits_every_unique_validator ... ok"
    "recovery_leader_rotation_repeats_only_after_a_full_validator_cycle ... ok"
    "recovery_round_cadence_matches_stall_timeout_then_check_interval ... ok"
    "delayed_wakes_replay_missed_recovery_rounds_without_skipping_leaders ... ok"
    "heartbeat_create_returns_none_when_config_disabled ... ok"
    "heartbeat_create_returns_none_when_max_parents_is_one ... ok"
    "heartbeat_create_returns_none_when_check_interval_is_zero ... ok"
    "heartbeat_create_returns_some_when_all_conditions_met ... ok"
    "do_heartbeat_check_triggers_propose_with_pending_deploys ... ok"
    "do_heartbeat_check_triggers_one_recovery_proposal_after_observed_lfb_stall ... ok"
    "do_heartbeat_check_retains_deferred_leader_round_for_retry ... ok"
    "do_heartbeat_check_completes_nonleader_round_without_proposing ... ok"
    "do_heartbeat_check_skips_when_not_bonded ... ok"
    "peer_user_deploy_observation_does_not_authorize_support_proposal ... ok"
    "do_heartbeat_check_proposes_when_storage_has_deploys_but_deploys_in_scope_empty ... ok"
    "do_heartbeat_check_suppresses_empty_frontier_when_unfinalized_width_is_high ... ok"
    "do_heartbeat_check_allows_pending_deploys_under_empty_frontier_pressure ... ok"
    "due_recovery_takes_priority_and_composes_with_pending_deploys ... ok"
    "selected_recovery_round_completes_only_after_started_or_success ... ok"
    "pending_deploy_backstop_has_no_empty_block_authority ... ok"
    "pending_deploy_grace_refresh_requires_started_or_success ... ok"
  )
  if cargo test -p node heartbeat_proposer >"$LOG_DIR/ff_rust_heartbeat.log" 2>&1 \
       && all_markers_present "$LOG_DIR/ff_rust_heartbeat.log" "${heartbeat_markers[@]}" \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/ff_rust_heartbeat.log"; then
    pass "Rust heartbeat recovery markers (observed-LFB reset, separated cadence, delayed wakes, canonical rotation, deferred retry, and exact backpressure boundary)"
  else
    fail "Rust heartbeat/finality recovery regressions failed (see $LOG_DIR/ff_rust_heartbeat.log)"; tail -20 "$LOG_DIR/ff_rust_heartbeat.log" | sed 's/^/      /'
  fi
  proposer_coalescer_markers=(
    "pending_collisions_create_one_forced_follow_up ... ok"
    "manual_and_recovery_collisions_are_not_replayed ... ok"
    "pending_finish_race_never_loses_work ... ok"
    "pending_cancel_race_leaves_no_abandoned_owner ... ok"
    "finite_dirty_epochs_eventually_return_to_idle ... ok"
    "loom_pending_finish_race_never_loses_work ... ok"
    "loom_many_pending_collisions_create_one_follow_up ... ok"
    "loom_manual_and_recovery_collisions_do_not_dirty ... ok"
    "loom_all_observed_states_are_legal ... ok"
    "loom_finite_work_eventually_returns_idle ... ok"
    "loom_pending_cancel_race_leaves_no_abandoned_owner ... ok"
  )
  if cargo test -p node proposer_coalescer --lib >"$LOG_DIR/ff_rust_proposer_coalescer.log" 2>&1 \
       && all_markers_present "$LOG_DIR/ff_rust_proposer_coalescer.log" "${proposer_coalescer_markers[@]}" \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/ff_rust_proposer_coalescer.log"; then
    pass "Rust/Loom proposer coalescer markers (single dirty epoch, no lost wake, no duplicated manual/recovery request, and eventual idle)"
  else
    fail "Rust/Loom proposer coalescer regressions failed (see $LOG_DIR/ff_rust_proposer_coalescer.log)"; tail -20 "$LOG_DIR/ff_rust_proposer_coalescer.log" | sed 's/^/      /'
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
  for loom_protocol in loom_committee_transition loom_objective_equivocation loom_certified_causal_admission loom_consensus_projection_freeze loom_finalization_atomicity loom_live_minority_fork_recovery; do
    loom_protocol_log="$LOG_DIR/ff_${loom_protocol}.log"
    if env RUSTFLAGS='--cfg loom -C target-cpu=native' LOOM_MAX_PREEMPTIONS=3 \
      cargo test -p cost-accounting-loom-models --test "$loom_protocol" >"$loom_protocol_log" 2>&1; then
      if grep -qE "test result: ok\. [1-9][0-9]* passed" "$loom_protocol_log"; then
        pass "Loom ${loom_protocol#loom_} protocol interleavings"
      else
        fail "Loom $loom_protocol completed without executing tests (see $loom_protocol_log)"
      fi
    else
      fail "Loom $loom_protocol found a protocol interleaving failure (see $loom_protocol_log)"
      tail -20 "$loom_protocol_log" | sed 's/^/      /'
    fi
  done
  if cargo test -p casper --test loom_finalization_carrier_wakeup >"$LOG_DIR/ff_loom_witness_carrier_wakeup.log" 2>&1 \
       && grep -qE "test result: ok\. 3 passed" "$LOG_DIR/ff_loom_witness_carrier_wakeup.log"; then
    pass "Loom semantic witness-carrier wakeup (divergent digest, exact state, and duplicate-admission coalescing)"
  else
    fail "Loom semantic witness-carrier wakeup failed (see $LOG_DIR/ff_loom_witness_carrier_wakeup.log)"
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
