#!/bin/bash

set -e

if command -v clang >/dev/null 2>&1; then
  export CC="${CC:-clang}"
fi
if command -v clang++ >/dev/null 2>&1; then
  export CXX="${CXX:-clang++}"
fi

# Serialize against sibling checkouts on this host — see
# F1R3FLY-io/f1r3node-rust#1014 and #1102.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=lib/test-lock.sh
source "$REPO_ROOT/scripts/lib/test-lock.sh"
acquire_test_lock
trap 'rc=$?; release_test_lock; exit $rc' EXIT
trap 'release_test_lock; exit 130' INT
trap 'release_test_lock; exit 143' TERM

cd "$REPO_ROOT/rspace++/"
cargo test --release

cd ../rholang
cargo test --release

cd ../casper
cargo test --release

cd ../models
cargo test --release

cd ../crypto
cargo test --release

cd ../shared
cargo test --release

cd ../graphz
cargo test --release

cd ../block-storage
cargo test --release

cd ../comm
cargo test --release

cd ../node
cargo test --release

cd ../deploy-queue
cargo test --release
