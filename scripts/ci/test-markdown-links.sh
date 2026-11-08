#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
cat > "$tmp/bin/lychee" <<'SH'
#!/usr/bin/env bash
cp "$TEST_REPORT" lychee/out.json
exit "$TEST_LYCHEE_STATUS"
SH
cat > "$tmp/bin/gh" <<'SH'
#!/usr/bin/env bash
grep -Fxq -- "$4" "$TEST_VALID_ENDPOINTS"
SH
chmod +x "$tmp/bin/lychee" "$tmp/bin/gh"
export PATH="$tmp/bin:$PATH"
export TEST_REPORT="$tmp/report.json"
export TEST_VALID_ENDPOINTS="$tmp/endpoints"
export TEST_LYCHEE_STATUS=2
export GITHUB_STEP_SUMMARY="$tmp/summary"
printf '%s\n' \
    'repos/owner/repo/contents/file.md?ref=reviewed' \
    'repos/owner/repo/pulls/7' \
    'repos/owner/repo/issues/8' \
    'repos/owner/repo/actions/runs/9' \
    'users/owner' > "$TEST_VALID_ENDPOINTS"

check() {
    local name="$1" url="$2" expected="$3" total="${4:-1}" result=0
    jq -n --arg url "$url" --argjson total "$total" \
        '{total:$total,successful:0,unknown:0,unsupported:0,timeouts:0,
          errors:(if $url == "" then 0 else 1 end),timeout_map:{},
          error_map:(if $url == "" then {} else {"README.md":[{url:$url}]} end)}' \
        > "$TEST_REPORT"
    (cd "$tmp" && bash "$root/scripts/ci/check-markdown-links.sh") > "$tmp/$name.log" 2>&1 || result=$?
    if [[ ( "$expected" == pass && "$result" -ne 0 ) || ( "$expected" == fail && "$result" -eq 0 ) ]]; then
        cat "$tmp/$name.log"
        printf 'Unexpected link verdict: %s (%s)\n' "$name" "$result" >&2
        exit 1
    fi
}

check exact-file https://github.com/owner/repo/blob/reviewed/file.md pass
check wrong-revision https://github.com/owner/repo/blob/unreviewed/file.md fail
check missing-file https://github.com/owner/repo/blob/reviewed/missing.md fail
check pull-request https://github.com/owner/repo/pull/7 pass
check missing-pull-request https://github.com/owner/repo/pull/700 fail
check issue https://github.com/owner/repo/issues/8 pass
check workflow-run https://github.com/owner/repo/actions/runs/9 pass
check account https://github.com/owner pass
check local-file file:///missing.md fail
check other-host https://gitlab.com/owner/repo fail
check unsupported-path https://github.com/owner/repo/unknown fail
TEST_LYCHEE_STATUS=0 check successful "" pass
TEST_LYCHEE_STATUS=0 check empty "" fail 0
TEST_LYCHEE_STATUS=1 check checker-failure "" fail
printf 'Markdown link API fallback controls passed.\n'
