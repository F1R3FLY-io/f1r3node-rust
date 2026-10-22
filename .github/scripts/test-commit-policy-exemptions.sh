#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/commit-policy-exemptions.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

A=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
B=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
C=cccccccccccccccccccccccccccccccccccccccc
D=dddddddddddddddddddddddddddddddddddddddd
E=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee

mkdir -p "$TMP/bin"
# The fake reads each answer from a file named for the API path, and applies
# the --jq filter as gh does. FAIL_ON makes a matching call fail.
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
filter=""
path=""
args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
	case "${args[i]}" in
		--jq) filter="${args[i + 1]}"; i=$((i + 1)) ;;
		repos/*) path="${args[i]}" ;;
	esac
done
[ -n "${FAIL_ON:-}" ] && [[ "$path" == *"$FAIL_ON"* ]] && { echo "simulated failure: $path" >&2; exit 1; }
file="$FAKE_DIR/$(printf '%s' "${path%%\?*}" | tr '/' '_').json"
[ -f "$file" ] || { echo "unexpected gh call: $path" >&2; exit 1; }
jq -r "$filter" "$file"
SH
chmod +x "$TMP/bin/gh"

# commits FILE SHA:LOGIN...: writes a commits API answer.
commits() {
	local file="$1"
	shift
	printf '%s\n' "$@" | jq -R 'split(":") | {sha: .[0], author: (if .[1] == "" then null else {login: .[1]} end)}' | jq -s . >"$file"
}
# pull FILE BODY HEAD_REPO USER TYPE: writes a pull request API answer.
pull() {
	jq -n --arg body "$2" --arg head "$3" --arg user "$4" --arg type "$5" '{
		body: $body,
		head: {repo: (if $head == "" then null else {full_name: $head} end)},
		base: {repo: {full_name: "example/repo"}},
		user: {login: $user, type: $type}
	}' >"$1"
}

setup() {
	rm -rf "$TMP/fake"
	mkdir -p "$TMP/fake"
	F="$TMP/fake"
	commits "$F/repos_example_repo_pulls_20_commits.json" "$A:contrib" "$B:other" "$C:maint" "$D:"
	pull "$F/repos_example_repo_pulls_20.json" "${BODY:-Imports fork pull request #10.}" example/repo maint User
	commits "$F/repos_example_repo_pulls_10_commits.json" "$A:contrib" "$B:other" "$E:contrib"
	pull "$F/repos_example_repo_pulls_10.json" "A change" "${SOURCE_HEAD-contrib/repo}" contrib "${SOURCE_TYPE:-User}"
}

# run [NAME=VALUE...]: sets status, out (stdout), and err.
run() {
	set +e
	out="$(env PATH="$TMP/bin:$PATH" FAKE_DIR="$TMP/fake" GITHUB_REPOSITORY=example/repo PR_NUMBER=20 \
		PR_AUTHOR=maint PR_AUTHOR_TYPE=User "$@" bash "$TOOL" 2>"$TMP/err")"
	status=$?
	set -e
	err="$(cat "$TMP/err")"
}
expect() {
	local label="$1" want="$2"
	[ "$status" -eq 0 ] || fail "$label failed: $err"
	[ "$out" = "$want" ] || fail "$label: expected [$want], got [$out] ($err)"
}

setup
run
expect "an import" "$(printf '%s\n' "$A" "$C")"
[[ "$err" == *"Imported 1 commits of contrib from fork pull request #10."* ]] || fail "the import is not reported: $err"

BODY='No import.' setup
run
expect "a pull request without an import" "$C"

BODY=$'Summary\r\nImports fork pull request #10\r\nMore text' setup
run
expect "an import line with CRLF and no period" "$(printf '%s\n' "$A" "$C")"

BODY='Do not merge yet. Imports fork pull request #10.' setup
run
expect "an import phrase inside a line" "$C"

setup
run PR_AUTHOR_TYPE=Bot
expect "a bot author with an import" "$A"
[[ "$err" == *"no author exemption"* ]] || fail "the bot author is not reported: $err"

SOURCE_HEAD=example/repo setup
run
expect "a source from this repository" "$C"
[[ "$err" == *"not a pull request into example/repo from a fork"* ]] || fail "a same-repository source is not reported: $err"

SOURCE_HEAD='' setup
run
expect "a source whose fork was deleted" "$C"

SOURCE_TYPE=Bot setup
run
expect "a source opened by a bot" "$C"

BODY='Imports fork pull request #20.' setup
pull "$TMP/fake/repos_example_repo_pulls_20.json" 'Imports fork pull request #20.' contrib/repo other User
run
expect "a pull request that imports itself" "$C"

BODY=$'Imports fork pull request #10.\nImports fork pull request #11.' setup
run
[ "$status" -eq 2 ] || fail "two imports passed: $out $err"

setup
run FAIL_ON=pulls/20/commits
[ "$status" -ne 0 ] || fail "a failed commits listing passed: $out"
setup
run FAIL_ON=pulls/10
[ "$status" -ne 0 ] || fail "a failed source read passed: $out"

for bad in PR_NUMBER=x GITHUB_REPOSITORY=bad; do
	setup
	run "$bad"
	[ "$status" -eq 2 ] || fail "the tool accepted $bad: $out $err"
done

# shellcheck disable=SC2016 # Ruby reads the literal text $BASE_SHA and $exempt in the workflow step.
ruby -ryaml -e '
	step = YAML.load_file(ARGV[0]).dig("jobs", "commit_policy", "steps").find { |s| s["name"] == "Verify commit identities in pull request commits" }
	abort "the Commit Policy step is missing" unless step
	run = step["run"].to_s
	abort "Commit Policy does not run commit-policy-exemptions.sh from the base commit" unless
		run.include?("${BASE_SHA}:.github/scripts/commit-policy-exemptions.sh") && run.include?("--exempt-authors \"$exempt\"")
' "$ROOT/.github/workflows/ci.yml" || fail "ci.yml wiring"

printf 'PASS: Commit Policy exempts the commits that GitHub attributes to a human pull request author. A maintainer import of a fork pull request also exempts the source author commits that both pull requests list. A source from this repository, a deleted fork, a bot source, a self import, and an inline phrase exempt nothing more. Two imports, a failed commits listing, and a failed source read fail. Commit Policy runs the base copy of the script.\n'
