#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/commit-policy-source.sh"
CHECKER="$ROOT/.github/scripts/check-commit-trailers.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[ "${FAIL_API:-0}" = 0 ] || exit 1
case "$2" in
    repos/example/repo/pulls/20) jq -r "$4" "$FIXTURE_DIR/pull.json" ;;
    repos/example/repo/branches/*)
        [ "${FAIL_SOURCE:-0}" = 0 ] || exit 1
        [ "${2##*/}" = "$EXPECTED_SOURCE" ] || exit 1
        jq -r "$4" "$FIXTURE_DIR/source.json"
        ;;
    *) exit 1 ;;
esac
SH
chmod +x "$TMP/bin/gh"

git init -q --bare "$TMP/origin.git"
git -C "$TMP/origin.git" config uploadpack.allowFilter true
git init -q "$TMP/repo"
git -C "$TMP/repo" config user.name 'Policy Test'
git -C "$TMP/repo" config user.email 'base@example.com'
git -C "$TMP/repo" -c core.hooksPath=/dev/null commit -q --allow-empty -m base
BASE="$(git -C "$TMP/repo" rev-parse HEAD)"
GIT_AUTHOR_NAME='Sync Bot' GIT_AUTHOR_EMAIL='sync-bot@example.com' \
	git -C "$TMP/repo" -c core.hooksPath=/dev/null commit -q --allow-empty -m accepted
ACCEPTED="$(git -C "$TMP/repo" rev-parse HEAD)"
GIT_AUTHOR_NAME='Later Author' GIT_AUTHOR_EMAIL='later@example.com' \
	git -C "$TMP/repo" -c core.hooksPath=/dev/null commit -q --allow-empty -m later
TIP="$(git -C "$TMP/repo" rev-parse HEAD)"
git -C "$TMP/repo" remote add origin "$TMP/origin.git"
git -C "$TMP/repo" -c core.hooksPath=/dev/null push -q origin HEAD:refs/heads/dev
GIT_AUTHOR_NAME='Unknown Author' GIT_AUTHOR_EMAIL='unknown@example.com' \
	git -C "$TMP/repo" -c core.hooksPath=/dev/null commit -q --allow-empty -m unaccepted
UNACCEPTED="$(git -C "$TMP/repo" rev-parse HEAD)"

fixture() {
	jq -n --arg base "${1:-ca-canary}" --arg head "${2:-ca/sync-dev-snapshot}" \
		--arg sha "${3:-$ACCEPTED}" --arg repo "${HEAD_REPO:-example/repo}" \
		'{base: {repo: {full_name: "example/repo"}, ref: $base}, head: {repo: {full_name: $repo}, ref: $head, sha: $sha}}' >"$TMP/pull.json"
	jq -n --arg sha "$TIP" --argjson protected "${PROTECTED:-true}" \
		'{protected: $protected, commit: {sha: $sha}}' >"$TMP/source.json"
}

run() {
	set +e
	out="$(cd "$TMP/repo" && env PATH="$TMP/bin:$PATH" FIXTURE_DIR="$TMP" \
		GITHUB_REPOSITORY=example/repo PR_NUMBER=20 HEAD_SHA="$ACCEPTED" \
		EXPECTED_SOURCE=dev "$@" bash "$TOOL" 2>"$TMP/err")"
	status=$?
	set -e
}

fixture
run
[ "$status" -eq 0 ] && [ "$out" = "$ACCEPTED" ] || fail "the accepted sync snapshot was not trusted"
if bash "$CHECKER" range "$BASE..$ACCEPTED" --repo "$TMP/repo" --ratified-from "$BASE" >"$TMP/out" 2>&1; then
	fail "target history alone ratified the sync author"
fi
bash "$CHECKER" range "$BASE..$ACCEPTED" --repo "$TMP/repo" --ratified-from "$BASE" --ratified-from "$out"

for route in 'master promote/release dev' 'ca-release promote/ca-release-release ca-canary' \
	'dev backmerge/master-snapshot master' 'ca-canary backmerge/ca-release-snapshot ca-release'; do
	read -r target head source_branch <<<"$route"
	fixture "$target" "$head"
	run EXPECTED_SOURCE="$source_branch"
	[ "$status" -eq 0 ] && [ "$out" = "$ACCEPTED" ] || fail "approved route failed: $route"
done

