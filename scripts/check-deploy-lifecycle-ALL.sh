#!/usr/bin/env bash
# scripts/check-deploy-lifecycle-ALL.sh
#
# LOCAL-ONLY verification gate for the F1R3FLY deploy lifecycle (block
# admission -> finalization) re-proposal invariant. Runs the formal TLA+ layer
# under the bounded memory envelope of scripts/lib/tlc-run.sh:
#
#   1. TLA+  (fail-soft) — TLC on the POST-fix MC_DeployLifecycle.cfg (must PASS
#      with no violation, exhausting the bounded state space) and TWO PRE-fix
#      regression cfgs, each of which must REPRODUCE its counterexample:
#        * MC_DeployLifecycle_pre_fix.cfg (AdmissionFiltersFinalized=FALSE) ->
#          Inv_NoFinalizedReproposable;
#        * MC_DeployLifecycle_quarantine_pre_fix.cfg (QuarantineBothStores=FALSE)
#          -> Inv_NoToxicReproposable (the §3c proposer-side quarantine that
#          leaves a refund-failing re-proposed deploy lingering re-proposable).
#      SKIPPED if there is no TLC jar ($TLC_JAR) and no `tlc` on PATH.
#
#   2. TLA+ source-aware deploy occurrence convergence, plus a signature-only
#      pre-fix counterexample.
#   3. Rust admission and occurrence-reducer properties.
#
# POLICY: this script is for LOCAL use only. Do NOT wire this into
# .github/workflows/* (or any TLA+ step) — an earlier formal-CI workflow was
# deliberately removed. See the finalized-floor gate (check-finalized-floor-ALL.sh)
# for the sibling policy.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TLA_DIR="$REPO_ROOT/formal/tlaplus/deploy_lifecycle"
OCCURRENCE_TLA_DIR="$REPO_ROOT/formal/tlaplus/deploy_occurrence"
RECOVERY_TLA_DIR="$REPO_ROOT/formal/tlaplus/deploy_recovery"
LOG_DIR="$REPO_ROOT/target/verification/deploy-lifecycle"
mkdir -p "$LOG_DIR"
VERIFY_TMP="$LOG_DIR/tmp"
mkdir -p "$VERIFY_TMP"
export TMPDIR="$VERIFY_TMP"
export TLC_METADIR_ROOT="$VERIFY_TMP/tlc-metadir"
mkdir -p "$TLC_METADIR_ROOT"
trap 'rm -rf "$VERIFY_TMP"' EXIT

rc=0
pass() { printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; rc=1; }
skip() { printf '  \033[33mSKIP\033[0m %s\n' "$1"; }

echo "== [1/4] deploy lifecycle TLA+ (fail-soft) =="
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
if [[ -f "$TLC_JAR" ]] || command -v tlc >/dev/null 2>&1; then
  # shellcheck disable=SC1091
  source "$REPO_ROOT/scripts/lib/tlc-run.sh"
  # POST-fix: must pass (no violation, bounded space exhausted).
  if tlc_run "$(tlc_metadir dl_post_gate)" "$TLA_DIR/MC_DeployLifecycle.cfg" "$TLA_DIR/DeployLifecycle.tla" >"$LOG_DIR/dl_tlc_post.log" 2>&1; then
    pass "TLA+ post-fix Spec, AdmissionFiltersFinalized=TRUE + QuarantineBothStores=TRUE"
    rm -f "$LOG_DIR/dl_tlc_post.log"
  else
    fail "TLA+ post-fix MC_DeployLifecycle.cfg did NOT pass (see $LOG_DIR/dl_tlc_post.log)"
  fi
  # PRE-fix (finalization): must FAIL (counterexample). Inverted sense.
  if tlc_run "$(tlc_metadir dl_pre_gate)" "$TLA_DIR/MC_DeployLifecycle_pre_fix.cfg" "$TLA_DIR/DeployLifecycle.tla" >"$LOG_DIR/dl_tlc_pre.log" 2>&1; then
    fail "TLA+ pre-fix should VIOLATE Inv_NoFinalizedReproposable but passed (the regression demo is broken)"
  else
    if grep -q "Inv_NoFinalizedReproposable is violated" "$LOG_DIR/dl_tlc_pre.log"; then
      pass "TLA+ pre-fix reproduces the finalized-deploy re-proposal counterexample"
      rm -f "$LOG_DIR/dl_tlc_pre.log"
    else
      fail "TLA+ pre-fix failed for the wrong reason (see $LOG_DIR/dl_tlc_pre.log)"
    fi
  fi
  # PRE-fix (quarantine): must FAIL (Inv_NoToxicReproposable counterexample). The
  # §3c proposer-side quarantine with QuarantineBothStores=FALSE leaves a toxic
  # refund-failing deploy lingering re-proposable in rejectedBuf.
  if tlc_run "$(tlc_metadir dl_quar_pre_gate)" "$TLA_DIR/MC_DeployLifecycle_quarantine_pre_fix.cfg" "$TLA_DIR/DeployLifecycle.tla" >"$LOG_DIR/dl_tlc_quar.log" 2>&1; then
    fail "TLA+ quarantine pre-fix should VIOLATE Inv_NoToxicReproposable but passed (the regression demo is broken)"
  else
    if grep -q "Inv_NoToxicReproposable is violated" "$LOG_DIR/dl_tlc_quar.log"; then
      pass "TLA+ quarantine pre-fix reproduces the toxic re-proposal counterexample"
      rm -f "$LOG_DIR/dl_tlc_quar.log"
    else
      fail "TLA+ quarantine pre-fix failed for the wrong reason (see $LOG_DIR/dl_tlc_quar.log)"
    fi
  fi
