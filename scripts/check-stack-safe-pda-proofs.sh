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
rspace_commit_rocq_output="$(
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/RSpaceCommit.v 2>&1
)"
printf '%s\n' "$rspace_commit_rocq_output"
rspace_commit_closed_capstones="$({
  printf '%s\n' "$rspace_commit_rocq_output" | grep -c '^Closed under the global context$'
} || true)"
if [[ "$rspace_commit_closed_capstones" != "6" ]]; then
  echo "stack-safe-pda: expected all six RSpace commit Rocq capstones to be closed; got $rspace_commit_closed_capstones" >&2
  exit 1
fi
tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/EPM1.v
rmp2_rocq_output="$(
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/RMP2.v 2>&1
)"
printf '%s\n' "$rmp2_rocq_output"
rmp2_closed_capstones="$({
  printf '%s\n' "$rmp2_rocq_output" | grep -c '^Closed under the global context$'
} || true)"
if [[ "$rmp2_closed_capstones" != "7" ]]; then
  echo "stack-safe-pda: expected all seven RMP2 Rocq capstones to be closed; got $rmp2_closed_capstones" >&2
  exit 1
fi
tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/SpatialMatcher.v
free_map_rocq_output="$(
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/FreeMapJournal.v 2>&1
)"
printf '%s\n' "$free_map_rocq_output"
free_map_closed_capstones="$({
  printf '%s\n' "$free_map_rocq_output" | grep -c '^Closed under the global context$'
} || true)"
if [[ "$free_map_closed_capstones" != "2" ]]; then
  echo "stack-safe-pda: expected both FreeMap Rocq capstones to be closed; got $free_map_closed_capstones" >&2
  exit 1
fi
accounting_demand_rocq_output="$(
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/AccountingDemand.v 2>&1
)"
printf '%s\n' "$accounting_demand_rocq_output"
accounting_demand_closed_capstones="$({
  printf '%s\n' "$accounting_demand_rocq_output" | grep -c '^Closed under the global context$'
} || true)"
if [[ "$accounting_demand_closed_capstones" != "4" ]]; then
  echo "stack-safe-pda: expected all four accounting-demand Rocq capstones to be closed; got $accounting_demand_closed_capstones" >&2
  exit 1
fi
channel_image_rocq_output="$(
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/ChannelImage.v 2>&1
)"
printf '%s\n' "$channel_image_rocq_output"
channel_image_closed_capstones="$({
  printf '%s\n' "$channel_image_rocq_output" | grep -c '^Closed under the global context$'
} || true)"
if [[ "$channel_image_closed_capstones" != "11" ]]; then
  echo "stack-safe-pda: expected all eleven ChannelImage Rocq capstones to be closed; got $channel_image_closed_capstones" >&2
  exit 1
fi
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
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/rmp2_par_bag_transcript.rs"
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/free_map_transaction.rs"
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/accounting_demand.rs"
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/channel_image_cursor.rs"
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/rspace_canonical_egress.rs"

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
tlc_run "$tla_metadir" RSpaceCanonicalEgress.cfg RSpaceCanonicalEgress.tla -cleanup
if canonical_egress_mutant="$({
  tlc_run "$tla_metadir" \
    RSpaceCanonicalEgressMutant.cfg RSpaceCanonicalEgress.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$canonical_egress_mutant"
  echo "stack-safe-pda: premature canonical-egress publication mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$canonical_egress_mutant"
if ! grep -q 'Invariant VisibleImageIsAtomic is violated' \
    <<<"$canonical_egress_mutant"; then
  echo "stack-safe-pda: canonical-egress mutant failed without the expected invariant witness" >&2
  exit 1
fi
tlc_run "$tla_metadir" FreeMapJournal.cfg FreeMapJournal.tla -cleanup
tlc_run "$tla_metadir" AccountingDemand.cfg AccountingDemand.tla -cleanup
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

