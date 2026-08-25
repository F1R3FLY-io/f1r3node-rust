#!/usr/bin/env bash
# scripts/check-rspace-guards-ALL.sh
#
# LOCAL-ONLY verification gate for the RSpace check_commit guard-parity
# development (CLAIM-RSPACE-001, docs/claims/rspace-check-commit-play-replay.md).
# Structural sibling of scripts/check-fork-choice-ALL.sh, currently Rocq-only:
#
#   1. Rocq (AUTHORITATIVE) — builds formal/rocq/rspace_guards and asserts the
#      five capstones (rspace_first_match_guard, rspace_play_guard_complete,
#      rspace_replay_log_gated, rspace_replay_equivalent,
#      rspace_replay_guard_complete) are axiom-free ("Closed under the global
#      context"). Any failure here fails the gate.
#
# POLICY: this script is for LOCAL use only. Do NOT wire it (or any Rocq step)
# into .github/workflows/* — an earlier formal-CI workflow was deliberately
# removed.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ROCQ_DIR="$REPO_ROOT/formal/rocq/rspace_guards"

CAPSTONES=(
  rspace_first_match_guard
  rspace_play_guard_complete
  rspace_replay_log_gated
  rspace_replay_equivalent
  rspace_replay_guard_complete
)

run_rocq() {
  if command -v coqc >/dev/null 2>&1; then
    "$@"
  else
    opam exec -- "$@"
  fi
}

echo "== Rocq: build formal/rocq/rspace_guards =="
( cd "$ROCQ_DIR" && run_rocq coq_makefile -f _CoqProject -o Makefile.local ) >/dev/null 2>&1
if ! run_rocq make -C "$ROCQ_DIR" -f Makefile.local -j1 >/tmp/rspace_guards_build.log 2>&1; then
  echo "FAIL: Rocq build failed (see /tmp/rspace_guards_build.log)"
  exit 1
fi
echo "build OK"

echo "== Rocq: assert closed trust base =="
ASSUMPTIONS_DIR="$(mktemp -d -t rspace_guards_XXXXXX)"
ASSUMPTIONS_V="$ASSUMPTIONS_DIR/assumptions_probe.v"
{
  echo "From RSpaceGuards Require Import MainTheorem."
  for cap in "${CAPSTONES[@]}"; do
    echo "Print Assumptions ${cap}."
  done
} > "$ASSUMPTIONS_V"

OUT="$(run_rocq coqc -Q "$ROCQ_DIR/theories" RSpaceGuards "$ASSUMPTIONS_V" 2>&1)"
rm -rf "$ASSUMPTIONS_DIR"

CLOSED_COUNT="$(printf '%s\n' "$OUT" | grep -c 'Closed under the global context')"
if [[ "$CLOSED_COUNT" -ne "${#CAPSTONES[@]}" ]]; then
  echo "FAIL: expected ${#CAPSTONES[@]} closed capstones, found $CLOSED_COUNT"
  printf '%s\n' "$OUT"
  exit 1
fi
echo "all ${#CAPSTONES[@]} capstones: Closed under the global context"
echo "PASS: rspace_guards gate green"
