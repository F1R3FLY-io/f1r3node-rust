#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export TLC_REPO_ROOT="$repo_root"
export TLC_HEAP="${TLC_HEAP:-512m}"
export TLC_WORKERS="${TLC_WORKERS:-1}"
export TLC_MEMORY_HIGH="${TLC_MEMORY_HIGH:-768M}"
export TLC_RSS="${TLC_RSS:-1G}"
export TLC_CPU_QUOTA="${TLC_CPU_QUOTA:-100%}"
export TLC_TASKS_MAX="${TLC_TASKS_MAX:-64}"
export TLC_RUNTIME_MAX_SEC="${TLC_RUNTIME_MAX_SEC:-900}"

# shellcheck source=scripts/lib/tlc-run.sh
source "$repo_root/scripts/lib/tlc-run.sh"

rocq_root="$repo_root/formal/rocq/stack_safe_pda"
coqc_bin="${COQC:-$HOME/.opam/default/bin/coqc}"
if [[ ! -x "$coqc_bin" ]]; then
  echo "stack-safe-pda: Rocq compiler not found at $coqc_bin" >&2
  exit 3
fi
coqchk_bin="${COQCHK:-$(dirname "$coqc_bin")/coqchk}"
if [[ ! -x "$coqchk_bin" ]]; then
  echo "stack-safe-pda: Rocq kernel checker not found at $coqchk_bin" >&2
  exit 3
fi

rocq_compile_with_closed_count() {
  local source="$1" expected="$2" description="$3" output actual
  output="$(tlc_bounded "$coqc_bin" -Q theories StackSafePDA "$source" 2>&1)"
  printf '%s\n' "$output"
  actual="$({
    printf '%s\n' "$output" | grep -c '^Closed under the global context$'
  } || true)"
  if [[ "$actual" != "$expected" ]]; then
    echo "stack-safe-pda: expected $expected closed-context reports for $description; got $actual" >&2
    return 1
  fi
}

rocq_check_modules() {
  local description="$1" output
  shift
  output="$(tlc_bounded "$coqchk_bin" -Q theories StackSafePDA "$@" 2>&1)"
  printf '%s\n' "$output"
  if ! grep -q '^Modules were successfully checked$' <<<"$output"; then
    echo "stack-safe-pda: Rocq kernel check did not complete for $description" >&2
    return 1
  fi
}

rocq_reject_placeholders() {
  local source
  for source in "$@"; do
    if grep -En '\b(Admitted|Axiom|Axioms|Parameter|Parameters|TODO|FIXME)\b|\badmit\b' \
        "$source"; then
      echo "stack-safe-pda: forbidden proof placeholder in $source" >&2
      return 1
    fi
  done
}

pushd "$rocq_root" >/dev/null
tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/StackSafePDA.v
tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/EPathMap.v
tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/Bag.v
tlc_bounded "$coqc_bin" -Q theories StackSafePDA \
  theories/PathMapCanonicalStreamPublicationBase.v
rocq_compile_with_closed_count \
  theories/PathMapCanonicalStreamPublication.v 5 \
  "the exact 45-invariant canonical PathMap stream-publication authority"
rocq_compile_with_closed_count \
  theories/PathMapCanonicalStreamPublicationMutants.v 3 \
  "the exact 72-mutant canonical PathMap stream-publication controls"
rocq_check_modules "canonical PathMap stream publication" \
  StackSafePDA.PathMapCanonicalStreamPublicationBase \
  StackSafePDA.PathMapCanonicalStreamPublication \
  StackSafePDA.PathMapCanonicalStreamPublicationMutants
rocq_compile_with_closed_count \
  theories/CandidateCatalogAuthorityIsolation.v 5 \
  "the exact 14-invariant candidate-catalog isolation authority"
rocq_compile_with_closed_count \
  theories/CandidateCatalogAuthorityIsolationMutants.v 2 \
  "the exact 10-mutant candidate-catalog isolation controls"
rocq_check_modules "candidate-catalog authority isolation" \
  StackSafePDA.CandidateCatalogAuthorityIsolation \
  StackSafePDA.CandidateCatalogAuthorityIsolationMutants
rocq_compile_with_closed_count \
  theories/PathMapPhysicalOwnership.v 8 \
  "the exact eleven-invariant PathMap physical-ownership authority"
rocq_compile_with_closed_count \
  theories/PathMapPhysicalOwnershipMutants.v 2 \
  "the exact eleven-mutant PathMap physical-ownership controls"
rocq_check_modules "PathMap physical ownership" \
  StackSafePDA.PathMapPhysicalOwnership \
  StackSafePDA.PathMapPhysicalOwnershipMutants
rocq_compile_with_closed_count \
  theories/PathMapReversePruneTeardown.v 23 \
  "the exact 25-primary/10-quarantine PathMap teardown authority and scoped production policies"
rocq_compile_with_closed_count \
  theories/PathMapReversePruneTeardownMutants.v 13 \
  "the exact 27-teardown/13-quarantine PathMap teardown controls"
rocq_check_modules "PathMap reverse teardown" \
  StackSafePDA.PathMapReversePruneTeardown \
  StackSafePDA.PathMapReversePruneTeardownMutants
rocq_compile_with_closed_count \
  theories/ChannelPrefixReplacement.v 5 \
  "the exact 27-invariant channel-prefix replacement authority"
rocq_compile_with_closed_count \
  theories/ChannelPrefixReplacementMutants.v 2 \
  "the exact 20-mutant channel-prefix replacement controls"
rocq_check_modules "channel-prefix replacement" \
  StackSafePDA.ChannelPrefixReplacement \
  StackSafePDA.ChannelPrefixReplacementMutants
rocq_reject_placeholders \
  theories/PathMapCanonicalStreamPublicationBase.v \
  theories/PathMapCanonicalStreamPublication.v \
  theories/PathMapCanonicalStreamPublicationMutants.v \
  theories/CandidateCatalogAuthorityIsolation.v \
  theories/CandidateCatalogAuthorityIsolationMutants.v \
  theories/PathMapPhysicalOwnership.v \
  theories/PathMapPhysicalOwnershipMutants.v \
  theories/PathMapReversePruneTeardown.v \
  theories/PathMapReversePruneTeardownMutants.v \
  theories/ChannelPrefixReplacement.v \
  theories/ChannelPrefixReplacementMutants.v
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
generation_checkpoint_rocq_output="$(
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA \
    theories/GenerationCheckpointPublication.v 2>&1
)"
printf '%s\n' "$generation_checkpoint_rocq_output"
generation_checkpoint_closed_capstones="$({
  printf '%s\n' "$generation_checkpoint_rocq_output" | \
    grep -c '^Closed under the global context$'
} || true)"
if [[ "$generation_checkpoint_closed_capstones" != "17" ]]; then
  echo "stack-safe-pda: expected all seventeen generation-checkpoint Rocq capstones to be closed; got $generation_checkpoint_closed_capstones" >&2
  exit 1
