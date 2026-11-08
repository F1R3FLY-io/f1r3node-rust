#!/usr/bin/env bash
set -euo pipefail

mkdir -p lychee
status=0
lychee --format json --output lychee/out.json \
    --no-progress \
    --exclude '^https?://' \
    --include '^https?://(www\.)?github\.com' \
    --include '^https?://raw\.githubusercontent\.com' \
    --include '^https?://(www\.)?gitlab\.com' \
    --max-retries 2 --retry-wait-time 5 --timeout 45 \
    --accept 200..=299,403,429,503,504 \
    './**/*.md' || status=$?

{
    printf '# Link check\n\n```json\n'
    jq '{total,successful,errors,timeouts,unknown,unsupported,error_map,timeout_map}' lychee/out.json
    printf '```\n'
} | tee -a "${GITHUB_STEP_SUMMARY:-/dev/null}"

jq -e '.total > 0 and .unknown == 0 and .unsupported == 0' lychee/out.json >/dev/null
[ "$status" -ne 0 ] || exit 0
[ "$status" -eq 2 ] || exit "$status"
mapfile -t failures < <(jq -r '[.error_map[][]?.url, .timeout_map[][]?.url] | unique[]' lychee/out.json)
[ "${#failures[@]}" -gt 0 ] || exit 1
failed=0
for url in "${failures[@]}"; do
    link="${url%%#*}"
    endpoint=""
    if [[ "$link" =~ ^https://github\.com/([^/?]+)/([^/?]+)/blob/([^/?]+)/([^?]+)$ ]]; then
        endpoint="repos/${BASH_REMATCH[1]}/${BASH_REMATCH[2]}/contents/${BASH_REMATCH[4]}?ref=${BASH_REMATCH[3]}"
    elif [[ "$link" =~ ^https://github\.com/([^/?]+)/([^/?]+)/pull/([0-9]+)/?$ ]]; then
        endpoint="repos/${BASH_REMATCH[1]}/${BASH_REMATCH[2]}/pulls/${BASH_REMATCH[3]}"
    elif [[ "$link" =~ ^https://github\.com/([^/?]+)/([^/?]+)/issues/([0-9]+)/?$ ]]; then
        endpoint="repos/${BASH_REMATCH[1]}/${BASH_REMATCH[2]}/issues/${BASH_REMATCH[3]}"
    elif [[ "$link" =~ ^https://github\.com/([^/?]+)/([^/?]+)/actions/runs/([0-9]+)/?$ ]]; then
        endpoint="repos/${BASH_REMATCH[1]}/${BASH_REMATCH[2]}/actions/runs/${BASH_REMATCH[3]}"
    elif [[ "$link" =~ ^https://github\.com/([^/?]+)/([^/?]+)/?$ ]]; then
        endpoint="repos/${BASH_REMATCH[1]}/${BASH_REMATCH[2]}"
    elif [[ "$link" =~ ^https://github\.com/([^/?]+)/?$ ]]; then
        endpoint="users/${BASH_REMATCH[1]}"
    fi
    if [ -n "$endpoint" ] && gh api --method GET "$endpoint" --silent; then
        printf 'Verified through GitHub API: %s\n' "$url" | tee -a "${GITHUB_STEP_SUMMARY:-/dev/null}"
    else
        printf 'Unresolved link: %s\n' "$url" >&2
        failed=1
    fi
done
exit "$failed"
