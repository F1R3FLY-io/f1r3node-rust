#!/usr/bin/env bash
set -euo pipefail

# Test net slot pins (docs/plans/test-net.md sections 3 and 4).
#
# A slot file pins the image digest of each validator slot in one test net
# shard: three Anchors and an optional soaking slot. The file holds digests
# only. The registry repository comes from TESTNET_IMAGE_REPOSITORY at
# deploy time, because the OCIR repository path is not public.
#
# The file is parsed, never sourced, so a value cannot run a command.

CONSENSUS_MODELS=(cbc-casper)
KEYS=(CONSENSUS_MODEL ANCHOR1_DIGEST ANCHOR2_DIGEST ANCHOR3_DIGEST SOAK_DIGEST SOAK_CANDIDATE_TAG)

fail() {
	printf 'testnet slots error: %s\n' "$1" >&2
	exit 1
}

is_digest() {
	[[ "$1" =~ ^sha256:[0-9a-f]{64}$ ]]
}

check() {
	local file="$1" line key value model soak soak_tag
	local -A slot=()
	[ -f "$file" ] || fail "missing slot file: $file"
	while IFS= read -r line || [ -n "$line" ]; do
		[[ "$line" =~ ^[[:space:]]*(#.*)?$ ]] && continue
		[[ "$line" =~ ^([A-Z0-9_]+)=([A-Za-z0-9:._-]*)$ ]] || fail "line is not KEY=VALUE with a plain value: $line"
		key="${BASH_REMATCH[1]}"
		value="${BASH_REMATCH[2]}"
		[[ " ${KEYS[*]} " == *" $key "* ]] || fail "unknown key: $key"
		[ -z "${slot[$key]+set}" ] || fail "duplicate key: $key"
		slot[$key]="$value"
	done <"$file"
	model="${slot[CONSENSUS_MODEL]:-}"
	[[ " ${CONSENSUS_MODELS[*]} " == *" $model "* ]] || fail "consensus model '$model' has no test net"
	for key in ANCHOR1_DIGEST ANCHOR2_DIGEST ANCHOR3_DIGEST; do
		is_digest "${slot[$key]:-}" || fail "$key must be an image digest (sha256:...)"
	done
	soak="${slot[SOAK_DIGEST]:-}"
	soak_tag="${slot[SOAK_CANDIDATE_TAG]:-}"
	if [ -n "$soak" ]; then
		is_digest "$soak" || fail "SOAK_DIGEST must be an image digest (sha256:...)"
		[[ "$soak_tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-canary\.[1-9][0-9]*$ ]] ||
			fail "SOAK_CANDIDATE_TAG must name the canary of the soaking digest"
		for key in ANCHOR1_DIGEST ANCHOR2_DIGEST ANCHOR3_DIGEST; do
			[ "$soak" != "${slot[$key]}" ] || fail "the soaking slot runs the release of $key"
		done
	else
		[ -z "$soak_tag" ] || fail "SOAK_CANDIDATE_TAG is set without SOAK_DIGEST"
	fi
	jq -n \
		--arg model "$model" \
		--arg a1 "${slot[ANCHOR1_DIGEST]}" \
		--arg a2 "${slot[ANCHOR2_DIGEST]}" \
		--arg a3 "${slot[ANCHOR3_DIGEST]}" \
		--arg soak "$soak" \
		--arg soak_tag "$soak_tag" '
		{
			consensus_model: $model,
			anchors: [$a1, $a2, $a3],
			anchor_releases: ([$a1, $a2, $a3] | unique | length),
			soak: (if $soak == "" then null else {digest: $soak, candidate_tag: $soak_tag} end)
		}'
}

usage() {
	printf '%s\n' "usage: $0 check SLOTS_FILE" >&2
	exit 2
}

command="${1:-}"
case "$command" in
check)
	[ "$#" -eq 2 ] || usage
	check "$2"
	;;
*) usage ;;
esac
