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
LOG_DIR="$REPO_ROOT/target/verification/deploy-lifecycle"
mkdir -p "$LOG_DIR"

rc=0
pass() { printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; rc=1; }
skip() { printf '  \033[33mSKIP\033[0m %s\n' "$1"; }

echo "== [1/3] deploy lifecycle TLA+ (fail-soft) =="
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

echo "== [2/3] deploy occurrence TLA+ (fail-soft) =="
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

echo "== [3/3] Rust admission and occurrence units (fail-soft) =="
# The DL-1 deploy-lifecycle invariant (no finalized deploy stays re-proposable) is NOT
# enforced by a finalization-time rejected-deploy-buffer purge: that purge was re-derived
# and MEASURED harmful during the 2026-07-15 dev merge (it evicts keep-one losers before
# recovery — see the "DO NOT re-add" note atop finalization_runner.rs) and is deliberately
# absent. The hazard is handled at ADMISSION instead: block_creator / `canonical_won_sigs`
# drop already-canonical sigs when a deploy lands, pinned by the
# `interpreter_util::backstop_tests` recovery-admission suite (the TLA+ layer above proves
# the invariant; this proves the Rust realization enforces it). SKIPPED if cargo is absent;
# a failure fails the gate.
if command -v cargo >/dev/null 2>&1; then
  if cargo test -p casper --lib interpreter_util::backstop_tests >"$LOG_DIR/dl_rust_admission.log" 2>&1 \
       && cargo test -p casper --lib deploy_finalization_status::tests >>"$LOG_DIR/dl_rust_admission.log" 2>&1 \
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
