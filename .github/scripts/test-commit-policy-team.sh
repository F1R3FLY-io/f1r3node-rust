#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/commit-policy-team.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
mkdir -p "$TMP/bin" "$TMP/repo" "$TMP/fake"
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = api ]
shift
if [ "$1" = --paginate ]; then shift; fi
endpoint="$1"
shift
printf '%s\n' "$endpoint" >>"$FAKE/calls"
if [ -n "${FAIL_ON:-}" ] && [[ "$endpoint" == *"$FAIL_ON"* ]]; then exit 19; fi
case "$endpoint" in
orgs/example/teams/ci-maintainers/members\?per_page=100) file=team.json ;;
repos/example/repo/actions/artifacts\?*) file=artifacts.json ;;
repos/example/repo/actions/runs/77) file=producer.json ;;
repos/example/repo/actions/artifacts/88/zip) cat "$FAKE/snapshot.zip"; exit ;;
repos/example/repo/pulls/20/commits\?per_page=100) file=commits.json ;;
repos/example/repo/commits/*) file="${endpoint##*/}.json" ;;
*) exit 18 ;;
esac
if [ "$#" -eq 0 ]; then cat "$FAKE/$file"; else
  [ "$1" = --jq ]; jq -r "$2" "$FAKE/$file"
fi
SH
printf '#!/usr/bin/env bash\nexit 0\n' >"$TMP/bin/sleep"
chmod +x "$TMP/bin/gh" "$TMP/bin/sleep"
export PATH="$TMP/bin:$PATH" FAKE="$TMP/fake" GITHUB_REPOSITORY=example/repo
export CI_RUN_ID=55 CI_RUN_ATTEMPT=2 PR_NUMBER=20
git -C "$TMP/repo" init -q
git -C "$TMP/repo" config user.name 'Base Author'
git -C "$TMP/repo" config user.email base@example.com
git -C "$TMP/repo" config core.hooksPath /dev/null
git -C "$TMP/repo" commit -q --allow-empty -m accepted
BASE="$(git -C "$TMP/repo" rev-parse HEAD)"
: >"$TMP/existing"
printf 'denied@example.com\n@blocked.example\n' >"$TMP/denied-list"

commit() {
	git -C "$TMP/repo" -c user.name=Contributor -c user.email="$2" commit -q --allow-empty -m "$3"
	git -C "$TMP/repo" rev-parse HEAD >"$TMP/$1"
}
commit member member@example.com member
commit fallback other-member@example.com beyond-the-PR-list-limit
commit outsider outsider@example.com outsider
commit unmapped unmapped@example.com unmapped
commit bot bot@example.com bot
commit removed removed@example.com removed
commit coauthor member@example.com $'member with unknown coauthor\n\nCo-authored-by: Unknown <unratified@example.com>'
commit denied denied@example.com denied
commit domain member@blocked.example domain-denied
commit trailer member@example.com $'member session\n\nClaude-Session: forbidden'

fixture() {
	printf '[{"login":"Maintainer","type":"User"}]\n[{"login":"Another","type":"User"}]\n' >"$FAKE/team.json"
	printf '{"artifacts":[{"name":"commit-policy-maintainers-55-2","id":88,"workflow_run":{"id":77},"expired":false}]}' >"$FAKE/artifacts.json"
	printf '{"repository":{"full_name":"example/repo"},"event":"workflow_run","path":".github/workflows/commit-policy-team.yml","head_branch":"master","status":"completed","conclusion":"success"}' >"$FAKE/producer.json"
	jq -n --arg run "$CI_RUN_ID" --arg attempt "$CI_RUN_ATTEMPT" \
		'{schema:"commit-policy-team/v1", repository:"example/repo", run_id:$run, run_attempt:$attempt,
		team:"ci-maintainers", members:["maintainer","another"]}' >"$FAKE/maintainers.json"
	: >"$FAKE/rows"
	for name in member outsider unmapped bot removed coauthor denied domain trailer; do
		login=Maintainer kind=User
		case "$name" in outsider) login=outsider ;; unmapped) login=-; kind=- ;; bot) kind=Bot ;; removed) login=former ;;
		esac
		jq -n --arg sha "$(cat "$TMP/$name")" --arg login "$login" --arg kind "$kind" \
			'{sha:$sha, author:(if $login == "-" then null else {login:$login,type:$kind} end)}' >>"$FAKE/rows"
	done
	jq -s . "$FAKE/rows" >"$FAKE/commits.json"
	jq -n --arg sha "$(cat "$TMP/fallback")" '{sha:$sha,author:{login:"another",type:"User"}}' >"$FAKE/$(cat "$TMP/fallback").json"
	pack
	: >"$FAKE/calls"
}

