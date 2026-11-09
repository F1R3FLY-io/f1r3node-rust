#!/usr/bin/env bash
# Reruns the newest ci.yml pull_request run of a pull request head after the
# ci-heavy label is added, or after a ci-maintainers waiver removes it. ci.yml does not run on labeled events: a label run
# replaces the checks of the head, so the queued and dequeued labels of the
# merge queue made the required checks pending again and the merge failed
# (pull request #826). A rerun keeps the pull_request event, so Commit Policy
# and Promotion Source keep their pull request checks, and build_base reads the
# current labels to start or skip the heavy pipeline.
#
# A running run is cancelled first, because a run in progress cannot be rerun.
# The cancel also stops its OCI jobs.
#
# WANT_HEAVY (true or false) is whether the pull request carries ci-heavy now.
# A run whose Integration Pipeline jobs already run or passed is not rerun when
# WANT_HEAVY is true, so the label that Mergify adds at open does not restart
# the run that the open started. A completed run without Integration Pipeline
# jobs is not rerun when WANT_HEAVY is false. Without WANT_HEAVY, the run is
# always rerun.
#
# Environment: REPOSITORY (owner/name), HEAD_SHA (40 hex), GH_TOKEN,
# WANT_HEAVY, and for tests RERUN_POLL_SECONDS and RERUN_POLL_ATTEMPTS.
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
want="${WANT_HEAVY:-}"
if [ -n "$want" ]; then
	[[ "$want" =~ ^(true|false)$ ]] || {
		echo "::error::WANT_HEAVY must be true or false" >&2
		exit 2
	}
	heavy="$(gh api "repos/${REPOSITORY}/actions/runs/${run_id}/jobs?per_page=100" \
		--jq '[.jobs[] | select(.name | startswith("Integration Pipeline /")) | select(.conclusion != "skipped")]
			| if length == 0 then "none"
			elif any(.[]; .status != "completed" or .conclusion == "success") and all(.[]; .status != "completed" or .conclusion == "success") then "active"
			else "failed" end')"
	if [ "$want" = true ] && [ "$heavy" = active ]; then
		echo "CI run ${run_id} for ${HEAD_SHA} already runs the heavy pipeline. No rerun."
		exit 0
	fi
	if [ "$want" = false ] && [ "$heavy" = none ] && [ "$status" = completed ]; then
		echo "CI run ${run_id} for ${HEAD_SHA} has no heavy pipeline. No rerun."
		exit 0
	fi
fi
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
echo "Reran CI run ${run_id} for ${HEAD_SHA} with the current labels."
