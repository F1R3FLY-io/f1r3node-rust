#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export TLC_REPO_ROOT="$repo_root"
export TLC_HEAP="${TLC_HEAP:-512m}"
export TLC_WORKERS="${TLC_WORKERS:-1}"
export TLC_RSS="${TLC_RSS:-1G}"

# shellcheck source=scripts/lib/tlc-run.sh
source "$repo_root/scripts/lib/tlc-run.sh"

rocq_root="$repo_root/formal/rocq/stack_safe_pda"
coqc_bin="${COQC:-$HOME/.opam/default/bin/coqc}"
if [[ ! -x "$coqc_bin" ]]; then
  echo "stack-safe-pda: Rocq compiler not found at $coqc_bin" >&2
  exit 3
fi

pushd "$rocq_root" >/dev/null
tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/StackSafePDA.v
tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/EPathMap.v
popd >/dev/null

z3_output="$(tlc_bounded z3 "$repo_root/formal/smt/stack_safe_pda_modes.smt2")"
if [[ "$z3_output" != "unsat" ]]; then
  echo "stack-safe-pda: expected Z3 to prove no mode-law counterexample; got: $z3_output" >&2
  exit 1
fi

tla_root="$repo_root/formal/tlaplus/stack_safe_pda"
tla_metadir="$(tlc_metadir stack-safe-pda-equivalence)"
pushd "$tla_root" >/dev/null
tlc_run "$tla_metadir" PDAEquivalence.cfg PDAEquivalence.tla -cleanup
popd >/dev/null

echo "stack-safe-pda: Rocq, Z3, and TLC verification passed"
