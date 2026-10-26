#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python3 "$repo_root/scripts/build-pathmap-obligation-registry.py" --check
python3 "$repo_root/scripts/build-pathmap-obligation-registry.py" --self-test
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
  local namespace="${4-StackSafePDA}"
  if ! output="$(tlc_bounded "$coqc_bin" -Q theories "$namespace" "$source" 2>&1)"; then
    printf '%s\n' "$output"
    return 1
  fi
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
  rocq_check_modules_at StackSafePDA "$@"
}

rocq_check_modules_at() {
  local namespace="$1" description="$2" output
  shift 2
  if ! output="$(tlc_bounded "$coqchk_bin" -Q theories "$namespace" "$@" 2>&1)"; then
    printf '%s\n' "$output"
    return 1
  fi
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

rpb3_run_tests() {
  local expected="$1" output
  shift
  if ! output="$(tlc_bounded cargo test -p models "$@" -- --nocapture 2>&1)"; then
    printf '%s\n' "$output"
    return 1
  fi
  printf '%s\n' "$output"
  if ! grep -Eq "^test result: ok\\. $expected passed; 0 failed; [0-9]+ ignored;" <<<"$output"; then
    echo "stack-safe-pda: expected $expected nonignored RPB3 tests for $*" >&2
    return 1
  fi
}

rholang_run_tests() {
  local expected="$1" output
  shift
  if ! output="$(TLC_MEMORY_HIGH=4G TLC_RSS=6G TLC_CPU_QUOTA=400% TLC_TASKS_MAX=512 \
      tlc_bounded cargo test --release -j4 -p rholang "$@" -- --test-threads=4 2>&1)"; then
    printf '%s\n' "$output"
    return 1
  fi
  printf '%s\n' "$output"
  if ! grep -Eq "^test result: ok\\. $expected passed; 0 failed; [0-9]+ ignored;" <<<"$output"; then
    echo "stack-safe-pda: expected $expected nonignored Rholang tests for $*" >&2
    return 1
  fi
}

matcher_count_proofs() {
  local TLC_MEMORY_HIGH=1G TLC_RSS=2G TLC_CPU_QUOTA=200% TLC_TASKS_MAX=128
  rocq_reject_placeholders <(
    sed -n '/^Module ParCountIntervals\.$/,/^End ParCountIntervals\.$/p' theories/SpatialMatcher.v
  )
  rocq_compile_with_closed_count theories/SpatialMatcher.v 10 \
    "exact ParCount interval and partition contracts"
  rocq_check_modules "spatial matcher and exact ParCount intervals" StackSafePDA.SpatialMatcher
}

matcher_ground_new_query_proofs() {
  local TLC_MEMORY_HIGH=1G TLC_RSS=2G TLC_CPU_QUOTA=200% TLC_TASKS_MAX=128
  rocq_reject_placeholders theories/ConcreteNewQueryCorrespondence.v
  rocq_compile_with_closed_count theories/ConcreteNewQueryCorrespondence.v 3 \
    "ground New candidate projection and encoded-exact URI negative control"
  rocq_check_modules "ground New candidate correspondence" \
    StackSafePDA.ConcreteNewQueryCorrespondence
}

matcher_count_byte_native_proofs() {
  local TLC_MEMORY_HIGH=1G TLC_RSS=2G TLC_CPU_QUOTA=200% TLC_TASKS_MAX=128
  rocq_reject_placeholders theories/ParCountByteNative.v
  rocq_compile_with_closed_count theories/ParCountByteNative.v 21 \
    "byte-native ParCount projection, cursor and resource contracts"
  rocq_check_modules "byte-native ParCount refinements" StackSafePDA.ParCountByteNative
}

matcher_count_runtime() {
  local PROPTEST_RNG_SEED="${PROPTEST_RNG_SEED:-41315855}"
  export PROPTEST_RNG_SEED
  rholang_run_tests 13 --test matcher_count_intervals
  rholang_run_tests 3 --test matcher_count_folds
  rholang_run_tests 16 --test matcher_connective_isolation
  rholang_run_tests 8 --test matcher_if_family
  rholang_run_tests 60 --lib rust::interpreter::matcher::spatial_matcher_pda_equivalence::
  rholang_run_tests 1 --lib rust::interpreter::matcher::ground_new_query_correspondence::
  rholang_run_tests 11 --lib rust::interpreter::matcher::par_count::byte_native_tests::
  TLC_MEMORY_HIGH=4G TLC_RSS=6G TLC_CPU_QUOTA=400% TLC_TASKS_MAX=512 \
    rpb3_run_tests 3 --release -j4 --test par_bag canonical_count_expr_is_hole
  TLC_MEMORY_HIGH=4G TLC_RSS=6G TLC_CPU_QUOTA=400% TLC_TASKS_MAX=512 \
    rpb3_run_tests 4 --release -j4 --lib par_count_header_
  matcher_count_original_stack_probe
}

matcher_count_original_stack_probe() {
  local output
  if ! output="$(TLC_MEMORY_HIGH=4G TLC_RSS=6G TLC_CPU_QUOTA=400% TLC_TASKS_MAX=512 \
      tlc_bounded env PROBE_WHAT=min_max_par PROBE_DEPTH=4096 PROBE_STACK=131072 \
      cargo test --release -j4 -p rholang --test stack_depth_probe \
      -- --ignored --exact probe --nocapture --test-threads=1 2>&1)"; then
    printf '%s\n' "$output"
    return 1
  fi
  printf '%s\n' "$output"
  if ! grep -Eq '^test result: ok\. 1 passed; 0 failed; 0 ignored;' <<<"$output" \
      || ! grep -q '^PROBE OK depth=4096 stack=131072$' <<<"$output"; then
    echo "stack-safe-pda: the original depth-4096 ParCount probe did not complete" >&2
    return 1
  fi
}

free_map_journal_rocq() {
  local TLC_MEMORY_HIGH=1G TLC_RSS=2G TLC_CPU_QUOTA=200% TLC_TASKS_MAX=128
  local free_map_rocq_output free_map_closed_capstones
  rocq_reject_placeholders theories/FreeMapJournal.v
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
  rocq_check_modules "FreeMap journal refinement" StackSafePDA.FreeMapJournal
}

free_map_journal_verus() {
  local verus_bin
  verus_bin="${VERUS:-$(command -v verus || :)}"
  if [[ -z "$verus_bin" || ! -x "$verus_bin" ]]; then
    echo "stack-safe-pda: Verus not found on PATH" >&2
    return 3
  fi
  tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
    "$repo_root/formal/verus/free_map_transaction.rs"
}

canonical_connective_owner_verus() {
  local model="$repo_root/formal/verus/canonical_connective_owner_cursor.rs"
  local generator="$repo_root/scripts/generate-canonical-connective-owner-mutant.py"
  local evidence_dir="$repo_root/target/verification/connective-owner"
  local baseline_log mutant mutant_source mutant_log expected_obligation
  mkdir -p "$evidence_dir"
  baseline_log="$evidence_dir/normal-gate.log"
  if ! tlc_bounded "$verus_bin" --no-cheating --num-threads 1 --rlimit 30 \
    "$model" >"$baseline_log" 2>&1; then
    cat "$baseline_log"
    echo "stack-safe-pda: canonical Connective owner proof failed" >&2
    return 1
  fi
  if ! rg -q '^verification results:: 10 verified, 0 errors$' "$baseline_log"; then
    cat "$baseline_log"
    echo "stack-safe-pda: expected ten canonical Connective owner verifications" >&2
    return 1
  fi
  for mutant in length_only pointer_only skip_child wrong_range early_exhaustion; do
    mutant_source="$evidence_dir/$mutant.rs"
    mutant_log="$evidence_dir/$mutant.log"
    case "$mutant" in
      length_only|pointer_only) expected_obligation='ensures result == bind_spec(' ;;
      skip_child|wrong_range) expected_obligation='match modeled_next(children, position) {' ;;
      early_exhaustion) expected_obligation='modeled_next(children, position).is_some(),' ;;
    esac
    python3 "$generator" "$mutant" "$model" "$mutant_source"
    if tlc_bounded "$verus_bin" --no-cheating --num-threads 1 --rlimit 30 \
      "$mutant_source" >"$mutant_log" 2>&1; then
      cat "$mutant_log"
      echo "stack-safe-pda: canonical Connective $mutant mutant unexpectedly verified" >&2
      return 1
    fi
    if ! rg -Fq 'error: postcondition not satisfied' "$mutant_log" \
      || ! rg -Fq "$expected_obligation" "$mutant_log"; then
      cat "$mutant_log"
      echo "stack-safe-pda: canonical Connective $mutant failed outside its expected obligation" >&2
      return 1
    fi
    if ! rg -q '^verification results:: [0-9]+ verified, [1-9][0-9]* errors$' "$mutant_log"; then
      cat "$mutant_log"
      echo "stack-safe-pda: canonical Connective $mutant had no verifier counterexample" >&2
      return 1
    fi
  done
  cat "$baseline_log"
  echo "stack-safe-pda: five canonical Connective owner mutants rejected"
}