fi
incremental_prepare_streaming_rocq_output="$(
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA \
    theories/IncrementalPrepareStreaming.v 2>&1
)"
printf '%s\n' "$incremental_prepare_streaming_rocq_output"
incremental_prepare_streaming_closed_capstones="$({
  printf '%s\n' "$incremental_prepare_streaming_rocq_output" | \
    grep -c '^Closed under the global context$'
} || true)"
if [[ "$incremental_prepare_streaming_closed_capstones" != "15" ]]; then
  echo "stack-safe-pda: expected all fifteen incremental-PREPARE Rocq capstones to be closed; got $incremental_prepare_streaming_closed_capstones" >&2
  exit 1
fi
streaming_codec_rocq_output="$(
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA \
    theories/StreamingCodecPDA.v 2>&1
)"
printf '%s\n' "$streaming_codec_rocq_output"
streaming_codec_closed_capstones="$({
  printf '%s\n' "$streaming_codec_rocq_output" | \
    grep -c '^Closed under the global context$'
} || true)"
if [[ "$streaming_codec_closed_capstones" != "20" ]]; then
  echo "stack-safe-pda: expected all twenty streaming-codec Rocq capstones to be closed; got $streaming_codec_closed_capstones" >&2
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

actree03_rocq_root="$repo_root/formal/rocq/actree03_completeness"
previous_tlc_rss="$TLC_RSS"
TLC_RSS=1536M
pushd "$actree03_rocq_root" >/dev/null
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1CursorAuthorityV2.v
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1CursorPositiveV3.v
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1CursorPositiveV4.v
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1CursorPositiveV4Mutants.v
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1ActConcreteV5.v
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1ActConcreteV5Mutants.v
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1PositionalAssociationV6.v
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1PositionalAssociationV6Mutants.v
popd >/dev/null
TLC_RSS="$previous_tlc_rss"

verus_bin="${VERUS:-$(command -v verus || true)}"
if [[ -z "$verus_bin" || ! -x "$verus_bin" ]]; then
  echo "stack-safe-pda: Verus not found on PATH" >&2
  exit 3
fi
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/epathmap_canonical_worklist.rs"
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/pathmap_reverse_prune_teardown.rs"
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/epm1_positional_association.rs"
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
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/incremental_prepare_streaming.rs"
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/streaming_codec_pda.rs"

z3_output="$(tlc_bounded z3 "$repo_root/formal/smt/stack_safe_pda_modes.smt2")"
if [[ "$z3_output" != "unsat" ]]; then
  echo "stack-safe-pda: expected Z3 to prove no mode-law counterexample; got: $z3_output" >&2
  exit 1
fi

tla_root="$repo_root/formal/tlaplus/stack_safe_pda"
tla_metadir="$(tlc_metadir stack-safe-pda-equivalence)"
pushd "$tla_root" >/dev/null
tlc_run "$tla_metadir" PDAEquivalence.cfg PDAEquivalence.tla -cleanup
tlc_run "$tla_metadir" \
  PathMapAnchorRootTeardown.cfg PathMapAnchorRootTeardown.tla -cleanup
for pathmap_anchor_mutant in \
  StrongCountSnapshot:PathMapAnchorRootSnapshotMutant.cfg:DeadAnchorRequiresExactlyOnePda \
  PerWrapperRawHeader:PathMapAnchorRootPerWrapperHeaderMutant.cfg:OneRawHeaderPerActiveAnchor \
  ManualFamilyCounter:PathMapAnchorRootManualFamilyMutant.cfg:OneRawHeaderPerActiveAnchor \
  UnregisteredMakeMut:PathMapAnchorRootUnregisteredMakeMutMutant.cfg:HeaderAndRootRegistrationAgree; do
  pathmap_anchor_label="${pathmap_anchor_mutant%%:*}"
  pathmap_anchor_tail="${pathmap_anchor_mutant#*:}"
  pathmap_anchor_cfg="${pathmap_anchor_tail%%:*}"
  pathmap_anchor_invariant="${pathmap_anchor_tail#*:}"
  if pathmap_anchor_output="$({
    tlc_run "$tla_metadir" "$pathmap_anchor_cfg" \
      PathMapAnchorRootTeardown.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$pathmap_anchor_output"
    echo "stack-safe-pda: ${pathmap_anchor_label} PathMap-anchor mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$pathmap_anchor_output"
  if ! grep -q "Invariant ${pathmap_anchor_invariant} is violated" \
      <<<"$pathmap_anchor_output"; then
    echo "stack-safe-pda: ${pathmap_anchor_label} mutant failed without ${pathmap_anchor_invariant}" >&2
    exit 1
  fi
done
"$repo_root/scripts/check-pathmap-anchor-loom.sh"
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
if canonical_egress_header_mutant="$({
  tlc_run "$tla_metadir" \
    RSpaceCanonicalEgressHeaderMutant.cfg RSpaceCanonicalEgress.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$canonical_egress_header_mutant"
  echo "stack-safe-pda: missing-envelope canonical-egress mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$canonical_egress_header_mutant"
if ! grep -q 'Invariant PrivateImageCompletesBeforePublish is violated' \
    <<<"$canonical_egress_header_mutant"; then
  echo "stack-safe-pda: canonical-egress header mutant failed without the expected invariant witness" >&2
  exit 1
fi
if canonical_egress_validation_mutant="$({
  tlc_run "$tla_metadir" \
    RSpaceCanonicalEgressValidationMutant.cfg RSpaceCanonicalEgress.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$canonical_egress_validation_mutant"
  echo "stack-safe-pda: skipped-validation canonical-egress mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$canonical_egress_validation_mutant"
if ! grep -q 'Invariant PreflightCompletesBeforeBuild is violated' \
    <<<"$canonical_egress_validation_mutant"; then
  echo "stack-safe-pda: canonical-egress validation mutant failed without the expected invariant witness" >&2
  exit 1
fi
tlc_run "$tla_metadir" \
  RSpaceSplitIndexAuthority.cfg RSpaceSplitIndexAuthority.tla -cleanup
for split_authority_mutant in \
  "partial-roots:RSpaceSplitIndexAuthorityPartialRootsMutant.cfg:VisibleRootsAreAtomic" \
  "incomplete-seal:RSpaceSplitIndexAuthorityIncompleteSealMutant.cfg:SealRequiresCompleteStaging" \
  "publish-before-seal:RSpaceSplitIndexAuthorityPublishBeforeSealMutant.cfg:PublicationRequiresSealedAuthority"
do
  split_authority_label="${split_authority_mutant%%:*}"
  split_authority_tail="${split_authority_mutant#*:}"
  split_authority_cfg="${split_authority_tail%%:*}"
  split_authority_invariant="${split_authority_tail#*:}"
  if split_authority_output="$({
    tlc_run "$tla_metadir" \
      "$split_authority_cfg" RSpaceSplitIndexAuthority.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$split_authority_output"
    echo "stack-safe-pda: ${split_authority_label} split-index authority mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$split_authority_output"
  if ! grep -q "Invariant ${split_authority_invariant} is violated" \
      <<<"$split_authority_output"; then
    echo "stack-safe-pda: ${split_authority_label} split-index authority mutant failed without ${split_authority_invariant}" >&2
    exit 1
  fi