git -C "$TMP/repo" checkout -q --detach "$BASE"
git -C "$TMP/repo" -c core.hooksPath=/dev/null commit -q --allow-empty -m target-update
TARGET="$(git -C "$TMP/repo" rev-parse HEAD)"
git -C "$TMP/repo" checkout -q --detach "$ACCEPTED"
git -C "$TMP/repo" -c core.hooksPath=/dev/null merge -q --no-ff --no-edit "$TARGET"
UPDATED="$(git -C "$TMP/repo" rev-parse HEAD)"
fixture ca-canary ca/sync-dev-snapshot "$UPDATED"
run HEAD_SHA="$UPDATED"
[ "$status" -eq 0 ] && [ "$out" = "$ACCEPTED" ] || fail "a clean base update lost source trust"
bash "$CHECKER" range "$TARGET..$UPDATED" --repo "$TMP/repo" --ratified-from "$TARGET" --ratified-from "$out"

fixture dev fix/feature
run
[ "$status" -eq 0 ] && [ -z "$out" ] || fail "a feature PR gained source trust"
fixture ca-release ca/sync-dev-snapshot
run
[ "$status" -eq 0 ] && [ -z "$out" ] || fail "an unsupported route gained source trust"
HEAD_REPO=contributor/repo fixture
run
[ "$status" -eq 0 ] && [ -z "$out" ] || fail "a fork gained source trust"
PROTECTED=false fixture
run
[ "$status" -ne 0 ] || fail "an unprotected source passed"
fixture ca-canary ca/sync-dev-snapshot "$UNACCEPTED"
run HEAD_SHA="$UNACCEPTED"
[ "$status" -eq 0 ] && [ "$out" = "$TIP" ] || fail "source trust included a PR-only commit"
if bash "$CHECKER" range "$BASE..$UNACCEPTED" --repo "$TMP/repo" --ratified-from "$BASE" \
	--ratified-from "$out" >"$TMP/out" 2>&1; then
	fail "an author absent from the source passed"
fi
fixture
run HEAD_SHA="$UNACCEPTED"
[ "$status" -ne 0 ] || fail "a changed PR head passed"
run FAIL_API=1
[ "$status" -ne 0 ] || fail "an API failure passed"
run FAIL_SOURCE=1
[ "$status" -ne 0 ] || fail "a failed source read passed"

for invalid in HEAD_SHA=invalid PR_NUMBER=invalid GITHUB_REPOSITORY=invalid; do
	run "$invalid"
	[ "$status" -ne 0 ] || fail "invalid metadata passed: $invalid"
done

printf '%s\n' '@example.com' >"$TMP/denied.txt"
if bash "$CHECKER" range "$BASE..$ACCEPTED" --repo "$TMP/repo" --ratified-from "$BASE" \
	--ratified-from "$ACCEPTED" --denied "$TMP/denied.txt" >"$TMP/out" 2>&1; then
	fail "source ratification bypassed denied identities"
fi

if bash "$CHECKER" range "$ACCEPTED..$TIP" --repo "$TMP/repo" --ratified-from "$BASE" \
	--ratified-from "$ACCEPTED" >"$TMP/out" 2>&1; then
	fail "a later source author was ratified by an earlier snapshot"
fi

git -C "$TMP/repo" -c core.hooksPath=/dev/null commit -q --allow-empty \
	-m $'accepted coauthor\n\nCo-authored-by: Sync Bot <sync-bot@example.com>'
bash "$CHECKER" range 'HEAD~1..HEAD' --repo "$TMP/repo" --ratified-from "$BASE" --ratified-from "$ACCEPTED"
printf '%s\n' 'sync-bot@example.com' >"$TMP/denied.txt"
if bash "$CHECKER" range 'HEAD~1..HEAD' --repo "$TMP/repo" --ratified-from "$BASE" \
	--ratified-from "$ACCEPTED" --denied "$TMP/denied.txt" >"$TMP/out" 2>&1; then
	fail "source ratification bypassed a denied coauthor"
fi
git -C "$TMP/repo" -c core.hooksPath=/dev/null commit -q --allow-empty \
	-m $'unaccepted coauthor\n\nCo-authored-by: Unknown Author <unknown@example.com>'
if bash "$CHECKER" range 'HEAD~1..HEAD' --repo "$TMP/repo" --ratified-from "$BASE" \
	--ratified-from "$ACCEPTED" >"$TMP/out" 2>&1; then
	fail "an unaccepted coauthor was ratified"
fi
git -C "$TMP/repo" -c core.hooksPath=/dev/null commit -q --allow-empty \
	-m $'forbidden trailer\n\nClaude-Session: example'
if bash "$CHECKER" range 'HEAD~1..HEAD' --repo "$TMP/repo" --ratified-from "$BASE" \
	--ratified-from "$ACCEPTED" >"$TMP/out" 2>&1; then
	fail "source ratification bypassed a forbidden trailer"
