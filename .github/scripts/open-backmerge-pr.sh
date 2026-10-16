#!/usr/bin/env bash
# Opens a master -> dev back-merge pull request from a temporary backmerge/*
# branch at the current origin/master commit.
#
# Do not open the back-merge from master itself. A pull request with head
# master shows "Update branch", which merges dev into master through the
# admin bypass of masterProtect. On #747 that pushed dev onto master outside
# promotion (bf4915d57, 2026-10-16). The Promotion Source check in ci.yml
# rejects a pull request with head master.
#
# The branch is created through the GitHub API, so no local checkout or push
# (and no pre-push hook) is needed. The pull request is opened with the
# caller's gh login, so CI runs on it. The dev merge queue merges with a merge
# commit, so dev keeps the master history.
set -euo pipefail

remote="${BACKMERGE_REMOTE:-origin}"
repo="${BACKMERGE_REPOSITORY:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}"
stamp="${BACKMERGE_TIMESTAMP:-$(date -u +%Y%m%d-%H%M%S)}"

git fetch --quiet "$remote" dev master
sha="$(git rev-parse "$remote/master")"
[[ "$sha" =~ ^[0-9a-f]{40}$ ]] || { echo "ERROR: $remote/master does not resolve to a commit." >&2; exit 2; }

if git merge-base --is-ancestor "$sha" "$remote/dev"; then
	echo "Nothing to back-merge: $remote/dev already contains $remote/master (${sha:0:9})."
	exit 0
fi

count="$(git rev-list --count "$remote/dev..$sha")"
branch="backmerge/$stamp"
gh api "repos/$repo/git/refs" -f ref="refs/heads/$branch" -f sha="$sha" >/dev/null
echo "Created $branch at ${sha:0:9} ($remote/master, $count commits not on dev)."

if ! gh pr create --repo "$repo" --base dev --head "$branch" \
	--title "Back-merge master into dev ($stamp)" \
	--body "Merges \`master\` at \`${sha:0:9}\` into \`dev\` ($count commits).

This pull request comes from the temporary branch \`$branch\`, created by \`just backmerge\`. GitHub deletes \`$branch\` after the merge. Merge it with a merge commit, not a squash or a rebase, so that \`dev\` keeps the \`master\` history."; then
	echo "ERROR: the pull request was not opened. Deleting $branch." >&2
	gh api -X DELETE "repos/$repo/git/refs/heads/$branch" >/dev/null ||
		echo "WARNING: delete $branch by hand." >&2
	exit 1
fi