done
tlc_run "$tla_metadir" \
  RSpaceAffineImportLifecycle.cfg RSpaceAffineImportLifecycle.tla -cleanup
for affine_import_mutant in \
  "incomplete:RSpaceAffineImportLifecycleIncompleteMutant.cfg:PublicationRequiresConsumedSeal" \
  "stale-activation:RSpaceAffineImportLifecycleStaleActivationMutant.cfg:AtMostOneRacingActivationPublishes" \
  "preserve-mutation:RSpaceAffineImportLifecyclePreserveMutationMutant.cfg:PreserveCurrentNeverMovesAuthority"
do
  affine_import_label="${affine_import_mutant%%:*}"
  affine_import_tail="${affine_import_mutant#*:}"
  affine_import_cfg="${affine_import_tail%%:*}"
  affine_import_invariant="${affine_import_tail#*:}"
  if affine_import_output="$({
    tlc_run "$tla_metadir" \
      "$affine_import_cfg" RSpaceAffineImportLifecycle.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$affine_import_output"
    echo "stack-safe-pda: ${affine_import_label} affine-import mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$affine_import_output"
  if ! grep -q "Invariant ${affine_import_invariant} is violated" \
      <<<"$affine_import_output"; then
    echo "stack-safe-pda: ${affine_import_label} affine-import mutant failed without ${affine_import_invariant}" >&2
    exit 1
  fi
done
tlc_run "$tla_metadir" \
  RSpaceAtomicCheckpointAuthority.cfg RSpaceAtomicCheckpointAuthority.tla -cleanup
for checkpoint_authority_mutant in \
  "split:RSpaceAtomicCheckpointAuthoritySplitMutant.cfg:NoUnpublishedCheckpointObjects" \
  "root-only:RSpaceAtomicCheckpointAuthorityRootOnlyMutant.cfg:CommitUsesExactBase" \
  "trust-target:RSpaceAtomicCheckpointAuthorityTrustTargetMutant.cfg:VerifiedRootsHaveCompleteClosure"
do
  checkpoint_authority_label="${checkpoint_authority_mutant%%:*}"
  checkpoint_authority_tail="${checkpoint_authority_mutant#*:}"
  checkpoint_authority_cfg="${checkpoint_authority_tail%%:*}"
  checkpoint_authority_invariant="${checkpoint_authority_tail#*:}"
  if checkpoint_authority_output="$({
    tlc_run "$tla_metadir" \
      "$checkpoint_authority_cfg" RSpaceAtomicCheckpointAuthority.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$checkpoint_authority_output"
    echo "stack-safe-pda: ${checkpoint_authority_label} atomic-checkpoint authority mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$checkpoint_authority_output"
  if ! grep -q "Invariant ${checkpoint_authority_invariant} is violated" \
      <<<"$checkpoint_authority_output"; then
    echo "stack-safe-pda: ${checkpoint_authority_label} atomic-checkpoint authority mutant failed without ${checkpoint_authority_invariant}" >&2
    exit 1
  fi
done
tlc_run "$tla_metadir" \
  RSpaceRecordReplayAuthorityLifecycle.cfg \
  RSpaceRecordReplayAuthorityLifecycle.tla -cleanup
if record_replay_skip_reset_mutant="$({
  tlc_run "$tla_metadir" \
    RSpaceRecordReplayAuthorityLifecycleSkipResetMutant.cfg \
    RSpaceRecordReplayAuthorityLifecycle.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$record_replay_skip_reset_mutant"
  echo "stack-safe-pda: record/replay skip-reset mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$record_replay_skip_reset_mutant"
if ! grep -q 'Invariant ReplayCommitRequiresReactivation is violated' \
    <<<"$record_replay_skip_reset_mutant"; then
  echo "stack-safe-pda: record/replay skip-reset mutant failed without ReplayCommitRequiresReactivation" >&2
  exit 1
fi
tlc_run "$tla_metadir" \
  RSpaceStateImportFfiBoundary.cfg \
  RSpaceStateImportFfiBoundary.tla -cleanup
for state_import_ffi_mutant in \
  "capability-exposure:RSpaceStateImportFfiBoundaryExposeMutant.cfg:CapabilityOutputRequiresEncodedSuccess" \
  "published-misclassification:RSpaceStateImportFfiBoundaryMisclassifyMutant.cfg:PublicationResponseIsTruthful" \
  "escaped-unwind:RSpaceStateImportFfiBoundaryEscapeMutant.cfg:NoOrdinaryUnwindEscapesBoundary" \
  "usable-staged-open:RSpaceStateImportFfiBoundaryUsableStagedOpenMutant.cfg:StageEncodingFailurePoisonsMutatedOpen" \
  "duplicate-publication:RSpaceStateImportFfiBoundaryDuplicatePublicationMutant.cfg:PublicationIsMonotoneAndUnique"
do
  state_import_ffi_label="${state_import_ffi_mutant%%:*}"
  state_import_ffi_tail="${state_import_ffi_mutant#*:}"
  state_import_ffi_cfg="${state_import_ffi_tail%%:*}"
  state_import_ffi_invariant="${state_import_ffi_tail#*:}"
  if state_import_ffi_output="$({
    tlc_run "$tla_metadir" \
      "$state_import_ffi_cfg" RSpaceStateImportFfiBoundary.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$state_import_ffi_output"
    echo "stack-safe-pda: ${state_import_ffi_label} state-import FFI mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$state_import_ffi_output"
  if ! grep -q "Invariant ${state_import_ffi_invariant} is violated" \
      <<<"$state_import_ffi_output"; then
    echo "stack-safe-pda: ${state_import_ffi_label} state-import FFI mutant failed without ${state_import_ffi_invariant}" >&2
    exit 1
  fi
done
if state_import_ffi_liveness_mutant="$({
  tlc_run "$tla_metadir" \
    RSpaceStateImportFfiBoundaryNoFairnessMutant.cfg \
    RSpaceStateImportFfiBoundary.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$state_import_ffi_liveness_mutant"
  echo "stack-safe-pda: no-fairness state-import FFI mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$state_import_ffi_liveness_mutant"
if ! grep -q 'Temporal properties were violated' \
    <<<"$state_import_ffi_liveness_mutant"; then
  echo "stack-safe-pda: no-fairness state-import FFI mutant failed without the liveness witness" >&2
  exit 1
fi
if state_import_ffi_two_publications="$({
  tlc_run "$tla_metadir" \
    RSpaceStateImportFfiBoundaryTwoPublicationsWitness.cfg \
    RSpaceStateImportFfiBoundary.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$state_import_ffi_two_publications"
  echo "stack-safe-pda: second legal state-import publication was unreachable" >&2
  exit 1
fi
printf '%s\n' "$state_import_ffi_two_publications"
if ! grep -q 'Invariant NeverTwoPublications is violated' \
    <<<"$state_import_ffi_two_publications"; then
  echo "stack-safe-pda: second state-import publication witness failed without NeverTwoPublications" >&2
  exit 1
