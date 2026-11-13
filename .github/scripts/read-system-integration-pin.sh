#!/usr/bin/env bash
set -euo pipefail

reject() {
	printf '::error::%s\n' "$1" >&2
	exit 1
}

if [ "$#" -eq 3 ] && [ "$1" = --repository ]; then
	repository="$2"
	revision="$3"
	[[ "$repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || reject 'Invalid candidate repository.'
	[[ "$revision" =~ ^[0-9a-f]{40}$ ]] || reject 'The candidate revision must be a full commit SHA.'
	file="$(mktemp)"
	trap 'rm -f "$file"' EXIT
	env -u GITHUB_TOKEN gh api --method GET -H 'Accept: application/vnd.github.raw+json' \
		"repos/$repository/contents/.github/oci-validation.env?ref=$revision" >"$file"
elif [ "$#" -eq 1 ]; then
	file="$1"
	[ "$file" != - ] || file=/dev/stdin
else
	reject 'Usage: read-system-integration-pin.sh FILE | --repository OWNER/REPO COMMIT_SHA'
fi

count=0
pin=""
while IFS= read -r line || [ -n "$line" ]; do
	case "$line" in
		SYSTEM_INTEGRATION_REF=*)
			count=$((count + 1))
			pin="${line#SYSTEM_INTEGRATION_REF=}"
			pin="$(printf '%s' "$pin" | sed -E 's/[[:space:]]*$//')"
			;;
	esac
done <"$file"

[ "$count" -eq 1 ] || reject 'The candidate must declare SYSTEM_INTEGRATION_REF exactly once.'
[[ "$pin" =~ ^[0-9a-f]{40}$ || "$pin" =~ ^refs/tags/[A-Za-z0-9._/-]+$ ]] ||
	reject 'The candidate system-integration pin must be a full lowercase commit SHA or refs/tags/<tag>.'
printf '%s\n' "$pin"
