#!/usr/bin/env bash
set -euo pipefail

# Commit identity check (CLAUDE.md "Commit Messages").
#
# Every commit author and every Co-authored-by trailer must be a ratified
# identity: an email that already authored a commit in the base history.
# Each commit on the base branch arrived through an accepted pull request,
# so its author is ratified by that process. A trailer never ratifies an
# identity. GitHub lists each co-author email that maps to an account in
# the contributors graph, so an unratified identity, human or AI, must not
# enter the history. Claude-Session trailers are always rejected: they are
# not identities, and they publish assistant session links.
#
#   message FILE
#       one message, ratified from the local HEAD history (commit-msg hook)
#   range RANGE --ratified-from REV [--repo DIR] [--exempt-authors FILE]
#       every commit in a range, ratified from REV history (CI)
#
# CI passes the pull request's base commit as REV, so a pull request cannot
# ratify its own identities. CI also passes the commits that GitHub
# attributes to the pull request author: an outside contributor's own
# commits pass, and maintainer review decides their acceptance. Co-authors
# of those commits are still checked.

REPO=.
RATIFIED_FROM=""
FORBIDDEN='^claude-session:'
declare -A RATIFIED=()
declare -A EXEMPT=()
CHECK_IDENTITIES=0

fail() {
	printf 'commit identity check error: %s\n' "$1" >&2
	exit 2
}

load_ratified() {
	local email
	while IFS= read -r email; do
		[ -n "$email" ] && RATIFIED[${email,,}]=1
	done < <(git -C "$REPO" log "$1" --format=%ae)
	CHECK_IDENTITIES=1
}

load_exempt() {
	local sha
	[ -f "$1" ] || fail "missing exempt-author file: $1"
	while IFS= read -r sha; do
		[[ "$sha" =~ ^[0-9a-f]{40}$ ]] && EXEMPT[$sha]=1
	done <"$1"
	return 0
}

# Prints one problem per line for a message and its author identity.
# Git strips comment lines from a message before it records the commit.
problems() {
	local text="$1" author="$2" ident email
	text="$(grep -v '^#' <<<"$text" || true)"
	grep -i -E "$FORBIDDEN" <<<"$text" | sed 's/^/forbidden trailer: /' || true
	[ "$CHECK_IDENTITIES" -eq 1 ] || return 0
	while IFS= read -r ident; do
		[ -n "$ident" ] || continue
		email="${ident##*<}"
		email="${email%%>*}"
		[ -n "${RATIFIED[${email,,}]+set}" ] || printf 'unratified identity: %s\n' "$ident"
	done < <(
		printf '%s\n' "$author"
		grep -i -E '^co-authored-by:' <<<"$text" | sed -E 's/^[^:]*:[[:space:]]*//' || true
	)
}

report() {
	printf '%s:\n' "$1" >&2
	sed 's/^/  /' <<<"$2" >&2
}

check_message() {
	local file="$1" author found
	[ -f "$file" ] || fail "missing message file: $file"
	author=""
	if git -C "$REPO" rev-parse --verify --quiet HEAD >/dev/null; then
		load_ratified HEAD
		author="$(git -C "$REPO" var GIT_AUTHOR_IDENT | sed -E 's/>.*/>/')"
	fi
	found="$(problems "$(cat "$file")" "$author")"
	[ -z "$found" ] && return 0
	report "The commit message" "$found"
	printf 'Remove forbidden trailers. Identities are ratified when a maintainer merges a pull request that one of them authored.\n' >&2
	return 1
}

check_range() {
	local range="$1" commits sha author found status=0
	[ -n "$RATIFIED_FROM" ] || usage
	git -C "$REPO" rev-parse --verify --quiet "${RATIFIED_FROM}^{commit}" >/dev/null ||
		fail "cannot resolve the ratification base $RATIFIED_FROM"
	load_ratified "$RATIFIED_FROM"
	commits="$(git -C "$REPO" rev-list "$range")" || fail "cannot list commits in $range"
	for sha in $commits; do
		author="$(git -C "$REPO" log -1 --format='%an <%ae>' "$sha")"
		[ -z "${EXEMPT[$sha]+set}" ] || author=""
		found="$(problems "$(git -C "$REPO" log -1 --format=%B "$sha")" "$author")"
		[ -z "$found" ] && continue
		report "Commit $(git -C "$REPO" rev-parse --short "$sha")" "$found"
		status=1
	done
	if [ "$status" -ne 0 ]; then
		printf 'Reword or re-author the commits (git rebase -i). A co-author is ratified after a merged pull request that they authored.\n' >&2
	fi
	return "$status"
}

usage() {
	printf '%s\n' "usage: $0 message FILE" \
		"       $0 range RANGE --ratified-from REV [--repo DIR] [--exempt-authors FILE]" >&2
	exit 2
}

[ "$#" -ge 2 ] || usage
command="$1"
target="$2"
shift 2
while [ "$#" -gt 0 ]; do
	case "$1" in
	--ratified-from) [ "$#" -ge 2 ] || usage; RATIFIED_FROM="$2"; shift 2 ;;
	--repo) [ "$#" -ge 2 ] || usage; REPO="$2"; shift 2 ;;
	--exempt-authors) [ "$#" -ge 2 ] || usage; load_exempt "$2"; shift 2 ;;
	*) usage ;;
	esac
done
case "$command" in
message) check_message "$target" ;;
range) check_range "$target" ;;
*) usage ;;
esac
