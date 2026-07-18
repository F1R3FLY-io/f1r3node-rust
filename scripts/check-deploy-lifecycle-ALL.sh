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
#        * MC_DeployLifecycle_pre_fix.cfg (PurgeRejectedBuf=FALSE) ->
#          Inv_NoFinalizedReproposable (the dropped finalization buffer purge);
#        * MC_DeployLifecycle_quarantine_pre_fix.cfg (QuarantineBothStores=FALSE)
#          -> Inv_NoToxicReproposable (the §3c proposer-side quarantine that
#          leaves a refund-failing re-proposed deploy lingering re-proposable).
#      SKIPPED if there is no TLC jar ($TLC_JAR) and no `tlc` on PATH.
#
# What it guards: casper/src/rust/engine/multi_parent_casper/finalization_runner.rs
# (:234-241) documents a regression — the casper_engine split once DROPPED the
# rejected_deploy_buffer purge, so record-driven recovery re-proposed an
# already-finalized deploy, double-applying it (a second write to a single-value
# cell -> IntegerAdd invariant violation). The purge was restored; this gate
# keeps the fix pinned (post-fix cfg) and the regression reproducible (pre-fix
# cfg). Modelled code: block_admission.rs (add_deploy / accepted-deploy
# retention) + finalization_runner.rs (the two finalization purges).
#
# POLICY: this script is for LOCAL use only. Do NOT wire this into
# .github/workflows/* (or any TLA+ step) — an earlier formal-CI workflow was
# deliberately removed. See the finalized-floor gate (check-finalized-floor-ALL.sh)
# for the sibling policy.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TLA_DIR="$REPO_ROOT/formal/tlaplus/deploy_lifecycle"

rc=0
pass() { printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; rc=1; }
skip() { printf '  \033[33mSKIP\033[0m %s\n' "$1"; }

echo "== [1/2] TLA+ (fail-soft) =="
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
if [[ -f "$TLC_JAR" ]] || command -v tlc >/dev/null 2>&1; then
  # shellcheck disable=SC1091
  source "$REPO_ROOT/scripts/lib/tlc-run.sh"
  # POST-fix: must pass (no violation, bounded space exhausted).
  if tlc_run "$(tlc_metadir dl_post_gate)" "$TLA_DIR/MC_DeployLifecycle.cfg" "$TLA_DIR/DeployLifecycle.tla" >/tmp/dl_tlc_post.log 2>&1; then
    pass "TLA+ post-fix Spec, PurgeRejectedBuf=TRUE + QuarantineBothStores=TRUE (TypeOK, Inv_NoFinalizedReproposable, Inv_NoLossBeforeFinal, Inv_NoToxicReproposable)"
  else
    fail "TLA+ post-fix MC_DeployLifecycle.cfg did NOT pass (see /tmp/dl_tlc_post.log)"
  fi
  # PRE-fix (finalization): must FAIL (counterexample). Inverted sense.
  if tlc_run "$(tlc_metadir dl_pre_gate)" "$TLA_DIR/MC_DeployLifecycle_pre_fix.cfg" "$TLA_DIR/DeployLifecycle.tla" >/tmp/dl_tlc_pre.log 2>&1; then
    fail "TLA+ pre-fix should VIOLATE Inv_NoFinalizedReproposable but passed (the regression demo is broken)"
  else
    if grep -q "Inv_NoFinalizedReproposable is violated" /tmp/dl_tlc_pre.log; then
      pass "TLA+ pre-fix reproduces the finalized-deploy re-proposal counterexample"
    else
      fail "TLA+ pre-fix failed for the wrong reason (see /tmp/dl_tlc_pre.log)"
    fi
  fi
  # PRE-fix (quarantine): must FAIL (Inv_NoToxicReproposable counterexample). The
  # §3c proposer-side quarantine with QuarantineBothStores=FALSE leaves a toxic
  # refund-failing deploy lingering re-proposable in rejectedBuf.
  if tlc_run "$(tlc_metadir dl_quar_pre_gate)" "$TLA_DIR/MC_DeployLifecycle_quarantine_pre_fix.cfg" "$TLA_DIR/DeployLifecycle.tla" >/tmp/dl_tlc_quar.log 2>&1; then
    fail "TLA+ quarantine pre-fix should VIOLATE Inv_NoToxicReproposable but passed (the regression demo is broken)"
  else
    if grep -q "Inv_NoToxicReproposable is violated" /tmp/dl_tlc_quar.log; then
      pass "TLA+ quarantine pre-fix reproduces the toxic re-proposal counterexample"
    else
      fail "TLA+ quarantine pre-fix failed for the wrong reason (see /tmp/dl_tlc_quar.log)"
    fi
  fi
else
  skip "no TLC jar (\$TLC_JAR) or 'tlc' on PATH"
fi

echo "== [2/2] Rust purge unit (fail-soft) =="
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
  if cargo test -p casper --lib interpreter_util::backstop_tests >/tmp/dl_rust_admission.log 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" /tmp/dl_rust_admission.log; then
    pass "Rust admission-drop unit (canonical-won deploys blocked from re-proposal at admission; visible win blocks / later rejection reopens recovery)"
  else
    fail "Rust admission-drop unit failed (see /tmp/dl_rust_admission.log)"; tail -20 /tmp/dl_rust_admission.log | sed 's/^/      /'
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
