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
tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/Bag.v
tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/EPM1.v
tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/SpatialMatcher.v
popd >/dev/null

verus_bin="${VERUS:-$(command -v verus || true)}"
if [[ -z "$verus_bin" || ! -x "$verus_bin" ]]; then
  echo "stack-safe-pda: Verus not found on PATH" >&2
  exit 3
fi
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/epathmap_canonical_worklist.rs"
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/bag_checked_delta.rs"

z3_output="$(tlc_bounded z3 "$repo_root/formal/smt/stack_safe_pda_modes.smt2")"
if [[ "$z3_output" != "unsat" ]]; then
  echo "stack-safe-pda: expected Z3 to prove no mode-law counterexample; got: $z3_output" >&2
  exit 1
fi

tla_root="$repo_root/formal/tlaplus/stack_safe_pda"
tla_metadir="$(tlc_metadir stack-safe-pda-equivalence)"
pushd "$tla_root" >/dev/null
tlc_run "$tla_metadir" PDAEquivalence.cfg PDAEquivalence.tla -cleanup
tlc_run "$tla_metadir" SpatialMatcherEquivalence.cfg SpatialMatcherEquivalence.tla -cleanup
tlc_run "$tla_metadir" RSpaceBagCommit.cfg RSpaceBagCommit.tla -cleanup
popd >/dev/null

mcrl2_root="$repo_root/formal/mcrl2/stack_safe_pda"
for tool in mcrl22lps lps2lts ltscompare lps2pbes pbes2bool; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "stack-safe-pda: required mCRL2 tool not found on PATH: $tool" >&2
    exit 3
  fi
done

mkdir -p "$repo_root/target"
mcrl2_tmp="$(mktemp -d "$repo_root/target/rspace-bag-mcrl2.XXXXXX")"
cleanup_mcrl2() {
  rm -rf -- "$mcrl2_tmp"
}
trap cleanup_mcrl2 EXIT

tlc_bounded mcrl22lps \
  "$mcrl2_root/rspace_bag_commit_impl.mcrl2" "$mcrl2_tmp/impl.lps"
tlc_bounded mcrl22lps \
  "$mcrl2_root/rspace_bag_commit_spec.mcrl2" "$mcrl2_tmp/spec.lps"
tlc_bounded lps2lts "$mcrl2_tmp/impl.lps" "$mcrl2_tmp/impl.lts"
tlc_bounded lps2lts "$mcrl2_tmp/spec.lps" "$mcrl2_tmp/spec.lts"

bisimulation="$(
  tlc_bounded ltscompare --equivalence=branching-bisim \
    "$mcrl2_tmp/impl.lts" "$mcrl2_tmp/spec.lts" 2>&1 | tail -n 1
)"
if [[ "$bisimulation" != "true" ]]; then
  echo "stack-safe-pda: expected RSpace Bag branching bisimulation; got: $bisimulation" >&2
  exit 1
fi

tlc_bounded lps2pbes -f "$mcrl2_root/rspace_bag_no_partial.mcf" \
  "$mcrl2_tmp/impl.lps" "$mcrl2_tmp/no-partial.pbes"
no_partial="$(tlc_bounded pbes2bool "$mcrl2_tmp/no-partial.pbes" 2>&1 | tail -n 1)"
if [[ "$no_partial" != "true" ]]; then
  echo "stack-safe-pda: expected no partial RSpace Bag publication; got: $no_partial" >&2
  exit 1
fi

echo "stack-safe-pda: Rocq, Verus, Z3, TLC, and mCRL2 verification passed"