else
  skip "no TLC jar (\$TLC_JAR) or 'tlc' on PATH"
fi

echo "== [2/4] deploy occurrence TLA+ (fail-soft) =="
if [[ -f "$TLC_JAR" ]] || command -v tlc >/dev/null 2>&1; then
  if tlc_run "$(tlc_metadir occurrence_post_gate)" "$OCCURRENCE_TLA_DIR/MC_DeployOccurrence.cfg" "$OCCURRENCE_TLA_DIR/DeployOccurrence.tla" >"$LOG_DIR/occurrence_tlc_post.log" 2>&1; then
    pass "TLA+ exact occurrence projection preserves one winner and converges"
    rm -f "$LOG_DIR/occurrence_tlc_post.log"
  else
    fail "TLA+ exact occurrence projection did NOT pass (see $LOG_DIR/occurrence_tlc_post.log)"
  fi
  if tlc_run "$(tlc_metadir occurrence_pre_gate)" "$OCCURRENCE_TLA_DIR/MC_DeployOccurrence_sig_only_pre_fix.cfg" "$OCCURRENCE_TLA_DIR/DeployOccurrence.tla" >"$LOG_DIR/occurrence_tlc_pre.log" 2>&1; then
    fail "TLA+ signature-only pre-fix should VIOLATE Inv_OneWinnerPreserved but passed"
  elif grep -q "Inv_OneWinnerPreserved is violated" "$LOG_DIR/occurrence_tlc_pre.log"; then
    pass "TLA+ signature-only pre-fix reproduces winner loss"
    rm -f "$LOG_DIR/occurrence_tlc_pre.log"
  else
    fail "TLA+ signature-only pre-fix failed for the wrong reason (see $LOG_DIR/occurrence_tlc_pre.log)"
  fi
else
  skip "no TLC jar (\$TLC_JAR) or 'tlc' on PATH"
fi

