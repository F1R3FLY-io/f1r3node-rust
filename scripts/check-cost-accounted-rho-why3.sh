#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MLW="$ROOT/formal/why3/cost_accounting/join_conservation.mlw"

echo "Checking cost-accounted rho join conservation (Why3)..."

if [ ! -f "$MLW" ]; then
  echo "error: Why3 development not found at $MLW" >&2
  exit 1
fi

if ! command -v why3 >/dev/null 2>&1; then
  echo "error: why3 is required" >&2
  exit 1
fi

provers_list="$(why3 config list-provers 2>/dev/null || true)"
for prover in alt-ergo cvc5; do
  if ! printf '%s\n' "$provers_list" | grep -qiE "^${prover//-/[- ]}"; then
    echo "error: required Why3 prover is not configured: $prover" >&2
    exit 1
  fi

  if ! out="$(timeout 300 why3 prove -P "$prover" -t 60 "$MLW" 2>&1)"; then
    echo "error: Why3 ($prover) failed:" >&2
    printf '%s\n' "$out" >&2
    exit 1
  fi
  if printf '%s\n' "$out" | grep -qiE 'Unknown|Timeout|Failure|Invalid|StepLimitExceeded|error'; then
    echo "error: Why3 ($prover) did not discharge a goal:" >&2
    printf '%s\n' "$out" >&2
    exit 1
  fi
  n_valid="$(printf '%s\n' "$out" | grep -c 'Valid' || true)"
  if [ "$n_valid" -ne 3 ]; then
    echo "error: Why3 ($prover) returned $n_valid Valid goals; expected exactly 3" >&2
    printf '%s\n' "$out" >&2
    exit 1
  fi
  echo "  Why3 ($prover): $n_valid goals Valid (sig_size_pos, key_ge, join_no_weakening)."
done
echo "Why3 join-conservation cross-witness passed."