fi
tlc_run "$tla_metadir" \
  RSpaceGenerationCheckpointPublication.cfg \
  RSpaceGenerationCheckpointPublication.tla -cleanup
for generation_checkpoint_mutant in \
  "affine-clone:RSpaceGenerationCheckpointPublicationAffineCloneMutant.cfg:AtMostOneMutableAuthority" \
  "lease-gap:RSpaceGenerationCheckpointPublicationLeaseGapMutant.cfg:AuthorityIsContinuous" \
  "phase-skip:RSpaceGenerationCheckpointPublicationPhaseSkipMutant.cfg:PhaseFollowsAffineProtocol" \
  "premature-catalog:RSpaceGenerationCheckpointPublicationPrematureCatalogMutant.cfg:NewAuthorityRequiresRestartGradeGeneration" \
  "membership-substitution:RSpaceGenerationCheckpointPublicationMembershipSubstitutionMutant.cfg:CheckpointAndIndexExactlyAdmitted" \
  "stale-cas:RSpaceGenerationCheckpointPublicationStaleCasMutant.cfg:CatalogWriteUsesExactOldImage" \
  "third-authority:RSpaceGenerationCheckpointPublicationThirdAuthorityMutant.cfg:CrashRecoverySelectsOnlyValidAuthority" \
  "early-ack:RSpaceGenerationCheckpointPublicationEarlyAckMutant.cfg:AcknowledgementRequiresRestartPathReadmission" \
  "inventory-count:RSpaceGenerationCheckpointPublicationInventoryCountMutant.cfg:DescriptorInventoriesAreExact" \
  "inventory-bytes:RSpaceGenerationCheckpointPublicationInventoryBytesMutant.cfg:DescriptorInventoriesAreExact" \
  "inventory-digest:RSpaceGenerationCheckpointPublicationInventoryDigestMutant.cfg:OrderedInventoryIsExact" \
  "dirty-slot-selection:RSpaceGenerationCheckpointPublicationDirtySlotSelectionMutant.cfg:CrashRecoverySelectsOnlyValidAuthority" \
  "catalog-failure-dnp:RSpaceGenerationCheckpointPublicationCatalogFailureDnpMutant.cfg:OutcomeBoundaryIsExact" \
  "skip-catalog-directory:RSpaceGenerationCheckpointPublicationSkipCatalogDirectoryMutant.cfg:PhysicalRecoveryRequiresCatalogDirectory" \
  "cleanup-after-uncertainty:RSpaceGenerationCheckpointPublicationCleanupAfterUncertaintyMutant.cfg:PreflightAndCleanupPrecedeMutation" \
  "wire-layout:RSpaceGenerationCheckpointPublicationWireLayoutMutant.cfg:WireLayoutIsExact" \
  "receipt-binding:RSpaceGenerationCheckpointPublicationBindingMutationMutant.cfg:ReceiptBindingsAreExact" \
  "minus-one-accepted:RSpaceGenerationCheckpointPublicationMinusOneAcceptedMutant.cfg:PreflightRequiresExactResourceAdmission" \
  "exact-boundary-rejected:RSpaceGenerationCheckpointPublicationExactBoundaryRejectedMutant.cfg:ExactResourceBoundaryIsAccepted" \
  "late-admission-refusal:RSpaceGenerationCheckpointPublicationLateAdmissionRefusalMutant.cfg:ResourceRefusalPrecedesAllMutation" \
  "refusal-mutates-authority:RSpaceGenerationCheckpointPublicationRefusalMutatesAuthorityMutant.cfg:ResourceRefusalPrecedesAllMutation" \
  "omitted-durable-resource:RSpaceGenerationCheckpointPublicationOmittedDurableResourceMutant.cfg:PreflightRequiresExactResourceAdmission" \
  "omitted-wal-resource:RSpaceGenerationCheckpointPublicationOmittedWalResourceMutant.cfg:PreflightRequiresExactResourceAdmission" \
  "omitted-admission-resource:RSpaceGenerationCheckpointPublicationOmittedAdmissionResourceMutant.cfg:PreflightRequiresExactResourceAdmission" \
  "fallback-damaged-successor:RSpaceGenerationCheckpointPublicationFallbackDamagedSuccessorMutant.cfg:CrashRecoverySelectsOnlyValidAuthority" \
  "charge-pollution:RSpaceGenerationCheckpointPublicationChargePollutionMutant.cfg:CheckpointPreservesSemanticAccounting" \
  "diagnostic-trace-pollution:RSpaceGenerationCheckpointPublicationDiagnosticTracePollutionMutant.cfg:CheckpointPreservesSemanticAccounting" \
  "physical-metering:RSpaceGenerationCheckpointPublicationPhysicalMeteringMutant.cfg:PhysicalAccountingIsSeparated"
do
  generation_checkpoint_label="${generation_checkpoint_mutant%%:*}"
  generation_checkpoint_tail="${generation_checkpoint_mutant#*:}"
  generation_checkpoint_cfg="${generation_checkpoint_tail%%:*}"
  generation_checkpoint_invariant="${generation_checkpoint_tail#*:}"
  if generation_checkpoint_output="$({
    tlc_run "$tla_metadir" \
      "$generation_checkpoint_cfg" \
      RSpaceGenerationCheckpointPublication.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$generation_checkpoint_output"
    echo "stack-safe-pda: ${generation_checkpoint_label} generation-checkpoint mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$generation_checkpoint_output"
  if ! grep -q "Invariant ${generation_checkpoint_invariant} is violated" \
      <<<"$generation_checkpoint_output"; then
    echo "stack-safe-pda: ${generation_checkpoint_label} generation-checkpoint mutant failed without ${generation_checkpoint_invariant}" >&2
    exit 1
  fi
done
"$repo_root/scripts/check-generation-checkpoint-accounting-boundary.sh"
for generation_checkpoint_witness in \
  "acknowledged:RSpaceGenerationCheckpointPublicationNeverAcknowledged.cfg:NeverAcknowledged" \
  "recovered-old:RSpaceGenerationCheckpointPublicationNeverRecoveredOld.cfg:NeverRecoveredOld" \
  "recovered-new:RSpaceGenerationCheckpointPublicationNeverRecoveredNew.cfg:NeverRecoveredNew" \
  "published-failure:RSpaceGenerationCheckpointPublicationNeverPublishedFailure.cfg:NeverPublishedFailure" \
  "indeterminate:RSpaceGenerationCheckpointPublicationNeverIndeterminate.cfg:NeverIndeterminate" \
  "durable-resource-refusal:RSpaceGenerationCheckpointPublicationNeverDurableResourceRefusal.cfg:NeverDurableResourceRefusal" \
  "invalid-resource-configuration:RSpaceGenerationCheckpointPublicationNeverInvalidResourceConfiguration.cfg:NeverInvalidResourceConfiguration" \
  "wal-resource-refusal:RSpaceGenerationCheckpointPublicationNeverWalResourceRefusal.cfg:NeverWalResourceRefusal" \
  "admission-resource-refusal:RSpaceGenerationCheckpointPublicationNeverAdmissionResourceRefusal.cfg:NeverAdmissionResourceRefusal" \
  "durable-exact-cleanup:RSpaceGenerationCheckpointPublicationNeverDurableExactCleanup.cfg:NeverDurableExactCleanup" \
  "wal-exact-cleanup:RSpaceGenerationCheckpointPublicationNeverWalExactCleanup.cfg:NeverWalExactCleanup" \
  "admission-exact-cleanup:RSpaceGenerationCheckpointPublicationNeverAdmissionExactCleanup.cfg:NeverAdmissionExactCleanup" \
  "repairable-index-recovery:RSpaceGenerationCheckpointPublicationNeverRepairableIndexRecovery.cfg:NeverRepairableIndexRecovery" \
  "rejected-damaged-successor:RSpaceGenerationCheckpointPublicationNeverRejectedDamagedSuccessor.cfg:NeverRejectedDamagedSuccessor"
