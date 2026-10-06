#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/check-commit-trailers.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

expect_failure() {
	local label="$1"
	shift
	if "$@" >"$TMP/out" 2>"$TMP/err"; then
		printf 'expected failure: %s\n' "$label" >&2
		exit 1
	fi
}

message() {
	printf 'feat(node): add a feature\n\nBody text.\n'
	[ "$#" -eq 0 ] || printf '\n%s\n' "$@"
}

# --- Single message (commit-msg hook) ---------------------------------------
message >"$TMP/clean.txt"
"$TOOL" message "$TMP/clean.txt"
message 'Co-authored-by: Jane Smith <jane@example.com>' >"$TMP/human.txt"
"$TOOL" message "$TMP/human.txt"
message 'Co-Authored-By: Claude <noreply@anthropic.com>' >"$TMP/claude.txt"
expect_failure 'Anthropic co-author trailer' "$TOOL" message "$TMP/claude.txt"
grep -q 'Co-Authored-By: Claude' "$TMP/err"
message 'co-authored-by: Claude Fable 5.1 <NOREPLY@ANTHROPIC.COM>' >"$TMP/case.txt"
expect_failure 'trailer in another letter case' "$TOOL" message "$TMP/case.txt"
message 'Claude-Session: https://claude.ai/code/session_example' >"$TMP/session.txt"
expect_failure 'session link trailer' "$TOOL" message "$TMP/session.txt"
message '# Co-Authored-By: Claude <noreply@anthropic.com>' >"$TMP/comment.txt"
"$TOOL" message "$TMP/comment.txt"
printf 'docs: mention that Co-authored-by: lines with @anthropic.com are rejected\n' >"$TMP/subject.txt"
"$TOOL" message "$TMP/subject.txt"

# --- Commit range (CI) ------------------------------------------------------
REPO="$TMP/repo"
git init -q "$REPO"
git -C "$REPO" config user.name 'Trailer Test'
git -C "$REPO" config user.email 'test@example.com'
commit() { git -C "$REPO" commit -q --allow-empty -F "$1"; }
commit "$TMP/clean.txt"
BASE="$(git -C "$REPO" rev-parse HEAD)"
commit "$TMP/human.txt"
commit "$TMP/clean.txt"
"$TOOL" range "$BASE..HEAD" "$REPO"
commit "$TMP/claude.txt"
BAD="$(git -C "$REPO" rev-parse --short HEAD)"
commit "$TMP/clean.txt"
expect_failure 'range with an Anthropic co-author trailer' "$TOOL" range "$BASE..HEAD" "$REPO"
grep -q "$BAD" "$TMP/err"
"$TOOL" range "HEAD~1..HEAD" "$REPO"
expect_failure 'unknown range' "$TOOL" range "no-such-ref..HEAD" "$REPO"

# --- commit-msg hook ----------------------------------------------------------
HOOK="$ROOT/.githooks/commit-msg"
git -C "$REPO" config core.hooksPath "$ROOT/.githooks"
(cd "$REPO" && "$HOOK" "$TMP/clean.txt")
expect_failure 'commit-msg hook with an Anthropic co-author trailer' "$HOOK" "$TMP/claude.txt"
expect_failure 'commit through the hook with an Anthropic co-author trailer' \
	git -C "$REPO" -c core.hooksPath="$ROOT/.githooks" commit -q --allow-empty -F "$TMP/claude.txt"

printf 'commit trailer tests passed\n'