incremental_ledger_trial_verus() {
  local model="$repo_root/formal/verus/incremental_prepare_streaming.rs"
  local generator="$repo_root/scripts/generate-ledger-trial-verus-mutant.py"
  local evidence_dir="$repo_root/target/verification/ledger-trial"
  local baseline_log mutant mutant_source mutant_log expected_obligation
  mkdir -p "$evidence_dir"
  baseline_log="$evidence_dir/normal-gate.log"
  if ! tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
    "$model" >"$baseline_log" 2>&1; then
    cat "$baseline_log"
    echo "stack-safe-pda: incremental prepare streaming proof failed" >&2
    return 1
  fi
  if ! rg -q '^verification results:: 23 verified, 0 errors$' "$baseline_log"; then
    cat "$baseline_log"
    echo "stack-safe-pda: expected 23 incremental prepare streaming verifications" >&2
    return 1
  fi
  for mutant in partial_commit unchecked_limit scratch_conflation; do
    mutant_source="$evidence_dir/$mutant.rs"
    mutant_log="$evidence_dir/$mutant.log"
    case "$mutant" in
      partial_commit)
        expected_obligation='refused.is_some() ==> *final(usage) == *old(usage),' ;;
      unchecked_limit)
        expected_obligation='Some(total) => trial_reserve_spec(used as int, amount as int, limit as int) == Some(' ;;
      scratch_conflation)
        expected_obligation='refused == first_refused_axis_spec(*old(usage), *charge, *limits),' ;;
    esac
    python3 "$generator" "$mutant" "$model" "$mutant_source"
    if tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
      "$mutant_source" >"$mutant_log" 2>&1; then
      cat "$mutant_log"
      echo "stack-safe-pda: ledger trial $mutant mutant unexpectedly verified" >&2
      return 1
    fi
    if ! rg -Fq 'error: postcondition not satisfied' "$mutant_log" \
      || ! rg -Fq "$expected_obligation" "$mutant_log"; then
      cat "$mutant_log"
      echo "stack-safe-pda: ledger trial $mutant failed outside its expected obligation" >&2
      return 1
    fi
    if ! rg -q '^verification results:: [0-9]+ verified, [1-9][0-9]* errors$' "$mutant_log"; then
      cat "$mutant_log"
      echo "stack-safe-pda: ledger trial $mutant had no verifier counterexample" >&2
      return 1
    fi
  done
  cat "$baseline_log"
  echo "stack-safe-pda: three ledger trial reservation mutants rejected"
}

free_map_journal_tlc() {
  local tla_metadir
  tla_metadir="$(tlc_metadir free-map-journal)"
  pushd "$repo_root/formal/tlaplus/stack_safe_pda" >/dev/null
  tlc_run "$tla_metadir" FreeMapJournal.cfg FreeMapJournal.tla -cleanup
  popd >/dev/null
}

free_map_journal_mcrl2() {
  local mcrl2_root="$repo_root/formal/mcrl2/stack_safe_pda" mcrl2_tmp="$1"
  local free_map_bisimulation free_map_no_leak mutant mutant_bisimulation mutant_no_leak
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
  echo "FreeMap journal mCRL2: baseline bisimulation/no-leak true; both mutants false/false"
}

free_map_journal_runtime() {
  rholang_run_tests 7 --lib free_map_transaction::tests
  rholang_run_tests 60 --lib rust::interpreter::matcher::spatial_matcher_pda_equivalence::
  rholang_run_tests 1 --test matcher_freemap_journal binding_journal_allocation_growth_is_subquadratic_on_success_and_deep_refusal
  rholang_run_tests 1 --test matcher_freemap_journal sole_freevar_captures_a_deep_target_without_decoding_its_suffix
  TLC_MEMORY_HIGH=4G TLC_RSS=6G TLC_CPU_QUOTA=400% TLC_TASKS_MAX=512 \
    rpb3_run_tests 2 --release -j4 --test formal_equivalence_manifest freemap_journal
}

match_preparation_proof_clients() {
  rocq_reject_placeholders \
    theories/TwoLevelRSpaceMatchPreparation.v \
    theories/EncodedRSpaceCandidateCatalog.v \
    theories/TwoLevelRSpaceFlatDeltaTrace.v \
    theories/TwoLevelRSpaceMatchPreparationMutants.v
  rocq_compile_with_closed_count theories/TwoLevelRSpaceMatchPreparation.v 58 \
    "constructive RSpace preparation and grouped delta refinement"
  rocq_compile_with_closed_count theories/EncodedRSpaceCandidateCatalog.v 18 \
    "encoded whole-catalog and lazy semantic refinement"
  rocq_compile_with_closed_count theories/TwoLevelRSpaceFlatDeltaTrace.v 4 \
    "finite flat-delta traces and separate normalized supports"
  rocq_compile_with_closed_count theories/TwoLevelRSpaceMatchPreparationMutants.v 41 \
    "absolute-address and flat-delta mutation controls"
}

rspace_preparation_run_tests() {
  local expected="$1" output
  shift
  if ! output="$(TLC_MEMORY_HIGH=4G TLC_RSS=6G TLC_CPU_QUOTA=400% TLC_TASKS_MAX=512 \
      tlc_bounded cargo test --release -j4 -p rspace_plus_plus "$@" -- --test-threads=4 2>&1)"; then
    printf '%s\n' "$output"
    return 1
  fi
  printf '%s\n' "$output"
  if ! grep -Eq "^test result: ok\\. $expected passed; 0 failed; [0-9]+ ignored;" <<<"$output"; then
    echo "stack-safe-pda: expected $expected nonignored RSpace preparation tests for $*" >&2
    return 1
  fi
}

epathmap_import_proofs() {
  local TLC_MEMORY_HIGH=1G TLC_RSS=2G TLC_CPU_QUOTA=200% TLC_TASKS_MAX=128
  rocq_reject_placeholders theories/CanonicalEPathMapImport.v
  rocq_compile_with_closed_count theories/CanonicalEPathMapImport.v 48 \
    "canonical Var, envelope, carrier, signed GInt and singleton chain contracts"
  rocq_check_modules "canonical singleton EPathMap import" StackSafePDA.CanonicalEPathMapImport
}

opaque_canonical_segment_import_proofs() {
  local TLC_MEMORY_HIGH=1G TLC_RSS=2G TLC_CPU_QUOTA=200% TLC_TASKS_MAX=128
  rocq_reject_placeholders theories/OpaqueCanonicalSegmentImport.v
  rocq_compile_with_closed_count theories/OpaqueCanonicalSegmentImport.v 10 \
    "opaque stable segment, split path, rewrap and dropped-segment mutant"
  rocq_check_modules "opaque canonical segment import" StackSafePDA.OpaqueCanonicalSegmentImport
}

epathmap_score_proofs() {
  local TLC_MEMORY_HIGH=1G TLC_RSS=2G TLC_CPU_QUOTA=200% TLC_TASKS_MAX=128
  rocq_reject_placeholders theories/CanonicalEPathMapScore.v
  rocq_compile_with_closed_count theories/CanonicalEPathMapScore.v 17 \
    "concrete EPathMap score bytes, endpoint framing and checked capacity"
  rocq_check_modules "concrete EPathMap score" StackSafePDA.CanonicalEPathMapScore
}

canonical_key_protocol_proofs() {
  local TLC_MEMORY_HIGH=1536M TLC_RSS=2G TLC_CPU_QUOTA=100% TLC_TASKS_MAX=64
  rocq_reject_placeholders theories/RPB3Admission.v
  rocq_compile_with_closed_count theories/RPB3Admission.v 365 \
    "RPB3 fold, canonical-key transport and storage contracts"
  rocq_check_modules "standalone canonical-key transport" StackSafePDA.RPB3Admission
  canonical_act_batch_publication_proofs
  canonical_source_release_proofs
  rocq_reject_placeholders theories/IncrementalPrepareStreaming.v
  rocq_compile_with_closed_count theories/IncrementalPrepareStreaming.v 15 \
    "ordered source chunk folds and incremental preparation contracts"
  rocq_check_modules "ordered source chunk folds" StackSafePDA.IncrementalPrepareStreaming
}

canonical_sexpr_projection_proofs() {
  local TLC_MEMORY_HIGH=1536M TLC_RSS=2G TLC_CPU_QUOTA=100% TLC_TASKS_MAX=64
  rocq_reject_placeholders theories/CanonicalSexprProjection.v
  rocq_compile_with_closed_count theories/CanonicalSexprProjection.v 27 \
    "S-expression streaming projection, multiplicity and canonical-field view laws"
  rocq_check_modules "S-expression canonical-field projection" StackSafePDA.CanonicalSexprProjection
}

pathmap_leaf_extension_proofs() {
  local TLC_MEMORY_HIGH=1536M TLC_RSS=2G TLC_CPU_QUOTA=100% TLC_TASKS_MAX=64
  rocq_reject_placeholders theories/PathMapLeafExtension.v
  rocq_compile_with_closed_count theories/PathMapLeafExtension.v 62 \
    "leaf extension, removal dispatch, unit union, graft preparation, ownership and virtual cursor laws"
  rocq_check_modules "PathMap leaf-extension contract" StackSafePDA.PathMapLeafExtension
}

