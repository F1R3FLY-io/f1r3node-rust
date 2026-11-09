#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/tag-ca-release.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

SHA=1111111111111111111111111111111111111111
TAG_OBJECT=2222222222222222222222222222222222222222

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'FAKE'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_LOG"
filter=""
args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
	[ "${args[i]}" = --jq ] && filter="${args[i + 1]}"
done
emit() {
	if [ -n "$filter" ]; then jq -r "$filter" <<<"$1"; else printf '%s\n' "$1"; fi
}
case "$2" in
	*/git/ref/heads/ca-release) emit "{\"object\": {\"sha\": \"$FAKE_SHA\"}}" ;;
	*/git/ref/tags/*) [ "${FAKE_TAG_EXISTS:-0}" = 1 ] || exit 1; emit '{}' ;;
	*/commits/*/status) emit "${FAKE_STATUS:-{\"statuses\": []\}}" ;;
	*/git/tags) emit "{\"sha\": \"$FAKE_TAG_OBJECT\"}" ;;
	*/git/refs) emit '{}' ;;
	*) printf 'unexpected gh invocation: %s\n' "$*" >&2; exit 1 ;;
esac
FAKE
chmod +x "$TMP/bin/gh"

# run VERSION: sets status, out, and gh.log.
run() {
	: >"$TMP/gh.log"
	set +e
	out="$(PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" CA_TAG_REPOSITORY=example/repo FAKE_SHA="$SHA" \
		FAKE_TAG_OBJECT="$TAG_OBJECT" bash "$TOOL" "$1" 2>&1)"
	status=$?
	set -e
}

status_json() {
	jq -n --arg state "$1" --arg context "$2" '{statuses: [{context: $context, state: $state}]}'
}

for version in '' 0.5 v0.5.0 0.5.0-beta.1 '0.5.0; rm' 0.5.0-rc.; do
	run "$version"
	[ "$status" -eq 2 ] || fail "the version '$version' was accepted: $out"
	[[ ! -s "$TMP/gh.log" ]] || fail "the version '$version' still called gh: $(cat "$TMP/gh.log")"
done

run 0.5.0-rc.1
[ "$status" -eq 1 ] || fail "a commit without a soak gate status was tagged: $out"
[[ "$out" == *'no successful "Soak Gate (60h)" status (state: none)'* ]] || fail "a missing soak gate was not reported: $out"

FAKE_STATUS="$(status_json failure 'Soak Gate (60h)')" run 0.5.0-rc.1
[ "$status" -eq 1 ] || fail "a failed soak gate was tagged: $out"
[[ "$out" == *"(state: failure)"* ]] || fail "a failed soak gate was not reported: $out"

FAKE_STATUS="$(status_json success 'Soak Gate (22h)')" run 0.5.0-rc.1
[ "$status" -eq 1 ] || fail "a 22-hour soak gate counted as the 60-hour gate: $out"

export FAKE_STATUS
FAKE_STATUS="$(status_json success 'Soak Gate (60h)')"
FAKE_TAG_EXISTS=1 run 0.5.0-rc.1
[ "$status" -eq 1 ] || fail "an existing tag was moved: $out"
[[ "$out" == *"ca-v0.5.0-rc.1 already exists"* ]] || fail "an existing tag was not reported: $out"
if grep -q 'git/tags\|git/refs' "$TMP/gh.log"; then
	fail "an existing tag still created a tag object or a reference: $(cat "$TMP/gh.log")"
fi

run 0.5.0-rc.1
[ "$status" -eq 0 ] || fail "a soaked commit was not tagged: $out"
[[ "$out" == *"Created ca-v0.5.0-rc.1 at ca-release 111111111."* ]] || fail "the tag was not reported: $out"
grep -q "^api repos/example/repo/git/tags -f tag=ca-v0.5.0-rc.1 -f object=$SHA -f type=commit " "$TMP/gh.log" ||
	fail "the annotated tag object was not created at the ca-release head: $(cat "$TMP/gh.log")"
grep -Fxq "api repos/example/repo/git/refs -f ref=refs/tags/ca-v0.5.0-rc.1 -f sha=$TAG_OBJECT" "$TMP/gh.log" ||
	fail "the tag reference does not point to the tag object: $(cat "$TMP/gh.log")"

run 1.0.0
[ "$status" -eq 0 ] || fail "a release version without -rc was refused: $out"

printf 'PASS: just ca-tag accepts only MAJOR.MINOR.PATCH[-rc.N], tags the ca-release head as an annotated ca-v<version> tag only after a successful Soak Gate (60h) status, and never moves an existing tag.\n'
