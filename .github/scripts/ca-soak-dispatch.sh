#!/usr/bin/env bash
# Starts and invalidates the release train soaks (issue #791).
# ca-soak-dispatch.yml runs it with the event in EVENT.
#
#   labeled      the ca-soak label on a ca/ pull request into ca-canary. Removes
#                ca-soaked and starts the 22-hour soak of the pull request head
#                (gate ca-item).
#   synchronize  a push to that pull request. A push by Mergify only merges the
#                base, so it changes nothing. Any other push removes ca-soak and
#                ca-soaked and cancels the running soak of the pull request. A
#                new soak needs the ca-soak label again, so a push never starts
#                a soak VM.
#   push         a push to ca-release. Starts the 60-hour soak of the pushed
#                commit (gate ca-release).
#
# The soak runs from master, so its gate inputs exist only after this change
# reaches master. The soak names its run "Merge Recovery Soak [ca-item:#<PR>]",
# and the synchronize event finds the runs to cancel by that name.
set -euo pipefail

[[ "${GH_REPO:-}" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || { echo "ERROR: GH_REPO is not owner/name." >&2; exit 2; }
workflow_ref="${SOAK_WORKFLOW_REF:-master}"

# pr_checks: refuses a pull request that is not a ca/ item of this repository.
pr_checks() {
	[[ "${PR_NUMBER:-}" =~ ^[1-9][0-9]*$ ]] || { echo "ERROR: PR_NUMBER is not a pull request number." >&2; exit 2; }
	[[ "${HEAD_SHA:-}" =~ ^[0-9a-f]{40}$ ]] || { echo "ERROR: HEAD_SHA is not a commit SHA." >&2; exit 2; }
	if [ "${BASE_REF:-}" != ca-canary ] || [[ "${HEAD_REF:-}" != ca/* ]] || [ "${HEAD_REPO:-}" != "$GH_REPO" ]; then
		echo "Pull request #$PR_NUMBER ($HEAD_REPO:$HEAD_REF into $BASE_REF) is not a ca/ item of $GH_REPO into ca-canary. Nothing to do."
		exit 0
	fi
}

# remove_label NAME: removes a label from the pull request. A missing label is
# not an error.
remove_label() {
	if gh api --method DELETE "repos/$GH_REPO/issues/$PR_NUMBER/labels/$1" >/dev/null 2>&1; then
		echo "Removed $1 from pull request #$PR_NUMBER."
	fi
}

case "${EVENT:-}" in
	labeled)
		if [ "${LABEL:-}" != ca-soak ]; then
			echo "The label ${LABEL:-none} does not start a soak. Nothing to do."
			exit 0
		fi
		pr_checks
		remove_label ca-soaked
		gh workflow run merge-recovery-soak.yml --repo "$GH_REPO" --ref "$workflow_ref" \
			-f target_ref="$HEAD_SHA" -f duration=daily-24h -f gate=ca-item -f gate_pr="$PR_NUMBER"
		echo "Started the 22-hour soak of pull request #$PR_NUMBER at ${HEAD_SHA:0:9}."
		;;
	synchronize)
		pr_checks
		if [ "${SENDER:-}" = "mergify[bot]" ]; then
			echo "Mergify merged the base into pull request #$PR_NUMBER. The soak result stays."
			exit 0
		fi
		remove_label ca-soaked
		remove_label ca-soak
		title="Merge Recovery Soak [ca-item:#$PR_NUMBER]"
		# Every status that is not completed, on every page.
		runs="$(for run_status in queued in_progress waiting requested pending; do
			gh api --paginate "repos/$GH_REPO/actions/workflows/merge-recovery-soak.yml/runs?event=workflow_dispatch&status=$run_status&per_page=100" \
				--jq ".workflow_runs[] | select(.display_title == \"$title\" and .status != \"completed\") | .id" || exit 1
		done | sort -u)"
		for run in $runs; do
			gh api --method POST "repos/$GH_REPO/actions/runs/$run/cancel" >/dev/null
			echo "Cancelled soak run $run of pull request #$PR_NUMBER."
		done
		echo "A push changed pull request #$PR_NUMBER. Add the ca-soak label to soak ${HEAD_SHA:0:9}."
		;;
	push)
		[[ "${HEAD_SHA:-}" =~ ^[0-9a-f]{40}$ ]] || { echo "ERROR: HEAD_SHA is not a commit SHA." >&2; exit 2; }
		if [ "${PUSH_REF:-}" != refs/heads/ca-release ]; then
			echo "ERROR: only a push to ca-release starts the 60-hour soak, not ${PUSH_REF:-none}." >&2
			exit 2
		fi
		gh workflow run merge-recovery-soak.yml --repo "$GH_REPO" --ref "$workflow_ref" \
			-f target_ref="$HEAD_SHA" -f duration=weekend-60h -f gate=ca-release
		echo "Started the 60-hour soak of ca-release at ${HEAD_SHA:0:9}."
		;;
	*)
		echo "ERROR: EVENT must be labeled, synchronize, or push." >&2
		exit 2
		;;
esac
