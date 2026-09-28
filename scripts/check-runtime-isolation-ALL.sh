#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ROCQ_DIR="$REPO_ROOT/formal/rocq/runtime_isolation"
TLA_DIR="$REPO_ROOT/formal/tlaplus/runtime_isolation"
TLC_JAR="${TLC_JAR:-$HOME/.tla/tla2tools.jar}"
rc=0

pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s\n' "$1"; rc=1; }
skip() { printf 'SKIP %s\n' "$1"; }

if [[ -x "$HOME/.opam/default/bin/coqc" ]]; then
  eval "$(opam env 2>/dev/null)" 2>/dev/null || true
fi

if command -v coqc >/dev/null 2>&1; then
  if (
    cd "$ROCQ_DIR"
    coqc -Q theories CasperRuntimeIsolation theories/BlockHeapLifecycle.v
    coqc -Q theories CasperRuntimeIsolation theories/ShardRuntimeIsolation.v
  ) >/tmp/runtime_isolation_rocq.log 2>&1; then
    pass "Rocq runtime-isolation proofs"
  else
    fail "Rocq runtime-isolation proofs"
    tail -20 /tmp/runtime_isolation_rocq.log
  fi
else
  fail "coqc unavailable"
fi

if [[ -f "$TLC_JAR" ]] || command -v tlc >/dev/null 2>&1; then
  export TLC_JAR TLC_REPO_ROOT="$REPO_ROOT"
  source "$REPO_ROOT/scripts/lib/tlc-run.sh"

  safe_cases=(
    'EvaluationRootIsolation.cfg|EvaluationRootIsolation.tla'
    'ShardRuntimeIsolation.cfg|ShardRuntimeIsolation.tla'
    'ShardRuntimeIsolationCrash.cfg|ShardRuntimeIsolation.tla'
    'BlockHeapLifecycle.cfg|BlockHeapLifecycle.tla'
  )

  for entry in "${safe_cases[@]}"; do
    IFS='|' read -r cfg model <<<"$entry"
    log="/tmp/runtime_isolation_${cfg%.cfg}.log"
    if tlc_run "$(tlc_metadir "runtime_isolation_${cfg%.cfg}")" \
      "$TLA_DIR/$cfg" "$TLA_DIR/$model" >"$log" 2>&1; then
      pass "$cfg"
    else
      fail "$cfg"
      tail -12 "$log"
    fi
  done

  unsafe_cases=(
    'EvaluationSharedBaseAuthorityUnsafe.cfg|ExplicitBaseAuthority|EvaluationRootIsolation.tla'
    'EvaluationSharedRootPublicationUnsafe.cfg|AcceptedRootsAreOwnedAndRecorded|EvaluationRootIsolation.tla'
    'EvaluationRollbackRetainsCandidateUnsafe.cfg|RejectedEvaluationsAreStateAtomic|EvaluationRootIsolation.tla'
    'EvaluationRollbackDeletesForeignRootUnsafe.cfg|CheckpointedRootsRemainRecorded|EvaluationRootIsolation.tla'
    'EvaluationEvidenceBeforeAcceptanceUnsafe.cfg|EvidenceRequiresAcceptance|EvaluationRootIsolation.tla'
    'ShardBlindCommitUnsafe.cfg|LedgerMatchesOwnedCommits|ShardRuntimeIsolation.tla'
    'ShardStateWriteUnsafe.cfg|LedgerMatchesOwnedCommits|ShardRuntimeIsolation.tla'
    'ShardRootPublicationUnsafe.cfg|RecordedRootsMatchLedger|ShardRuntimeIsolation.tla'
    'ShardResourceLeakUnsafe.cfg|ResourceOwnershipExact|ShardRuntimeIsolation.tla'
    'BlockHeapLifecycleMissingBoundaryUnsafe.cfg|ResidentWithinIntervalEnvelope|BlockHeapLifecycle.tla'
  )

  for entry in "${unsafe_cases[@]}"; do
    IFS='|' read -r cfg invariant model <<<"$entry"
    log="/tmp/runtime_isolation_${cfg%.cfg}.log"
    if tlc_run "$(tlc_metadir "runtime_isolation_${cfg%.cfg}")" \
      "$TLA_DIR/$cfg" "$TLA_DIR/$model" >"$log" 2>&1; then
      fail "$cfg passed without the required counterexample"
    elif grep -q "$invariant is violated" "$log"; then
      pass "$cfg refuted $invariant"
    else
      fail "$cfg failed for an unexpected reason"
      tail -12 "$log"
    fi
  done
else
  skip "TLC unavailable"
fi

exit "$rc"