echo "== [3/4] deploy recovery TLA+ (fail-soft) =="
if [[ -f "$TLC_JAR" ]] || command -v tlc >/dev/null 2>&1; then
  if tlc_run "$(tlc_metadir recovery_post_gate)" "$RECOVERY_TLA_DIR/MC_DeployRecovery.cfg" "$RECOVERY_TLA_DIR/MC_DeployRecovery.tla" >"$LOG_DIR/recovery_tlc_post.log" 2>&1; then
    pass "TLA+ recovery protocol is occurrence-aware, expiry-bounded, per-finalized-view elected, and live"
    rm -f "$LOG_DIR/recovery_tlc_post.log"
  else
    fail "TLA+ recovery protocol did NOT pass (see $LOG_DIR/recovery_tlc_post.log)"
  fi

  recovery_negative_control() {
    local config="$1"
    local expected="$2"
    local label="$3"
    local log="$LOG_DIR/${config}.log"
    if tlc_run "$(tlc_metadir "$config")" "$RECOVERY_TLA_DIR/${config}.cfg" "$RECOVERY_TLA_DIR/MC_DeployRecovery.tla" >"$log" 2>&1; then
      fail "$label should produce a counterexample but passed"
    elif grep -q "$expected" "$log"; then
      pass "$label reproduces its counterexample"
      rm -f "$log"
    else
      fail "$label failed for the wrong reason (see $log)"
    fi
  }

  recovery_negative_control \
    MC_DeployRecovery_signature_pre_fix \
    "Inv_RetryRequiresNoActiveSource is violated" \
    "signature-wide retry authorization"
  recovery_negative_control \
    MC_DeployRecovery_expiry_pre_fix \
    "Inv_NoExpiredRetry is violated" \
    "recovered-deploy expiry bypass"
  recovery_negative_control \
    MC_DeployRecovery_multi_leader_pre_fix \
    "Inv_OneRecoveryProposerPerFinalizedView is violated" \
    "same-finalized-view retry storm"
  recovery_negative_control \
    MC_DeployRecovery_heartbeat_pre_fix \
    "Temporal properties were violated" \
    "offline recovery-leader heartbeat suppression"
  recovery_negative_control \
    MC_DeployRecovery_packaging_pre_fix \
    "Inv_SelectedRetrySurvivesSelfChainFilter is violated" \
    "selected recovery dropped by self-chain filtering"
  recovery_negative_control \
    MC_DeployRecovery_rehome_pre_fix \
    "Inv_SelectedRehomeSurvivesCandidateFilter is violated" \
    "excluded-branch deploy dropped by raw self-chain filtering"

  if tlc_run "$(tlc_metadir merge_recovery_post_gate)" "$RECOVERY_TLA_DIR/MC_MergeRecoveryCoherence.cfg" "$RECOVERY_TLA_DIR/MC_MergeRecoveryCoherence.tla" >"$LOG_DIR/merge_recovery_tlc_post.log" 2>&1; then
    pass "TLA+ finalized-base receipts, exact tombstones, chain filtering, and effect projection are coherent"
    rm -f "$LOG_DIR/merge_recovery_tlc_post.log"
  else
    fail "TLA+ merge/recovery coherence did NOT pass (see $LOG_DIR/merge_recovery_tlc_post.log)"
  fi

  merge_recovery_negative_control() {
    local config="$1"
    local expected="$2"
    local label="$3"
    local log="$LOG_DIR/${config}.log"
    if tlc_run "$(tlc_metadir "$config")" "$RECOVERY_TLA_DIR/${config}.cfg" "$RECOVERY_TLA_DIR/MC_MergeRecoveryCoherence.tla" >"$log" 2>&1; then
      fail "$label should produce a counterexample but passed"
    elif grep -q "$expected" "$log"; then
      pass "$label reproduces its counterexample"
      rm -f "$log"
    else
      fail "$label failed for the wrong reason (see $log)"
    fi
  }

  merge_recovery_negative_control \
    MC_MergeRecoveryCoherence_base_precedence_unsafe \
    "Inv_AtMostOneEffectPerSignature is violated" \
    "tombstone-masked finalized effect retry"
  merge_recovery_negative_control \
    MC_MergeRecoveryCoherence_tombstone_filter_unsafe \
    "Inv_TombstonedScopeNotApplied is violated" \
    "late exact tombstone filtering"
  merge_recovery_negative_control \
    MC_MergeRecoveryCoherence_base_duplicate_unsafe \
    "Inv_AtMostOneEffectPerSignature is violated" \
    "above-floor-only duplicate adjudication"
  merge_recovery_negative_control \
    MC_MergeRecoveryCoherence_metadata_coverage_unsafe \
    "Inv_TaggedNumberSingleDatum is violated" \
    "missing numeric merge metadata"
  merge_recovery_negative_control \
    MC_MergeRecoveryCoherence_tombstone_authority_unsafe \
    "Inv_InvalidTombstoneCannotErase is violated" \
    "non-causal tombstone authority"
  merge_recovery_negative_control \
    MC_MergeRecoveryCoherence_partial_chain_unsafe \
    "Inv_ChainAtomic is violated" \
    "partial dependent-chain rejection"
  merge_recovery_negative_control \
    MC_MergeRecoveryCoherence_ordinary_retention_unsafe \
    "Inv_StateRecordCoherence is violated" \
    "rejected ordinary-effect retention"
  merge_recovery_negative_control \
    MC_MergeRecoveryCoherence_mergeable_retention_unsafe \
    "Inv_StateRecordCoherence is violated" \
    "rejected mergeable-effect retention"
  merge_recovery_negative_control \
    MC_MergeRecoveryCoherence_effect_identity_unsafe \
    "Inv_EffectIdentityConsistency is violated" \
    "inconsistent repeated causal-effect identity"

  if tlc_run "$(tlc_metadir rejection_reason_post_gate)" "$RECOVERY_TLA_DIR/MC_RejectionReasonConfluence.cfg" "$RECOVERY_TLA_DIR/MC_RejectionReasonConfluence.tla" >"$LOG_DIR/rejection_reason_tlc_post.log" 2>&1; then
    pass "TLA+ concurrent rejection reasons converge under canonical join"
    rm -f "$LOG_DIR/rejection_reason_tlc_post.log"
  else
    fail "TLA+ rejection-reason confluence did NOT pass (see $LOG_DIR/rejection_reason_tlc_post.log)"
  fi

  if tlc_run "$(tlc_metadir rejection_reason_unsafe)" "$RECOVERY_TLA_DIR/MC_RejectionReasonConfluence_last_writer_unsafe.cfg" "$RECOVERY_TLA_DIR/MC_RejectionReasonConfluence.tla" >"$LOG_DIR/rejection_reason_tlc_unsafe.log" 2>&1; then
    fail "last-writer rejection reasons should produce a counterexample but passed"
  elif grep -q "Inv_EqualObservationConverges is violated" "$LOG_DIR/rejection_reason_tlc_unsafe.log"; then
    pass "last-writer rejection reasons reproduce observation-order divergence"
    rm -f "$LOG_DIR/rejection_reason_tlc_unsafe.log"
  else
    fail "last-writer rejection reasons failed for the wrong reason (see $LOG_DIR/rejection_reason_tlc_unsafe.log)"
  fi

  if tlc_run "$(tlc_metadir protocol_activation_post_gate)" "$RECOVERY_TLA_DIR/MC_ProtocolActivationCoherence.cfg" "$RECOVERY_TLA_DIR/MC_ProtocolActivationCoherence.tla" >"$LOG_DIR/protocol_activation_tlc_post.log" 2>&1; then
    pass "TLA+ protocol activation, record encoding, and legacy-floor composition are coherent"
    rm -f "$LOG_DIR/protocol_activation_tlc_post.log"
  else
    fail "TLA+ protocol activation coherence did NOT pass (see $LOG_DIR/protocol_activation_tlc_post.log)"
  fi

  protocol_activation_negative_control() {
    local config="$1"
    local expected="$2"
    local label="$3"
    local log="$LOG_DIR/${config}.log"
    if tlc_run "$(tlc_metadir "$config")" "$RECOVERY_TLA_DIR/${config}.cfg" "$RECOVERY_TLA_DIR/MC_ProtocolActivationCoherence.tla" >"$log" 2>&1; then
      fail "$label should produce a counterexample but passed"
    elif grep -q "$expected" "$log"; then
      pass "$label reproduces its counterexample"
      rm -f "$log"
    else
      fail "$label failed for the wrong reason (see $log)"
    fi
  }

  protocol_activation_negative_control \
    MC_ProtocolActivationCoherence_floor_version_unsafe \
    "Inv_AtMostOneEffectPerSignature is violated" \
    "floor-version-gated finalized receipt"
  protocol_activation_negative_control \
    MC_ProtocolActivationCoherence_mixed_scope_unsafe \
    "Inv_ActiveScopeVersionHomogeneous is violated" \
    "mixed above-floor protocol scope"
  protocol_activation_negative_control \
    MC_ProtocolActivationCoherence_encoding_unsafe \
    "Inv_EncodingMatchesVersion is violated" \
    "protocol-incompatible disposition encoding"

  for config in \
    MC_ProtocolVersionLifecycle \
    MC_ProtocolVersionLifecycle_legacy_rejected \
    MC_ProtocolVersionLifecycle_unsupported_rejected; do
    log="$LOG_DIR/${config}.log"
    if tlc_run "$(tlc_metadir "$config")" "$RECOVERY_TLA_DIR/${config}.cfg" "$RECOVERY_TLA_DIR/${config}.tla" >"$log" 2>&1; then
      pass "TLA+ protocol-version lifecycle ${config#MC_ProtocolVersionLifecycle} is coherent"
      rm -f "$log"
    else
      fail "TLA+ protocol-version lifecycle ${config} did NOT pass (see $log)"
    fi
  done

  protocol_version_negative_control() {
    local config="$1"
    local expected="$2"
    local label="$3"
    local log="$LOG_DIR/${config}.log"
    if tlc_run "$(tlc_metadir "$config")" "$RECOVERY_TLA_DIR/${config}.cfg" "$RECOVERY_TLA_DIR/MC_ProtocolVersionLifecycle.tla" >"$log" 2>&1; then
      fail "$label should produce a counterexample but passed"
    elif grep -q "$expected" "$log"; then
      pass "$label reproduces its counterexample"
      rm -f "$log"
    else
      fail "$label failed for the wrong reason (see $log)"
    fi
  }

  protocol_version_negative_control \
    MC_ProtocolVersionLifecycle_ceremony_unsafe \
    "Inv_CeremonyCandidateCurrent is violated" \
    "stale genesis ceremony protocol"
  protocol_version_negative_control \
    MC_ProtocolVersionLifecycle_adoption_unsafe \
    "Inv_RunningNodesAdoptApproved is violated" \
    "joiner retaining its local protocol"
  protocol_version_negative_control \
    MC_ProtocolVersionLifecycle_proposer_unsafe \
    "Inv_ProposalUsesApprovedVersion is violated" \
    "proposer bypassing the adopted protocol"
  protocol_version_negative_control \
    MC_ProtocolVersionLifecycle_receiver_unsafe \
    "Inv_AllReceiversAccept is violated" \
    "configured-v3 proposer versus approved-v1 receiver disagreement"
  protocol_version_negative_control \
    MC_ProtocolVersionLifecycle_unsupported_unsafe \
    "Inv_ApprovedVersionSupported is violated" \
    "unsupported approved protocol admission"

  if tlc_run "$(tlc_metadir startup_metadata_preflight_post_gate)" "$RECOVERY_TLA_DIR/MC_StartupMetadataPreflight.cfg" "$RECOVERY_TLA_DIR/StartupMetadataPreflight.tla" >"$LOG_DIR/startup_metadata_preflight_post.log" 2>&1; then
    pass "TLA+ startup metadata preflight verifies before Running and supervises asynchronous rejection"
    rm -f "$LOG_DIR/startup_metadata_preflight_post.log"
  else
    fail "TLA+ startup metadata preflight did NOT pass (see $LOG_DIR/startup_metadata_preflight_post.log)"
  fi

  if tlc_run "$(tlc_metadir startup_metadata_preflight_publish_unsafe)" "$RECOVERY_TLA_DIR/MC_StartupMetadataPreflight_publish_unsafe.cfg" "$RECOVERY_TLA_DIR/StartupMetadataPreflight.tla" >"$LOG_DIR/startup_metadata_preflight_publish_unsafe.log" 2>&1; then
    fail "publish-before-verification should violate Inv_RunningImpliesVerified but passed"
  elif grep -q "Inv_RunningImpliesVerified is violated" "$LOG_DIR/startup_metadata_preflight_publish_unsafe.log"; then
    pass "TLA+ publish-before-verification control reproduces observable unverified Running state"
    rm -f "$LOG_DIR/startup_metadata_preflight_publish_unsafe.log"
  else
    fail "publish-before-verification control failed for the wrong reason (see $LOG_DIR/startup_metadata_preflight_publish_unsafe.log)"
  fi

  if tlc_run "$(tlc_metadir startup_metadata_preflight_supervisor_unsafe)" "$RECOVERY_TLA_DIR/MC_StartupMetadataPreflight_supervisor_unsafe.cfg" "$RECOVERY_TLA_DIR/StartupMetadataPreflight.tla" >"$LOG_DIR/startup_metadata_preflight_supervisor_unsafe.log" 2>&1; then
    fail "unsupervised asynchronous rejection should violate termination liveness but passed"
  elif grep -q "Temporal properties were violated" "$LOG_DIR/startup_metadata_preflight_supervisor_unsafe.log"; then
    pass "TLA+ unsupervised rejection control reproduces a live process stranded outside Running"
    rm -f "$LOG_DIR/startup_metadata_preflight_supervisor_unsafe.log"
  else
    fail "unsupervised rejection control failed for the wrong reason (see $LOG_DIR/startup_metadata_preflight_supervisor_unsafe.log)"
  fi

  if tlc_run "$(tlc_metadir approved_state_replay_post_gate)" "$RECOVERY_TLA_DIR/MC_ApprovedStateReplay.cfg" "$RECOVERY_TLA_DIR/MC_ApprovedStateReplay.tla" >"$LOG_DIR/approved_state_replay_post.log" 2>&1; then
    pass "TLA+ approved-state bootstrap replays every historical block from its own consensus data"
    rm -f "$LOG_DIR/approved_state_replay_post.log"
  else
    fail "TLA+ approved-state replay did NOT pass (see $LOG_DIR/approved_state_replay_post.log)"
  fi

  if tlc_run "$(tlc_metadir approved_state_replay_unsafe)" "$RECOVERY_TLA_DIR/MC_ApprovedStateReplay_current_context_unsafe.cfg" "$RECOVERY_TLA_DIR/MC_ApprovedStateReplay.tla" >"$LOG_DIR/approved_state_replay_unsafe.log" 2>&1; then
    fail "current-context historical replay should produce a counterexample but passed"
  elif grep -q "Inv_ReplayUsesConsensusContext is violated" "$LOG_DIR/approved_state_replay_unsafe.log"; then
    pass "TLA+ current-context replay reproduces late-checkpoint root divergence"
    rm -f "$LOG_DIR/approved_state_replay_unsafe.log"
  else
    fail "current-context historical replay failed for the wrong reason (see $LOG_DIR/approved_state_replay_unsafe.log)"
  fi

  if tlc_run "$(tlc_metadir local_validation_recovery_post_gate)" "$RECOVERY_TLA_DIR/MC_LocalValidationRecovery.cfg" "$RECOVERY_TLA_DIR/MC_LocalValidationRecovery.tla" >"$LOG_DIR/local_validation_recovery_post.log" 2>&1; then
    pass "TLA+ local faults defer bounded recovery and keep descendants dependency-gated"
    rm -f "$LOG_DIR/local_validation_recovery_post.log"
  else
    fail "TLA+ local-validation recovery did NOT pass (see $LOG_DIR/local_validation_recovery_post.log)"
  fi

  if tlc_run "$(tlc_metadir local_validation_recovery_unsafe)" "$RECOVERY_TLA_DIR/MC_LocalValidationRecovery_ready_unsafe.cfg" "$RECOVERY_TLA_DIR/MC_LocalValidationRecovery.tla" >"$LOG_DIR/local_validation_recovery_unsafe.log" 2>&1; then
    fail "ready-queue local-fault retention should produce a counterexample but passed"
  elif grep -q "Inv_NoImmediateSelfRequeue is violated" "$LOG_DIR/local_validation_recovery_unsafe.log"; then
    pass "TLA+ ready-queue retention reproduces immediate self-requeue"
    rm -f "$LOG_DIR/local_validation_recovery_unsafe.log"
  else
    fail "ready-queue local-fault retention failed for the wrong reason (see $LOG_DIR/local_validation_recovery_unsafe.log)"
  fi

  if tlc_run "$(tlc_metadir funding_admission_lifecycle_post_gate)" "$RECOVERY_TLA_DIR/MC_FundingAdmissionLifecycle.cfg" "$RECOVERY_TLA_DIR/MC_FundingAdmissionLifecycle.tla" >"$LOG_DIR/funding_admission_lifecycle_post.log" 2>&1; then
    pass "TLA+ funding admission records an immutable terminal decision from proposal pre-state"
    rm -f "$LOG_DIR/funding_admission_lifecycle_post.log"
  else
    fail "TLA+ funding-admission lifecycle did NOT pass (see $LOG_DIR/funding_admission_lifecycle_post.log)"
  fi

  if tlc_run "$(tlc_metadir funding_admission_live_state_unsafe)" "$RECOVERY_TLA_DIR/MC_FundingAdmissionLifecycle_live_state_unsafe.cfg" "$RECOVERY_TLA_DIR/MC_FundingAdmissionLifecycle.tla" >"$LOG_DIR/funding_admission_live_state_unsafe.log" 2>&1; then
    fail "live-state funding revalidation should produce a counterexample but passed"
  elif grep -q "Inv_ValidatorUsesProposalPreState is violated" "$LOG_DIR/funding_admission_live_state_unsafe.log"; then
    pass "TLA+ live-state revalidation reproduces proposer/validator funding disagreement"
    rm -f "$LOG_DIR/funding_admission_live_state_unsafe.log"
  else
    fail "live-state funding revalidation failed for the wrong reason (see $LOG_DIR/funding_admission_live_state_unsafe.log)"
  fi

  if tlc_run "$(tlc_metadir funding_admission_pending_unsafe)" "$RECOVERY_TLA_DIR/MC_FundingAdmissionLifecycle_pending_unsafe.cfg" "$RECOVERY_TLA_DIR/MC_FundingAdmissionLifecycle.tla" >"$LOG_DIR/funding_admission_pending_unsafe.log" 2>&1; then
    fail "unrecorded underfunding should produce a counterexample but passed"
  elif grep -q "Inv_UnderfundedAttemptLeavesPending is violated" "$LOG_DIR/funding_admission_pending_unsafe.log"; then
    pass "TLA+ unrecorded underfunding reproduces an indefinitely pending deploy"
    rm -f "$LOG_DIR/funding_admission_pending_unsafe.log"
  else
    fail "unrecorded underfunding failed for the wrong reason (see $LOG_DIR/funding_admission_pending_unsafe.log)"
  fi

  if tlc_run "$(tlc_metadir admission_effect_alignment_post_gate)" "$RECOVERY_TLA_DIR/MC_AdmissionEffectAlignment.cfg" "$RECOVERY_TLA_DIR/AdmissionEffectAlignment.tla" >"$LOG_DIR/admission_effect_alignment_post.log" 2>&1; then
    pass "TLA+ admission/status records align only with effect-bearing merge metadata and preserve proposal liveness"
    rm -f "$LOG_DIR/admission_effect_alignment_post.log"
  else
    fail "TLA+ admission/effect alignment did NOT pass (see $LOG_DIR/admission_effect_alignment_post.log)"
  fi

  if tlc_run "$(tlc_metadir admission_effect_alignment_unsafe)" "$RECOVERY_TLA_DIR/MC_AdmissionEffectAlignment_status_count_unsafe.cfg" "$RECOVERY_TLA_DIR/AdmissionEffectAlignment.tla" >"$LOG_DIR/admission_effect_alignment_unsafe.log" 2>&1; then
    fail "status-record counting should produce a counterexample but passed"
  elif grep -q "Inv_StatusOnlyRecordCannotBlock is violated" "$LOG_DIR/admission_effect_alignment_unsafe.log"; then
    pass "TLA+ status-record counting reproduces validator proposal failure"
    rm -f "$LOG_DIR/admission_effect_alignment_unsafe.log"
  else
    fail "status-record counting failed for the wrong reason (see $LOG_DIR/admission_effect_alignment_unsafe.log)"
  fi
