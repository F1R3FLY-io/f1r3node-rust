#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL="$ROOT/formal/tlaplus/block_admission"
WORK="$ROOT/target/verification/block-admission"
mkdir -p "$WORK"

export TLC_REPO_ROOT="$ROOT"
source "$ROOT/scripts/lib/tlc-run.sh"

command -v apalache-mc >/dev/null 2>&1 || {
  echo "error: apalache-mc is required for block-admission verification" >&2
  exit 1
}

APALACHE_LENGTH="${APALACHE_LENGTH:-8}"
APALACHE_BLOCK_OUT="$WORK/apalache-block-safe"
APALACHE_BUFFER_OUT="$WORK/apalache-buffer-safe"
APALACHE_TRANSPORT_PAYLOAD_OUT="$WORK/apalache-transport-payload-safe"
APALACHE_TRANSPORT_PEER_OUT="$WORK/apalache-transport-peer-safe"
APALACHE_TRANSPORT_CONCURRENCY_OUT="$WORK/apalache-transport-concurrency-safe"

cleanup() {
  rm -rf "$TLC_METADIR_ROOT/block-admission-MC_BlockAdmission" \
    "$TLC_METADIR_ROOT/block-admission-MC_BufferScanResidency" \
    "$TLC_METADIR_ROOT/block-admission-MC_BlockAdmission_pre_fix" \
    "$TLC_METADIR_ROOT/block-admission-MC_BlockAdmission_drop_pre_fix" \
    "$TLC_METADIR_ROOT/block-admission-MC_BufferScanResidency_pre_fix" \
    "$TLC_METADIR_ROOT/block-admission-TransportPayloadResidency" \
    "$TLC_METADIR_ROOT/block-admission-TransportPayloadResidencyCountOnlyUnsafe" \
    "$TLC_METADIR_ROOT/block-admission-TransportPayloadResidencyDecodedOnlyUnsafe" \
    "$TLC_METADIR_ROOT/block-admission-TransportPayloadResidencyEagerChunksUnsafe" \
    "$TLC_METADIR_ROOT/block-admission-TransportPayloadResidencyEnqueueSuccessUnsafe" \
    "$TLC_METADIR_ROOT/block-admission-TransportPeerLifecycle" \
    "$TLC_METADIR_ROOT/block-admission-TransportPeerLifecycleInitRaceUnsafe" \
    "$TLC_METADIR_ROOT/block-admission-TransportPeerLifecycleActiveEvictionUnsafe" \
    "$TLC_METADIR_ROOT/block-admission-TransportPeerLifecycleSharedContextUnsafe" \
    "$TLC_METADIR_ROOT/block-admission-TransportConcurrency" \
    "$TLC_METADIR_ROOT/block-admission-TransportConcurrencyWireLimitUnsafe" \
    "$TLC_METADIR_ROOT/block-admission-TransportConcurrencyItemLimitUnsafe" \
    "$TLC_METADIR_ROOT/block-admission-TransportConcurrencyHandlerLimitUnsafe" \
    "$APALACHE_BLOCK_OUT" \
    "$APALACHE_BUFFER_OUT" \
    "$APALACHE_TRANSPORT_PAYLOAD_OUT" \
    "$APALACHE_TRANSPORT_PEER_OUT" \
    "$APALACHE_TRANSPORT_CONCURRENCY_OUT"
}

trap cleanup EXIT
cleanup

run_safe() {
  local name="$1" output status
  set +e
  output="$(cd "$MODEL" && TLC_WALL_TIMEOUT=10m tlc_run \
    "$(tlc_metadir "block-admission-$name")" "$name.cfg" "$name.tla" 2>&1)"
  status=$?
  set -e
  if [[ "$status" -ne 0 ]] || ! rg -q 'No error has been found' <<<"$output"; then
    printf '%s\n' "$output" | tail -80 >&2
    echo "error: safe block-admission model $name failed" >&2
    return 1
  fi
}

run_expected_failure() {
  local name="$1" property="$2" output status
  set +e
  output="$(cd "$MODEL" && TLC_WALL_TIMEOUT=10m tlc_run \
    "$(tlc_metadir "block-admission-$name")" "$name.cfg" "$name.tla" 2>&1)"
  status=$?
  set -e
  if [[ "$status" -eq 0 ]] \
    || ! rg -q "Invariant ${property} is violated|Temporal properties were violated|Property ${property} is violated" <<<"$output"; then
    printf '%s\n' "$output" | tail -80 >&2
    echo "error: $name did not produce the required $property counterexample" >&2
    return 1
  fi
}

