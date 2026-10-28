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

labels=""
read_labels() {
	labels="$(gh api "$labels_path" --jq '.[].name')" || {
		echo "::error::The labels of pull request #${PR_NUMBER} could not be read." >&2
		exit 1
	}
}

has_label() {
	grep -qx "$1" <<<"$labels"
}

read_labels
if ! has_label ci-heavy-waived; then
	gh api --method POST "$labels_path" -f 'labels[]=ci-heavy-waived' >/dev/null
fi

removed=false
for _ in $(seq "$poll_attempts"); do
	read_labels
	if has_label ci-heavy; then
		gh api --method DELETE "${labels_path}/ci-heavy" >/dev/null
		echo "Removed ci-heavy again after Mergify added it."
		removed=true
		break
	fi
	sleep "$poll_seconds"
done

if [ "$removed" = true ]; then
	sleep "$poll_seconds"
fi
read_labels
if has_label ci-heavy; then
	echo "::warning::Pull request #${PR_NUMBER} has ci-heavy again. The waiver does not apply, and the heavy pipeline runs. Remove ci-heavy again to waive it."
	exit 0
fi

echo "Pull request #${PR_NUMBER} has ci-heavy-waived."
