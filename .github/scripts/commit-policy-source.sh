#!/usr/bin/env bash
set -euo pipefail

[[ "${GITHUB_REPOSITORY:-}" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || exit 2
[[ "${PR_NUMBER:-}" =~ ^[1-9][0-9]*$ ]] || exit 2
[[ "${HEAD_SHA:-}" =~ ^[0-9a-f]{40}$ ]] || exit 2

metadata="$(gh api "repos/$GITHUB_REPOSITORY/pulls/$PR_NUMBER" --jq \
	'[.base.repo.full_name, .head.repo.full_name, .base.ref, .head.ref, .head.sha] | @tsv')"
IFS=$'\t' read -r base_repo head_repo base_ref head_ref api_head <<<"$metadata"
[ "$api_head" = "$HEAD_SHA" ] || { echo "ERROR: the pull request head changed." >&2; exit 1; }
[ "$base_repo" = "$GITHUB_REPOSITORY" ] && [ "$head_repo" = "$GITHUB_REPOSITORY" ] || exit 0

case "$base_ref:$head_ref" in
	master:promote/*) source_branch=dev ;;
	ca-release:promote/ca-release-*) source_branch=ca-canary ;;
	dev:backmerge/master-*) source_branch=master ;;
	ca-canary:backmerge/ca-release-*) source_branch=ca-release ;;
	ca-canary:ca/sync-dev-*) source_branch=dev ;;
	*) exit 0 ;;
esac

source_metadata="$(gh api "repos/$GITHUB_REPOSITORY/branches/$source_branch" --jq \
	'[.protected, .commit.sha] | @tsv')"
IFS=$'\t' read -r protected source_sha <<<"$source_metadata"
[ "$protected" = true ] && [[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] ||
	{ echo "ERROR: the sync source is not a protected branch with a valid commit." >&2; exit 1; }

git fetch --quiet --no-tags --filter=tree:0 origin "$source_sha"
shared="$(git merge-base --all "$HEAD_SHA" "$source_sha")" ||
	{ echo "ERROR: the pull request shares no history with the protected source." >&2; exit 1; }
printf 'Commit Policy also ratifies from shared %s history at %s.\n' "$source_branch" "$shared" >&2
printf '%s\n' "$shared"
