#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TLA_DIR="$REPO_ROOT/formal/tlaplus/finalized_floor"
WORK_ROOT="$REPO_ROOT/target/verification/finalized-floor/parallel-validator-consensus"

export TLC_REPO_ROOT="$REPO_ROOT"
export TLC_WORKERS="${TLC_WORKERS:-1}"
export TLC_HEAP="${TLC_HEAP:-1g}"
export TLC_RSS="${TLC_RSS:-4G}"
export TLC_METADIR_ROOT="$WORK_ROOT/tlc"

source "$REPO_ROOT/scripts/lib/tlc-run.sh"

rm -rf "$WORK_ROOT"
mkdir -p "$TLC_METADIR_ROOT"
trap 'rm -rf "$TLC_METADIR_ROOT"' EXIT

run_safe() {
  local name="$1"
  local config="$2"
  local output
  output=$(tlc_run \
    "$(tlc_metadir "$name")" \
    "$TLA_DIR/$config" \
    "$TLA_DIR/ParallelValidatorConsensus.tla" \
    -deadlock 2>&1 || true)
  if ! grep -q "Model checking completed. No error has been found" <<<"$output"; then
    printf '%s\n' "$output" >&2
    return 1
  fi
  printf 'PASS %s\n' "$name"
}

run_unsafe() {
  local name="$1"
  local invariant="$2"
  local output
  output=$(tlc_run \
    "$(tlc_metadir "$name")" \
    "$TLA_DIR/${name}.cfg" \
    "$TLA_DIR/ParallelValidatorConsensus.tla" \
    -deadlock 2>&1 || true)
  if ! grep -Fq "Invariant ${invariant} is violated" <<<"$output" \
    && ! grep -Fq "The invariant of ${invariant} is equal to FALSE" <<<"$output"; then
    printf '%s\n' "$output" >&2
    return 1
  fi
  printf 'PASS %s refutes %s\n' "$name" "$invariant"
}

run_safe parallel-validator-safe MC_ParallelValidatorConsensus.cfg
run_safe parallel-validator-crash-safe MC_ParallelValidatorConsensus_crash.cfg
run_unsafe MC_ParallelValidatorConsensus_causal_only_unsafe AcceptedUsesExactReplay
run_unsafe MC_ParallelValidatorConsensus_early_support_unsafe SupportRequiresLocalAcceptance
run_unsafe MC_ParallelValidatorConsensus_local_replay_unsafe PromotedFloorUsesLocalReplay
run_unsafe MC_ParallelValidatorConsensus_shared_authority_unsafe ExplicitFloorAuthority
run_unsafe MC_ParallelValidatorConsensus_shared_publication_unsafe FloorPublicationIsAtomic
run_unsafe MC_ParallelValidatorConsensus_non_atomic_floor_unsafe FloorPublicationIsAtomic
run_unsafe MC_ParallelValidatorConsensus_stale_floor_unsafe CommittedEffectsRemainInFloor
run_unsafe MC_ParallelValidatorConsensus_crash_root_unsafe ReplayRootsRemainLocallyRecorded