pathmap_leaf_extension_prerequisites() {
  local TLC_MEMORY_HIGH=1536M TLC_RSS=2G TLC_CPU_QUOTA=100% TLC_TASKS_MAX=64
  rocq_reject_placeholders theories/PathMapPhysicalOwnership.v theories/PathMapReversePruneTeardown.v
  rocq_compile_with_closed_count theories/PathMapPhysicalOwnership.v 12 \
    "PathMap leaf-extension physical-ownership prerequisite"
  rocq_compile_with_closed_count theories/PathMapReversePruneTeardown.v 32 \
    "PathMap leaf-extension disposal prerequisite"
}

canonical_act_batch_publication_proofs() {
  local TLC_MEMORY_HIGH=1536M TLC_RSS=2G TLC_CPU_QUOTA=100% TLC_TASKS_MAX=64
  rocq_reject_placeholders theories/CanonicalActBatchPublication.v
  rocq_compile_with_closed_count theories/CanonicalActBatchPublication.v 21 \
    "ACT batch publication, exact scalar authority and constructive acceptance"
  rocq_check_modules "ACT batch publication" StackSafePDA.CanonicalActBatchPublication
}

canonical_source_release_proofs() {
  local TLC_MEMORY_HIGH=512M TLC_RSS=1G TLC_CPU_QUOTA=100% TLC_TASKS_MAX=64
  rocq_reject_placeholders theories/CanonicalSourceRelease.v
  rocq_compile_with_closed_count theories/CanonicalSourceRelease.v 68 \
    "canonical source ownership, release traces and ranked-frontier prerequisites"
  rocq_check_modules "canonical source release prerequisites" StackSafePDA.CanonicalSourceRelease
}

epathmap_import_runtime() {
  local TLC_MEMORY_HIGH=4G TLC_RSS=6G TLC_CPU_QUOTA=400% TLC_TASKS_MAX=512
  local -x RUST_TEST_THREADS=4
  local -x PROPTEST_RNG_SEED="${PROPTEST_RNG_SEED:-41315976}"
  rpb3_run_tests 5 --release -j4 --lib singleton_prefilter
  rpb3_run_tests 1 --release -j4 --lib canonical_var_cursor_accepts_exact_encoder_images_only
  rpb3_run_tests 3 --release -j4 --lib canonical_var_proof_
  rpb3_run_tests 5 --release -j4 --lib canonical_epathmap_envelope_proof_
  rpb3_run_tests 5 --release -j4 --lib canonical_epathmap_carrier_proof_
  rpb3_run_tests 5 --release -j4 --lib singleton_chain_proof_
  rpb3_run_tests 4 --release -j4 --lib canonical_epathmap_score_proof_
  rpb3_run_tests 5 --release -j4 --lib singleton_epathmap
  rpb3_run_tests 1 --release -j4 --lib \
    owned_canonical_epathmap_component_is_infallible_from_every_admission_lane
  rpb3_run_tests 37 --release -j4 --lib standalone_path_admission
  rpb3_run_tests 37 -j4 --lib standalone_path_admission
  rpb3_run_tests 26 --release -j4 --lib canonical_source_tree
  rpb3_run_tests 26 -j4 --lib canonical_source_tree
  rpb3_run_tests 33 --release -j4 --lib canonical_bytes_arena
  rpb3_run_tests 33 -j4 --lib canonical_bytes_arena
  rpb3_run_tests 8 --release -j4 --lib opaque_interior_tests
  rpb3_run_tests 3 --release -j4 --lib opaque_canonical_segment_import_properties::
  rpb3_run_tests 3 -j4 --lib opaque_canonical_segment_import_properties::
  rpb3_run_tests 38 --release -j4 --lib frame_storage_tests
  rpb3_run_tests 38 -j4 --lib frame_storage_tests
  rpb3_run_tests 3 --release -j4 --lib line_batch_tests
  rpb3_run_tests 3 -j4 --lib line_batch_tests
  rpb3_run_tests 6 --release -j4 --lib indexed_span_transport
  rpb3_run_tests 6 -j4 --lib indexed_span_transport
  rpb3_run_tests 15 --release -j4 --lib normalized_source_spans
  rpb3_run_tests 15 -j4 --lib normalized_source_spans
  rpb3_run_tests 5 --release -j4 --lib byte_policy_tests
  rpb3_run_tests 15 --release -j4 --lib rust::par_to_sexpr::tests
  rpb3_run_tests 15 -j4 --lib rust::par_to_sexpr::tests
}

act_line_transport_runtime() {
  rspace_preparation_run_tests 7 --locked --lib checked_line_transport
  act_line_transport_compile_controls
}

