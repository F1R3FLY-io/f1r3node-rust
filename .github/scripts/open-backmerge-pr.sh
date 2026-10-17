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
#
# The Promote and Back-merge workflow runs this script with a GitHub App token
# and sets BACKMERGE_TRIGGERED_BY. The app is then the pull request author, so
# the user who started the workflow could approve the back-merge alone. The
# body names that user and asks a different ci-maintainer to approve.
set -euo pipefail

# Check the login first, before any git or gh call.
creator="created by \`just backmerge\`"
review=""
if [ -n "${BACKMERGE_TRIGGERED_BY:-}" ]; then
	[[ "$BACKMERGE_TRIGGERED_BY" =~ ^[A-Za-z0-9](-?[A-Za-z0-9])*(\[bot\])?$ ]] ||
		{ echo "ERROR: BACKMERGE_TRIGGERED_BY is not a GitHub login." >&2; exit 2; }
	creator="created by the Promote and Back-merge workflow for @$BACKMERGE_TRIGGERED_BY"
	review="

@$BACKMERGE_TRIGGERED_BY started this back-merge. A different member of @F1R3FLY-io/ci-maintainers approves it."
fi

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

# One back-merge at a time. A second pull request would merge the same master
# commits twice and split the review.
if ! open_prs="$(gh pr list --repo "$repo" --base dev --state open --json number,headRefName \
	--jq '[.[] | select(.headRefName | startswith("backmerge/")) | "#\(.number)"] | join(" ")')"; then
	echo "ERROR: the open pull requests to dev could not be listed." >&2
	exit 1
fi
if [ -n "$open_prs" ]; then
	echo "A back-merge pull request is already open: $open_prs. Merge or close it, then run just backmerge again."
	exit 0
fi

count="$(git rev-list --count "$remote/dev..$sha")"
# The branch is named after the master commit. A second run for the same
# commit, also a concurrent one, then fails at branch creation instead of
# opening a second pull request.
branch="backmerge/master-${sha:0:12}"
if ! gh api "repos/$repo/git/refs" -f ref="refs/heads/$branch" -f sha="$sha" >/dev/null; then
	echo "ERROR: $branch was not created. If the branch exists, a back-merge of this master commit is already in progress." >&2
	exit 1
fi
echo "Created $branch at ${sha:0:9} ($remote/master, $count commits not on dev)."

if ! gh pr create --repo "$repo" --base dev --head "$branch" \
	--title "Back-merge master into dev ($stamp)" \
	--body "Merges \`master\` at \`${sha:0:9}\` into \`dev\` ($count commits).

This pull request comes from the temporary branch \`$branch\`, $creator. GitHub deletes \`$branch\` after the merge. Merge it with a merge commit, not a squash or a rebase, so that \`dev\` keeps the \`master\` history.$review"; then
	echo "ERROR: the pull request was not opened. Deleting $branch." >&2
	gh api -X DELETE "repos/$repo/git/refs/heads/$branch" >/dev/null ||
		echo "WARNING: delete $branch by hand." >&2
	exit 1
fi
