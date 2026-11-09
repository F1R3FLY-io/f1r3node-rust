#!/usr/bin/env bash
# Reruns the newest ci.yml pull_request run of a pull request head after the
# ci-heavy label is added. ci.yml does not run on labeled events: a label run
# replaces the checks of the head, so the queued and dequeued labels of the
# merge queue made the required checks pending again and the merge failed
# (pull request #826). A rerun keeps the pull_request event, so Commit Policy
# and Promotion Source keep their pull request checks, and build_base reads the
# current labels to start the heavy pipeline.
#
# A running run is cancelled first, because a run in progress cannot be rerun.
#
# Environment: REPOSITORY (owner/name), HEAD_SHA (40 hex), GH_TOKEN, and for
# tests RERUN_POLL_SECONDS and RERUN_POLL_ATTEMPTS.
set -euo pipefail

: "${REPOSITORY:?REPOSITORY is required}"
[[ "${HEAD_SHA:-}" =~ ^[0-9a-f]{40}$ ]] || {
	echo "::error::HEAD_SHA must be a full commit SHA" >&2
	exit 2
}
poll_seconds="${RERUN_POLL_SECONDS:-10}"
poll_attempts="${RERUN_POLL_ATTEMPTS:-60}"

run_id="$(gh api "repos/${REPOSITORY}/actions/workflows/ci.yml/runs?head_sha=${HEAD_SHA}&event=pull_request&per_page=100" \
	--jq '[.workflow_runs[] | select(.path == ".github/workflows/ci.yml")] | max_by(.id) | .id // empty')"
if [ -z "$run_id" ]; then
	echo "::notice::No CI run exists for ${HEAD_SHA}. The next push to the pull request runs the heavy pipeline."
	exit 0
fi
[[ "$run_id" =~ ^[1-9][0-9]*$ ]] || {
	echo "::error::The CI run id is invalid: ${run_id}" >&2
	exit 1
}

status="$(gh api "repos/${REPOSITORY}/actions/runs/${run_id}" --jq '.status')"
if [ "$status" != completed ]; then
	gh api -X POST "repos/${REPOSITORY}/actions/runs/${run_id}/cancel" >/dev/null
	for _ in $(seq 1 "$poll_attempts"); do
		status="$(gh api "repos/${REPOSITORY}/actions/runs/${run_id}" --jq '.status')"
		[ "$status" = completed ] && break
		sleep "$poll_seconds"
	done
	[ "$status" = completed ] || {
		echo "::error::CI run ${run_id} did not complete after the cancel request" >&2
		exit 1
	}
fi

gh api -X POST "repos/${REPOSITORY}/actions/runs/${run_id}/rerun" >/dev/null
echo "Reran CI run ${run_id} for ${HEAD_SHA} with the ci-heavy label."
