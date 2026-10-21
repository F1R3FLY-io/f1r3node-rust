#!/usr/bin/env bash
# Opens a back-merge pull request from a temporary backmerge/* branch at the
# current commit of the source branch. Two back-merges exist:
#
#   master -> dev            `just backmerge`, branch backmerge/master-<sha>
#   ca-release -> ca-canary  `just ca-backmerge`, branch backmerge/ca-release-<sha>
#   dev -> ca-canary         `just ca-sync`, branch ca/sync-dev-<sha>, the dev sync
#
# BACKMERGE_SOURCE and BACKMERGE_TARGET select the back-merge. They default to
# master and dev, and any other pair is refused. The ca-release -> ca-canary
# back-merge runs each day from ca-daily-sync.yml, and Mergify merges it. A
# maintainer runs the dev sync now and then. Its ca/ head puts it in the
# ca-item queue of Mergify, so it needs the 22-hour soak like any ca/ item.
#
# Do not open the back-merge from the source branch itself. A pull request
# with head master shows "Update branch", which merges dev into master through
# the admin bypass of masterProtect. On #747 that pushed dev onto master
# outside promotion (bf4915d57, 2026-10-16). The Promotion Source check in
# ci.yml rejects a pull request with a protected head.
#
# The branch is created through the GitHub API, so no local checkout or push
# (and no pre-push hook) is needed. The pull request is opened with the
# caller's gh login, so CI runs on it. The dev merge queue merges with a merge
# commit, so dev keeps the master history. The ca-canary queue must merge the
# same way.
#
# The Promote and Back-merge workflow runs this script with a GitHub App token
# and sets BACKMERGE_TRIGGERED_BY. The app is then the pull request author, so
# the user who started the workflow could approve the back-merge alone. The
# body names that user and asks a different ci-maintainer to approve.
set -euo pipefail

source_branch="${BACKMERGE_SOURCE:-master}"
target_branch="${BACKMERGE_TARGET:-dev}"
case "$source_branch:$target_branch" in
	master:dev) recipe="just backmerge" prefix="backmerge/" kind="back-merge" title="Back-merge" ;;
	ca-release:ca-canary) recipe="just ca-backmerge" prefix="backmerge/" kind="back-merge" title="Back-merge" ;;
	dev:ca-canary) recipe="just ca-sync" prefix="ca/sync-" kind="sync" title="Sync" ;;
	*)
		echo "ERROR: no back-merge from '$source_branch' to '$target_branch'. Use master to dev, ca-release to ca-canary, or dev to ca-canary." >&2
		exit 2
		;;
esac

# Check the login first, before any git or gh call.
creator="created by \`$recipe\`"
review=""
if [ -n "${BACKMERGE_TRIGGERED_BY:-}" ]; then
	login="${BACKMERGE_TRIGGERED_BY%"[bot]"}"
	[[ ${#login} -le 39 && "$BACKMERGE_TRIGGERED_BY" =~ ^[A-Za-z0-9](-?[A-Za-z0-9])*(\[bot\])?$ ]] ||
		{ echo "ERROR: BACKMERGE_TRIGGERED_BY is not a GitHub login." >&2; exit 2; }
	creator="created by the Promote and Back-merge workflow for @$BACKMERGE_TRIGGERED_BY"
	review="

@$BACKMERGE_TRIGGERED_BY started this $kind. A different member of @F1R3FLY-io/ci-maintainers approves it."
elif [ "${BACKMERGE_SCHEDULED:-}" = 1 ]; then
	case "$source_branch:$target_branch" in
		ca-release:ca-canary) ;;
		*) echo "ERROR: BACKMERGE_SCHEDULED applies only to the ca-release to ca-canary back-merge." >&2; exit 2 ;;
	esac
	creator="created by the scheduled Release Train Daily Back-merge workflow"
	review="

Mergify merges this $kind when its CI passes. It needs no review."
fi

# The sync has no later push, so it gets ci-heavy now. The ca-soak label starts
# a 22-hour soak VM, so a maintainer adds it.
labels=()
if [ "$kind" = sync ]; then
	labels=(--label ci-heavy)
	review="$review

Add the \`ca-soak\` label to start the 22-hour soak. Mergify queues this sync after the soak passes and a different member of @F1R3FLY-io/ci-maintainers approves it."
fi

remote="${BACKMERGE_REMOTE:-origin}"
repo="${BACKMERGE_REPOSITORY:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}"
stamp="${BACKMERGE_TIMESTAMP:-$(date -u +%Y%m%d-%H%M%S)}"

git fetch --quiet "$remote" "$target_branch" "$source_branch"
sha="$(git rev-parse "$remote/$source_branch")"
[[ "$sha" =~ ^[0-9a-f]{40}$ ]] || { echo "ERROR: $remote/$source_branch does not resolve to a commit." >&2; exit 2; }

if git merge-base --is-ancestor "$sha" "$remote/$target_branch"; then
	echo "Nothing to $kind: $remote/$target_branch already contains $remote/$source_branch (${sha:0:9})."
	exit 0
fi

# One open pull request of each kind at a time. A second pull request would
# merge the same source commits twice and split the review.
open_filter="[.[] | select(.headRefName | startswith(\"$prefix\")) | \"#\\(.number)\"] | join(\" \")"
if ! open_prs="$(gh pr list --repo "$repo" --base "$target_branch" --state open --json number,headRefName \
	--jq "$open_filter")"; then
	echo "ERROR: the open pull requests to $target_branch could not be listed." >&2
	exit 1
fi
if [ -n "$open_prs" ]; then
	echo "A $kind pull request is already open: $open_prs. Merge or close it, then run $recipe again."
	exit 0
fi

count="$(git rev-list --count "$remote/$target_branch..$sha")"
# The branch is named after the source commit. A second run for the same
# commit, also a concurrent one, then fails at branch creation instead of
# opening a second pull request.
branch="$prefix$source_branch-${sha:0:12}"
if ! gh api "repos/$repo/git/refs" -f ref="refs/heads/$branch" -f sha="$sha" >/dev/null; then
	echo "ERROR: $branch was not created. If the branch exists, a $kind of this $source_branch commit is already in progress." >&2
	exit 1
fi
echo "Created $branch at ${sha:0:9} ($remote/$source_branch, $count commits not on $target_branch)."

if ! gh pr create --repo "$repo" --base "$target_branch" --head "$branch" ${labels[@]+"${labels[@]}"} \
	--title "$title $source_branch into $target_branch ($stamp)" \
	--body "Merges \`$source_branch\` at \`${sha:0:9}\` into \`$target_branch\` ($count commits).

This pull request comes from the temporary branch \`$branch\`, $creator. GitHub deletes \`$branch\` after the merge. Merge it with a merge commit, not a squash or a rebase, so that \`$target_branch\` keeps the \`$source_branch\` history.$review"; then
	echo "ERROR: the pull request was not opened. Deleting $branch." >&2
	gh api -X DELETE "repos/$repo/git/refs/heads/$branch" >/dev/null ||
		echo "WARNING: delete $branch by hand." >&2
	exit 1
fi
