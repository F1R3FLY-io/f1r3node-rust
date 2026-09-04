#!/usr/bin/env bash
# Re-pin the trusted F1R3FLY-io/system-integration harness across every pin
# site in one command:
#
#   .github/oci-validation.env                    (source of truth; sourced at runtime)
#   .github/workflows/_integration-pipeline.yml   (workflow-level env literal)
#   .github/workflows/merge-recovery-soak.yml     (workflow-level env literal)
#
# The workflow literals cannot read the env file at env-resolution time
# (GitHub resolves workflow-level `env:` before any step can run), so the pin
# is deliberately duplicated and check-workflow-invariants.sh fails CI when
# the sites disagree. This helper makes a bump one reviewed command instead
# of three hand edits that can drift.
#
# Usage: scripts/repin-system-integration.sh <40-hex commit sha>
#   or:  just repin-system-integration <40-hex commit sha>
#
# Env overrides (for tests and offline use):
#   REPIN_ROOT                repo root override
#   REPIN_SKIP_REMOTE_CHECK=1 skip the gh commit-existence probe
#   REPIN_SKIP_INVARIANTS=1   skip the post-rewrite invariants checker
set -euo pipefail

ROOT=${REPIN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}
ENV_FILE="$ROOT/.github/oci-validation.env"
WORKFLOWS=(
	"$ROOT/.github/workflows/_integration-pipeline.yml"
	"$ROOT/.github/workflows/merge-recovery-soak.yml"
)

new_sha="${1:-}"
case "$new_sha" in
'' | *[!0-9a-f]*)
	echo "usage: $0 <40-hex lowercase commit sha>" >&2
	echo "tags and branch names are rejected by policy: an immutable SHA is the supply-chain guarantee for the secret-bearing launch jobs" >&2
	exit 1
	;;
esac
if [ "${#new_sha}" -ne 40 ]; then
	echo "error: '$new_sha' is ${#new_sha} characters; a full 40-character commit SHA is required" >&2
	exit 1
fi

old_sha="$(grep -E '^SYSTEM_INTEGRATION_REF=' "$ENV_FILE" | cut -d= -f2-)"
if ! [[ "$old_sha" =~ ^[0-9a-f]{40}$ ]]; then
	echo "error: $ENV_FILE carries unexpected SYSTEM_INTEGRATION_REF '$old_sha'" >&2
	exit 1
fi

# Refuse to paper over drift. If a workflow already disagrees with the env
# file, the repo is in the state check-workflow-invariants.sh exists to catch,
# and a blind rewrite would erase the evidence of which value was reviewed.
for wf in "${WORKFLOWS[@]}"; do
	current="$(grep -E '^[[:space:]]*SYSTEM_INTEGRATION_REF:' "$wf" | head -1 |
		sed -E 's/^[^:]*:[[:space:]]*//; s/[[:space:]]*(#.*)?$//; s/"//g')"
	if [ "$current" != "$old_sha" ]; then
		echo "error: $wf pins '$current' but $ENV_FILE pins '$old_sha' — the sites have drifted; reconcile by hand before using this helper" >&2
		exit 1
	fi
done

if [ "$new_sha" = "$old_sha" ]; then
	echo "already pinned to $new_sha; nothing to do"
	exit 0
fi

# Best-effort existence probe: a warning rather than a hard failure, because
# offline and restricted-token environments must still be able to repin. The
# reviewed PR, not this probe, is the real gate.
if [ "${REPIN_SKIP_REMOTE_CHECK:-0}" != "1" ] && command -v gh >/dev/null 2>&1; then
	if gh api "repos/F1R3FLY-io/system-integration/commits/$new_sha" --jq .sha >/dev/null 2>&1; then
		echo "verified: $new_sha exists in F1R3FLY-io/system-integration"
	else
		echo "warning: could not verify $new_sha in F1R3FLY-io/system-integration (offline, unauthorized, or unknown SHA); review before committing" >&2
	fi
fi

sed -E -i.bak "s|^SYSTEM_INTEGRATION_REF=.*$|SYSTEM_INTEGRATION_REF=${new_sha}|" "$ENV_FILE"
rm -f "$ENV_FILE.bak"
for wf in "${WORKFLOWS[@]}"; do
	sed -E -i.bak "s|^([[:space:]]*)SYSTEM_INTEGRATION_REF:[[:space:]]*${old_sha}[[:space:]]*$|\\1SYSTEM_INTEGRATION_REF: ${new_sha}|" "$wf"
	rm -f "$wf.bak"
done

echo "re-pinned $old_sha -> $new_sha in:"
printf '  %s\n' "$ENV_FILE" "${WORKFLOWS[@]}"

if [ "${REPIN_SKIP_INVARIANTS:-0}" != "1" ]; then
	bash "$ROOT/.github/scripts/check-workflow-invariants.sh"
fi

echo "reminder: add a dated entry to the pin-history comment above the pin in .github/workflows/merge-recovery-soak.yml — what the new ref carries and how it was verified"
