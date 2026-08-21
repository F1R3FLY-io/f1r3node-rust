#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CHECK="$REPO_ROOT/.github/scripts/check-rholang-parser-pin.sh"
FIXTURE_ROOT="$REPO_ROOT/target/rholang-parser-pin-guard-test"
REVISION="02cef80"
RESOLVED="02cef80d32214ecdac3ee7d3b91c3786a34174d5"

cleanup() {
    rm -rf "$FIXTURE_ROOT"
}
trap cleanup EXIT
cleanup
mkdir -p "$FIXTURE_ROOT"

write_manifest() {
    printf '%s\n' \
        '[workspace]' \
        '[workspace.dependencies]' \
        'rholang-parser = { git = "https://github.com/F1R3FLY-io/rholang-rs", rev = "02cef80" }' \
        >"$FIXTURE_ROOT/Cargo.toml"
}

write_lock() {
    : >"$FIXTURE_ROOT/Cargo.lock"
    for package in rholang-parser rholang-tree-sitter rholang-tree-sitter-proc-macro; do
        printf '%s\n' \
            '[[package]]' \
            "name = \"$package\"" \
            'version = "0.1.0"' \
            "source = \"git+https://github.com/F1R3FLY-io/rholang-rs?rev=$REVISION#$RESOLVED\"" \
            >>"$FIXTURE_ROOT/Cargo.lock"
    done
}

expect_rejection() {
    if "$CHECK" "$FIXTURE_ROOT" >/dev/null 2>&1; then
        printf 'error: parser-pin guard accepted %s\n' "$1" >&2
        exit 1
    fi
}

write_manifest
write_lock
"$CHECK" "$FIXTURE_ROOT" >/dev/null

printf '%s\n' \
    '' \
    '[patch."https://github.com/F1R3FLY-io/rholang-rs"]' \
    'rholang-parser = { path = "../local-parser" }' \
    >>"$FIXTURE_ROOT/Cargo.toml"
expect_rejection 'a local parser patch'

write_manifest
write_lock
sed -i '0,/rev=02cef80/s//rev=deadbee/' "$FIXTURE_ROOT/Cargo.lock"
expect_rejection 'a lockfile revision mismatch'

write_manifest
write_lock
sed -i '/name = "rholang-tree-sitter-proc-macro"/,+2d' "$FIXTURE_ROOT/Cargo.lock"
expect_rejection 'a missing parser package lock entry'

printf 'rholang parser pin guard regressions passed\n'
