#!/usr/bin/env bash
# scripts/lib/test-lock.sh
#
# Sourced helper: host-wide test lock. Two sibling checkouts of this repo
# running their test suites at the same wall-clock instant compete for CPU on
# the timing sentinels (see .config/nextest.toml), for file descriptors under
# kern.maxfilesperproc, for VM address space across LMDB mmaps, and for the
# per-user SysV semaphore pool. Serialize on this lock so the second run
# blocks instead of corrupting the first. See F1R3FLY-io/f1r3node-rust#1014
# and #1102 for the incidents that motivated this.
#
# mkdir is atomic on all POSIX filesystems. The pid file records who owns
# the lock; a dead pid in it means the previous holder was SIGKILL'd (traps
# don't fire), so the next acquirer treats it as stale and takes over.
#
# Opt out with SKIP_TEST_LOCK=1 when you know you're the only run on the
# host and need the lock out of the way.
#
# API:
#   acquire_test_lock     block until the host lock is held (or return if SKIP_TEST_LOCK=1)
#   release_test_lock     release the lock if this process is the holder
#
# Variables (export to override before sourcing):
#   TEST_LOCK_DIR         lock directory (default: $TMPDIR/f1r3node-pre-push.lock.d)
#   SKIP_TEST_LOCK        1 to skip acquisition (default: 0)
#
# This file is SOURCED, not executed; it defines functions and defaults and
# must not enable `set -e` of its own (the sourcing script owns shell flags).

TEST_LOCK_DIR="${TEST_LOCK_DIR:-${TMPDIR:-/tmp}/f1r3node-pre-push.lock.d}"
SKIP_TEST_LOCK="${SKIP_TEST_LOCK:-0}"
TEST_LOCK_HELD="${TEST_LOCK_HELD:-0}"

# Colors — reuse the caller's if already set, otherwise define locally so the
# helper works standalone.
: "${RED:=$'\033[0;31m'}"
: "${GREEN:=$'\033[0;32m'}"
: "${YELLOW:=$'\033[0;33m'}"
: "${CYAN:=$'\033[0;36m'}"
: "${NC:=$'\033[0m'}"

acquire_test_lock() {
    if [[ "$SKIP_TEST_LOCK" == "1" ]]; then
        echo -e "${YELLOW}[test-lock] skipped (SKIP_TEST_LOCK=1)${NC}"
        return 0
    fi

    echo -e "${CYAN}[test-lock] attempting to acquire $TEST_LOCK_DIR${NC}"

    local waited=0 holder_pid holder_info sleep_s=5 repo_root
    repo_root="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
    while true; do
        if mkdir "$TEST_LOCK_DIR" 2>/dev/null; then
            echo "$$" > "$TEST_LOCK_DIR/pid"
            {
                echo "pid=$$"
                echo "host=$(hostname -s 2>/dev/null || echo unknown)"
                echo "repo=$repo_root"
                echo "acquired=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
            } > "$TEST_LOCK_DIR/info" 2>/dev/null || true
            TEST_LOCK_HELD=1
            if (( waited > 0 )); then
                echo -e "${CYAN}[test-lock] acquired after ${waited}s${NC}"
            else
                echo -e "${CYAN}[test-lock] acquired${NC}"
            fi
            return 0
        fi

        holder_pid="$(cat "$TEST_LOCK_DIR/pid" 2>/dev/null || true)"
        if [[ -n "$holder_pid" ]] && ! kill -0 "$holder_pid" 2>/dev/null; then
            echo -e "${YELLOW}[test-lock] stale lock from dead pid $holder_pid; clearing${NC}"
            rm -rf "$TEST_LOCK_DIR"
            continue
        fi

        if (( waited == 0 )); then
            holder_info="$(cat "$TEST_LOCK_DIR/info" 2>/dev/null | tr '\n' ' ' || true)"
            echo -e "${CYAN}[test-lock] waiting for holder pid ${holder_pid:-?} (${holder_info:-no info})${NC}"
        elif (( waited % 60 == 0 )); then
            echo -e "${CYAN}[test-lock] still waiting... (${waited}s elapsed, holder pid ${holder_pid:-?})${NC}"
        fi
        sleep "$sleep_s"
        waited=$((waited + sleep_s))
    done
}

release_test_lock() {
    if [[ "$TEST_LOCK_HELD" == "1" ]]; then
        rm -rf "$TEST_LOCK_DIR"
        TEST_LOCK_HELD=0
    fi
}
