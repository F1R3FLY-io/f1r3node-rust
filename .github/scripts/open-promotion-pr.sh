#!/usr/bin/env bash
# Opens a promotion pull request from a temporary promote/* branch at the
# current commit of the source branch. Two promotions exist:
#
#   dev -> master            `just promote`, branch promote/<stamp>
#   ca-canary -> ca-release  `just ca-promote`, branch promote/ca-release-<stamp>
#
# PROMOTE_SOURCE and PROMOTE_TARGET select the promotion. They default to dev
# and master, and any other pair is refused.
#
# Do not promote from the source branch itself. The repository deletes the
# head branch of a merged pull request, and GitHub then retargets every open
# pull request based on that branch to the merged pull request's base. A
# promotion with head dev moved all open dev pull requests to master
# (2026-10-04 and 2026-10-09). The Promotion Source check in ci.yml rejects a
# pull request with a protected head.
#
# The branch is created through the GitHub API, so no local checkout or push
# (and no pre-push hook) is needed. The pull request is opened with the
# caller's gh login, so CI runs on it.
#
# The Promote and Back-merge workflow runs this script with a GitHub App token
# and sets PROMOTE_TRIGGERED_BY. The app is then the pull request author, so the
# user who started the workflow could approve the promotion alone. The body
# names that user and asks a different ci-maintainer to approve.
set -euo pipefail

source_branch="${PROMOTE_SOURCE:-dev}"
target_branch="${PROMOTE_TARGET:-master}"
case "$source_branch:$target_branch" in
	dev:master)
		recipe="just promote"
		prefix="promote/"
		;;
	ca-canary:ca-release)
		recipe="just ca-promote"
		prefix="promote/ca-release-"
		;;
	*)
		echo "ERROR: no promotion from '$source_branch' to '$target_branch'. Use dev to master or ca-canary to ca-release." >&2
		exit 2
		;;
esac

# Check the login first, before any git or gh call.
creator="created by \`$recipe\`"
review=""
if [ -n "${PROMOTE_TRIGGERED_BY:-}" ]; then
	login="${PROMOTE_TRIGGERED_BY%"[bot]"}"
	[[ ${#login} -le 39 && "$PROMOTE_TRIGGERED_BY" =~ ^[A-Za-z0-9](-?[A-Za-z0-9])*(\[bot\])?$ ]] ||
		{ echo "ERROR: PROMOTE_TRIGGERED_BY is not a GitHub login." >&2; exit 2; }
	creator="created by the Promote and Back-merge workflow for @$PROMOTE_TRIGGERED_BY"
	review="

@$PROMOTE_TRIGGERED_BY started this promotion. A different member of @F1R3FLY-io/ci-maintainers approves it."
fi

remote="${PROMOTE_REMOTE:-origin}"
repo="${PROMOTE_REPOSITORY:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}"
stamp="${PROMOTE_TIMESTAMP:-$(date -u +%Y%m%d-%H%M%S)}"

git fetch --quiet "$remote" "$source_branch" "$target_branch"
sha="$(git rev-parse "$remote/$source_branch")"
[[ "$sha" =~ ^[0-9a-f]{40}$ ]] || { echo "ERROR: $remote/$source_branch does not resolve to a commit." >&2; exit 2; }

if git merge-base --is-ancestor "$sha" "$remote/$target_branch"; then
	echo "Nothing to promote: $remote/$target_branch already contains $remote/$source_branch (${sha:0:9})."
	exit 0
fi

count="$(git rev-list --count "$remote/$target_branch..$sha")"
branch="$prefix$stamp"
gh api "repos/$repo/git/refs" -f ref="refs/heads/$branch" -f sha="$sha" >/dev/null
echo "Created $branch at ${sha:0:9} ($remote/$source_branch, $count commits not on $target_branch)."

if ! gh pr create --repo "$repo" --base "$target_branch" --head "$branch" \
	--title "Promote $source_branch to $target_branch ($stamp)" \
	--body "Promotes \`$source_branch\` at \`${sha:0:9}\` to \`$target_branch\` ($count commits).

This pull request comes from the temporary branch \`$branch\`, $creator. GitHub deletes \`$branch\` after the merge. No open pull request targets it, so no pull request is retargeted.$review"; then
	echo "ERROR: the pull request was not opened. Deleting $branch." >&2
	gh api -X DELETE "repos/$repo/git/refs/heads/$branch" >/dev/null ||
		echo "WARNING: delete $branch by hand." >&2
	exit 1
fi
