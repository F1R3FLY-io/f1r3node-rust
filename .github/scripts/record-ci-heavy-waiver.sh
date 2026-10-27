#!/usr/bin/env bash
# Records a ci-heavy removal by the pull request author as a waiver.
#
# A ci-maintainers member waives the heavy pipeline on the member's own pull
# request by removing ci-heavy. Mergify keeps no state, so its rule adds
# ci-heavy again after every removal unless ci-heavy-waived is present. This
# script adds ci-heavy-waived, and then removes ci-heavy once if Mergify added
# it again before it saw the waiver.
#
# The dev, master, and ca-item queues accept the waiver only when the author
# is in the ci-maintainers team. For another author, Mergify adds ci-heavy
# again and keeps it, so the heavy pipeline still runs.
#
# Environment: REPOSITORY (owner/name), PR_NUMBER, GH_TOKEN, and for tests
# WAIVER_POLL_SECONDS and WAIVER_POLL_ATTEMPTS.
set -euo pipefail

: "${REPOSITORY:?REPOSITORY is required}"
[[ "${PR_NUMBER:-}" =~ ^[1-9][0-9]*$ ]] || {
	echo "::error::PR_NUMBER must be a pull request number" >&2
	exit 2
}
poll_seconds="${WAIVER_POLL_SECONDS:-5}"
poll_attempts="${WAIVER_POLL_ATTEMPTS:-6}"
labels_path="repos/${REPOSITORY}/issues/${PR_NUMBER}/labels"

has_label() {
	gh api "$labels_path" --jq '.[].name' | grep -qx "$1"
}

if ! has_label ci-heavy-waived; then
	gh api --method POST "$labels_path" -f 'labels[]=ci-heavy-waived' >/dev/null
fi

for _ in $(seq "$poll_attempts"); do
	if has_label ci-heavy; then
		gh api --method DELETE "${labels_path}/ci-heavy" >/dev/null
		echo "Removed ci-heavy again after Mergify added it."
		break
	fi
	sleep "$poll_seconds"
done

echo "Pull request #${PR_NUMBER} has ci-heavy-waived."
