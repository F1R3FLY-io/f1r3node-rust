#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MODEL_ROOT="$ROOT/formal/tlaplus/finalized_floor"
MODEL="$MODEL_ROOT/SignedFloorReplayReadiness.tla"
LOG_ROOT="$ROOT/target/verification/signed-floor-replay-readiness"
APALACHE_TIMEOUT="${APALACHE_TIMEOUT:-15m}"
APALACHE_RSS="${APALACHE_RSS:-8G}"
mkdir -p "$LOG_ROOT"
WORK="$(mktemp -d "$LOG_ROOT/run.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

for command_name in tla2sany tlc apalache-mc systemd-run timeout; do
  command -v "$command_name" >/dev/null 2>&1 || {
    printf 'error: %s is required for signed-floor replay verification\n' "$command_name" >&2
    exit 1
  }
done

export TLC_REPO_ROOT="$ROOT"
export TLC_METADIR_ROOT="$WORK/tlc"
export TLC_HEAP="${TLC_HEAP:-3g}"
export TLC_RSS="${TLC_RSS:-5G}"
export TLC_WORKERS="${TLC_WORKERS:-1}"
export TLC_WALL_TIMEOUT="${TLC_WALL_TIMEOUT:-15m}"
source "$ROOT/scripts/lib/tlc-run.sh"

run_tlc_safe() {
  local log="$LOG_ROOT/tlc-safe.log"
  tlc_run "$WORK/tlc-safe" "$MODEL_ROOT/MC_SignedFloorReplayReadiness.cfg" "$MODEL" >"$log" 2>&1
  grep -Fq 'Model checking completed. No error has been found.' "$log"
  grep -Fq 'The depth of the complete state graph search is 20.' "$log"
}

run_tlc_unsafe() {
  local name="$1" config="$2" invariant="$3"
  local log="$LOG_ROOT/tlc-$name-unsafe.log"
  if tlc_run "$WORK/tlc-$name-unsafe" "$MODEL_ROOT/$config" "$MODEL" >"$log" 2>&1; then
    printf 'error: TLC control %s preserved %s\n' "$name" "$invariant" >&2
    return 1
  fi
  grep -Fq "Invariant $invariant is violated" "$log" \
    || grep -Fq "The invariant of $invariant is equal to FALSE" "$log"
}

run_apalache() {
  local name="$1" config="$2" length="$3" log="$4" invariants="${5:-}"
  local -a invariant_args=()
  if [[ -n "$invariants" ]]; then
    invariant_args=(--inv="$invariants")
  fi
  (
    cd "$MODEL_ROOT"
    systemd-run --user --scope --quiet \
      -p "MemoryMax=$APALACHE_RSS" \
      -p MemorySwapMax=0 \
      -p CPUQuota=400% \
      -p TasksMax=512 \
      timeout --signal=TERM --kill-after=30 "$APALACHE_TIMEOUT" \
      apalache-mc --out-dir="$WORK/apalache-$name" check \
      --config="$config" --length="$length" "${invariant_args[@]}" "$MODEL"
  ) >"$log" 2>&1
}

run_apalache_safe() {
  local name length invariants log
  local -a safe_cases=(
    'concurrency:6:Inv_CapturedTuplesAreCoherent,Inv_FinalizerDoesNotCancelCreatedProposal,Inv_EqualEvidenceHasEqualReceiverDecision'
    'exact-artifacts:10:Inv_AcceptanceRequiresExactArtifacts'
    'signed-tuple:10:Inv_AcceptanceRequiresSignedCommitment,Inv_AcceptanceRequiresCanonicalCommitmentState,Inv_AcceptanceRequiresCanonicalCommitmentHeight,Inv_AcceptanceRequiresCertificateHash,Inv_AcceptanceRequiresCertificateState,Inv_AcceptanceRequiresCertificateHeight,Inv_CommitmentBindsCompleteCertificate,Inv_CertificateThresholdMatchesShard,Inv_CertificateUsesStrictFtt'
    'occurrence:10:Inv_AcceptanceRequiresAcceptedOccurrence,Inv_AcceptanceRequiresStoredBlockFloor,Inv_AcceptanceRequiresStoredBlockState,Inv_AcceptanceRequiresStoredBlockHeight,Inv_AcceptanceRequiresStoredMetadataFloor,Inv_AcceptanceRequiresStoredMetadataState,Inv_AcceptanceRequiresStoredMetadataHeight,Inv_AcceptanceRequiresFloorAncestry'
    'authority:10:Inv_AcceptanceRequiresCompleteLatestMessages,Inv_AcceptanceRequiresActiveSender,Inv_AcceptedAuthorityUsesCapturedFloor,Inv_CertificateAuthorityUsesPredecessorFloor,Inv_ProposalAuthorityUsesTargetFloor,Inv_DistinctCommitteesHaveDistinctAuthorityDigests'
    'deferral:9:Inv_MissingArtifactIsNeverAccepted,Inv_MissingArtifactIsNeverInvalid,Inv_MissingLocalStatePreventsProposal'
    'replay:11:Inv_AcceptedReplayUsesCapturedFloor'
  )
  for case_spec in "${safe_cases[@]}"; do
    IFS=: read -r name length invariants <<<"$case_spec"
    log="$LOG_ROOT/apalache-safe-$name.log"
    run_apalache "safe-$name" MC_SignedFloorReplayReadinessApalache.cfg \
      "$length" "$log" "$invariants"
    grep -qE 'The outcome is: NoError|EXITCODE: OK' "$log"
  done
}