else
  skip "no TLC jar (\$TLC_JAR) or 'tlc' on PATH"
fi

if command -v apalache-mc >/dev/null 2>&1; then
  apalache_out="$(mktemp -d "$LOG_DIR/apalache-admission-effect.XXXXXX")"
  safe_output="$(cd "$RECOVERY_TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/safe" check --config=MC_AdmissionEffectAlignmentApalache.cfg --length=8 AdmissionEffectAlignment.tla 2>&1)"
  safe_rc=$?
  printf '%s\n' "$safe_output" >"$LOG_DIR/admission_effect_alignment_apalache.log"
  if [[ $safe_rc -eq 0 ]] && grep -qE 'The outcome is: NoError|EXITCODE: OK' "$LOG_DIR/admission_effect_alignment_apalache.log"; then
    pass "Apalache admission/effect alignment remains safe through the complete lifecycle bound"
    rm -f "$LOG_DIR/admission_effect_alignment_apalache.log"
  else
    fail "Apalache admission/effect alignment failed (see $LOG_DIR/admission_effect_alignment_apalache.log)"
  fi

  unsafe_output="$(cd "$RECOVERY_TLA_DIR" && timeout 300 apalache-mc --out-dir="$apalache_out/unsafe" check --config=MC_AdmissionEffectAlignmentUnsafeApalache.cfg --length=2 AdmissionEffectAlignment.tla 2>&1)"
  unsafe_rc=$?
  printf '%s\n' "$unsafe_output" >"$LOG_DIR/admission_effect_alignment_unsafe_apalache.log"
  if [[ $unsafe_rc -ne 0 ]] && grep -q 'state invariant 1 violated' "$LOG_DIR/admission_effect_alignment_unsafe_apalache.log" && grep -q 'The outcome is: Error' "$LOG_DIR/admission_effect_alignment_unsafe_apalache.log"; then
    pass "Apalache status-record counting reproduces validator proposal failure"
    rm -f "$LOG_DIR/admission_effect_alignment_unsafe_apalache.log"
  else
    fail "Apalache status-record negative control failed for the wrong reason (see $LOG_DIR/admission_effect_alignment_unsafe_apalache.log)"
  fi
  rm -rf "$apalache_out"
