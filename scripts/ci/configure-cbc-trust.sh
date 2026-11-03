#!/usr/bin/env bash
set -euo pipefail
if [ -z "${CBC_ACCEPTANCE_ALLOWED_SIGNERS:-}" ]; then
    printf 'CBC_ACCEPTANCE_ALLOWED_SIGNERS must contain the operator-approved public signers.\n' >&2
    exit 4
fi
trust="$(mktemp "${RUNNER_TEMP:?RUNNER_TEMP is required}/cbc-allowed-signers.XXXXXXXX")"
chmod 600 "$trust"
printf '%s\n' "$CBC_ACCEPTANCE_ALLOWED_SIGNERS" >"$trust"
git config --local cbc.acceptanceAllowedSigners "$trust"
