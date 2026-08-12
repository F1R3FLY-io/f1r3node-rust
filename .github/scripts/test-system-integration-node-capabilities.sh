#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
READER="$ROOT/.github/scripts/read-system-integration-node-capabilities.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

expect_failure() {
	if "$READER" "$1" >"$TMP/stdout" 2>"$TMP/stderr"; then
		printf 'expected capability validation to fail for %s\n' "$1" >&2
		exit 1
	fi
}

: >"$TMP/empty"
test -z "$("$READER" "$TMP/empty")"

cat >"$TMP/valid" <<'EOF'
# released node behavior
expired-deploy-admission
observer-missing-block-retry
EOF
printf '%s\n' expired-deploy-admission observer-missing-block-retry >"$TMP/expected"
"$READER" "$TMP/valid" >"$TMP/actual"
cmp "$TMP/expected" "$TMP/actual"

printf '%s\n' 'Invalid-Capability' >"$TMP/invalid"
expect_failure "$TMP/invalid"

printf '%s\n' duplicate-capability duplicate-capability >"$TMP/duplicate"
expect_failure "$TMP/duplicate"

expect_failure "$TMP/missing"

for workflow in \
	"$ROOT/.github/workflows/_integration-pipeline.yml" \
	"$ROOT/.github/workflows/reusable-oci-validation.yml"; do
	grep -Fq 'read-system-integration-node-capabilities.sh' "$workflow"
	grep -Fq '"${NODE_CAPABILITY_ARGS[@]}"' "$workflow"
done

"$READER" "$ROOT/.github/system-integration-node-capabilities.txt" >/dev/null

expected_ref=$(grep '^SYSTEM_INTEGRATION_REF=' "$ROOT/.github/oci-validation.env" | cut -d= -f2-)
for workflow in \
	"$ROOT/.github/workflows/_integration-pipeline.yml" \
	"$ROOT/.github/workflows/merge-recovery-soak.yml"; do
	actual_ref=$(grep -E '^[[:space:]]*SYSTEM_INTEGRATION_REF:' "$workflow" | head -1 | sed -E 's/^[^:]*:[[:space:]]*//; s/[[:space:]]*(#.*)?$//; s/"//g')
	test "$actual_ref" = "$expected_ref"
done

printf 'system-integration node capability contract checks passed\n'