act_line_transport_compile_controls() {
  local TLC_MEMORY_HIGH=4G TLC_RSS=6G TLC_CPU_QUOTA=400% TLC_TASKS_MAX=512
  local target="${CARGO_TARGET_DIR:-$repo_root/target}" fixture case_name expected_code output
  [[ "$target" = /* ]] || target="$repo_root/$target"
  fixture="$repo_root/rspace++/tests/compile_fail/act_line_span_borrows.rs"
  tlc_bounded cargo build --release --locked -j4 -p rspace_plus_plus --lib --target-dir "$target"
  local result_dir="$target/verification/act-line-span-borrows"
  mkdir -p "$result_dir"
  local compiler=(rustc --edition=2021 --crate-type=lib --emit=metadata
    --extern "rspace_plus_plus=$target/release/librspace_plus_plus.rlib"
    -L "dependency=$target/release/deps" "$fixture")
  if ! output="$(tlc_bounded "${compiler[@]}" -o "$result_dir/positive.rmeta" 2>&1)"; then
    printf '%s\n' "$output"
    return 1
  fi
  printf '%s\n' "$output"
  for case_name in live_rewind live_next_event live_replace double_commit clone_offer copy_offer escape_cursor substitute_cursor \
    prepared_live_rewind prepared_live_next_event prepared_live_replace prepared_double_commit \
    prepared_clone_offer prepared_copy_offer prepared_escape_cursor prepared_substitute_source; do
    case "$case_name" in
      live_rewind|live_next_event|prepared_live_rewind|prepared_live_next_event) expected_code=E0499 ;;
      live_replace|prepared_live_replace) expected_code=E0506 ;;
      double_commit|prepared_double_commit) expected_code=E0382 ;;
      clone_offer|prepared_clone_offer) expected_code=E0599 ;;
      copy_offer|prepared_copy_offer) expected_code=E0277 ;;
      escape_cursor|prepared_escape_cursor) expected_code=E0515 ;;
      substitute_cursor|prepared_substitute_source) expected_code=E0616 ;;
    esac
    if output="$(tlc_bounded "${compiler[@]}" --cfg "$case_name" -o "$result_dir/$case_name.rmeta" 2>&1)"; then
      printf '%s\n' "$output"
      echo "stack-safe-pda: ACT span negative control $case_name unexpectedly compiled" >&2
      return 1
    fi
    printf '%s\n' "$output"
    if ! grep -Fq "error[$expected_code]" <<<"$output"; then
      echo "stack-safe-pda: ACT span control $case_name did not produce $expected_code" >&2
      return 1
    fi
  done
  echo "stack-safe-pda: ACT span positive compile and sixteen exact compiler-rejection controls passed"
}

if [[ "${1:-}" == "--act-line-transport" ]]; then
  pushd "$repo_root" >/dev/null
  act_line_transport_runtime
  popd >/dev/null
  exit 0
fi

if [[ "${1:-}" == "--act-line-transport-compile" ]]; then
  pushd "$repo_root" >/dev/null
  act_line_transport_compile_controls
  popd >/dev/null
  echo "stack-safe-pda: ACT capability compiler checks passed; runtime qualification is separate"
  exit 0
fi

if [[ "${1:-}" == "--canonical-source-release" ]]; then
  pushd "$rocq_root" >/dev/null
  canonical_source_release_proofs
  popd >/dev/null
  echo "stack-safe-pda: canonical source release prerequisite proofs passed"
  exit 0
fi

if [[ "${1:-}" == "--canonical-key-proofs" ]]; then
  pushd "$rocq_root" >/dev/null
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/StackSafePDA.v
  canonical_key_protocol_proofs
  canonical_sexpr_projection_proofs
  popd >/dev/null
  echo "stack-safe-pda: canonical-key prerequisite proofs passed; runtime qualification is separate"
  exit 0
fi

if [[ "${1:-}" == "--pathmap-leaf-extension-proofs" ]]; then
  export TLC_MEMORY_HIGH=1536M TLC_RSS=2G TLC_CPU_QUOTA=100% TLC_TASKS_MAX=64
  pushd "$rocq_root" >/dev/null
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/StackSafePDA.v
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/EPathMap.v
  pathmap_leaf_extension_prerequisites
  pathmap_leaf_extension_proofs
  popd >/dev/null
  echo "stack-safe-pda: leaf-extension model proofs passed; native qualification is separate"
  exit 0
fi

if [[ "${1:-}" == "--epathmap-import" ]]; then
  export TLC_MEMORY_HIGH=1G TLC_RSS=2G TLC_CPU_QUOTA=200% TLC_TASKS_MAX=128
  pushd "$rocq_root" >/dev/null
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/StackSafePDA.v
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/EPathMap.v
  rocq_reject_placeholders theories/EPM1.v
  rocq_compile_with_closed_count theories/EPM1.v 7 "EPM1 mode and positional-marker regressions"
  rocq_check_modules "EPM1 mode contracts" StackSafePDA.EPM1
  epathmap_import_proofs
  opaque_canonical_segment_import_proofs
  rocq_compile_with_closed_count theories/ChannelImage.v 11 "RCI2 byte framing"
  rocq_compile_with_closed_count theories/RhoPathV1Datum.v 21 "RhoPath zero framing"
  epathmap_score_proofs
  canonical_key_protocol_proofs
  canonical_sexpr_projection_proofs
  pathmap_leaf_extension_prerequisites
  pathmap_leaf_extension_proofs
  popd >/dev/null
  pushd "$repo_root" >/dev/null
  epathmap_import_runtime
  act_line_transport_runtime
  popd >/dev/null
  echo "stack-safe-pda: focused Var/EPathMap proof and runtime gates passed"
  exit 0
fi

if [[ "${1:-}" == "--match-preparation" ]]; then
  export PROPTEST_RNG_SEED="${PROPTEST_RNG_SEED:-41316588}"
  export TLC_MEMORY_HIGH=1G TLC_RSS=2G TLC_CPU_QUOTA=200% TLC_TASKS_MAX=128
  pushd "$rocq_root" >/dev/null
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/PathMapCanonicalStreamPublicationBase.v
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/TwoLevelRSpaceAuthority.v
  rocq_compile_with_closed_count theories/RhoPathV1Datum.v 21 "exact datum framing and partition"
  match_preparation_proof_clients
  rocq_check_modules "RSpace preparation, trace refinement and mutation controls" \
    StackSafePDA.TwoLevelRSpaceMatchPreparation \
    StackSafePDA.EncodedRSpaceCandidateCatalog \
    StackSafePDA.TwoLevelRSpaceFlatDeltaTrace \
    StackSafePDA.TwoLevelRSpaceMatchPreparationMutants
  popd >/dev/null
  pushd "$repo_root" >/dev/null
  rspace_preparation_run_tests 69 --lib rspace::data_index::
  rspace_preparation_run_tests 1 --lib catalog_hash_preimages_are_exact_raw_identity_bytes
  rspace_preparation_run_tests 15 --lib accepted_match_formal_bindings
  rspace_preparation_run_tests 28 --test data_index_spec
  rspace_preparation_run_tests 5 --test serializer_byte_goldens
  rspace_preparation_run_tests 2 --test bag_spec transaction_delta
  rspace_preparation_run_tests 1 --test bag_spec multiplicity_rejects_zero_and_detects_overflow
  rspace_preparation_run_tests 24 --test rho_path_spec
  TLC_MEMORY_HIGH=1792M TLC_RSS=2G TLC_CPU_QUOTA=400% TLC_TASKS_MAX=512 \
    rpb3_run_tests 1 --release --locked -j4 --test cold_store_records \
      par_typed_datum_decode_matches_the_derived_oracle
  popd >/dev/null
  exit 0
fi

if [[ "${1:-}" == "--free-map-journal" ]]; then
  pushd "$rocq_root" >/dev/null
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/StackSafePDA.v
  matcher_count_proofs
  free_map_journal_rocq
  popd >/dev/null
  free_map_journal_verus
  free_map_journal_tlc
  mkdir -p "$repo_root/target"
  free_map_journal_mcrl2 "$(mktemp -d "$repo_root/target/free-map-journal-mcrl2.XXXXXX")"
  pushd "$repo_root" >/dev/null
  free_map_journal_runtime
  popd >/dev/null
  echo "stack-safe-pda: focused FreeMap journal four-backend and 69-test gate passed"
  exit 0
fi

if [[ "${1:-}" == "--matcher-counts" || "${1:-}" == "--matcher-count-byte-native-proofs" ]]; then
  pushd "$rocq_root" >/dev/null
  TLC_MEMORY_HIGH=1G TLC_RSS=2G TLC_CPU_QUOTA=200% TLC_TASKS_MAX=128 \
    tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/StackSafePDA.v
  matcher_count_proofs
  matcher_count_byte_native_proofs
  if [[ "${1:-}" == "--matcher-count-byte-native-proofs" ]]; then
    popd >/dev/null
    exit 0
  fi
  popd >/dev/null
  pushd "$repo_root" >/dev/null
  matcher_count_runtime
  popd >/dev/null
  exit 0
fi

if [[ "${1:-}" == "--ground-new-query-proofs" ]]; then
  pushd "$rocq_root" >/dev/null
  matcher_ground_new_query_proofs
  popd >/dev/null
  echo "stack-safe-pda: ground New query correspondence and URI negative control passed"
  exit 0
fi

pushd "$rocq_root" >/dev/null
tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/StackSafePDA.v
rocq_reject_placeholders theories/EPathMap.v
rocq_compile_with_closed_count theories/EPathMap.v 9 \
  "set subtraction and restriction under prefix-overlapping keys"
tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/Bag.v
rocq_reject_placeholders theories/ScalarProjection.v
rocq_compile_with_closed_count theories/ScalarProjection.v 10 \
  "the exact scalar-projection admission and fallback contract"
rocq_check_modules "scalar-projection admission" StackSafePDA.ScalarProjection
rocq_reject_placeholders theories/ReplayArchive.v theories/ReplayArchiveMutants.v
rocq_compile_with_closed_count theories/ReplayArchive.v 34 \
  "the captured replay archive acceptance contract"
rocq_compile_with_closed_count theories/ReplayArchiveMutants.v 21 \
  "the replay archive nonvacuity and omission controls"
rocq_check_modules "captured replay archive" \
  StackSafePDA.ReplayArchive StackSafePDA.ReplayArchiveMutants
rocq_compile_with_closed_count theories/ChannelImage.v 11 \
  "the channel-image codec capstones"
python3 "$repo_root/scripts/check-rci2-channel-image-obligations.py" --check
python3 "$repo_root/scripts/check-rci2-channel-image-obligations.py" --self-test
rocq_reject_placeholders theories/RSpaceCallerAuthority.v theories/RSpaceCallerAuthorityMutants.v
rocq_compile_with_closed_count theories/RSpaceCallerAuthority.v 36 \
  "the exact caller-authority contract capstones"
rocq_compile_with_closed_count theories/RSpaceCallerAuthorityMutants.v 43 \
  "the caller-authority negative controls"
rocq_check_modules "RSpace caller-authority contract" \
  StackSafePDA.RSpaceCallerAuthority StackSafePDA.RSpaceCallerAuthorityMutants
rocq_reject_placeholders theories/RSpaceConsensusIdentity.v theories/RSpaceConsensusIdentityMutants.v
rocq_compile_with_closed_count theories/RSpaceConsensusIdentity.v 28 \
  "the preserved legacy consensus-identity contract capstones"
rocq_compile_with_closed_count theories/RSpaceConsensusIdentityMutants.v 10 \
  "the consensus-identity nonvacuity witness and negative controls"
rocq_check_modules "RSpace consensus-identity contract" \
  StackSafePDA.RSpaceConsensusIdentity StackSafePDA.RSpaceConsensusIdentityMutants
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
  theories/PathMapPhysicalOwnership.v 12 \
  "the eleven-invariant PathMap ownership authority and four live-write capstones"
rocq_compile_with_closed_count \
  theories/PathMapPhysicalOwnershipMutants.v 3 \
  "the eleven-mutant PathMap ownership controls and live-write negative controls"
rocq_check_modules "PathMap physical ownership" \
  StackSafePDA.PathMapPhysicalOwnership \
  StackSafePDA.PathMapPhysicalOwnershipMutants
rocq_compile_with_closed_count \
  theories/PathMapReversePruneTeardown.v 32 \
  "the exact 25-primary/10-quarantine PathMap teardown authority, scoped policies and eight navigation invariants"
rocq_compile_with_closed_count \
  theories/PathMapReversePruneTeardownMutants.v 13 \
  "the exact 27-teardown/13-quarantine PathMap teardown controls"
rocq_check_modules "PathMap reverse teardown" \
  StackSafePDA.PathMapReversePruneTeardown \
  StackSafePDA.PathMapReversePruneTeardownMutants
pathmap_leaf_extension_proofs
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
  theories/ChannelPrefixReplacementMutants.v \
  theories/RPB3Admission.v \
  theories/RPB3AdmissionMutants.v
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
canonical_key_protocol_proofs
canonical_sexpr_projection_proofs
rpb3_admission_mutants_rocq_output="$(
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA theories/RPB3AdmissionMutants.v 2>&1
)"
printf '%s\n' "$rpb3_admission_mutants_rocq_output"
rpb3_admission_mutants_closed_capstones="$({
  printf '%s\n' "$rpb3_admission_mutants_rocq_output" | grep -c '^Closed under the global context$'
} || true)"
if [[ "$rpb3_admission_mutants_closed_capstones" != "2" ]]; then
  echo "stack-safe-pda: expected both RPB3 admission mutant capstones to be closed; got $rpb3_admission_mutants_closed_capstones" >&2
  exit 1
fi
rocq_check_modules "RPB3 admission" \
  StackSafePDA.RPB3Admission \
  StackSafePDA.RPB3AdmissionMutants
generation_checkpoint_rocq_output="$(
  tlc_bounded "$coqc_bin" -Q theories StackSafePDA \
    theories/GenerationCheckpointPublication.v 2>&1
)"
printf '%s\n' "$generation_checkpoint_rocq_output"
generation_checkpoint_closed_capstones="$({
  printf '%s\n' "$generation_checkpoint_rocq_output" | \
    grep -c '^Closed under the global context$'
} || true)"
if [[ "$generation_checkpoint_closed_capstones" != "112" ]]; then
  echo "stack-safe-pda: expected all 112 publication/journal/inventory/cursor/work/checked-arithmetic Rocq capstones to be closed; got $generation_checkpoint_closed_capstones" >&2
  exit 1
fi
rocq_reject_placeholders theories/IncrementalLedgerTrial.v
rocq_compile_with_closed_count theories/IncrementalLedgerTrial.v 20 \
  "failure-atomic bounded private ledger trials"
rocq_check_modules "failure-atomic bounded private ledger trials" \
  StackSafePDA.GenerationCheckpointPublication StackSafePDA.IncrementalLedgerTrial
rocq_reject_placeholders theories/IncrementalDomainComposition.v
rocq_compile_with_closed_count theories/IncrementalDomainComposition.v 11 \
  "current, retained and metadata inventory composition"
rocq_check_modules "current, retained and metadata inventory composition" \
  StackSafePDA.GenerationCheckpointPublication StackSafePDA.IncrementalLedgerTrial \
  StackSafePDA.IncrementalDomainComposition
rocq_reject_placeholders theories/CreditedClosureLedger.v
rocq_compile_with_closed_count theories/CreditedClosureLedger.v 8 \
  "credited cold construction of the per-domain closure ledgers"
rocq_check_modules "credited cold construction of the per-domain closure ledgers" \
  StackSafePDA.GenerationCheckpointPublication StackSafePDA.IncrementalLedgerTrial \
  StackSafePDA.IncrementalDomainComposition StackSafePDA.CreditedClosureLedger
rocq_reject_placeholders theories/RetentionV2Format.v
rocq_compile_with_closed_count theories/RetentionV2Format.v 21 \
  "shared retention V2 format and V1 migration"
rocq_check_modules "shared retention V2 format and V1 migration" \
  StackSafePDA.ChannelImage StackSafePDA.GenerationCheckpointPublication \
  StackSafePDA.IncrementalLedgerTrial StackSafePDA.IncrementalDomainComposition \
  StackSafePDA.RetentionV2Format
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
rocq_reject_placeholders theories/EPM1.v
rocq_compile_with_closed_count theories/EPM1.v 7 "EPM1 mode and positional-marker regressions"
rocq_check_modules "EPM1 mode contracts" StackSafePDA.EPM1
epathmap_import_proofs
opaque_canonical_segment_import_proofs
rocq_reject_placeholders theories/UnaryEListConstruction.v theories/UnaryEListConstructionMutants.v
rocq_compile_with_closed_count theories/UnaryEListConstruction.v 22 \
  "concrete unary-EList protobuf, score and checked construction"
rocq_compile_with_closed_count theories/UnaryEListConstructionMutants.v 14 \
  "unary-EList encoding, metadata and refusal controls"
rocq_check_modules "unary-EList construction" \
  StackSafePDA.UnaryEListConstruction StackSafePDA.UnaryEListConstructionMutants
rocq_reject_placeholders theories/SingletonMapConstruction.v
rocq_compile_with_closed_count theories/SingletonMapConstruction.v 18 \
  "fixed-key singleton EPathMap construction and checked publication"
rocq_check_modules "singleton EPathMap construction" StackSafePDA.SingletonMapConstruction
rocq_reject_placeholders theories/CollectionCollisionElision.v
rocq_compile_with_closed_count theories/CollectionCollisionElision.v 20 \
  "empty and singleton collection collision-elision equivalence"
rocq_check_modules "collection collision elision" StackSafePDA.CollectionCollisionElision
rocq_reject_placeholders theories/DecodedComponentConstruction.v
rocq_compile_with_closed_count theories/DecodedComponentConstruction.v 73 \
  "decoded incomplete-component planned construction"
rocq_check_modules "decoded incomplete-component construction" StackSafePDA.DecodedComponentConstruction
rocq_reject_placeholders theories/DecoderOracleRouting.v
rocq_compile_with_closed_count theories/DecoderOracleRouting.v 10 \
  "test-only frozen decoder routing"
rocq_check_modules "test-only frozen decoder routing" StackSafePDA.DecoderOracleRouting
rocq_reject_placeholders theories/ProtobufParAccumulation.v
rocq_compile_with_closed_count theories/ProtobufParAccumulation.v 41 \
  "ordered protobuf Par field accumulation"
python3 "$repo_root/scripts/check-parbag-egress-obligations.py" --check
python3 "$repo_root/scripts/check-parbag-egress-obligations.py" --self-test
rocq_check_modules "ordered protobuf Par field accumulation" StackSafePDA.ProtobufParAccumulation
rocq_reject_placeholders theories/EpmPayloadRepresentation.v
rocq_compile_with_closed_count theories/EpmPayloadRepresentation.v 49 \
  "EPM payload representation and scaffold erasure"
rocq_check_modules "EPM payload representation and scaffold erasure" StackSafePDA.EpmPayloadRepresentation
rocq_reject_placeholders theories/ProtobufOwnedSlots.v
rocq_compile_with_closed_count theories/ProtobufOwnedSlots.v 51 \
  "protobuf owned slot projection and schema rank"
rocq_check_modules "protobuf owned slot projection and schema rank" StackSafePDA.ProtobufOwnedSlots
rocq_reject_placeholders theories/ProtobufDecoderComposition.v
rocq_compile_with_closed_count theories/ProtobufDecoderComposition.v 26 \
  "protobuf decoder control composition"
rocq_check_modules "protobuf decoder control composition" StackSafePDA.ProtobufDecoderComposition
rocq_reject_placeholders theories/EpmOwnedRetirement.v
rocq_compile_with_closed_count theories/EpmOwnedRetirement.v 32 \
  "stateful EPM barriers and affine ownership retirement"
rocq_check_modules "stateful EPM ownership retirement" StackSafePDA.EpmOwnedRetirement
rocq_reject_placeholders theories/DepthProbeClassification.v
rocq_compile_with_closed_count theories/DepthProbeClassification.v 28 \
  "depth-probe termination classification"
rocq_check_modules "depth-probe termination classification" StackSafePDA.DepthProbeClassification
rocq_reject_placeholders theories/DepthProbeSearch.v
rocq_compile_with_closed_count theories/DepthProbeSearch.v 21 \
  "typed cap-first depth-probe search"
rocq_check_modules "typed depth-probe search" StackSafePDA.DepthProbeSearch
rocq_reject_placeholders theories/DepthProbeLifecycle.v
rocq_compile_with_closed_count theories/DepthProbeLifecycle.v 29 \
  "bounded child-probe lifecycle"
rocq_check_modules "bounded child-probe lifecycle" StackSafePDA.DepthProbeLifecycle
rocq_reject_placeholders theories/SplitCarrierPrefilter.v
rocq_compile_with_closed_count theories/SplitCarrierPrefilter.v 20 \
  "conditional split-carrier classification equivalence"
rocq_check_modules "split-carrier prefilter" StackSafePDA.SplitCarrierPrefilter
rocq_reject_placeholders theories/SubstitutionRegionRanges.v
rocq_compile_with_closed_count theories/SubstitutionRegionRanges.v 18 \
  "substitution immutable region bounds and ordered cut ordinals"
rocq_check_modules "substitution region ranges" StackSafePDA.SubstitutionRegionRanges
rocq_reject_placeholders theories/SubstitutionRegionEmission.v
rocq_compile_with_closed_count theories/SubstitutionRegionEmission.v 35 \
  "conditional three-mode substitution region emission and checked lengths"
rocq_check_modules "substitution region emission" StackSafePDA.SubstitutionRegionEmission
rocq_reject_placeholders theories/SubstitutionRegionPublication.v
rocq_compile_with_closed_count theories/SubstitutionRegionPublication.v 21 \
  "postorder substitution region publication and certified source reconstruction"
rocq_check_modules "substitution region publication" StackSafePDA.SubstitutionRegionPublication
rocq_reject_placeholders theories/SubstitutionRegionFraming.v
rocq_compile_with_closed_count theories/SubstitutionRegionFraming.v 19 \
  "canonical substitution field framing and exact source partition"
rocq_check_modules "substitution region framing" StackSafePDA.SubstitutionRegionFraming
rocq_reject_placeholders theories/SubstitutionVarintAccumulator.v
rocq_compile_with_closed_count theories/SubstitutionVarintAccumulator.v 20 \
  "native substitution varint accumulator and canonical-width equivalence"
rocq_check_modules "substitution varint accumulator" StackSafePDA.SubstitutionVarintAccumulator
rocq_reject_placeholders theories/SubstitutionRegionDecomposition.v
rocq_compile_with_closed_count theories/SubstitutionRegionDecomposition.v 34 \
  "substitution field decomposition and suspended-parent source certificates"
rocq_check_modules "substitution region decomposition" StackSafePDA.SubstitutionRegionDecomposition
rocq_reject_placeholders theories/SubstitutionSchedule.v
rocq_compile_with_closed_count theories/SubstitutionSchedule.v 21 \
  "substitution binder-family schedules and result placement"
rocq_check_modules "substitution binder-family schedules" StackSafePDA.SubstitutionSchedule
rocq_reject_placeholders theories/SubstitutionCost.v
rocq_compile_with_closed_count theories/SubstitutionCost.v 24 \
  "substitution cost-event accounting and conditional partition budget"
rocq_check_modules "substitution cost-event accounting" StackSafePDA.SubstitutionCost
rocq_reject_placeholders theories/SubstitutionRegionTables.v
rocq_compile_with_closed_count theories/SubstitutionRegionTables.v 30 \
  "append-only substitution tables and supported publication transport"
rocq_check_modules "substitution region tables" StackSafePDA.SubstitutionRegionTables
rocq_reject_placeholders theories/SubstitutionRegionOrdinals.v
rocq_compile_with_closed_count theories/SubstitutionRegionOrdinals.v 16 \
  "input-bounded native occurrence and EPM1 enumeration ordinals"
rocq_check_modules "substitution region ordinals" StackSafePDA.SubstitutionRegionOrdinals
rocq_reject_placeholders theories/SubstitutionRegionProvenance.v
rocq_compile_with_closed_count theories/SubstitutionRegionProvenance.v 42 \
  "region-backed path and table registration provenance"
rocq_check_modules "substitution region provenance" StackSafePDA.SubstitutionRegionProvenance
rocq_reject_placeholders theories/SubstitutionValueRegions.v
rocq_compile_with_closed_count theories/SubstitutionValueRegions.v 22 \
  "exact physical EPM1 value-range selection and retained provenance"
rocq_check_modules "substitution value regions" StackSafePDA.SubstitutionValueRegions
rocq_reject_placeholders theories/SubstitutionSnapshotPrefix.v
rocq_compile_with_closed_count theories/SubstitutionSnapshotPrefix.v 12 \
  "EPM1 prefix offsets and complete framing reconstruction"
rocq_check_modules "substitution snapshot prefix" StackSafePDA.SubstitutionSnapshotPrefix
python3 "$repo_root/scripts/check-rmp2-obligations.py" --check
python3 "$repo_root/scripts/check-rmp2-obligations.py" --self-test
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
matcher_count_proofs
matcher_count_byte_native_proofs
matcher_ground_new_query_proofs
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceAuthority.v 5 \
  "the exact two-level RSpace logical authority"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceOperations.v 8 \
  "the exact two-level RSpace operation authority"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceDelta.v 8 \
  "the exact two-level RSpace delta authority"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceRepresentation.v 5 \
  "the exact two-level RSpace representation authority"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceGuardedOwnership.v 6 \
  "the exact two-level RSpace guarded-ownership authority"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceCheckpointCodec.v 4 \
  "the exact RBA1 codec authority"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceCheckpointStreaming.v 4 \
  "the exact RBA1 streaming authority"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceCheckpointCompatibility.v 5 \
  "the exact RBA1 compatibility authority"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceAuthorityMutants.v 3 \
  "the two-level RSpace logical-authority causal controls"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceOperationsMutants.v 2 \
  "the two-level RSpace operation causal controls"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceDeltaMutants.v 3 \
  "the two-level RSpace delta causal controls"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceRepresentationMutants.v 3 \
  "the two-level RSpace representation causal controls"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceGuardedOwnershipMutants.v 2 \
  "the two-level RSpace guarded-ownership causal controls"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceCheckpointMutants.v 3 \
  "the RBA1 codec, streaming, and compatibility causal controls"
rocq_compile_with_closed_count \
  theories/RhoPathV1Datum.v 21 \
  "the exact RhoPath V1 datum-key schema and partition"
rocq_compile_with_closed_count \
  theories/RhoPathV2.v 4 \
  "the RhoPath V2 outer-grammar capstones"
epathmap_score_proofs
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceContinuationAuthority.v 39 \
  "the exact two-level RSpace continuation and join authority"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceContinuationAuthorityMutants.v 34 \
  "the continuation and join authority causal controls"
match_preparation_proof_clients
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceMatchTransaction.v 25 \
  "the exact two-level RSpace accepted-match authority"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceMatchTransactionMutants.v 20 \
  "the two-level RSpace accepted-match causal controls"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceCheckpointRecovery.v 14 \
  "the exact separated-generation RBA1 recovery authority"
rocq_compile_with_closed_count \
  theories/TwoLevelRSpaceCheckpointRecoveryMutants.v 6 \
  "the separated-generation RBA1 recovery causal controls"
rocq_reject_placeholders \
  theories/TwoLevelRSpaceAuthority.v \
  theories/TwoLevelRSpaceOperations.v \
  theories/TwoLevelRSpaceDelta.v \
  theories/TwoLevelRSpaceRepresentation.v \
  theories/TwoLevelRSpaceGuardedOwnership.v \
  theories/TwoLevelRSpaceCheckpointCodec.v \
  theories/TwoLevelRSpaceCheckpointStreaming.v \
  theories/TwoLevelRSpaceCheckpointCompatibility.v \
  theories/TwoLevelRSpaceAuthorityMutants.v \
  theories/TwoLevelRSpaceOperationsMutants.v \
  theories/TwoLevelRSpaceDeltaMutants.v \
  theories/TwoLevelRSpaceRepresentationMutants.v \
  theories/TwoLevelRSpaceGuardedOwnershipMutants.v \
  theories/TwoLevelRSpaceCheckpointMutants.v \
  theories/RhoPathV1Datum.v \
  theories/TwoLevelRSpaceContinuationAuthority.v \
  theories/TwoLevelRSpaceContinuationAuthorityMutants.v \
  theories/TwoLevelRSpaceMatchPreparation.v \
  theories/EncodedRSpaceCandidateCatalog.v \
  theories/TwoLevelRSpaceFlatDeltaTrace.v \
  theories/TwoLevelRSpaceMatchPreparationMutants.v \
  theories/TwoLevelRSpaceMatchTransaction.v \
  theories/TwoLevelRSpaceMatchTransactionMutants.v \
  theories/TwoLevelRSpaceCheckpointRecovery.v \
  theories/TwoLevelRSpaceCheckpointRecoveryMutants.v
rocq_check_modules "two-level RSpace authority and RBA1 checkpoint recovery" \
  StackSafePDA.TwoLevelRSpaceAuthority \
  StackSafePDA.TwoLevelRSpaceOperations \
  StackSafePDA.TwoLevelRSpaceDelta \
  StackSafePDA.TwoLevelRSpaceRepresentation \
  StackSafePDA.TwoLevelRSpaceGuardedOwnership \
  StackSafePDA.TwoLevelRSpaceCheckpointCodec \
  StackSafePDA.TwoLevelRSpaceCheckpointStreaming \
  StackSafePDA.TwoLevelRSpaceCheckpointCompatibility \
  StackSafePDA.TwoLevelRSpaceAuthorityMutants \
  StackSafePDA.TwoLevelRSpaceOperationsMutants \
  StackSafePDA.TwoLevelRSpaceDeltaMutants \
  StackSafePDA.TwoLevelRSpaceRepresentationMutants \
  StackSafePDA.TwoLevelRSpaceGuardedOwnershipMutants \
  StackSafePDA.TwoLevelRSpaceCheckpointMutants \
  StackSafePDA.RhoPathV1Datum \
  StackSafePDA.TwoLevelRSpaceContinuationAuthority \
  StackSafePDA.TwoLevelRSpaceContinuationAuthorityMutants \
  StackSafePDA.TwoLevelRSpaceMatchPreparation \
  StackSafePDA.EncodedRSpaceCandidateCatalog \
  StackSafePDA.TwoLevelRSpaceFlatDeltaTrace \
  StackSafePDA.TwoLevelRSpaceMatchPreparationMutants \
  StackSafePDA.TwoLevelRSpaceMatchTransaction \
  StackSafePDA.TwoLevelRSpaceMatchTransactionMutants \
  StackSafePDA.TwoLevelRSpaceCheckpointRecovery \
  StackSafePDA.TwoLevelRSpaceCheckpointRecoveryMutants
python3 "$repo_root/scripts/check-free-map-journal-obligations.py" --check
python3 "$repo_root/scripts/check-free-map-journal-obligations.py" --self-test
free_map_journal_rocq
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
popd >/dev/null

actree03_rocq_root="$repo_root/formal/rocq/actree03_completeness"
previous_tlc_rss="$TLC_RSS"
TLC_RSS=1536M
pushd "$actree03_rocq_root" >/dev/null
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1CursorAuthorityV2.v
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1CursorPositiveV3.v
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1CursorPositiveV4.v
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1CursorPositiveV4Mutants.v
# The concrete ACT admission development has a substantially larger checked
# term environment than the stack-safe PDA units above.  Keep it bounded, but
# give only these two compilation units the measured 4 GiB ceiling they need.
TLC_MEMORY_HIGH=3G TLC_RSS=4G \
  tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1ActConcreteV5.v
TLC_MEMORY_HIGH=3G TLC_RSS=4G \
  tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1ActConcreteV5Mutants.v
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1PositionalAssociationV6.v
tlc_bounded "$coqc_bin" -Q theories '' theories/Epm1PositionalAssociationV6Mutants.v
rocq_reject_placeholders theories/Epm1ValueSummary.v
rocq_compile_with_closed_count theories/Epm1ValueSummary.v 16 \
  "ACT cached summary and source-preorder refinement" ''
rocq_check_modules_at '' "ACT cached value summary" Epm1ValueSummary
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
canonical_connective_owner_verus
free_map_journal_verus
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/accounting_demand.rs"
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/channel_image_cursor.rs"
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/rpb3_admission_cursor.rs"
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/rspace_canonical_egress.rs"
incremental_ledger_trial_verus
tlc_bounded "$verus_bin" --no-cheating --num-threads 1 \
  "$repo_root/formal/verus/streaming_codec_pda.rs"

# The generated-table differential is the executable production binding for
# the RPB3 Rocq/Verus boundary.  Keep it under the same cgroup envelope so a
# green formal run cannot silently omit the Rust oracle comparison.
pushd "$repo_root" >/dev/null
rpb3_run_tests 22 --lib protobuf_admission::tests
epathmap_import_runtime
rpb3_run_tests 1 --lib \
  production_component_admission_is_differentially_equivalent_for_all_ten_families
rpb3_run_tests 3 --test par_bag admitted_
rpb3_run_tests 4 --test par_bag canonical_connective_cursor_
rpb3_run_tests 4 --release --test par_bag canonical_connective_cursor_
rpb3_run_tests 1 --test par_bag generated_connective_cursor_owner_pairing_and_exhaustion
rpb3_run_tests 1 --release --test par_bag generated_connective_cursor_owner_pairing_and_exhaustion
TLC_MEMORY_HIGH=4G TLC_RSS=6G TLC_CPU_QUOTA=400% TLC_TASKS_MAX=512 \
  rpb3_run_tests 23 --release -j4 --lib emission_tests
rholang_run_tests 9 --lib region_slot_tests
rholang_run_tests 3 --lib new_injection_preserves_
rpb3_run_tests 9 --lib incomplete_preservation_tests
rpb3_run_tests 14 --lib protobuf_decoder_oracle::
rpb3_run_tests 2 --release -j4 --features protobuf-projection-experiment --lib protobuf_decoder_oracle::measurement::measurement_
rpb3_run_tests 2 --lib oracle_body_guards::
rpb3_run_tests 5 --lib decoder_control_tests::
rpb3_run_tests 3 --lib snapshot_continuation_tests::
rpb3_run_tests 6 --lib admission_retained_history::
rpb3_run_tests 4 --lib planned_fragment_comparison_tests::
rpb3_run_tests 4 --lib planned_ground_leaf_tests::
rpb3_run_tests 5 --lib planned_fragment_publication_tests::
rpb3_run_tests 5 --lib planned_pooled_storage_tests::
rpb3_run_tests 9 --lib root_stream_
rspace_preparation_run_tests 3 --lib reweight_tests::
rpb3_run_tests 1 --release -j4 --features protobuf-projection-experiment --lib deep_general_decoder_materializes_once_and_retires_failed_frames_on_small_stack
rpb3_run_tests 6 --lib protobuf_par_accumulation_tests
rpb3_run_tests 8 --lib payload_representation_tests
rpb3_run_tests 5 --lib owned_cursor_tests
rpb3_run_tests 10 --lib payload_mapping_tests
rpb3_run_tests 6 --lib resident_ownership_tests
rpb3_run_tests 8 --lib protobuf_owned_slots_tests
rpb3_run_tests 7 --lib protobuf_construction_tests
rspace_preparation_run_tests 5 --lib stateful_retirement
rpb3_run_tests 7 --lib owned_retirement_tests
rpb3_run_tests 8 --lib planned_epm_ownership_tests
rpb3_run_tests 4 --lib snapshot_admission_tests
rpb3_run_tests 7 --lib legacy_ingress_tests
rpb3_run_tests 1 --release -j4 --features protobuf-projection-experiment --lib planned_handles_keep_exact_associations_without_resident_emission
rpb3_run_tests 1 --release -j4 --features protobuf-projection-experiment --lib accumulation_emits_no_resident_root_until_requested
rpb3_run_tests 1 --release -j4 --features protobuf-projection-experiment --lib incomplete_planning_has_no_resident_finish_events
rpb3_run_tests 6 --release -j4 --lib collection_collision_elision
rpb3_run_tests 2 --release -j4 --lib par_construction::tests::ac_
rpb3_run_tests 7 --release -j4 --features protobuf-projection-experiment --lib substitution_cost
rpb3_run_tests 8 --release -j4 --features protobuf-projection-experiment \
  --test protobuf_region_differential
rholang_run_tests 4 --features protobuf-projection-experiment \
  --test stack_depth_gate substitution_scaling::costs
rholang_run_tests 1 --features protobuf-projection-experiment \
  --lib every_corpus_case_agrees_under_both_environments
rholang_run_tests 3 --features protobuf-projection-experiment \
  --lib region_cost_
rholang_run_tests 1 --features protobuf-projection-experiment \
  --lib substitute_oracle::differential_substitute_worklist::error_paths_agree
rholang_run_tests 1 --test epathmap_replay_equivalence_spec
rholang_run_tests 1 --test replay_output_value_stack_safety
rholang_run_tests 3 --test output_value_write_side_reachability
rholang_run_tests 4 --lib scalar_projection
rholang_run_tests 1 --features epathmap-fusion-differential \
  --test epathmap_fusion_equivalence_spec \
  fused_differentials::scalar_projection_fused_unfused_carriers_preserve_observations
matcher_count_runtime
popd >/dev/null

# Bind the RhoPath V2 lifecycle refinement to the end-to-end Rust differential
# oracle.  This must run under the same finite envelope as the formal stages;
# the model manifest alone is not sufficient evidence of implementation
# correspondence.
pushd "$repo_root" >/dev/null
live_write_test_output="$(tlc_bounded cargo test -j4 -p rspace_plus_plus --lib live_write_ 2>&1)"
printf '%s\n' "$live_write_test_output"
if ! grep -Eq '^test result: ok\. 3 passed; 0 failed; 0 ignored;' <<<"$live_write_test_output"; then
  echo "stack-safe-pda: expected exactly three executed live-write gateway witnesses" >&2
  exit 1
fi
tlc_bounded cargo test -p rspace_plus_plus --test mod \
  continuation_join_lifecycle_equivalence::legacy_and_pathmap_continuation_join_lifecycles_are_identical \
  -- --nocapture
popd >/dev/null

z3_output="$(tlc_bounded z3 "$repo_root/formal/smt/stack_safe_pda_modes.smt2")"
if [[ "$z3_output" != "unsat" ]]; then
  echo "stack-safe-pda: expected Z3 to prove no mode-law counterexample; got: $z3_output" >&2
  exit 1
fi

tla_root="$repo_root/formal/tlaplus/stack_safe_pda"
tla_metadir="$(tlc_metadir stack-safe-pda-equivalence)"
python3 "$repo_root/scripts/check-epathmap-core-obligations.py" --check
python3 "$repo_root/scripts/check-epathmap-core-obligations.py" --self-test
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
python3 "$repo_root/scripts/check-spatial-matcher-obligations.py" --check
python3 "$repo_root/scripts/check-spatial-matcher-obligations.py" --self-test
python3 "$repo_root/scripts/check-retained-set-automaton-obligations.py" --check
python3 "$repo_root/scripts/check-retained-set-automaton-obligations.py" --self-test
tlc_run "$tla_metadir" SpatialMatcherEquivalence.cfg SpatialMatcherEquivalence.tla -cleanup
python3 "$repo_root/scripts/check-accepted-match-obligations.py" --check
python3 "$repo_root/scripts/check-accepted-match-obligations.py" --self-test
python3 "$repo_root/scripts/check-bag-obligations.py" --check
python3 "$repo_root/scripts/check-bag-obligations.py" --self-test
python3 "$repo_root/scripts/check-workspace-regression-obligations.py" --check
python3 "$repo_root/scripts/check-workspace-regression-obligations.py" --self-test
tlc_run "$tla_metadir" RSpaceBagCommit.cfg RSpaceBagCommit.tla -cleanup
for accepted_match_mutant in \
  "RSpaceBagCommitSplitPublicationMutant.cfg:RSpaceBagCommitMutants.tla:PlayPublishesOneCompleteState" \
  "RSpaceBagCommitEarlyEventMutant.cfg:RSpaceBagCommitMutants.tla:EventPublicationFollowsCommit" \
  "RSpaceBagCommitDropJoinMutant.cfg:RSpaceBagCommitMutants.tla:ContinuationAndJoinCommitTogether"; do
  accepted_match_cfg="${accepted_match_mutant%%:*}"
  accepted_match_rest="${accepted_match_mutant#*:}"
  accepted_match_tla="${accepted_match_rest%%:*}"
  accepted_match_invariant="${accepted_match_rest#*:}"
  if accepted_match_output="$({
    tlc_run "$tla_metadir" "$accepted_match_cfg" "$accepted_match_tla" -cleanup
  } 2>&1)"; then
    printf '%s\n' "$accepted_match_output"
    echo "stack-safe-pda: accepted-match mutant unexpectedly passed: $accepted_match_cfg" >&2
    exit 1
  fi
  printf '%s\n' "$accepted_match_output"
  if ! grep -q "Invariant ${accepted_match_invariant} is violated" <<<"$accepted_match_output"; then
    echo "stack-safe-pda: accepted-match mutant failed without ${accepted_match_invariant}: $accepted_match_cfg" >&2
    exit 1
  fi
done
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
python3 "$repo_root/scripts/check-split-index-authority-obligations.py" --check
python3 "$repo_root/scripts/check-split-index-authority-obligations.py" --self-test
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
python3 "$repo_root/scripts/check-affine-import-obligations.py" --check
python3 "$repo_root/scripts/check-affine-import-obligations.py" --self-test
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
python3 "$repo_root/scripts/check-atomic-checkpoint-obligations.py" --check
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
python3 "$repo_root/scripts/check-rho-path-v2-obligations.py" --check
python3 "$repo_root/scripts/check-rho-path-v2-obligations.py" --self-test
tlc_run "$tla_metadir" \
  RhoPathV2Lifecycle.cfg \
  RhoPathV2Lifecycle.tla -cleanup
if rho_path_v2_digest_only_mutant="$({
  tlc_run "$tla_metadir" \
    RhoPathV2LifecycleDigestOnlyMutant.cfg \
    RhoPathV2LifecycleDigestOnlyMutant.tla -cleanup
} 2>&1)"; then
  printf '%s\n' "$rho_path_v2_digest_only_mutant"
  echo "stack-safe-pda: RhoPath V2 digest-only mutant unexpectedly passed" >&2
  exit 1
fi
printf '%s\n' "$rho_path_v2_digest_only_mutant"
if ! grep -q 'Invariant DistinctCollisionRepresentativesPreserved is violated' \
    <<<"$rho_path_v2_digest_only_mutant"; then
  echo "stack-safe-pda: RhoPath V2 digest-only mutant failed without the collision invariant" >&2
  exit 1
fi
for rho_path_v2_mutant in \
  "RhoPathV2LifecycleOverwriteMutant.cfg:RhoPathV2LifecycleOverwriteMutant.tla:DistinctCollisionRepresentativesPreserved" \
  "RhoPathV2LifecycleDecodeLegacyMutant.cfg:RhoPathV2LifecycleDecodeLegacyMutant.tla:FullImageIdentityPreserved"; do
  rho_path_v2_mutant_cfg="${rho_path_v2_mutant%%:*}"
  rho_path_v2_mutant_rest="${rho_path_v2_mutant#*:}"
  rho_path_v2_mutant_tla="${rho_path_v2_mutant_rest%%:*}"
  rho_path_v2_mutant_invariant="${rho_path_v2_mutant_rest#*:}"
  if rho_path_v2_mutant_output="$({
    tlc_run "$tla_metadir" "$rho_path_v2_mutant_cfg" "$rho_path_v2_mutant_tla" -cleanup
  } 2>&1)"; then
    printf '%s\n' "$rho_path_v2_mutant_output"
    echo "stack-safe-pda: RhoPath V2 mutant unexpectedly passed: $rho_path_v2_mutant_cfg" >&2
    exit 1
  fi
  printf '%s\n' "$rho_path_v2_mutant_output"
  if ! grep -q "Invariant ${rho_path_v2_mutant_invariant} is violated" <<<"$rho_path_v2_mutant_output"; then
    echo "stack-safe-pda: RhoPath V2 mutant failed without ${rho_path_v2_mutant_invariant}: $rho_path_v2_mutant_cfg" >&2
    exit 1
  fi
done
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
  RSpaceRetentionV2Upgrade.cfg \
  RSpaceRetentionV2Upgrade.tla -cleanup
for retention_v2_mutant in \
  "catalog-envelope:RSpaceRetentionV2UpgradeCatalogEnvelopeMutant.cfg:AdoptionIsNewestPublished" \
  "selection-divergence:RSpaceRetentionV2UpgradeSelectionDivergenceMutant.cfg:ReadersSelectTheSameSlot" \
  "fallback-on-unsupported:RSpaceRetentionV2UpgradeFallbackOnUnsupportedMutant.cfg:AdoptionIsNewestPublished" \
  "adopt-unsupported:RSpaceRetentionV2UpgradeAdoptUnsupportedMutant.cfg:NoReaderAdoptsUnsupportedContent"
do
  retention_v2_label="${retention_v2_mutant%%:*}"
  retention_v2_tail="${retention_v2_mutant#*:}"
  retention_v2_cfg="${retention_v2_tail%%:*}"
  retention_v2_invariant="${retention_v2_tail#*:}"
  if retention_v2_output="$({
    tlc_run "$tla_metadir" \
      "$retention_v2_cfg" RSpaceRetentionV2Upgrade.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$retention_v2_output"
    echo "stack-safe-pda: ${retention_v2_label} retention V2 mutant unexpectedly passed" >&2
    exit 1
  fi
  printf '%s\n' "$retention_v2_output"
  if ! grep -q "Invariant ${retention_v2_invariant} is violated" \
      <<<"$retention_v2_output"; then
    echo "stack-safe-pda: ${retention_v2_label} retention V2 mutant failed without ${retention_v2_invariant}" >&2
    exit 1
  fi
done
for retention_v2_witness in \
  "old-reader-refusal:RSpaceRetentionV2UpgradeNeverOldReaderRefusesV2.cfg:NeverOldReaderRefusesV2" \
  "dual-reader-adoption:RSpaceRetentionV2UpgradeNeverDualReaderAdoptsV2.cfg:NeverDualReaderAdoptsV2" \
  "torn-successor:RSpaceRetentionV2UpgradeNeverAdoptsBehindTornSuccessor.cfg:NeverAdoptsBehindTornSuccessor"
do
  retention_v2_witness_label="${retention_v2_witness%%:*}"
  retention_v2_witness_tail="${retention_v2_witness#*:}"
  retention_v2_witness_cfg="${retention_v2_witness_tail%%:*}"
  retention_v2_witness_invariant="${retention_v2_witness_tail#*:}"
  if retention_v2_witness_output="$({
    tlc_run "$tla_metadir" \
      "$retention_v2_witness_cfg" RSpaceRetentionV2Upgrade.tla -cleanup
  } 2>&1)"; then
    printf '%s\n' "$retention_v2_witness_output"
    echo "stack-safe-pda: ${retention_v2_witness_label} retention V2 non-vacuity witness was unreachable" >&2
    exit 1
  fi
  printf '%s\n' "$retention_v2_witness_output"
  if ! grep -q "Invariant ${retention_v2_witness_invariant} is violated" \
      <<<"$retention_v2_witness_output"; then
    echo "stack-safe-pda: ${retention_v2_witness_label} retention V2 witness failed without ${retention_v2_witness_invariant}" >&2
    exit 1
  fi
done
bash "$repo_root/scripts/check-rspace-journal-owner-model.sh"
TLC_RSS=4G bash "$repo_root/scripts/check-rspace-compaction-model.sh"
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
python3 "$repo_root/scripts/check-epm1-positional-obligations.py" --check
python3 "$repo_root/scripts/check-epm1-positional-obligations.py" --self-test
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
free_map_journal_tlc
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
  "$mcrl2_root/rho_path_v2_lifecycle_impl.mcrl2" "$mcrl2_tmp/rho-v2-impl.lps"
tlc_bounded mcrl22lps \
  "$mcrl2_root/rho_path_v2_lifecycle_spec.mcrl2" "$mcrl2_tmp/rho-v2-spec.lps"
tlc_bounded lps2lts "$mcrl2_tmp/rho-v2-impl.lps" "$mcrl2_tmp/rho-v2-impl.lts"
tlc_bounded lps2lts "$mcrl2_tmp/rho-v2-spec.lps" "$mcrl2_tmp/rho-v2-spec.lts"
rho_path_v2_bisimulation="$({
  tlc_bounded ltscompare --equivalence=branching-bisim \
    "$mcrl2_tmp/rho-v2-impl.lts" "$mcrl2_tmp/rho-v2-spec.lts" 2>&1
} | tail -n 1)"
if [[ "$rho_path_v2_bisimulation" != "true" ]]; then
  echo "stack-safe-pda: expected RhoPath V2 branching bisimulation; got: $rho_path_v2_bisimulation" >&2
  exit 1
fi

free_map_journal_mcrl2 "$mcrl2_tmp"

echo "stack-safe-pda: Rocq, Verus, Z3, TLC, and mCRL2 verification passed"
