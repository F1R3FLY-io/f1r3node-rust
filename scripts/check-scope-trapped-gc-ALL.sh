#!/usr/bin/env bash
# scripts/check-scope-trapped-gc-ALL.sh
#
# LOCAL-ONLY verification gate for the scope-trapped unforgeable GC
# proof (CLAIM-RHOLANG-001, docs/claims/rholang-scope-trapped-gc.md).
# Structural sibling of scripts/check-rspace-guards-ALL.sh.
#
#   1. Rocq (AUTHORITATIVE) — builds formal/rocq/scope_trapped_gc and
#      asserts the two capstones (rholang_scope_trapped_invariant,
#      rholang_scope_trapped_sweep_safe) are axiom-free
#      ("Closed under the global context"). Any failure here fails
#      the gate.
#
# POLICY: this script is for LOCAL use only; do NOT wire it into
# .github/workflows/* directly. CI runs the same capstone assertions
# through scripts/ci/check-formal-invariants.sh --rocq (rocq-build
# job in slashing-tests.yml).
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ROCQ_DIR="$REPO_ROOT/formal/rocq/scope_trapped_gc"

CAPSTONES=(
  rholang_scope_trapped_invariant
  rholang_scope_trapped_sweep_safe
)

run_rocq() {
  if command -v coqc >/dev/null 2>&1; then
    "$@"
  else
    opam exec -- "$@"
  fi
}

echo "== Rocq: build formal/rocq/scope_trapped_gc =="
( cd "$ROCQ_DIR" && run_rocq coq_makefile -f _CoqProject -o Makefile.local ) >/dev/null 2>&1
if ! run_rocq make -C "$ROCQ_DIR" -f Makefile.local -j1 >/tmp/scope_trapped_gc_build.log 2>&1; then
  echo "FAIL: Rocq build failed (see /tmp/scope_trapped_gc_build.log)"
  exit 1
fi
echo "build OK"

echo "== Rocq: assert closed trust base =="
ASSUMPTIONS_DIR="$(mktemp -d -t scope_trapped_gc_XXXXXX)"
ASSUMPTIONS_V="$ASSUMPTIONS_DIR/assumptions_probe.v"
{
  echo "From ScopeTrappedGC Require Import MainTheorem."
  for cap in "${CAPSTONES[@]}"; do
    echo "Print Assumptions ${cap}."
  done
} > "$ASSUMPTIONS_V"

OUT="$(run_rocq coqc -Q "$ROCQ_DIR/theories" ScopeTrappedGC "$ASSUMPTIONS_V" 2>&1)"
rm -rf "$ASSUMPTIONS_DIR"

CLOSED_COUNT="$(printf '%s\n' "$OUT" | grep -c 'Closed under the global context')"
if [[ "$CLOSED_COUNT" -ne "${#CAPSTONES[@]}" ]]; then
  echo "FAIL: expected ${#CAPSTONES[@]} closed capstones, found $CLOSED_COUNT"
  printf '%s\n' "$OUT"
  exit 1
fi
echo "all ${#CAPSTONES[@]} capstones: Closed under the global context"
echo "PASS: scope_trapped_gc gate green"
