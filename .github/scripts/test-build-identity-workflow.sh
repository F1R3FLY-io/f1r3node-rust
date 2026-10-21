#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

extract_run() {
	awk -v name="$1" '
		$0 == "      - name: " name { step = 1; next }
		step && /^        run: \|/ { body = 1; next }
		body && /^          / { print substr($0, 11); found = 1; next }
		body && /^[[:space:]]*$/ { print ""; next }
		body { exit }
		END { if (!found) exit 1 }
	' "$ROOT/.github/workflows/_integration-pipeline.yml" >"$2"
}

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

extract_run 'Resolve build identity' "$TMP/resolve.sh"
extract_run 'Verify node build identity' "$TMP/verify.sh"
cd "$ROOT"

REQUESTED_BUILD_ID=v0.4.46-ci-selected GITHUB_OUTPUT="$TMP/explicit" bash -euo pipefail "$TMP/resolve.sh"
grep -Fxq 'build_id=v0.4.46-ci-selected' "$TMP/explicit" || fail 'The selected CI identity changed.'

REQUESTED_BUILD_ID= GITHUB_OUTPUT="$TMP/fallback" bash -euo pipefail "$TMP/resolve.sh"
grep -Fxq "build_id=$(git describe --tags --always)" "$TMP/fallback" || fail 'The Git fallback differs from the source checkout.'

for invalid in unknown '   ' $'build\ncargo=value' $'build\tvalue'; do
	if REQUESTED_BUILD_ID="$invalid" GITHUB_OUTPUT="$TMP/invalid" bash -euo pipefail "$TMP/resolve.sh" >"$TMP/invalid.log" 2>&1; then
		fail 'An invalid CI identity was accepted.'
	fi
done

mkdir "$TMP/bin"
cat >"$TMP/bin/curl" <<'BASH'
#!/usr/bin/env bash
case "${*: -1}" in
	*/api/status) printf '{"version":{"node":"%s"}}\n' "$STATUS_VERSION" ;;
	*/version) printf '%s\n' "$HTTP_VERSION" ;;
	*) exit 1 ;;
esac
BASH
cat >"$TMP/bin/docker" <<'BASH'
#!/usr/bin/env bash
case "$*" in
	*grpcurl*) printf '{"status":{"version":{"node":"%s"}}}\n' "$GRPC_VERSION_VALUE" ;;
	*logs*) printf '%s\n' "$STARTUP_VERSION" ;;
	*) exit 1 ;;
esac
BASH
chmod +x "$TMP/bin/curl" "$TMP/bin/docker"

export PATH="$TMP/bin:$PATH"
export EXPECTED_BUILD_ID=v0.4.46-ci-selected NODE_VERSION=0.4.46
expected="F1r3node Rust $NODE_VERSION ($EXPECTED_BUILD_ID)"
export STATUS_VERSION="$expected" HTTP_VERSION="$expected" GRPC_VERSION_VALUE="$expected" STARTUP_VERSION="$expected"
bash -euo pipefail "$TMP/verify.sh" >"$TMP/valid.log" 2>&1 || fail 'Matching versions failed the smoke check.'

for surface in STATUS_VERSION HTTP_VERSION GRPC_VERSION_VALUE STARTUP_VERSION; do
	if env "$surface=F1r3node Rust 0.4.46 ()" bash -euo pipefail "$TMP/verify.sh" >"$TMP/mismatch.log" 2>&1; then
		fail "$surface accepted an incorrect identity."
	fi
done
if EXPECTED_BUILD_ID= bash -euo pipefail "$TMP/verify.sh" >"$TMP/missing.log" 2>&1; then
	fail 'The smoke check accepted a missing expected identity.'
fi

printf '%s\n' 'PASS: CI preserves the selected identity and rejects invalid identities and inconsistent node version output.'
