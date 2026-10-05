#!/usr/bin/env bash
# Keep the local rholang-rs [patch] of the Reified RSpaces campaign out of every commit
# (plan decisions 6 and 23, I-14, and Phase 0 step 4).
#
# The node worktree builds the v3 parser through an uncommitted [patch] in Cargo.toml. This
# gate reads every Cargo.toml manifest in the git index and in HEAD, and fails when one of them
# holds a [patch] entry that is not in the allowlist. The allowlist is empty, because no commit
# of the campaign holds a [patch]: the final pin replaces the [patch] with a published revision.
# The working tree is not read, because it holds the local [patch] by design.
#
# Usage:
#   check-reified-rspaces-patch-gate.sh            patch_gate_clean: the index and HEAD hold no
#                                                  [patch]; needs a git checkout
#   check-reified-rspaces-patch-gate.sh --control  patch_gate_detects_parser_override: the same
#                                                  check reads the negative-control fixture,
#                                                  which holds the parser [patch], and must fail
#
# Exit status: 0 when no manifest holds a [patch] entry outside the allowlist, 1 when at least
# one does, and 2 on a usage or environment error.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTROL_FIXTURE="scripts/verify-reified/tests/fixtures/patch-gate/parser-override.toml"

# Each allowed entry is "<manifest path>|<patch source>|<crate>". The list is empty.
ALLOWED_PATCHES=()

usage() {
    printf 'usage: %s [--control]\n' "$(basename "$0")" >&2
    exit 2
}

# Reads a manifest on standard input and prints one "<source>\t<crate>" line for each entry of
# its [patch] tables. A [patch] source that is not a table prints the crate "*".
patch_entries() {
    python3 -c '
import sys
import tomllib

try:
    manifest = tomllib.loads(sys.stdin.read())
except tomllib.TOMLDecodeError as error:
    print(f"the manifest is not valid TOML: {error}", file=sys.stderr)
    sys.exit(3)
patch = manifest.get("patch", {})
if not isinstance(patch, dict):
    print("*\t*")
    sys.exit(0)
for source in sorted(patch):
    crates = patch[source]
    if isinstance(crates, dict) and crates:
        for crate in sorted(crates):
            print(f"{source}\t{crate}")
    else:
        print(f"{source}\t*")
'
}

is_allowed() {
    local entry="$1"
    local allowed
    for allowed in "${ALLOWED_PATCHES[@]}"; do
        if [[ "$allowed" == "$entry" ]]; then
            return 0
        fi
    done
    return 1
}

checked=0
violations=0

# check_manifest LABEL PATH: reads the manifest text on standard input.
check_manifest() {
    local label="$1"
    local path="$2"
    local entries
    local status=0
    checked=$((checked + 1))
    entries="$(patch_entries)" || status=$?
    if [[ "$status" -ne 0 ]]; then
        printf 'patch gate: %s %s cannot be read as a manifest\n' "$label" "$path"
        violations=$((violations + 1))
        return 0
    fi
    local source
    local crate
    while IFS=$'\t' read -r source crate; do
        if [[ -z "$source" ]]; then
            continue
        fi
        if ! is_allowed "$path|$source|$crate"; then
            printf 'patch gate: %s %s holds [patch."%s"] %s\n' "$label" "$path" "$source" "$crate"
            violations=$((violations + 1))
        fi
    done <<<"$entries"
}

manifest_paths() {
    grep -E '(^|/)Cargo\.toml$' || true
}

case "${1:-}" in
    "")
        if ! git -C "$REPO_ROOT" rev-parse --verify --quiet HEAD >/dev/null; then
            printf 'patch gate: %s is not a git checkout with a HEAD commit\n' "$REPO_ROOT" >&2
            exit 2
        fi
        index_files="$(git -C "$REPO_ROOT" ls-files --cached)"
        head_files="$(git -C "$REPO_ROOT" ls-tree -r --name-only HEAD)"
        mapfile -t staged < <(manifest_paths <<<"$index_files")
        mapfile -t committed < <(manifest_paths <<<"$head_files")
        if [[ "${#staged[@]}" -eq 0 || "${#committed[@]}" -eq 0 ]]; then
            printf 'patch gate: the index or HEAD holds no Cargo.toml\n' >&2
            exit 2
        fi
        for path in "${staged[@]}"; do
            manifest="$(git -C "$REPO_ROOT" show ":$path")"
            check_manifest staged "$path" <<<"$manifest"
        done
        for path in "${committed[@]}"; do
            manifest="$(git -C "$REPO_ROOT" show "HEAD:$path")"
            check_manifest committed "$path" <<<"$manifest"
        done
        ;;
    --control)
        if [[ ! -f "$REPO_ROOT/$CONTROL_FIXTURE" ]]; then
            printf 'patch gate: the control fixture %s is missing\n' "$CONTROL_FIXTURE" >&2
            exit 2
        fi
        check_manifest staged "$CONTROL_FIXTURE" <"$REPO_ROOT/$CONTROL_FIXTURE"
        ;;
    *)
        usage
        ;;
esac

printf 'patch gate: checked %d manifests, %d violations\n' "$checked" "$violations"
if [[ "$violations" -ne 0 ]]; then
    exit 1
fi
