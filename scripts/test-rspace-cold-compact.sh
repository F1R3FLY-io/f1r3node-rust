#!/usr/bin/env bash
# Smoke test for scripts/rspace-cold-compact.sh.
#
# Covers argument parsing and precondition checks. The happy-path
# end-to-end case (actually running mdb_copy on a real env and
# verifying the swap) is left to manual operator verification
# because a real LMDB env requires the mdb tooling and a populated
# source — not worth the test fixture complexity for a one-off
# operator script.

set -euo pipefail

cd "$(dirname "$0")/.."
SCRIPT="$(pwd)/scripts/rspace-cold-compact.sh"

pass=0
fail=0

expect_fail() {
    local description="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        echo "FAIL: $description (expected non-zero exit)" >&2
        fail=$((fail + 1))
    else
        echo "ok: $description"
        pass=$((pass + 1))
    fi
}

expect_pass() {
    local description="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        echo "ok: $description"
        pass=$((pass + 1))
    else
        echo "FAIL: $description" >&2
        fail=$((fail + 1))
    fi
}

expect_pass "--help exits 0" "$SCRIPT" --help
expect_fail "missing --data-dir" "$SCRIPT"
# Pass --nonsense as the FIRST argument so the unknown-argument
# branch fires before any precondition check: otherwise the script
# fails on the missing-dir check first and the test passes for the
# wrong reason.
expect_fail "unknown argument" "$SCRIPT" --nonsense
expect_fail "nonexistent data dir" "$SCRIPT" --data-dir /definitely/does/not/exist

# A data dir with a valid rspace/ layout but no data.mdb.
TMP=$(mktemp -d)
trap 'rm -rf -- "$TMP"' EXIT
mkdir -p "$TMP/rspace/cold"
expect_fail "missing data.mdb" "$SCRIPT" --data-dir "$TMP"

# Residue from a prior failed run (cold.new directory exists).
touch "$TMP/rspace/cold/data.mdb"
mkdir "$TMP/rspace/cold.new"
expect_fail "rspace/cold.new residue is a hard error" "$SCRIPT" --data-dir "$TMP"
rmdir "$TMP/rspace/cold.new"

# Same for cold.old.
mkdir "$TMP/rspace/cold.old"
expect_fail "rspace/cold.old residue is a hard error" "$SCRIPT" --data-dir "$TMP"
rmdir "$TMP/rspace/cold.old"

# Dry run on a clean layout should succeed (mdb_copy not called).
expect_pass "dry run on clean layout" "$SCRIPT" --data-dir "$TMP" --dry-run

# Dry run must not modify state.
if [[ -d "$TMP/rspace/cold.new" || -d "$TMP/rspace/cold.old" ]]; then
    echo "FAIL: dry run created residue" >&2
    fail=$((fail + 1))
else
    echo "ok: dry run left state unchanged"
    pass=$((pass + 1))
fi

echo
echo "$pass passed, $fail failed"
[[ $fail -eq 0 ]] || exit 1
