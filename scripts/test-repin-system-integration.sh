#!/usr/bin/env bash
# Exercises repin-system-integration.sh against a fixture repo root: the
# happy-path rewrite of all three pin sites, the no-op short-circuit, input
# validation (short / non-hex / branch-name refs), and the drift refusal.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPIN="$SCRIPT_DIR/repin-system-integration.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

OLD=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
NEW=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
THIRD=cccccccccccccccccccccccccccccccccccccccc

ENV_FILE="$TMP/root/.github/oci-validation.env"
PIPELINE="$TMP/root/.github/workflows/_integration-pipeline.yml"
SOAK="$TMP/root/.github/workflows/merge-recovery-soak.yml"

make_fixture() {
	rm -rf "$TMP/root"
	mkdir -p "$TMP/root/.github/workflows"
	printf 'SYSTEM_INTEGRATION_REF=%s\n' "$1" >"$ENV_FILE"
	printf 'env:\n  SYSTEM_INTEGRATION_REF: %s\n  OTHER_VAR: unrelated\n' "$2" >"$PIPELINE"
	printf 'env:\n  SYSTEM_INTEGRATION_REF: %s\njobs:\n  x:\n    steps:\n      - uses: actions/checkout@v5\n        with:\n          ref: ${{ env.SYSTEM_INTEGRATION_REF }}\n' "$3" >"$SOAK"
}

run_repin() {
	REPIN_ROOT="$TMP/root" REPIN_SKIP_REMOTE_CHECK=1 REPIN_SKIP_INVARIANTS=1 \
		"$REPIN" "$@"
}

expect_failure() {
	if run_repin "$@" >"$TMP/out" 2>"$TMP/err"; then
		printf 'expected repin to fail for: %s\n' "$*" >&2
		exit 1
	fi
}

make_fixture "$OLD" "$OLD" "$OLD"
run_repin "$NEW" >/dev/null
grep -q "^SYSTEM_INTEGRATION_REF=$NEW\$" "$ENV_FILE"
grep -q "^  SYSTEM_INTEGRATION_REF: $NEW\$" "$PIPELINE"
grep -q "^  SYSTEM_INTEGRATION_REF: $NEW\$" "$SOAK"
if grep -q "$OLD" "$ENV_FILE" "$PIPELINE" "$SOAK"; then
	echo 'old sha survived the rewrite' >&2
	exit 1
fi
grep -q 'OTHER_VAR: unrelated' "$PIPELINE"
grep -qF 'ref: ${{ env.SYSTEM_INTEGRATION_REF }}' "$SOAK"

run_repin "$NEW" | grep -q 'nothing to do'

make_fixture "$OLD" "$OLD" "$OLD"
expect_failure abc123
expect_failure AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
expect_failure main
expect_failure ''
grep -q "^SYSTEM_INTEGRATION_REF=$OLD\$" "$ENV_FILE"

make_fixture "$OLD" "$THIRD" "$OLD"
expect_failure "$NEW"
grep -q 'drifted' "$TMP/err"
grep -q "^SYSTEM_INTEGRATION_REF=$OLD\$" "$ENV_FILE"
grep -q "^  SYSTEM_INTEGRATION_REF: $THIRD\$" "$PIPELINE"

printf 'repin helper checks passed\n'
