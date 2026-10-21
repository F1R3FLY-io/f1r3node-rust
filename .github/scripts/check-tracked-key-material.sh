#!/usr/bin/env bash
# Fails when validator private keys come back into the tracked files that
# issue #745 cleaned up.
#
# Anyone who reads the repository can sign with a tracked key. Issue #745 found
# keys in a tracked docker/.env, in the Helm chart values, and inline in a
# workflow. docker/.env.example keeps the public development keys on purpose
# and is not checked.
#
#   check-tracked-key-material.sh [REPO_ROOT]
#
# The check fails when:
#   - docker/.env is tracked (.gitignore lists .env, compose auto-loads it),
#   - a file under docker/helm/ sets privateKey to a value (the chart reads the
#     keys from a Secret, see validatorKeys.existingSecret),
#   - a workflow passes an inline hex key to --validator-private-key.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo="${1:-$ROOT}"
git -C "$repo" rev-parse --git-dir >/dev/null 2>&1 || { echo "ERROR: $repo is not a git repository." >&2; exit 2; }

failed=0

if [ -n "$(git -C "$repo" ls-files -- docker/.env)" ]; then
	echo "ERROR: docker/.env is tracked. It holds local node keys. Untrack it with git rm --cached docker/.env and keep docker/.env.example as the template." >&2
	failed=1
fi

helm_keys="$(git -C "$repo" grep -nE "privateKey:[[:space:]]*[\"']?[0-9a-fA-F]{64}" -- 'docker/helm/' || true)"
if [ -n "$helm_keys" ]; then
	echo "ERROR: the Helm chart sets privateKey values. Read the keys from the Secret in validatorKeys.existingSecret instead:" >&2
	sed -E 's/(privateKey:[[:space:]]*).*/\1<redacted>/; s/^/  /' <<<"$helm_keys" >&2
	failed=1
fi

workflow_keys="$(git -C "$repo" grep -nE -- '--validator-private-key=[0-9a-fA-F]{64}' -- '.github/workflows/' || true)"
if [ -n "$workflow_keys" ]; then
	echo "ERROR: a workflow passes an inline validator private key. Read it from docker/.env.example or a secret instead:" >&2
	sed -E 's/(--validator-private-key=)[0-9a-fA-F]+/\1<redacted>/; s/^/  /' <<<"$workflow_keys" >&2
	failed=1
fi

[ "$failed" -eq 0 ] || exit 1
echo "No validator private keys in the checked tracked files."
