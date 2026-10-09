#!/usr/bin/env bash
# Opens a dev -> master promotion pull request from a temporary promote/*
# branch at the current origin/dev commit.
#
# Do not promote from dev itself. The repository deletes the head branch of a
# merged pull request, and GitHub then retargets every open pull request based
# on that branch to the merged pull request's base. A promotion with head dev
# moved all open dev pull requests to master (2026-10-04 and 2026-10-09). The
# Promotion Source check in ci.yml rejects a pull request from dev to master.
#
# The branch is created through the GitHub API, so no local checkout or push
# (and no pre-push hook) is needed. The pull request is opened with the
# caller's gh login, so CI runs on it.
set -euo pipefail

remote="${PROMOTE_REMOTE:-origin}"
repo="${PROMOTE_REPOSITORY:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}"
stamp="${PROMOTE_TIMESTAMP:-$(date -u +%Y%m%d-%H%M)}"

git fetch --quiet "$remote" dev master
sha="$(git rev-parse "$remote/dev")"
[[ "$sha" =~ ^[0-9a-f]{40}$ ]] || { echo "ERROR: $remote/dev does not resolve to a commit." >&2; exit 2; }

if git merge-base --is-ancestor "$sha" "$remote/master"; then
	echo "Nothing to promote: $remote/master already contains $remote/dev (${sha:0:9})."
	exit 0
fi

count="$(git rev-list --count "$remote/master..$sha")"
branch="promote/$stamp"
gh api "repos/$repo/git/refs" -f ref="refs/heads/$branch" -f sha="$sha" >/dev/null
echo "Created $branch at ${sha:0:9} ($remote/dev, $count commits not on master)."

gh pr create --repo "$repo" --base master --head "$branch" \
	--title "Promote dev to master ($stamp)" \
	--body "Promotes \`dev\` at \`${sha:0:9}\` to \`master\` ($count commits).

This pull request comes from the temporary branch \`$branch\`, created by \`just promote\`. GitHub deletes \`$branch\` after the merge. No open pull request targets it, so no pull request is retargeted."
