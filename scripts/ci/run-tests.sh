#!/usr/bin/env bash
# scripts/ci/run-tests.sh
#
# Sanctioned direct-test entry point. Acquires the host-wide test lock
# (scripts/lib/test-lock.sh) before running the forwarded command so that
# concurrent test runs across sibling checkouts of this repo cannot compete
# for shared resources (LMDB SysV semaphores, test tempdirs, FDs, sockets).
# See F1R3FLY-io/f1r3node-rust#1014 and #1102.
#
# Usage:
#   scripts/ci/run-tests.sh                           # cargo nextest run
#   scripts/ci/run-tests.sh cargo nextest run -p casper
#   scripts/ci/run-tests.sh cargo test --release -p rholang
#
# Environment:
#   SKIP_TEST_LOCK=1    skip the lock (single-run case).
#   TEST_LOCK_DIR=path  override the lock directory.
#   CI / GITHUB_ACTIONS / GITLAB_CI auto-sets SKIP_TEST_LOCK=1.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# shellcheck source=../lib/test-lock.sh
source "$REPO_ROOT/scripts/lib/test-lock.sh"

# In CI there is one test runner per host; the lock has nothing to serialize
# against and only adds noise to logs if contested cross-job.
if [[ -n "${CI:-}${GITHUB_ACTIONS:-}${GITLAB_CI:-}" ]]; then
    SKIP_TEST_LOCK=1
fi

acquire_test_lock
trap 'rc=$?; release_test_lock; exit $rc' EXIT
trap 'release_test_lock; exit 130' INT
trap 'release_test_lock; exit 143' TERM

cd "$REPO_ROOT"

if [[ $# -eq 0 ]]; then
    cargo nextest run
else
    "$@"
fi
