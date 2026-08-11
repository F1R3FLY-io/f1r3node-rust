#!/usr/bin/env bash
# Multi-prover cross-witness (LOCAL-ONLY, fail-soft): a finite gated-COMM instance
# checked with the mCRL2 toolset — (1) the rho translation is BRANCHING-BISIMILAR to
# the CA reduction (translation faithfulness up to internal steps, via ltscompare),
# and (2) the no-leak modal-μ property holds (a COMM cannot fire without a token,
# via lps2pbes/pbes2bool). Corroborates the Rocq translation faithfulness +
# WrappingSubjectReduction.no_leak_requires_token.
#
# Fail-soft: any absent toolset binary is reported and skipped (exit 0). Present
# tools that do not return 'true' for both checks IS a failure.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="$ROOT/formal/mcrl2/cost_accounting"

echo "Checking cost-accounted rho bisimulation + no-leak (mCRL2)..."

for f in ca_instance.mcrl2 rho_translation.mcrl2 no_leak.mcf; do
  [ -f "$DIR/$f" ] || { echo "error: mCRL2 artifact $f not found" >&2; exit 1; }
done

for t in mcrl22lps lps2lts ltscompare lps2pbes pbes2bool; do
  command -v "$t" >/dev/null 2>&1 || { echo "  $t not found on PATH — skipped (fail-soft)."; exit 0; }
done

d="$(mktemp -d)"; trap 'rm -rf "$d"' EXIT
mcrl22lps "$DIR/ca_instance.mcrl2"    "$d/ca.lps" >/dev/null 2>&1
mcrl22lps "$DIR/rho_translation.mcrl2" "$d/tr.lps" >/dev/null 2>&1
lps2lts "$d/ca.lps" "$d/ca.lts" >/dev/null 2>&1
lps2lts "$d/tr.lps" "$d/tr.lts" >/dev/null 2>&1
bb="$(ltscompare -ebranching-bisim "$d/ca.lts" "$d/tr.lts" 2>&1 | tail -1)"
lps2pbes -f "$DIR/no_leak.mcf" "$d/ca.lps" "$d/nl.pbes" >/dev/null 2>&1
nl="$(pbes2bool "$d/nl.pbes" 2>&1 | tail -1)"

status=0
if [ "$bb" = "true" ]; then echo "  branching-bisim (CA ~= translation): true"
else echo "  branching-bisim: '$bb' (expected true)" >&2; status=1; fi
if [ "$nl" = "true" ]; then echo "  no-leak modal-mu: true"
else echo "  no-leak: '$nl' (expected true)" >&2; status=1; fi

if [ "$status" -eq 0 ]; then echo "mCRL2 cross-witness passed."
else echo "error: mCRL2 cross-witness failed" >&2; fi
exit "$status"