do
  generation_checkpoint_label="${generation_checkpoint_witness%%:*}"
  generation_checkpoint_tail="${generation_checkpoint_witness#*:}"
  generation_checkpoint_cfg="${generation_checkpoint_tail%%:*}"
  generation_checkpoint_invariant="${generation_checkpoint_tail#*:}"
  if generation_checkpoint_output="$({
    tlc_run "$tla_metadir" \
      "$generation_checkpoint_cfg" \
      RSpaceGenerationCheckpointPublication.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$generation_checkpoint_output"
    echo "stack-safe-pda: ${generation_checkpoint_label} generation-checkpoint non-vacuity witness was unreachable" >&2
    exit 1
  fi
  printf '%s\n' "$generation_checkpoint_output"
  if ! grep -q "Invariant ${generation_checkpoint_invariant} is violated" \
      <<<"$generation_checkpoint_output"; then
    echo "stack-safe-pda: ${generation_checkpoint_label} generation-checkpoint witness failed without ${generation_checkpoint_invariant}" >&2
    exit 1
  fi
done
tlc_run "$tla_metadir" \
  RSpaceIncrementalPrepareStreaming.cfg \
  RSpaceIncrementalPrepareStreaming.tla -cleanup
for incremental_prepare_mutant in \
  "whole-record-buffer:RSpaceIncrementalPrepareStreamingWholeRecordBufferMutant.cfg:NoWholeRecordBuffer" \
  "record-sized-scratch:RSpaceIncrementalPrepareStreamingScratchRecordMutant.cfg:ScratchIsBounded" \
  "read-before-admission:RSpaceIncrementalPrepareStreamingReadBeforeAdmissionMutant.cfg:VariableReadsFollowAdmission" \
  "accept-minus-one:RSpaceIncrementalPrepareStreamingAcceptMinusOneMutant.cfg:MinusOneIsRejectedBeforeVariableRead" \
  "reject-exact:RSpaceIncrementalPrepareStreamingRejectExactMutant.cfg:ExactBoundaryIsNotRejected" \
  "cursor-off-by-one:RSpaceIncrementalPrepareStreamingCursorOffByOneMutant.cfg:CursorProgressIsExact" \
  "incomplete-transcript:RSpaceIncrementalPrepareStreamingIncompleteTranscriptMutant.cfg:AuthenticationTranscriptIsExact" \
  "semantic-before-crc:RSpaceIncrementalPrepareStreamingSemanticBeforeCrcMutant.cfg:CrcDominatesSemanticError" \
  "drop-semantic-error:RSpaceIncrementalPrepareStreamingDropSemanticErrorMutant.cfg:OutcomeRefinesContiguousOracle" \
  "wrong-identity:RSpaceIncrementalPrepareStreamingWrongIdentityMutant.cfg:AcceptedIdentityIsExact" \
  "skip-commit-binding:RSpaceIncrementalPrepareStreamingSkipCommitBindingMutant.cfg:NoPrivateIndexBeforeCommitBinding" \
  "skip-chain-binding:RSpaceIncrementalPrepareStreamingSkipChainBindingMutant.cfg:NoPrivateIndexBeforeChainBinding" \
  "swap-replay-layouts:RSpaceIncrementalPrepareStreamingSwapReplayLayoutsMutant.cfg:ReplayTranscriptMatchesFirstPass" \
  "skip-replay-transcript:RSpaceIncrementalPrepareStreamingSkipReplayTranscriptMutant.cfg:ReplayTranscriptMatchesFirstPass" \
  "accept-unstable-source:RSpaceIncrementalPrepareStreamingAcceptUnstableSourceMutant.cfg:StableSourceRequiredForSuccess" \
  "early-index-exposure:RSpaceIncrementalPrepareStreamingEarlyIndexExposureMutant.cfg:NoPublicStateBeforeCatalogVerification" \
  "publish-before-catalog:RSpaceIncrementalPrepareStreamingPublishBeforeCatalogMutant.cfg:NoPublicStateBeforeCatalogVerification" \
  "accept-catalog-mismatch:RSpaceIncrementalPrepareStreamingAcceptCatalogMismatchMutant.cfg:CatalogVerificationIsExact" \
  "publish-rejected-catalog:RSpaceIncrementalPrepareStreamingPublishRejectedCatalogMutant.cfg:NoRejectedCandidatePublishes"
do
  incremental_prepare_label="${incremental_prepare_mutant%%:*}"
  incremental_prepare_tail="${incremental_prepare_mutant#*:}"
  incremental_prepare_cfg="${incremental_prepare_tail%%:*}"
  incremental_prepare_invariant="${incremental_prepare_tail#*:}"
  if incremental_prepare_output="$({
    tlc_run "$tla_metadir" \
      "$incremental_prepare_cfg" \
      RSpaceIncrementalPrepareStreaming.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$incremental_prepare_output"
    echo "stack-safe-pda: ${incremental_prepare_label} incremental-PREPARE mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$incremental_prepare_output"
  if ! grep -q "Invariant ${incremental_prepare_invariant} is violated" \
      <<<"$incremental_prepare_output"; then
    echo "stack-safe-pda: ${incremental_prepare_label} incremental-PREPARE mutant failed without ${incremental_prepare_invariant}" >&2
    exit 1
  fi
