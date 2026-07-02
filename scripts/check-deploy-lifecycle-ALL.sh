#!/usr/bin/env bash
# scripts/check-deploy-lifecycle-ALL.sh
#
# LOCAL-ONLY verification gate for the F1R3FLY deploy lifecycle (block
# admission -> finalization) re-proposal invariant. Runs the formal TLA+ layer
# under the bounded memory envelope of scripts/lib/tlc-run.sh:
#
#   1. TLA+  (fail-soft) — TLC on the POST-fix MC_DeployLifecycle.cfg (must PASS
#      with no violation, exhausting the bounded state space) and the PRE-fix
#      MC_DeployLifecycle_pre_fix.cfg (must REPRODUCE its counterexample to
#      Inv_NoFinalizedReproposable). SKIPPED if there is no TLC jar ($TLC_JAR)
#      and no `tlc` on PATH.
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

echo "== [1/1] TLA+ (fail-soft) =="
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
if [[ -f "$TLC_JAR" ]] || command -v tlc >/dev/null 2>&1; then
  # shellcheck disable=SC1091
  source "$REPO_ROOT/scripts/lib/tlc-run.sh"
  # POST-fix: must pass (no violation, bounded space exhausted).
  if tlc_run "$(tlc_metadir dl_post_gate)" "$TLA_DIR/MC_DeployLifecycle.cfg" "$TLA_DIR/DeployLifecycle.tla" >/tmp/dl_tlc_post.log 2>&1; then
    pass "TLA+ post-fix Spec, PurgeRejectedBuf=TRUE (TypeOK, Inv_NoFinalizedReproposable, Inv_NoLossBeforeFinal)"
  else
    fail "TLA+ post-fix MC_DeployLifecycle.cfg did NOT pass (see /tmp/dl_tlc_post.log)"
  fi
  # PRE-fix: must FAIL (counterexample). Inverted sense.
  if tlc_run "$(tlc_metadir dl_pre_gate)" "$TLA_DIR/MC_DeployLifecycle_pre_fix.cfg" "$TLA_DIR/DeployLifecycle.tla" >/tmp/dl_tlc_pre.log 2>&1; then
    fail "TLA+ pre-fix should VIOLATE Inv_NoFinalizedReproposable but passed (the regression demo is broken)"
  else
    if grep -q "Inv_NoFinalizedReproposable is violated" /tmp/dl_tlc_pre.log; then
      pass "TLA+ pre-fix reproduces the finalized-deploy re-proposal counterexample"
    else
      fail "TLA+ pre-fix failed for the wrong reason (see /tmp/dl_tlc_pre.log)"
    fi
  fi
else
  skip "no TLC jar (\$TLC_JAR) or 'tlc' on PATH"
fi

echo
if [[ $rc -eq 0 ]]; then
  printf '\033[32m== deploy-lifecycle verification: ALL GATES OK ==\033[0m\n'
else
  printf '\033[31m== deploy-lifecycle verification: FAILURES ABOVE ==\033[0m\n'
fi
exit $rc
