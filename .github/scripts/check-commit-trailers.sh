#!/usr/bin/env bash
set -euo pipefail

# Commit trailer check (CLAUDE.md "Commit Messages").
#
# Rejects trailers that credit an AI tool as a commit co-author or link an
# assistant session. GitHub maps a co-author email such as
# noreply@anthropic.com to a GitHub account and lists that account in the
# repository contributors graph. Human Co-authored-by trailers stay allowed.
#
#   message FILE        check one commit message (the commit-msg hook)
#   range RANGE [REPO]  check every commit in a git range (CI)

FORBIDDEN='^(co-authored-by:.*@anthropic\.com|claude-session:)'

fail() {
	printf 'commit trailer check error: %s\n' "$1" >&2
	exit 2
}

# Prints each forbidden line of a message. Git strips comment lines from a
# message before it records the commit, so they are not checked.
forbidden_lines() {
	grep -v '^#' | grep -i -E "$FORBIDDEN" || true
}

report() {
	local label="$1" lines="$2"
	printf '%s carries a forbidden trailer:\n' "$label" >&2
	sed 's/^/  /' <<<"$lines" >&2
}

check_message() {
	local file="$1" lines
	[ -f "$file" ] || fail "missing message file: $file"
	lines="$(forbidden_lines <"$file")"
	[ -z "$lines" ] && return 0
	report "The commit message" "$lines"
	printf 'Remove the line. Turn off the co-author attribution setting of your assistant tool.\n' >&2
	return 1
}

check_range() {
	local range="$1" repo="${2:-.}" commits sha lines status=0
	commits="$(git -C "$repo" rev-list "$range")" || fail "cannot list commits in $range"
	for sha in $commits; do
		lines="$(git -C "$repo" log -1 --format=%B "$sha" | forbidden_lines)"
		[ -z "$lines" ] && continue
		report "Commit $(git -C "$repo" rev-parse --short "$sha")" "$lines"
		status=1
	done
	if [ "$status" -ne 0 ]; then
		printf 'Reword the commits without these lines (git rebase -i), then push again.\n' >&2
	fi
	return "$status"
}

usage() {
	printf '%s\n' "usage: $0 message FILE" "       $0 range RANGE [REPO]" >&2
	exit 2
}

command="${1:-}"
case "$command" in
message)
	[ "$#" -eq 2 ] || usage
	check_message "$2"
	;;
range)
	[ "$#" -eq 2 ] || [ "$#" -eq 3 ] || usage
	check_range "$2" "${3:-.}"
	;;
*) usage ;;
esac