done
for incremental_prepare_witness in \
  "success:RSpaceIncrementalPrepareStreamingNeverSuccess.cfg:NeverSuccess" \
  "header-rejection:RSpaceIncrementalPrepareStreamingNeverHeaderRejection.cfg:NeverHeaderRejection" \
  "resource-rejection:RSpaceIncrementalPrepareStreamingNeverResourceRejection.cfg:NeverResourceRejection" \
  "pair-rejection:RSpaceIncrementalPrepareStreamingNeverPairRejection.cfg:NeverPairRejection" \
  "read-rejection:RSpaceIncrementalPrepareStreamingNeverReadRejection.cfg:NeverReadRejection" \
  "crc-rejection:RSpaceIncrementalPrepareStreamingNeverCrcRejection.cfg:NeverCrcRejection" \
  "mutation-rejection:RSpaceIncrementalPrepareStreamingNeverMutationRejection.cfg:NeverMutationRejection" \
  "object-rejection:RSpaceIncrementalPrepareStreamingNeverObjectRejection.cfg:NeverObjectRejection" \
  "commit-rejection:RSpaceIncrementalPrepareStreamingNeverCommitRejection.cfg:NeverCommitRejection" \
  "chain-rejection:RSpaceIncrementalPrepareStreamingNeverChainRejection.cfg:NeverChainRejection" \
  "source-rejection:RSpaceIncrementalPrepareStreamingNeverSourceRejection.cfg:NeverSourceRejection" \
  "catalog-rejection:RSpaceIncrementalPrepareStreamingNeverCatalogRejection.cfg:NeverCatalogRejection" \
  "private-candidate-awaiting-catalog:RSpaceIncrementalPrepareStreamingNeverPrivateCandidateAwaitingCatalog.cfg:NeverPrivateCandidateAwaitingCatalog"
do
  incremental_prepare_label="${incremental_prepare_witness%%:*}"
  incremental_prepare_tail="${incremental_prepare_witness#*:}"
  incremental_prepare_cfg="${incremental_prepare_tail%%:*}"
  incremental_prepare_invariant="${incremental_prepare_tail#*:}"
  if incremental_prepare_output="$({
    tlc_run "$tla_metadir" \
      "$incremental_prepare_cfg" \
      RSpaceIncrementalPrepareStreaming.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$incremental_prepare_output"
    echo "stack-safe-pda: ${incremental_prepare_label} incremental-PREPARE non-vacuity witness was unreachable" >&2
    exit 1
  fi
  printf '%s\n' "$incremental_prepare_output"
  if ! grep -q "Invariant ${incremental_prepare_invariant} is violated" \
      <<<"$incremental_prepare_output"; then
    echo "stack-safe-pda: ${incremental_prepare_label} incremental-PREPARE witness failed without ${incremental_prepare_invariant}" >&2
    exit 1
  fi
done
tlc_run "$tla_metadir" \
  RSpaceStreamingCodecPDA.cfg \
  RSpaceStreamingCodecPDA.tla -cleanup
for streaming_codec_mutant in \
  "whole-input-buffer:RSpaceStreamingCodecPDAWholeInputBufferMutant.cfg:ScratchIsBounded" \
  "whole-field-buffer:RSpaceStreamingCodecPDAWholeFieldBufferMutant.cfg:NoWholeFieldRetention" \
  "read-before-limit:RSpaceStreamingCodecPDALimitAfterReadMutant.cfg:VariableReadsFollowAdmission" \
  "per-section-limit:RSpaceStreamingCodecPDAPerSectionLimitMutant.cfg:OutcomeRefinesCodecOracle" \
  "order-before-key-validation:RSpaceStreamingCodecPDAOrderBeforeKeyValidationMutant.cfg:OrderingFollowsKeyValidation" \
  "noop-before-order:RSpaceStreamingCodecPDANoopBeforeOrderMutant.cfg:OutcomeRefinesCodecOracle" \
  "trust-unstable-reread:RSpaceStreamingCodecPDATrustRereadMutant.cfg:RereadStabilityPrecedesOrderAuthority" \
  "aggregate-after-fields:RSpaceStreamingCodecPDAAggregateAfterFieldsMutant.cfg:AggregatePrecedesFieldValidation" \
  "skip-object-grammar:RSpaceStreamingCodecPDASkipObjectGrammarMutant.cfg:OutcomeRefinesCodecOracle" \
  "accept-dangling-escape:RSpaceStreamingCodecPDAAcceptDanglingEscapeMutant.cfg:OutcomeRefinesCodecOracle" \
  "any-escape-terminates:RSpaceStreamingCodecPDAAnyEscapeTerminatesMutant.cfg:OutcomeRefinesCodecOracle" \
  "group-inside-frame:RSpaceStreamingCodecPDAGroupInsideFrameMutant.cfg:OutcomeRefinesCodecOracle" \
  "accept-empty-continuation:RSpaceStreamingCodecPDAAcceptEmptyContinuationMutant.cfg:OutcomeRefinesCodecOracle" \
  "accept-continuation-trailing:RSpaceStreamingCodecPDAAcceptContinuationTrailingMutant.cfg:OutcomeRefinesCodecOracle" \
  "require-framed-datum-suffix:RSpaceStreamingCodecPDARequireFramedDatumSuffixMutant.cfg:OutcomeRefinesCodecOracle" \
  "require-framed-join-suffix:RSpaceStreamingCodecPDARequireFramedJoinSuffixMutant.cfg:OutcomeRefinesCodecOracle" \
  "accept-equal-keys:RSpaceStreamingCodecPDAAcceptEqualKeysMutant.cfg:OutcomeRefinesCodecOracle" \
  "compare-first-chunk-only:RSpaceStreamingCodecPDACompareFirstChunkOnlyMutant.cfg:OutcomeRefinesCodecOracle" \
  "empty-before-trailing:RSpaceStreamingCodecPDAEmptyBeforeTrailingMutant.cfg:OutcomeRefinesCodecOracle"
do
  streaming_codec_label="${streaming_codec_mutant%%:*}"
  streaming_codec_tail="${streaming_codec_mutant#*:}"
  streaming_codec_cfg="${streaming_codec_tail%%:*}"
  streaming_codec_invariant="${streaming_codec_tail#*:}"
  if streaming_codec_output="$({
    tlc_run "$tla_metadir" \
      "$streaming_codec_cfg" \
      RSpaceStreamingCodecPDA.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$streaming_codec_output"
    echo "stack-safe-pda: ${streaming_codec_label} streaming-codec mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$streaming_codec_output"
  if ! grep -q "Invariant ${streaming_codec_invariant} is violated" \
      <<<"$streaming_codec_output"; then
    echo "stack-safe-pda: ${streaming_codec_label} streaming-codec mutant failed without ${streaming_codec_invariant}" >&2
    exit 1
  fi