for property in log_after_commit stale_refusal; do
  tlc_bounded lps2pbes -f "$mcrl2_root/rspace_bag_${property}.mcf" \
    "$mcrl2_tmp/impl.lps" "$mcrl2_tmp/${property}.pbes"
  property_result="$(
    tlc_bounded pbes2bool "$mcrl2_tmp/${property}.pbes" 2>&1 | tail -n 1
  )"
  if [[ "$property_result" != "true" ]]; then
    echo "stack-safe-pda: expected RSpace Bag ${property} property; got: $property_result" >&2
    exit 1
  fi
done

tlc_bounded mcrl22lps \
  "$mcrl2_root/free_map_journal_impl.mcrl2" "$mcrl2_tmp/free-map-impl.lps"
tlc_bounded mcrl22lps \
  "$mcrl2_root/free_map_journal_spec.mcrl2" "$mcrl2_tmp/free-map-spec.lps"
tlc_bounded lps2lts "$mcrl2_tmp/free-map-impl.lps" "$mcrl2_tmp/free-map-impl.lts"
tlc_bounded lps2lts "$mcrl2_tmp/free-map-spec.lps" "$mcrl2_tmp/free-map-spec.lts"

free_map_bisimulation="$(
  tlc_bounded ltscompare --equivalence=branching-bisim \
    "$mcrl2_tmp/free-map-impl.lts" "$mcrl2_tmp/free-map-spec.lts" 2>&1 | tail -n 1
)"
if [[ "$free_map_bisimulation" != "true" ]]; then
  echo "stack-safe-pda: expected FreeMap journal branching bisimulation; got: $free_map_bisimulation" >&2
  exit 1
fi

tlc_bounded lps2pbes -f "$mcrl2_root/free_map_journal_no_leak.mcf" \
  "$mcrl2_tmp/free-map-impl.lps" "$mcrl2_tmp/free-map-no-leak.pbes"
free_map_no_leak="$(
  tlc_bounded pbes2bool "$mcrl2_tmp/free-map-no-leak.pbes" 2>&1 | tail -n 1
)"
if [[ "$free_map_no_leak" != "true" ]]; then
  echo "stack-safe-pda: expected no FreeMap journal binding leak; got: $free_map_no_leak" >&2
  exit 1
fi

for mutant in or_leak partial_apply; do
  tlc_bounded mcrl22lps \
    "$mcrl2_root/free_map_journal_${mutant}_mutant.mcrl2" \
    "$mcrl2_tmp/free-map-${mutant}.lps"
  tlc_bounded lps2lts \
    "$mcrl2_tmp/free-map-${mutant}.lps" "$mcrl2_tmp/free-map-${mutant}.lts"
  mutant_bisimulation="$(
    tlc_bounded ltscompare --equivalence=branching-bisim \
      "$mcrl2_tmp/free-map-${mutant}.lts" "$mcrl2_tmp/free-map-spec.lts" 2>&1 | tail -n 1
  )"
  if [[ "$mutant_bisimulation" != "false" ]]; then
    echo "stack-safe-pda: expected FreeMap ${mutant} mutant to break bisimulation; got: $mutant_bisimulation" >&2
    exit 1
  fi
  tlc_bounded lps2pbes -f "$mcrl2_root/free_map_journal_no_leak.mcf" \
    "$mcrl2_tmp/free-map-${mutant}.lps" "$mcrl2_tmp/free-map-${mutant}-leak.pbes"
  mutant_no_leak="$(
    tlc_bounded pbes2bool "$mcrl2_tmp/free-map-${mutant}-leak.pbes" 2>&1 | tail -n 1
  )"
  if [[ "$mutant_no_leak" != "false" ]]; then
    echo "stack-safe-pda: expected FreeMap ${mutant} mutant leak witness; got: $mutant_no_leak" >&2
    exit 1
  fi
done

echo "stack-safe-pda: Rocq, Verus, Z3, TLC, and mCRL2 verification passed"
