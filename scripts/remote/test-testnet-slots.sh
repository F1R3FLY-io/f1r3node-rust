#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/scripts/remote/testnet-slots.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

digest() { printf 'sha256:%s' "$(printf '%s' "$1" | sha256sum | awk '{print $1}')"; }
A="$(digest release-a)"
B="$(digest release-b)"
C="$(digest release-c)"
D="$(digest candidate-d)"

slots() {
	printf '%s\n' \
		'# CBC Casper test net, shard 1' \
		'CONSENSUS_MODEL=cbc-casper' \
		"ANCHOR1_DIGEST=$A" \
		"ANCHOR2_DIGEST=$B" \
		"ANCHOR3_DIGEST=$C" \
		'' \
		"SOAK_DIGEST=${1-$D}" \
		"SOAK_CANDIDATE_TAG=${2-v0.4.47-canary.2801}"
}

expect_failure() {
	local label="$1"
	shift
	if "$@" >"$TMP/out" 2>"$TMP/err"; then
		printf 'expected failure: %s\n' "$label" >&2
		exit 1
	fi
}

slots >"$TMP/slots.env"
"$TOOL" check "$TMP/slots.env" >"$TMP/report.json"
jq -e --arg d "$D" '
	.consensus_model == "cbc-casper"
	and (.anchors | length) == 3
	and .anchor_releases == 3
	and .soak.digest == $d
	and .soak.candidate_tag == "v0.4.47-canary.2801"' "$TMP/report.json" >/dev/null

slots '' '' >"$TMP/no-soak.env"
"$TOOL" check "$TMP/no-soak.env" | jq -e '.soak == null' >/dev/null

slots | sed "s/^ANCHOR2_DIGEST=.*/ANCHOR2_DIGEST=$A/" >"$TMP/shared-release.env"
"$TOOL" check "$TMP/shared-release.env" | jq -e '.anchor_releases == 2' >/dev/null

slots | sed 's/^CONSENSUS_MODEL=.*/CONSENSUS_MODEL=other-model/' >"$TMP/model.env"
expect_failure 'another consensus model' "$TOOL" check "$TMP/model.env"
slots | sed 's/^ANCHOR3_DIGEST=.*/ANCHOR3_DIGEST=latest/' >"$TMP/tag.env"
expect_failure 'an anchor pinned by tag' "$TOOL" check "$TMP/tag.env"
slots | grep -v '^ANCHOR3_DIGEST=' >"$TMP/missing.env"
expect_failure 'a missing anchor' "$TOOL" check "$TMP/missing.env"
slots "$B" >"$TMP/soak-is-anchor.env"
expect_failure 'a soaking slot on an Anchor release' "$TOOL" check "$TMP/soak-is-anchor.env"
slots "$D" '' >"$TMP/soak-no-tag.env"
expect_failure 'a soaking digest without its canary tag' "$TOOL" check "$TMP/soak-no-tag.env"
slots "$D" 'v0.4.47' >"$TMP/soak-stable-tag.env"
expect_failure 'a soaking slot on a stable tag' "$TOOL" check "$TMP/soak-stable-tag.env"
slots '' 'v0.4.47-canary.2801' >"$TMP/tag-no-digest.env"
expect_failure 'a canary tag without a soaking digest' "$TOOL" check "$TMP/tag-no-digest.env"
{ slots; printf 'TESTNET_IMAGE_REPOSITORY=registry.example/repo\n'; } >"$TMP/unknown.env"
expect_failure 'an unknown key' "$TOOL" check "$TMP/unknown.env"
{ slots; printf 'ANCHOR1_DIGEST=%s\n' "$C"; } >"$TMP/duplicate.env"
expect_failure 'a duplicate key' "$TOOL" check "$TMP/duplicate.env"
printf 'ANCHOR1_DIGEST=$(touch %s/pwned)\n' "$TMP" >"$TMP/command.env"
expect_failure 'a command substitution' "$TOOL" check "$TMP/command.env"
[ ! -e "$TMP/pwned" ] || { printf 'the slot file was executed\n' >&2; exit 1; }

printf 'testnet slot tests passed\n'
