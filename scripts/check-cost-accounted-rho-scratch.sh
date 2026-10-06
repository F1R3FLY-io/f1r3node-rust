#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/lib/verification-tmpdir.sh"

mkdir -p "$ROOT/target/verification"
TEST_PARENT="$(mktemp -d "$ROOT/target/verification/cost-accounted-rho-scratch.XXXXXX")"
RELATIVE_TEST_PARENT="${TEST_PARENT#"$ROOT"/}"
trap 'rm -rf -- "$TEST_PARENT"' EXIT

set +e
bash -c '
  set -euo pipefail
  source "$1"
  verification_tmpdir_install "$2"
  cd "$3"
  test "${TMPDIR#/}" != "$TMPDIR"
  test -d "$TMPDIR"
  mkdir -p "$TMPDIR/casper-shared-lmdb-regression"
  exit 17
' _ "$ROOT/scripts/lib/verification-tmpdir.sh" "$RELATIVE_TEST_PARENT" "$ROOT/models"
probe_status=$?
set -e

if [[ "$probe_status" -ne 17 ]]; then
  printf 'scratch cleanup probe returned %s, expected 17\n' "$probe_status" >&2
  exit 1
fi
if find "$TEST_PARENT" -mindepth 1 -maxdepth 1 -type d -name 'tmp.*' | grep -q .; then
  echo 'scratch cleanup probe retained an owned verification directory' >&2
  exit 1
fi
if verification_tmpdir_cleanup "$TEST_PARENT" "$TEST_PARENT"; then
  echo 'scratch cleanup accepted its parent as a deletion target' >&2
  exit 1
fi

echo 'Verification scratch ownership and cleanup passed.'
