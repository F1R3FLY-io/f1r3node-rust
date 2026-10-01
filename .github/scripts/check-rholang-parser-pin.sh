#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
MANIFEST="$REPO_ROOT/Cargo.toml"
LOCKFILE="$REPO_ROOT/Cargo.lock"
REMOTE="https://github.com/F1R3FLY-io/rholang-rs"

test -f "$MANIFEST"
test -f "$LOCKFILE"

if awk '
    /^\[patch\./ { section = $0 }
    /^\[/ && !/^\[patch\./ { section = "" }
    section ~ /rholang-rs/
' "$MANIFEST" | grep -q .; then
    printf 'error: local rholang-rs [patch] section is forbidden in a mergeable checkout\n' >&2
    exit 1
fi

mapfile -t revisions < <(
    sed -nE 's|^[[:space:]]*rholang-parser[[:space:]]*=.*git[[:space:]]*=[[:space:]]*"https://github.com/F1R3FLY-io/rholang-rs".*rev[[:space:]]*=[[:space:]]*"([0-9a-f]+)".*$|\1|p' "$MANIFEST"
)
if [[ "${#revisions[@]}" -ne 1 || ! "${revisions[0]}" =~ ^[0-9a-f]{7,40}$ ]]; then
    printf 'error: workspace rholang-parser must have one immutable rholang-rs git revision\n' >&2
    exit 1
fi
revision="${revisions[0]}"

lock_source() {
    local package="$1"
    awk -v package="$package" '
        /^\[\[package\]\]$/ { active = 0 }
        /^name = / {
            name = $0
            sub(/^name = "/, "", name)
            sub(/"$/, "", name)
            active = name == package
        }
        active && /^source = / {
            source = $0
            sub(/^source = "/, "", source)
            sub(/"$/, "", source)
            print source
            exit
        }
    ' "$LOCKFILE"
}

for package in rholang-parser rholang-tree-sitter rholang-tree-sitter-proc-macro; do
    source="$(lock_source "$package")"
    expected_prefix="git+$REMOTE?rev=$revision#"
    if [[ "$source" != "$expected_prefix"* ]]; then
        printf 'error: %s lock source does not match workspace parser revision %s\n' \
            "$package" "$revision" >&2
        exit 1
    fi
    resolved="${source##*#}"
    if [[ ! "$resolved" =~ ^[0-9a-f]{40}$ || "$resolved" != "$revision"* ]]; then
        printf 'error: %s lock source has an invalid resolved commit\n' "$package" >&2
        exit 1
    fi
done

if repository_root="$(git -C "$REPO_ROOT" rev-parse --show-toplevel 2>/dev/null)" \
    && [[ "$repository_root" == "$REPO_ROOT" ]]; then
    index_tag="$(git -C "$REPO_ROOT" ls-files -v -- Cargo.lock | cut -c1)"
    if [[ "$index_tag" == "S" || "$index_tag" == "s" ]]; then
        printf 'error: Cargo.lock is marked skip-worktree\n' >&2
        exit 1
    fi
fi

printf 'rholang parser pin invariants passed (%s)\n' "$revision"