fi

awk '
	/^          source_validator=/ { capture = 1 }
	capture && /^          count=/ { exit }
	capture { sub(/^          /, ""); print }
' "$ROOT/.github/workflows/ci.yml" >"$TMP/source-selection.sh"
[ -s "$TMP/source-selection.sh" ] || fail "the workflow source-selection block is missing"
cat >>"$TMP/source-selection.sh" <<'SH'
printf '%s\n' "${ratification[@]}" >"$RATIFICATION_FILE"
SH
mkdir -p "$TMP/runner" "$TMP/repo/.github/scripts"
cat >"$TMP/working-validator.sh" <<'SH'
printf ran >"$VALIDATOR_MARKER"
printf '%s\n' "$HEAD_SHA"
SH
cp "$TMP/working-validator.sh" "$TMP/repo/.github/scripts/commit-policy-source.sh"

selection() {
	(cd "$TMP/repo" && env BASE_SHA="$1" HEAD_SHA="$UNACCEPTED" \
		RUNNER_TEMP="$TMP/runner" VALIDATOR_MARKER="$TMP/validator-ran" \
		RATIFICATION_FILE="$TMP/ratification" TRUSTED_SOURCE="$ACCEPTED" \
		bash -euo pipefail "$TMP/source-selection.sh" >"$TMP/selection-out" 2>"$TMP/selection-err")
}

selection "$BASE" || fail "an absent base validator stopped target-only checks"
[ ! -e "$TMP/validator-ran" ] || fail "the workflow ran a working-copy validator absent from the base"
[ "$(cat "$TMP/ratification")" = "$(printf '%s\n' --ratified-from "$BASE")" ] ||
	fail "an absent base validator added source ratification"
if bash "$CHECKER" range "$BASE..$UNACCEPTED" --repo "$TMP/repo" \
	--ratified-from "$BASE" >"$TMP/out" 2>&1; then
	fail "target-only rollout checks accepted a PR-only author"
fi

git -C "$TMP/repo" checkout -q --detach "$BASE"
cat >"$TMP/repo/.github/scripts/commit-policy-source.sh" <<'SH'
printf '%s\n' "$TRUSTED_SOURCE"
SH
git -C "$TMP/repo" add .github/scripts/commit-policy-source.sh
git -C "$TMP/repo" -c core.hooksPath=/dev/null commit -q -m trusted-validator
TRUSTED_BASE="$(git -C "$TMP/repo" rev-parse HEAD)"
cp "$TMP/working-validator.sh" "$TMP/repo/.github/scripts/commit-policy-source.sh"
selection "$TRUSTED_BASE" || fail "the trusted base validator failed"
[ ! -e "$TMP/validator-ran" ] || fail "a working-copy validator replaced the trusted base validator"
[ "$(cat "$TMP/ratification")" = "$(printf '%s\n' --ratified-from "$TRUSTED_BASE" --ratified-from "$ACCEPTED")" ] ||
	fail "the trusted validator did not add its source to target ratification"
bash "$CHECKER" range "$TRUSTED_BASE..$ACCEPTED" --repo "$TMP/repo" \
	--ratified-from "$TRUSTED_BASE" --ratified-from "$ACCEPTED"

printf '%s\n' 'exit 1' >"$TMP/repo/.github/scripts/commit-policy-source.sh"
git -C "$TMP/repo" add .github/scripts/commit-policy-source.sh
git -C "$TMP/repo" -c core.hooksPath=/dev/null commit -q -m failing-validator
FAILING_BASE="$(git -C "$TMP/repo" rev-parse HEAD)"
cp "$TMP/working-validator.sh" "$TMP/repo/.github/scripts/commit-policy-source.sh"
if selection "$FAILING_BASE"; then
	fail "the workflow ignored a trusted validator failure"
fi
[ ! -e "$TMP/validator-ran" ] || fail "a failed trusted validator fell back to the working copy"

for required in "\${BASE_SHA}:.github/scripts/commit-policy-source.sh" \
	"source_bases=\"\$(bash \"\$source_validator\")\"" "ratification+=(--ratified-from \"\$source_sha\")" \
	"\"\${ratification[@]}\" --exempt-authors \"\$exempt\" --denied \"\$denied\"" \
	'run: bash .github/scripts/test-commit-policy-source.sh'; do
	grep -Fq "$required" "$ROOT/.github/workflows/ci.yml" || fail "missing workflow wiring: $required"
done

printf 'PASS: protected source snapshots ratify accepted authors without trusting forks, unaccepted commits, or later source identities.\n'