run_apalache_unsafe() {
  local name="$1" config="$2" length="$3" invariant="$4"
  local log="$LOG_ROOT/apalache-$name-unsafe.log"
  if run_apalache "$name-unsafe" "$config" "$length" "$log"; then
    printf 'error: Apalache control %s preserved %s\n' "$name" "$invariant" >&2
    return 1
  fi
  grep -Fq "Using inv predicate(s) $invariant" "$log"
  grep -qE 'state invariant [0-9]+ violated' "$log"
  grep -qE 'The outcome is: Error|EXITCODE: VIOLATION' "$log"
}

unsafe_cases=(
  'receiver-disagreement:MC_SignedFloorReplayReadiness_receiver_disagreement_unsafe.cfg:MC_SignedFloorReplayReadiness_receiver_disagreement_unsafe_Apalache.cfg:1:Inv_EqualEvidenceHasEqualReceiverDecision'
  'torn-capture:MC_SignedFloorReplayReadiness_torn_capture_unsafe.cfg:MC_SignedFloorReplayReadiness_torn_capture_unsafe_Apalache.cfg:5:Inv_CapturedTuplesAreCoherent'
  'finalizer-cancellation:MC_SignedFloorReplayReadiness_finalizer_cancellation_unsafe.cfg:MC_SignedFloorReplayReadiness_finalizer_cancellation_unsafe_Apalache.cfg:6:Inv_FinalizerDoesNotCancelCreatedProposal'
  'missing-root-acceptance:MC_SignedFloorReplayReadiness_missing_root_acceptance_unsafe.cfg:MC_SignedFloorReplayReadiness_missing_root_acceptance_unsafe_Apalache.cfg:9:Inv_MissingArtifactIsNeverAccepted'
  'missing-root-invalidation:MC_SignedFloorReplayReadiness_missing_root_invalidation_unsafe.cfg:MC_SignedFloorReplayReadiness_missing_root_invalidation_unsafe_Apalache.cfg:9:Inv_MissingArtifactIsNeverInvalid'
  'certificate-hash-mismatch:MC_SignedFloorReplayReadiness_certificate_hash_mismatch_unsafe.cfg:MC_SignedFloorReplayReadiness_certificate_hash_mismatch_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresCertificateHash'
  'certificate-state-mismatch:MC_SignedFloorReplayReadiness_certificate_state_mismatch_unsafe.cfg:MC_SignedFloorReplayReadiness_certificate_state_mismatch_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresCertificateState'
  'certificate-height-mismatch:MC_SignedFloorReplayReadiness_certificate_height_mismatch_unsafe.cfg:MC_SignedFloorReplayReadiness_certificate_height_mismatch_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresCertificateHeight'
  'commitment-state-pair-mismatch:MC_SignedFloorReplayReadiness_commitment_state_pair_mismatch_unsafe.cfg:MC_SignedFloorReplayReadiness_commitment_state_pair_mismatch_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresCanonicalCommitmentState'
  'commitment-height-pair-mismatch:MC_SignedFloorReplayReadiness_commitment_height_pair_mismatch_unsafe.cfg:MC_SignedFloorReplayReadiness_commitment_height_pair_mismatch_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresCanonicalCommitmentHeight'
  'single-authority-context:MC_SignedFloorReplayReadiness_single_authority_context_unsafe.cfg:MC_SignedFloorReplayReadiness_single_authority_context_unsafe_Apalache.cfg:10:Inv_DistinctCommitteesHaveDistinctAuthorityDigests'
  'certificate-digest-omission:MC_SignedFloorReplayReadiness_certificate_digest_omission_unsafe.cfg:MC_SignedFloorReplayReadiness_certificate_digest_omission_unsafe_Apalache.cfg:10:Inv_CommitmentBindsCompleteCertificate'
  'target-committee-for-certificate:MC_SignedFloorReplayReadiness_target_committee_for_certificate_unsafe.cfg:MC_SignedFloorReplayReadiness_target_committee_for_certificate_unsafe_Apalache.cfg:10:Inv_CertificateAuthorityUsesPredecessorFloor'
  'predecessor-committee-for-proposal:MC_SignedFloorReplayReadiness_predecessor_committee_for_proposal_unsafe.cfg:MC_SignedFloorReplayReadiness_predecessor_committee_for_proposal_unsafe_Apalache.cfg:10:Inv_ProposalAuthorityUsesTargetFloor'
  'inclusive-ftt:MC_SignedFloorReplayReadiness_inclusive_ftt_unsafe.cfg:MC_SignedFloorReplayReadiness_inclusive_ftt_unsafe_Apalache.cfg:10:Inv_CertificateUsesStrictFtt'
  'certificate-threshold-mismatch:MC_SignedFloorReplayReadiness_certificate_threshold_mismatch_unsafe.cfg:MC_SignedFloorReplayReadiness_certificate_threshold_mismatch_unsafe_Apalache.cfg:10:Inv_CertificateThresholdMatchesShard'
  'missing-local-state-proposal:MC_SignedFloorReplayReadiness_missing_local_state_proposal_unsafe.cfg:MC_SignedFloorReplayReadiness_missing_local_state_proposal_unsafe_Apalache.cfg:5:Inv_MissingLocalStatePreventsProposal'
  'certificate-height-not-digest-bound:MC_SignedFloorReplayReadiness_certificate_height_not_digest_bound_unsafe.cfg:MC_SignedFloorReplayReadiness_certificate_height_not_digest_bound_unsafe_Apalache.cfg:10:Inv_CommitmentBindsCompleteCertificate'
  'rejected-occurrence:MC_SignedFloorReplayReadiness_rejected_occurrence_unsafe.cfg:MC_SignedFloorReplayReadiness_rejected_occurrence_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresAcceptedOccurrence'
  'stored-block-floor-mismatch:MC_SignedFloorReplayReadiness_stored_block_floor_mismatch_unsafe.cfg:MC_SignedFloorReplayReadiness_stored_block_floor_mismatch_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresStoredBlockFloor'
  'stored-block-state-mismatch:MC_SignedFloorReplayReadiness_stored_block_state_mismatch_unsafe.cfg:MC_SignedFloorReplayReadiness_stored_block_state_mismatch_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresStoredBlockState'
  'stored-block-height-mismatch:MC_SignedFloorReplayReadiness_stored_block_height_mismatch_unsafe.cfg:MC_SignedFloorReplayReadiness_stored_block_height_mismatch_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresStoredBlockHeight'
  'stored-metadata-floor-mismatch:MC_SignedFloorReplayReadiness_stored_metadata_floor_mismatch_unsafe.cfg:MC_SignedFloorReplayReadiness_stored_metadata_floor_mismatch_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresStoredMetadataFloor'
  'stored-metadata-state-mismatch:MC_SignedFloorReplayReadiness_stored_metadata_state_mismatch_unsafe.cfg:MC_SignedFloorReplayReadiness_stored_metadata_state_mismatch_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresStoredMetadataState'
  'stored-metadata-height-mismatch:MC_SignedFloorReplayReadiness_stored_metadata_height_mismatch_unsafe.cfg:MC_SignedFloorReplayReadiness_stored_metadata_height_mismatch_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresStoredMetadataHeight'
  'ancestry-omission:MC_SignedFloorReplayReadiness_ancestry_omission_unsafe.cfg:MC_SignedFloorReplayReadiness_ancestry_omission_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresFloorAncestry'
  'incomplete-latest:MC_SignedFloorReplayReadiness_incomplete_latest_unsafe.cfg:MC_SignedFloorReplayReadiness_incomplete_latest_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresCompleteLatestMessages'
  'inactive-sender:MC_SignedFloorReplayReadiness_inactive_sender_unsafe.cfg:MC_SignedFloorReplayReadiness_inactive_sender_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresActiveSender'
  'unsigned-mutation:MC_SignedFloorReplayReadiness_unsigned_mutation_unsafe.cfg:MC_SignedFloorReplayReadiness_unsigned_mutation_unsafe_Apalache.cfg:10:Inv_AcceptanceRequiresSignedCommitment'
  'candidate-committee:MC_SignedFloorReplayReadiness_candidate_committee_unsafe.cfg:MC_SignedFloorReplayReadiness_candidate_committee_unsafe_Apalache.cfg:10:Inv_AcceptedAuthorityUsesCapturedFloor'
  'floor-substitution:MC_SignedFloorReplayReadiness_floor_substitution_unsafe.cfg:MC_SignedFloorReplayReadiness_floor_substitution_unsafe_Apalache.cfg:11:Inv_AcceptedReplayUsesCapturedFloor'
)

tla2sany "$MODEL" >"$LOG_ROOT/sany.log" 2>&1
run_tlc_safe
for case_spec in "${unsafe_cases[@]}"; do
  IFS=: read -r name tlc_config apalache_config length invariant <<<"$case_spec"
  run_tlc_unsafe "$name" "$tlc_config" "$invariant"
done
run_apalache_safe
for case_spec in "${unsafe_cases[@]}"; do
  IFS=: read -r name tlc_config apalache_config length invariant <<<"$case_spec"
  run_apalache_unsafe "$name" "$apalache_config" "$length" "$invariant"
done

printf 'signed-floor replay readiness verification passed\n'