else
  skip "no apalache-mc on PATH"
fi

echo "== [4/4] Rust admission and occurrence units (fail-soft) =="
# The DL-1 deploy-lifecycle invariant (no finalized deploy stays re-proposable) is NOT
# enforced by a finalization-time rejected-deploy-buffer purge: that purge was re-derived
# and MEASURED harmful during the 2026-07-15 dev merge (it evicts keep-one losers before
# recovery — see DR-33 in cost-accounting-decision-records.md) and is deliberately
# absent. The hazard is handled at ADMISSION instead: block_creator / `canonical_won_sigs`
# drop already-canonical sigs when a deploy lands, pinned by the
# `interpreter_util::backstop_tests` recovery-admission suite (the TLA+ layer above proves
# the invariant; this proves the Rust realization enforces it). SKIPPED if cargo is absent;
# a failure fails the gate.
if command -v cargo >/dev/null 2>&1; then
  if cargo test -p casper --lib interpreter_util::backstop_tests >"$LOG_DIR/dl_rust_admission.log" 2>&1 \
       && cargo test -p casper --lib deploy_finalization_status::tests >>"$LOG_DIR/dl_rust_admission.log" 2>&1 \
       && cargo test -p casper --lib self_chain_filter_keeps_only_selected_recoveries >>"$LOG_DIR/dl_rust_admission.log" 2>&1 \
       && cargo test -p casper local_validation_fault_recovery >>"$LOG_DIR/dl_rust_admission.log" 2>&1 \
       && cargo test -p casper descendant_remains_blocked_after_locally_faulted_parent_leaves_ready_queue >>"$LOG_DIR/dl_rust_admission.log" 2>&1 \
       && cargo test -p casper --test mod physical_rejection_rolls_back_before_later_state_bound_execution >>"$LOG_DIR/dl_rust_admission.log" 2>&1 \
       && cargo test -p casper --test mod repeat_deploy_validation_rejects_duplicate_signatures_within_one_block >>"$LOG_DIR/dl_rust_admission.log" 2>&1 \
       && cargo test -p casper --test mod source_aware_rejection_in_secondary_parent_is_authoritative >>"$LOG_DIR/dl_rust_admission.log" 2>&1 \
       && cargo test -p models funding_admission_rejection_roundtrips_as_terminal_non_execution >>"$LOG_DIR/dl_rust_admission.log" 2>&1 \
       && cargo test -p casper --lib rust::merging::block_index::tests >>"$LOG_DIR/dl_rust_admission.log" 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/dl_rust_admission.log"; then
    pass "Rust admission and source-aware occurrence reducer units"
    rm -f "$LOG_DIR/dl_rust_admission.log"
  else
    fail "Rust admission-drop unit failed (see $LOG_DIR/dl_rust_admission.log)"; tail -20 "$LOG_DIR/dl_rust_admission.log" | sed 's/^/      /'
  fi
else
  skip "no cargo on PATH"
fi

echo
if [[ $rc -eq 0 ]]; then
  printf '\033[32m== deploy-lifecycle verification: ALL GATES OK ==\033[0m\n'
else
  printf '\033[31m== deploy-lifecycle verification: FAILURES ABOVE ==\033[0m\n'
fi
exit $rc
