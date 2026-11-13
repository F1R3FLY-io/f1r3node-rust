#!/usr/bin/env bash
set -euo pipefail

fail() { printf 'Commit Policy team check: %s\n' "$*" >&2; exit 2; }
[[ "${GITHUB_REPOSITORY:-}" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || fail 'invalid repository'
[[ "${CI_RUN_ID:-}" =~ ^[1-9][0-9]*$ ]] || fail 'invalid CI run'
[[ "${CI_RUN_ATTEMPT:-}" =~ ^[1-9][0-9]*$ ]] || fail 'invalid CI attempt'
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

snapshot() {
	local destination="$1"
	gh api --paginate "orgs/${GITHUB_REPOSITORY%%/*}/teams/ci-maintainers/members?per_page=100" \
		--jq '.[] | select(.type == "User") | .login | ascii_downcase' >"$WORK/members"
	jq -Rn --arg repository "$GITHUB_REPOSITORY" --arg run "$CI_RUN_ID" --arg attempt "$CI_RUN_ATTEMPT" \
		'{schema: "commit-policy-team/v1", repository: $repository, run_id: $run, run_attempt: $attempt,
		team: "ci-maintainers", members: ([inputs] | unique)}' <"$WORK/members" >"$destination"
	validate "$destination"
}

validate() {
	jq -e -s --arg repository "$GITHUB_REPOSITORY" --arg run "$CI_RUN_ID" --arg attempt "$CI_RUN_ATTEMPT" '
		length == 1 and (.[0] |
		.schema == "commit-policy-team/v1" and .repository == $repository and
		.run_id == $run and .run_attempt == $attempt and .team == "ci-maintainers" and
		(.members | type == "array") and
		all(.members[]; type == "string" and test("^[a-z0-9]([a-z0-9-]*[a-z0-9])?$")))
	' "$1" >/dev/null 2>&1 || fail 'invalid or mismatched membership snapshot'
}

read_snapshot() {
	local attempt artifact publisher producer observed name="commit-policy-maintainers-$CI_RUN_ID-$CI_RUN_ATTEMPT"
	for ((attempt = 0; attempt < 30; attempt++)); do
		gh api --paginate "repos/$GITHUB_REPOSITORY/actions/artifacts?name=$name&per_page=100" \
			--jq '.artifacts[] | select(.expired == false) | [.name, .id, .workflow_run.id] | @tsv' >"$WORK/artifacts"
		while IFS=$'\t' read -r observed artifact publisher; do
			[ "$observed" = "$name" ] || continue
			[[ "$artifact" =~ ^[1-9][0-9]*$ ]] && [[ "$publisher" =~ ^[1-9][0-9]*$ ]] || fail 'invalid artifact identity'
			producer="$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/$publisher")"
			if ! jq -e --arg repository "$GITHUB_REPOSITORY" '
				.repository.full_name == $repository and .event == "workflow_run" and
				.path == ".github/workflows/commit-policy-team.yml" and .head_branch == "master" and
				.status == "completed" and .conclusion == "success"
			' <<<"$producer" >/dev/null; then
				continue
			fi
			gh api "repos/$GITHUB_REPOSITORY/actions/artifacts/$artifact/zip" >"$WORK/snapshot.zip"
			[ "$(unzip -Z1 "$WORK/snapshot.zip")" = maintainers.json ] || fail 'unexpected membership artifact contents'
			unzip -p "$WORK/snapshot.zip" maintainers.json >"$WORK/maintainers.json"
			validate "$WORK/maintainers.json"
			jq -r '.members[]' "$WORK/maintainers.json" >"$WORK/members"
			return 0
		done <"$WORK/artifacts"
		sleep 10
	done
	fail 'trusted team snapshot unavailable; check Commit Policy Team on master and its membership-reader credential'
}

exempt() {
	local range="$1" existing="$2" sha email attribution actual login kind
	shift 2
	[ "$#" -gt 0 ] && [ -f "$existing" ] || fail 'missing accepted revisions or exemptions'
	[[ "${PR_NUMBER:-}" =~ ^[1-9][0-9]*$ ]] || fail 'invalid pull request'
	git log --format=%ae "$@" -- | tr '[:upper:]' '[:lower:]' | sort -u >"$WORK/ratified"
	git rev-list "$range" >"$WORK/commits"
	: >"$WORK/candidates"
	while IFS= read -r sha; do
		grep -Fxq -- "$sha" "$existing" && continue
		email="$(git log -1 --format=%ae "$sha" | tr '[:upper:]' '[:lower:]')"
		grep -Fxq -- "$email" "$WORK/ratified" || printf '%s\n' "$sha" >>"$WORK/candidates"
	done <"$WORK/commits"
	[ -s "$WORK/candidates" ] || return 0
	read_snapshot
	gh api --paginate "repos/$GITHUB_REPOSITORY/pulls/$PR_NUMBER/commits?per_page=100" \
		--jq '.[] | [.sha, (.author.login // "-"), (.author.type // "-")] | @tsv' >"$WORK/attribution"
	while IFS= read -r sha; do
		attribution="$(awk -F '\t' -v sha="$sha" '$1 == sha { print; exit }' "$WORK/attribution")"
		if [ -z "$attribution" ]; then
			attribution="$(gh api "repos/$GITHUB_REPOSITORY/commits/$sha" \
				--jq '[.sha, (.author.login // "-"), (.author.type // "-")] | @tsv')"
		fi
		IFS=$'\t' read -r actual login kind <<<"$attribution"
		[ "$actual" = "$sha" ] || fail 'commit attribution does not match the requested commit'
		login="$(printf '%s' "$login" | tr '[:upper:]' '[:lower:]')"
		if [ "$kind" = User ] && grep -Fxq -- "$login" "$WORK/members"; then
			printf '%s\n' "$sha"
		fi
	done <"$WORK/candidates"
}

case "${1:-}" in
snapshot) [ "$#" -eq 2 ] || fail 'snapshot requires an output file'; snapshot "$2" ;;
exempt) shift; [ "$#" -ge 3 ] || fail 'exempt requires a range, exemptions, and accepted revisions'; exempt "$@" ;;
*) fail 'expected snapshot or exempt' ;;
esac