done
for streaming_codec_witness in \
  "datum-success:RSpaceStreamingCodecPDANeverDatumSuccess.cfg:NeverDatumSuccess" \
  "continuation-success:RSpaceStreamingCodecPDANeverContinuationSuccess.cfg:NeverContinuationSuccess" \
  "join-success:RSpaceStreamingCodecPDANeverJoinSuccess.cfg:NeverJoinSuccess" \
  "mutation-success:RSpaceStreamingCodecPDANeverMutationSuccess.cfg:NeverMutationSuccess" \
  "dag-success:RSpaceStreamingCodecPDANeverDagSuccess.cfg:NeverDagSuccess" \
  "blob-success:RSpaceStreamingCodecPDANeverBlobSuccess.cfg:NeverBlobSuccess" \
  "semantic-success:RSpaceStreamingCodecPDANeverSemanticSuccess.cfg:NeverSemanticSuccess" \
  "accelerator-success:RSpaceStreamingCodecPDANeverAcceleratorSuccess.cfg:NeverAcceleratorSuccess" \
  "retention-success:RSpaceStreamingCodecPDANeverRetentionSuccess.cfg:NeverRetentionSuccess" \
  "frame-rejection:RSpaceStreamingCodecPDANeverFrameRejection.cfg:NeverFrameRejection" \
  "group-rejection:RSpaceStreamingCodecPDANeverGroupRejection.cfg:NeverGroupRejection" \
  "resource-rejection:RSpaceStreamingCodecPDANeverResourceRejection.cfg:NeverResourceRejection" \
  "key-rejection:RSpaceStreamingCodecPDANeverKeyRejection.cfg:NeverKeyRejection" \
  "order-rejection:RSpaceStreamingCodecPDANeverOrderRejection.cfg:NeverOrderRejection" \
  "source-rejection:RSpaceStreamingCodecPDANeverSourceRejection.cfg:NeverSourceRejection" \
  "aggregate-rejection:RSpaceStreamingCodecPDANeverAggregateRejection.cfg:NeverAggregateRejection" \
  "field-rejection:RSpaceStreamingCodecPDANeverFieldRejection.cfg:NeverFieldRejection"
do
  streaming_codec_label="${streaming_codec_witness%%:*}"
  streaming_codec_tail="${streaming_codec_witness#*:}"
  streaming_codec_cfg="${streaming_codec_tail%%:*}"
  streaming_codec_invariant="${streaming_codec_tail#*:}"
  if streaming_codec_output="$({
    tlc_run "$tla_metadir" \
      "$streaming_codec_cfg" \
      RSpaceStreamingCodecPDA.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$streaming_codec_output"
    echo "stack-safe-pda: ${streaming_codec_label} streaming-codec non-vacuity witness was unreachable" >&2
    exit 1
  fi
  printf '%s\n' "$streaming_codec_output"
  if ! grep -q "Invariant ${streaming_codec_invariant} is violated" \
      <<<"$streaming_codec_output"; then
    echo "stack-safe-pda: ${streaming_codec_label} streaming-codec witness failed without ${streaming_codec_invariant}" >&2
    exit 1
  fi
done
tlc_run "$tla_metadir" \
  ParBagProjectionLifecycle.cfg ParBagProjectionLifecycle.tla -cleanup
if projection_cap_mutant="$({
  tlc_run "$tla_metadir" \
    ParBagProjectionCapMutant.cfg ParBagProjectionLifecycle.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$projection_cap_mutant"
  echo "stack-safe-pda: strict projection-cap mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$projection_cap_mutant"
if ! grep -q 'Invariant RefusalOnlyWhenTooLarge is violated' \
    <<<"$projection_cap_mutant"; then
  echo "stack-safe-pda: projection-cap mutant failed without the expected invariant witness" >&2
  exit 1
fi
if projection_partial_mutant="$({
  tlc_run "$tla_metadir" \
    ParBagProjectionPartialMutant.cfg ParBagProjectionLifecycle.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$projection_partial_mutant"
  echo "stack-safe-pda: partial projection-publication mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$projection_partial_mutant"
if ! grep -q 'Invariant ReadyImageIsComplete is violated' \
    <<<"$projection_partial_mutant"; then
  echo "stack-safe-pda: projection-publication mutant failed without the expected invariant witness" >&2
  exit 1
fi
if projection_stale_mutant="$({
  tlc_run "$tla_metadir" \
    ParBagProjectionStaleMutant.cfg ParBagProjectionLifecycle.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$projection_stale_mutant"
  echo "stack-safe-pda: stale projection-cache mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$projection_stale_mutant"
if ! grep -q 'Invariant WriterCacheMatchesIdentity is violated' \
    <<<"$projection_stale_mutant"; then
  echo "stack-safe-pda: stale-cache mutant failed without the expected invariant witness" >&2
  exit 1
fi
if projection_read_mutant="$({
  tlc_run "$tla_metadir" \
    ParBagProjectionReadMutant.cfg ParBagProjectionLifecycle.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$projection_read_mutant"
  echo "stack-safe-pda: partial projection-read mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$projection_read_mutant"
if ! grep -q 'Invariant EveryReadIsComplete is violated' \
    <<<"$projection_read_mutant"; then
  echo "stack-safe-pda: partial-read mutant failed without the expected invariant witness" >&2
  exit 1
fi
if projection_clone_loss_mutant="$({
  tlc_run "$tla_metadir" \
    ParBagProjectionCloneLossMutant.cfg ParBagProjectionLifecycle.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$projection_clone_loss_mutant"
  echo "stack-safe-pda: untouched-clone loss mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$projection_clone_loss_mutant"
if ! grep -q 'Invariant UntouchedCloneSurvivesWriterMutation is violated' \
    <<<"$projection_clone_loss_mutant"; then
  echo "stack-safe-pda: clone-loss mutant failed without the expected invariant witness" >&2
  exit 1
fi
tlc_run "$tla_metadir" \
  ParBagProtobufSinkPublication.cfg ParBagProtobufSinkPublication.tla -cleanup
if sink_public_mutant="$({
  tlc_run "$tla_metadir" \
    ParBagProtobufSinkPublicMutant.cfg ParBagProtobufSinkPublication.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$sink_public_mutant"
  echo "stack-safe-pda: public partial-sink mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$sink_public_mutant"
if ! grep -q 'Invariant PublicImageAtomic is violated' \
    <<<"$sink_public_mutant"; then
  echo "stack-safe-pda: partial-sink mutant failed without the expected invariant witness" >&2
  exit 1
fi
if sink_preflight_mutant="$({
  tlc_run "$tla_metadir" \
    ParBagProtobufSinkPreflightMutant.cfg ParBagProtobufSinkPublication.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$sink_preflight_mutant"
  echo "stack-safe-pda: preflight-bypass sink mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$sink_preflight_mutant"
if ! grep -q 'Invariant InvalidInputHasNoPrefix is violated' \
    <<<"$sink_preflight_mutant"; then
  echo "stack-safe-pda: sink preflight mutant failed without the expected invariant witness" >&2
  exit 1
fi
if sink_premature_commit_mutant="$({
  tlc_run "$tla_metadir" \
    ParBagProtobufSinkPrematureCommitMutant.cfg ParBagProtobufSinkPublication.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$sink_premature_commit_mutant"
  echo "stack-safe-pda: premature complete-label sink mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$sink_premature_commit_mutant"
if ! grep -q 'Invariant CommitRequiresCompleteImage is violated' \
    <<<"$sink_premature_commit_mutant"; then
  echo "stack-safe-pda: premature-commit mutant failed without the expected invariant witness" >&2
  exit 1
fi
if sink_rollback_mutant="$({
  tlc_run "$tla_metadir" \
    ParBagProtobufSinkRollbackMutant.cfg ParBagProtobufSinkPublication.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$sink_rollback_mutant"
  echo "stack-safe-pda: private-frame rollback mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$sink_rollback_mutant"
if ! grep -q 'Invariant AbortedPrivateFrameIsDiscarded is violated' \
    <<<"$sink_rollback_mutant"; then
  echo "stack-safe-pda: rollback mutant failed without the expected discard witness" >&2
  exit 1