run_apalache_safe() {
  local model="$1" config="$2" output status out_dir length
  out_dir="$3"
  length="${4:-$APALACHE_LENGTH}"
  set +e
  output="$(cd "$MODEL" && apalache-mc \
    --out-dir="$out_dir" \
    check \
    --config="$config" \
    --length="$length" \
    "$model" 2>&1)"
  status=$?
  set -e
  if [[ "$status" -ne 0 ]] \
    || ! rg -q 'The outcome is: NoError' <<<"$output" \
    || ! rg -q 'EXITCODE: OK' <<<"$output"; then
    printf '%s\n' "$output" | tail -120 >&2
    echo "error: Apalache safety check failed for $model" >&2
    return 1
  fi
}

run_safe MC_BlockAdmission
run_safe MC_BufferScanResidency
run_expected_failure MC_BlockAdmission_pre_fix Inv_RetainedBytesBounded
run_expected_failure MC_BlockAdmission_drop_pre_fix Live_AllBroadcastProcessed
run_expected_failure MC_BufferScanResidency_pre_fix Inv_ScannerSinglePayload
run_safe TransportPayloadResidency
run_expected_failure TransportPayloadResidencyCountOnlyUnsafe Inv_ActualResidencyBounded
run_expected_failure TransportPayloadResidencyDecodedOnlyUnsafe Inv_ReservationCoversActual
run_expected_failure TransportPayloadResidencyEagerChunksUnsafe Inv_ReservationCoversActual
run_expected_failure TransportPayloadResidencyEnqueueSuccessUnsafe Inv_SuccessRequiresRemoteCompletion
run_safe TransportPeerLifecycle
run_expected_failure TransportPeerLifecycleInitRaceUnsafe Inv_InitializingOwnsMappedSlot
run_expected_failure TransportPeerLifecycleActiveEvictionUnsafe Inv_AcknowledgedWorkPreserved
run_expected_failure TransportPeerLifecycleSharedContextUnsafe Inv_ValidationUsesRequestContext
run_safe TransportConcurrency
run_expected_failure TransportConcurrencyWireLimitUnsafe Inv_NoRequestsRefused
run_expected_failure TransportConcurrencyItemLimitUnsafe Inv_NoPayloadBudgetRejection
run_expected_failure TransportConcurrencyHandlerLimitUnsafe Inv_PreReservationDecodedBounded
run_apalache_safe BlockAdmission.tla BlockAdmissionApalache.cfg "$APALACHE_BLOCK_OUT"
run_apalache_safe BufferScanResidency.tla BufferScanResidencyApalache.cfg "$APALACHE_BUFFER_OUT"
run_apalache_safe TransportPayloadResidency.tla TransportPayloadResidencyApalache.cfg "$APALACHE_TRANSPORT_PAYLOAD_OUT"
run_apalache_safe TransportPeerLifecycle.tla TransportPeerLifecycleApalache.cfg "$APALACHE_TRANSPORT_PEER_OUT"
run_apalache_safe TransportConcurrency.tla TransportConcurrencyApalache.cfg "$APALACHE_TRANSPORT_CONCURRENCY_OUT" 10

PROPTEST_CASES="${PROPTEST_CASES:-10000}" \
  cargo test -p casper --lib block_processing_queue::tests
PROPTEST_CASES="${PROPTEST_CASES:-10000}" \
  cargo test -p casper --lib buffer_resolver::tests
cargo test -p casper --lib admission_deferral_
cargo test -p casper --lib request_capacity_preserves_existing_work_and_defers_new_hashes
PROPTEST_CASES="${PROPTEST_CASES:-10000}" \
  cargo test -p comm transport::payload_budget::tests
PROPTEST_CASES="${PROPTEST_CASES:-10000}" \
  cargo test -p comm transport::grpc_transport_receiver::resource_envelope_tests
cargo test -p comm transport::activity_gate::tests
cargo test -p comm transport::chunker::tests
cargo test -p comm --test mod transport::stream_handler_spec
cargo test -p comm --test mod transport::transport_layer_spec::concurrent_sends_to_same_peer_should_all_succeed -- --test-threads=1

KANI_FLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg kani --cfg target_feature="aes" --cfg target_feature="sse2"'
env RUSTFLAGS="$KANI_FLAGS" cargo kani -p casper \
  --harness successful_reservation_is_exact_and_bounded
env RUSTFLAGS="$KANI_FLAGS" cargo kani -p casper \
  --harness failed_reservation_cannot_fit

env RUSTFLAGS='--cfg loom -C target-cpu=native' LOOM_MAX_PREEMPTIONS=3 \
  cargo test -p cost-accounting-loom-models --test loom_block_admission
env RUSTFLAGS='--cfg loom -C target-cpu=native' LOOM_MAX_PREEMPTIONS=3 \
  cargo test -p cost-accounting-loom-models --test loom_transport_payload_residency

if rg -q 'protocol\.encode_to_vec\(\)' \
  "$ROOT/comm/src/rust/transport/grpc_transport_receiver.rs"; then
  echo "error: unary ingress creates a full unreserved protocol copy" >&2
  exit 1
fi

echo "Block-admission and transport-residency TLC, Apalache, Rust property, Kani, and Loom refinement gate passed."
