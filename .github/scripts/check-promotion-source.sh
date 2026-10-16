#!/usr/bin/env bash
# Promotion Source check. Rejects a pull request from this repository whose
# head is master or dev, and closes it.
#
# A protected head is dangerous in two ways. The repository deletes the head
# branch of a merged pull request, and GitHub then retargets every open pull
# request based on it (promotions from dev, 2026-10-04 and 2026-10-09). The
# pull request also shows "Update branch", which merges the base into the
# protected head through the admin bypass of the ruleset (#747 pushed dev onto
# master as bf4915d57, 2026-10-16).
#
# A failing check does not remove the button, and devProtect does not require
# this check. A closed pull request has no "Update branch" button and cannot
# merge, so the check also closes the pull request when PR_NUMBER is set.
#
# A fork pull request from the fork's own master or dev is accepted.
set -euo pipefail

event="${EVENT_NAME:-}"
base="${BASE_REF:-}"
head="${HEAD_REF:-}"
head_repo="${HEAD_REPO:-}"
repository="${REPOSITORY:-}"
number="${PR_NUMBER:-}"

reject() {
	local message="$1"
	echo "::error::$message"
	if [[ "$number" =~ ^[0-9]+$ ]]; then
		if gh pr close "$number" --repo "$repository" --comment "Closed by the Promotion Source check. $message"; then
			echo "Closed pull request #$number."
		else
			echo "::warning::Pull request #$number was not closed. Close it by hand before anyone uses Update branch."
		fi
	fi
	exit 1
}

if [ "$event" = pull_request ] && [ -n "$repository" ] && [ "$head_repo" = "$repository" ]; then
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
	esac
fi

echo "Promotion source accepted: event=${event:-none} base=${base:-none} head=${head:-none}"
