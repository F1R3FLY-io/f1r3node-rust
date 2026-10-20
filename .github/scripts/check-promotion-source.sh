#!/usr/bin/env bash
# Promotion Source check. Rejects a pull request from this repository whose
# head is master, dev, or a release train branch (ca-canary, ca-release), and
# closes it. ca-canary has the role of dev and ca-release the role of master.
#
# A protected head is dangerous in two ways. The repository deletes the head
# branch of a merged pull request, and GitHub then retargets every open pull
# request based on it (promotions from dev, 2026-10-04 and 2026-10-09). The
# pull request also shows "Update branch", which merges the base into the
# protected head through the admin bypass of the ruleset (#747 pushed dev onto
# master as bf4915d57, 2026-10-16).
#
# ci.yml (event pull_request) runs this script with a read-only token and no
# PR_NUMBER, so it only reports. protected-head-guard.yml (event
# pull_request_target) runs it from the base branch with PR_NUMBER and a
# pull-requests: write token, so it also closes the pull request. A closed
# pull request has no "Update branch" button and cannot merge.
#
# A fork pull request from the fork's own protected branch name is accepted.
#
# The ca-daily queue of .mergify.yml merges a backmerge/ca-release-<sha> pull
# request into ca-canary with the light checks only and no review. The queue
# matches the branch name, and anyone with write access, or a fork, can use
# that name. So a pull request into ca-canary with that head must have the
# commit its name gives, and that commit must be on ca-release. Otherwise this
# check fails, and the queue cannot merge it. ci.yml gives this case GH_TOKEN
# and HEAD_SHA, and the GitHub compare API checks that ca-release contains the
# commit.
set -euo pipefail

event="${EVENT_NAME:-}"
base="${BASE_REF:-}"
head="${HEAD_REF:-}"
head_repo="${HEAD_REPO:-}"
repository="${REPOSITORY:-}"
number="${PR_NUMBER:-}"
head_sha="${HEAD_SHA:-}"
reason="Reason: GitHub retargets the open pull requests of a merged head branch, and Update branch on a protected head writes to that branch through the admin bypass."

reject() {
	local message="$1"
	echo "::error::$message"
	if [[ "$number" =~ ^[0-9]+$ ]]; then
		if gh pr close "$number" --repo "$repository" --comment "Closed by the protected head guard. $message $reason"; then
			echo "Closed pull request #$number."
		else
			echo "::warning::Pull request #$number was not closed. Close it by hand before anyone uses Update branch."
		fi
	fi
	exit 1
}

if { [ "$event" = pull_request ] || [ "$event" = pull_request_target ]; } &&
	[ -n "$repository" ] && [ "$head_repo" = "$repository" ]; then
	case "$head" in
		dev)
			if [ "$base" = master ]; then
				reject "Do not open a promotion with head dev. Run 'just promote' to open it from a promote/* branch."
			fi
			reject "Do not open a pull request with head dev. Open it from a temporary branch at the dev commit."
			;;
		master)
			if [ "$base" = dev ]; then
				reject "Do not open a back-merge with head master. Run 'just backmerge' to open it from a backmerge/* branch."
			fi
			reject "Do not open a pull request with head master. Open it from a temporary branch at the master commit."
			;;
		ca-canary)
			if [ "$base" = ca-release ]; then
				reject "Do not open a promotion with head ca-canary. Run 'just ca-promote' to open it from a promote/* branch."
			fi
			reject "Do not open a pull request with head ca-canary. Open it from a temporary branch at the ca-canary commit."
			;;
		ca-release)
			if [ "$base" = ca-canary ]; then
				reject "Do not open a back-merge with head ca-release. Run 'just ca-backmerge' to open it from a backmerge/* branch."
			fi
			reject "Do not open a pull request with head ca-release. Open it from a temporary branch at the ca-release commit."
			;;
	esac
fi

if { [ "$event" = pull_request ] || [ "$event" = pull_request_target ]; } &&
	[ "$base" = ca-canary ] && [[ "$head" == backmerge/ca-release-* ]]; then
	name_sha="${head#backmerge/ca-release-}"
	[[ "$name_sha" =~ ^[0-9a-f]{12}$ ]] ||
		reject "The back-merge head $head does not name a 12-character ca-release commit. Run 'just ca-backmerge'."
	[[ "$head_sha" =~ ^[0-9a-f]{40}$ && "$head_sha" == "$name_sha"* ]] ||
		reject "The back-merge head $head is at ${head_sha:-an unknown commit}, not at the commit its name gives. Run 'just ca-backmerge'."
	if ! compare="$(gh api "repos/$repository/compare/$head_sha...ca-release" --jq .status)"; then
		reject "The back-merge head $head could not be compared with ca-release."
	fi
	case "$compare" in
		identical | ahead) ;;
		*) reject "The back-merge head $head is not a ca-release commit (compare status ${compare:-none}). Run 'just ca-backmerge'." ;;
	esac
fi

echo "Promotion source accepted: event=${event:-none} base=${base:-none} head=${head:-none}"