pack() {
	(cd "$FAKE" && rm -f snapshot.zip && zip -q snapshot.zip maintainers.json)
}
run() {
	set +e
	(cd "$TMP/repo" && env "$@" bash "$TOOL" exempt "$BASE..HEAD" "$TMP/existing" "$BASE") >"$TMP/out" 2>"$TMP/err"
	result=$?
	set -e
}

fixture
bash "$TOOL" snapshot "$TMP/snapshot.json"
jq -e '.members == ["another","maintainer"] and .run_id == "55" and .run_attempt == "2"' "$TMP/snapshot.json" >/dev/null || fail 'paginated team members were not recorded'
run
[ "$result" -eq 0 ] || { cat "$TMP/err"; fail 'member lookup failed'; }
for name in member fallback coauthor denied domain trailer; do
	grep -Fxq "$(cat "$TMP/$name")" "$TMP/out" || fail "missing member commit: $name"
done
for name in outsider unmapped bot removed; do
	if grep -Fxq "$(cat "$TMP/$name")" "$TMP/out"; then fail "accepted nonmember commit: $name"; fi
done
grep -Fq "commits/$(cat "$TMP/fallback")" "$FAKE/calls" || fail 'a commit absent from the PR listing was not resolved'

for name in member fallback coauthor denied domain trailer; do
	sha="$(cat "$TMP/$name")"
	set +e
	bash "$ROOT/.github/scripts/check-commit-trailers.sh" range "$sha^..$sha" --repo "$TMP/repo" \
		--ratified-from "$BASE" --exempt-authors "$TMP/out" --denied "$TMP/denied-list" >"$TMP/check" 2>&1
	result=$?
	set -e
	case "$name" in
	member|fallback) [ "$result" -eq 0 ] || fail "a team author failed: $name" ;;
	*) [ "$result" -eq 1 ] || fail "team membership bypassed existing policy: $name" ;;
	esac
done

for mutation in '.run_attempt="1"' '.run_id="54"' '.repository="other/repo"' '.team="other-team"' '.members=["*"]'; do
	fixture
	jq "$mutation" "$FAKE/maintainers.json" >"$FAKE/changed"
	mv "$FAKE/changed" "$FAKE/maintainers.json"
	pack
	run
	[ "$result" -ne 0 ] || fail "accepted mismatched snapshot: $mutation"
done
for mutation in '.event="pull_request"' '.path=".github/workflows/ci.yml"' '.head_branch="feature/untrusted"' '.conclusion="failure"' '.repository.full_name="other/repo"'; do
	fixture
	jq "$mutation" "$FAKE/producer.json" >"$FAKE/changed"
	mv "$FAKE/changed" "$FAKE/producer.json"
	run
	[ "$result" -ne 0 ] || fail "accepted untrusted publisher: $mutation"
done
fixture
printf '{"artifacts":[]}' >"$FAKE/artifacts.json"
run
[ "$result" -eq 2 ] || fail 'missing snapshot passed'
for endpoint in actions/artifacts pulls/20/commits "commits/$(cat "$TMP/fallback")"; do
	fixture
	run FAIL_ON="$endpoint"
	[ "$result" -ne 0 ] || fail "API error passed: $endpoint"
done
fixture
set +e
FAIL_ON=teams/ci-maintainers bash "$TOOL" snapshot "$TMP/snapshot.json" >"$TMP/out" 2>"$TMP/err"
result=$?
set -e
[ "$result" -ne 0 ] || fail 'membership API error passed'
fixture
jq '.sha="0000000000000000000000000000000000000000"' "$FAKE/$(cat "$TMP/fallback").json" >"$FAKE/changed"
mv "$FAKE/changed" "$FAKE/$(cat "$TMP/fallback").json"
run
[ "$result" -eq 2 ] || fail 'wrong commit attribution passed'
fixture
git -C "$TMP/repo" rev-list "$BASE..HEAD" >"$TMP/existing"
run FAIL_ON=actions/artifacts
[ "$result" -eq 0 ] || fail 'existing exemptions acquired a team snapshot dependency'

printf 'PASS: team author exemptions preserve identity policy and require a trusted run-bound membership snapshot.\n'