fi
tlc_run "$tla_metadir" \
  ParBagInFrameMemo.cfg ParBagInFrameMemo.tla -cleanup
for certified_mutant in \
  CensusSkip:ParBagInFrameMemoCensusSkipMutant.cfg:CensusPrefixIsExact \
  ReplaySwap:ParBagInFrameMemoReplaySwapMutant.cfg:ReplayConsumesExactOccurrencePrefix \
  ReplayOverrun:ParBagInFrameMemoReplayOverrunMutant.cfg:ReplayConsumesExactOccurrencePrefix \
  ReplayRemainingCommit:ParBagInFrameMemoReplayRemainingCommitMutant.cfg:CommitRequiresExactReplay \
  EmptyMemo:ParBagInFrameMemoEmptyMemoMutant.cfg:MemoOnlyForCensusProvenRepeatedOwners \
  SingletonMemo:ParBagInFrameMemoSingletonMemoMutant.cfg:MemoOnlyForCensusProvenRepeatedOwners \
  EarlyEmission:ParBagInFrameMemoEarlyEmissionMutant.cfg:CensusCompletesBeforeEmission; do
  certified_label="${certified_mutant%%:*}"
  certified_tail="${certified_mutant#*:}"
  certified_cfg="${certified_tail%%:*}"
  certified_invariant="${certified_tail#*:}"
  if certified_output="$({
    tlc_run "$tla_metadir" "$certified_cfg" ParBagInFrameMemo.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$certified_output"
    echo "stack-safe-pda: ${certified_label} certified-egress mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$certified_output"
  if ! grep -q "Invariant ${certified_invariant} is violated" \
      <<<"$certified_output"; then
    echo "stack-safe-pda: ${certified_label} mutant failed without ${certified_invariant}" >&2
    exit 1
  fi
done
for in_frame_mutant in \
  Swap:ParBagInFrameMemoSwapMutant.cfg \
  Whole:ParBagInFrameMemoWholeMutant.cfg \
  WrongOwner:ParBagInFrameMemoWrongOwnerMutant.cfg \
  OffByOne:ParBagInFrameMemoOffByOneMutant.cfg; do
  in_frame_label="${in_frame_mutant%%:*}"
  in_frame_cfg="${in_frame_mutant#*:}"
  if in_frame_output="$({
    tlc_run "$tla_metadir" "$in_frame_cfg" ParBagInFrameMemo.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$in_frame_output"
    echo "stack-safe-pda: ${in_frame_label} in-frame memo mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$in_frame_output"
  if ! grep -q 'Invariant OutputMatchesDirect is violated' <<<"$in_frame_output"; then
    echo "stack-safe-pda: ${in_frame_label} in-frame mutant failed without the direct-equivalence witness" >&2
    exit 1
  fi
done
tlc_run "$tla_metadir" \
  CertifiedEgressTranscript.cfg CertifiedEgressTranscript.tla -cleanup
for transcript_mutant in \
  IncompleteSeal:CertifiedEgressTranscriptIncompleteSealMutant.cfg:SealIffWalkDoneAndAllComplete \
  DuplicateCompletion:CertifiedEgressTranscriptDuplicateCompletionMutant.cfg:CompletionExactlyOnce \
  EqualLengthKindSwap:CertifiedEgressTranscriptKindSwapMutant.cfg:TypedReplayConsumesExactPrefix \
  ReplayOverrun:CertifiedEgressTranscriptReplayOverrunMutant.cfg:TypeInvariant \
  RemainingCommit:CertifiedEgressTranscriptRemainingCommitMutant.cfg:AtomicPublication \
  SpillSlotCorruption:CertifiedEgressTranscriptSpillCorruptionMutant.cfg:StorageRefinesLogicalTranscript \
  PartialPublication:CertifiedEgressTranscriptPartialPublicationMutant.cfg:AtomicPublication; do
  transcript_label="${transcript_mutant%%:*}"
  transcript_tail="${transcript_mutant#*:}"
  transcript_cfg="${transcript_tail%%:*}"
  transcript_invariant="${transcript_tail#*:}"
  if transcript_output="$({
    tlc_run "$tla_metadir" "$transcript_cfg" CertifiedEgressTranscript.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$transcript_output"
    echo "stack-safe-pda: ${transcript_label} structural-transcript mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$transcript_output"
  if ! grep -q "Invariant ${transcript_invariant} is violated" \
      <<<"$transcript_output"; then
    echo "stack-safe-pda: ${transcript_label} mutant failed without ${transcript_invariant}" >&2
    exit 1
  fi
done
tlc_run "$tla_metadir" \
  Epm1PositionalAssociation.cfg Epm1PositionalAssociation.tla -cleanup
for epm1_positional_mutant in \
  SequentialMarker:Epm1PositionalSequentialMarkerMutant.cfg:MarkersAreZero \
  NonzeroMarker:Epm1PositionalNonzeroMarkerMutant.cfg:MarkersAreZero \
  RootOmission:Epm1PositionalRootOmissionMutant.cfg:AssignmentPrefixIsExact \
  AddressCollapse:Epm1PositionalAddressCollapseMutant.cfg:AssignmentPrefixIsExact \
  SkipEndpoint:Epm1PositionalSkipEndpointMutant.cfg:AssignmentPrefixIsExact \
  WrongEndpoint:Epm1PositionalWrongEndpointMutant.cfg:AssignmentPrefixIsExact \
  DuplicateEndpoint:Epm1PositionalDuplicateEndpointMutant.cfg:AssignmentPrefixIsExact \
  MissingBody:Epm1PositionalMissingBodyMutant.cfg:AssignmentPrefixIsExact \
  ExtraBody:Epm1PositionalExtraBodyMutant.cfg:AssignmentPrefixIsExact \
  EarlyPublish:Epm1PositionalEarlyPublishMutant.cfg:PublicationRequiresCompleteLockstep \
  DoublePublish:Epm1PositionalDoublePublishMutant.cfg:PublicationIsExactlyOnce \
  ErrorPrecedence:Epm1PositionalPrecedenceMutant.cfg:ChildErrorPrecedesDeferredAct; do
  epm1_positional_label="${epm1_positional_mutant%%:*}"
  epm1_positional_tail="${epm1_positional_mutant#*:}"
  epm1_positional_cfg="${epm1_positional_tail%%:*}"
  epm1_positional_invariant="${epm1_positional_tail#*:}"
  if epm1_positional_output="$({
    tlc_run "$tla_metadir" "$epm1_positional_cfg" \
      Epm1PositionalAssociation.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$epm1_positional_output"
    echo "stack-safe-pda: ${epm1_positional_label} EPM1 positional mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$epm1_positional_output"
  if ! grep -q "Invariant ${epm1_positional_invariant} is violated" \
      <<<"$epm1_positional_output"; then
    echo "stack-safe-pda: ${epm1_positional_label} mutant failed without ${epm1_positional_invariant}" >&2
    exit 1
  fi
done
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
