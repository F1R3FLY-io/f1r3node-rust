#!/usr/bin/env bash
# Multi-prover cross-witness (LOCAL-ONLY, fail-soft): the lock-free budget
# reconciliation's linearizability / schedule-independence, in Iris/HeapLang
# (concurrent separation logic) — the deepest leg. The HeapLang program + its
# logically-atomic specification (formal/iris/cost_accounting/Reconcile.v) type-
# check against coq-iris; this gate type-checks them when coq-iris is installed.
#
# Fail-soft: absent coqc OR absent coq-iris (iris.heap_lang) is reported and
# skipped (exit 0) — the same schedule-independence is covered empirically by the
# Rust `loom` model-checker and the RuntimeBudgetReplay TLA+ model (both present,
# LOCAL-ONLY). A present coq-iris under which Reconcile.v fails to type-check IS a
# failure.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
V="$ROOT/formal/iris/cost_accounting/Reconcile.v"

echo "Checking cost-accounted rho reconciliation linearizability (Iris)..."

[ -f "$V" ] || { echo "error: Iris development not found at $V" >&2; exit 1; }

if ! command -v coqc >/dev/null 2>&1 && ! command -v rocq >/dev/null 2>&1; then
  echo "  coqc/rocq not found on PATH — skipped (fail-soft)."
  exit 0
fi

# Probe for coq-iris (iris.heap_lang) before attempting the real check.
probe="$(mktemp -d)/probe.v"
printf 'From iris.heap_lang Require Import lang.\n' > "$probe"
if ! (cd "$(dirname "$probe")" && coqc probe.v) >/dev/null 2>&1; then
  rm -rf "$(dirname "$probe")"
  echo "  coq-iris (iris.heap_lang) not installed — skipped (fail-soft);"
  echo "  the reconciliation's schedule-independence is covered by the loom tests + RuntimeBudgetReplay TLA+ model."
  exit 0
fi
rm -rf "$(dirname "$probe")"

if (cd "$(dirname "$V")" && coqc "$(basename "$V")") >/dev/null 2>&1; then
  echo "  Iris: Reconcile.v verified (debit_spec + debit_atomic_spec — logically-atomic linearizability)."
  echo "Iris reconciliation cross-witness passed."
  exit 0
fi
echo "  Iris: Reconcile.v failed to type-check under coq-iris." >&2
exit 1
