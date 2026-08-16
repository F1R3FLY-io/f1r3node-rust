#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_ROOT="$ROOT/formal/tlaplus/cost_accounted_rho"
WORK_ROOT="$ROOT/target/verification/cost-accounted-rho/apalache"
mkdir -p "$WORK_ROOT"

if ! command -v apalache-mc >/dev/null 2>&1; then
  echo "error: apalache-mc is required for the cost-accounted-rho formal gate" >&2
  exit 1
fi

outdir="$(mktemp -d "$WORK_ROOT/run.XXXXXX")"
trap 'rm -rf "$outdir"' EXIT

run_check() {
  local name="$1"
  local detail="$2"
  shift 2
  local output rc
  output="$(cd "$MODEL_ROOT" && timeout 300 apalache-mc --out-dir="$outdir/$name" check "$@" 2>&1)"
  rc=$?
  if [ "$rc" -eq 0 ] && grep -qE 'The outcome is: NoError|EXITCODE: OK' <<<"$output"; then
    echo "  PASS $name: $detail"
    return 0
  fi
  echo "  FAIL $name" >&2
  grep -iE 'error|violat|outcome|EXITCODE' <<<"$output" | tail -20 >&2
  return 1
}

run_expected_violation() {
  local name="$1"
  local detail="$2"
  local invariant="$3"
  shift 3
  local output rc
  output="$(cd "$MODEL_ROOT" && timeout 300 apalache-mc --out-dir="$outdir/$name" check "$@" 2>&1)"
  rc=$?
  if [ "$rc" -ne 0 ] \
     && grep -q "found INVARIANTS: $invariant" <<<"$output" \
     && grep -qE 'state invariant [0-9]+ violated' <<<"$output" \
     && grep -q 'The outcome is: Error' <<<"$output"; then
    echo "  PASS $name: $detail"
    return 0
  fi
  echo "  FAIL $name" >&2
  grep -iE 'error|violat|outcome|EXITCODE|INVARIANTS' <<<"$output" | tail -20 >&2
  return 1
}

echo "Checking cost-accounted rho with Apalache 0.58.3+..."

overall=0
run_check nary-join \
  "symbolic authority conservation, partition invariance, and no weakening" \
  --init=Init --next=Next --inv=Inv --length=1 NaryJoin.tla || overall=1
run_check threats \
  "bounded replay, settlement, evidence, and slash-authorization safety" \
  --config=CostAccountingThreats.cfg MCCostAccountingThreats.tla || overall=1
run_check search-frontier \
  "bounded witness classification and promotion discipline" \
  --config=CostAccountingSearchFrontier.cfg CostAccountingSearchFrontier.tla || overall=1
run_check replay-root \
  "two-validator, two-deploy root materialization and replay agreement through length 8" \
  --config=ReplayRootMaterializationApalache.cfg --length=8 ReplayRootMaterialization.tla || overall=1
run_check oslf-located \
  "finite located spatial/modal checking through both independent spends" \
  --config=OslfLocatedTyping.cfg --length=3 OslfLocatedTyping.tla || overall=1
run_expected_violation oslf-contraction-unsafe \
  "linear contraction is independently refuted" \
  LinearNoContraction \
  --config=OslfLocatedTypingContractionUnsafe.cfg --length=0 OslfLocatedTyping.tla || overall=1
run_expected_violation oslf-weakening-unsafe \
  "linear weakening is independently refuted" \
  LinearNoWeakening \
  --config=OslfLocatedTypingWeakeningUnsafe.cfg --length=0 OslfLocatedTyping.tla || overall=1
run_expected_violation oslf-alias-unsafe \
  "cross-surface debit aliasing is independently refuted" \
  LocationIsolation \
  --config=OslfLocatedTypingAliasUnsafe.cfg --length=2 OslfLocatedTyping.tla || overall=1
run_expected_violation oslf-upper-modal-unsafe \
  "treating a conservative upper bound as exact modal evidence is independently refuted" \
  ModalEvidenceSound \
  --config=OslfLocatedTypingUpperModalUnsafe.cfg --length=1 OslfLocatedTyping.tla || overall=1
run_expected_violation oslf-candidate-credit-unsafe \
  "crediting candidate-created supply during authenticated funding is independently refuted" \
  AuthenticatedFundingOnly \
  --config=OslfLocatedTypingCandidateCreditUnsafe.cfg --length=1 OslfLocatedTyping.tla || overall=1

if [ "$overall" -ne 0 ]; then
  exit 1
fi

echo "Apalache cost-accounted-rho cross-witnesses passed."
