#!/usr/bin/env bash
# Reports the release train soak gate of a merge-recovery-soak.yml run as a
# commit status on the soaked commit (issue #791).
#
#   gate ca-item     context "Soak Gate (22h)" on the head of a ca/ pull request
#   gate ca-release  context "Soak Gate (60h)" on the ca-release commit
#
#   report-soak-gate.sh pending  posts pending when the run starts
#   report-soak-gate.sh final    posts the result when the run ends
#
# The result is success only when the soak job succeeded, reached its
# completion marker, passed its integration preflight, and the soak verdict is
# pass. Anything else is failure, and a cancelled soak is error.
#
# A ca-item success also adds the ca-soaked label, which the ca-item queue of
# .mergify.yml requires. The label is added only when the pull request is open
# and its head is still the soaked commit. A label stays when the head changes,
# so a label for an older head would pass code that no soak ran.
#
# Runs of the same commit can overlap, for example after a second ca-soak
# label. The status names its run, and a run never replaces the status of a
# newer run.
set -euo pipefail

mode="${1:-}"
case "$mode" in
	pending | final) ;;
	*) echo "ERROR: give the mode pending or final." >&2; exit 2 ;;
esac

case "${GATE:-}" in
	ca-item) context="Soak Gate (22h)" ;;
	ca-release) context="Soak Gate (60h)" ;;
	*) echo "ERROR: GATE must be ca-item or ca-release." >&2; exit 2 ;;
esac
[[ "${TARGET_SHA:-}" =~ ^[0-9a-f]{40}$ ]] || { echo "ERROR: TARGET_SHA is not a commit SHA." >&2; exit 2; }
if [ "$GATE" = ca-item ]; then
	[[ "${GATE_PR:-}" =~ ^[1-9][0-9]*$ ]] || { echo "ERROR: GATE_PR is not a pull request number." >&2; exit 2; }
fi
[[ "${GH_REPO:-}" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || { echo "ERROR: GH_REPO is not owner/name." >&2; exit 2; }
run_prefix="${GITHUB_SERVER_URL:-https://github.com}/$GH_REPO/actions/runs/"
[[ "${RUN_URL:-}" == "$run_prefix"* ]] || { echo "ERROR: RUN_URL is not a run of $GH_REPO." >&2; exit 2; }

# run_id URL: the run ID in a status target URL, or nothing.
run_id() {
	local tail="${1#"$run_prefix"}"
	[[ "$tail" != "$1" && "$tail" =~ ^([0-9]+)(/|$) ]] && printf '%s\n' "${BASH_REMATCH[1]}"
	return 0
}
own_run="$(run_id "$RUN_URL")"
[ -n "$own_run" ] || { echo "ERROR: RUN_URL names no run." >&2; exit 2; }

if [ "$mode" = pending ]; then
	state=pending
	description="The soak is running."
else
	if [ "${SOAK_RESULT:-}" = cancelled ] || [ "${LAUNCH_RESULT:-}" = cancelled ]; then
		state=error
		description="The soak was cancelled."
	elif [ "${SOAK_RESULT:-}" = success ] && [ "${COMPLETED:-}" = true ] &&
		[ "${PREFLIGHT:-}" = passed ] && [ "${VERDICT:-}" = pass ]; then
		state=success
		description="The soak passed."
	else
		state=failure
		description="The soak did not pass (soak ${SOAK_RESULT:-none}, preflight ${PREFLIGHT:-none}, verdict ${VERDICT:-none})."
	fi
fi

current_url="$(gh api "repos/$GH_REPO/commits/$TARGET_SHA/status" \
	--jq "[.statuses[] | select(.context == \"$context\")][0].target_url // \"\"")"
current_run="$(run_id "$current_url")"
if [ -n "$current_run" ] && { [ "${#current_run}" -gt "${#own_run}" ] ||
	{ [ "${#current_run}" -eq "${#own_run}" ] && [[ "$current_run" > "$own_run" ]]; }; }; then
	echo "Run $current_run is newer and owns \"$context\" on ${TARGET_SHA:0:9}. Nothing to report."
	exit 0
fi

gh api --method POST "repos/$GH_REPO/statuses/$TARGET_SHA" -f state="$state" -f context="$context" \
	-f description="$description" -f target_url="$RUN_URL" >/dev/null
echo "Posted \"$context\" $state on ${TARGET_SHA:0:9}."

[ "$state" = success ] && [ "$GATE" = ca-item ] || exit 0
pr="$(gh api "repos/$GH_REPO/pulls/$GATE_PR" --jq '[.state, .head.sha, .base.ref] | @tsv')"
IFS=$'\t' read -r pr_state pr_head pr_base <<<"$pr"
if [ "$pr_state" != open ] || [ "$pr_base" != ca-canary ] || [ "$pr_head" != "$TARGET_SHA" ]; then
	echo "Pull request #$GATE_PR is $pr_state into $pr_base at ${pr_head:0:9}, not open into ca-canary at the soaked ${TARGET_SHA:0:9}. ca-soaked is not added."
	exit 0
fi
gh api --method POST "repos/$GH_REPO/issues/$GATE_PR/labels" -f "labels[]=ca-soaked" >/dev/null
echo "Added ca-soaked to pull request #$GATE_PR."
