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
#
# The Promote dev to master workflow runs this script with a GitHub App token
# and sets PROMOTE_TRIGGERED_BY. The app is then the pull request author, so the
# user who started the workflow could approve the promotion alone. The body
# names that user and asks a different ci-maintainer to approve.
set -euo pipefail

# Check the login first, before any git or gh call.
creator="created by \`just promote\`"
review=""
if [ -n "${PROMOTE_TRIGGERED_BY:-}" ]; then
	[[ "$PROMOTE_TRIGGERED_BY" =~ ^[A-Za-z0-9](-?[A-Za-z0-9])*(\[bot\])?$ ]] ||
		{ echo "ERROR: PROMOTE_TRIGGERED_BY is not a GitHub login." >&2; exit 2; }
	creator="created by the Promote dev to master workflow for @$PROMOTE_TRIGGERED_BY"
	review="

@$PROMOTE_TRIGGERED_BY started this promotion. A different member of @F1R3FLY-io/ci-maintainers approves it."
fi

remote="${PROMOTE_REMOTE:-origin}"
repo="${PROMOTE_REPOSITORY:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}"
stamp="${PROMOTE_TIMESTAMP:-$(date -u +%Y%m%d-%H%M%S)}"

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

if ! gh pr create --repo "$repo" --base master --head "$branch" \
	--title "Promote dev to master ($stamp)" \
	--body "Promotes \`dev\` at \`${sha:0:9}\` to \`master\` ($count commits).

This pull request comes from the temporary branch \`$branch\`, $creator. GitHub deletes \`$branch\` after the merge. No open pull request targets it, so no pull request is retargeted.$review"; then
	echo "ERROR: the pull request was not opened. Deleting $branch." >&2
	gh api -X DELETE "repos/$repo/git/refs/heads/$branch" >/dev/null ||
		echo "WARNING: delete $branch by hand." >&2
	exit 1
fi
