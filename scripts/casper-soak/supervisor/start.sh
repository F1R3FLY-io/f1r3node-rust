#!/usr/bin/env bash
set -euo pipefail
umask 077
[[ -n "${CASPER_CAMPAIGN_CONFIG_JSON:-}" && -n "${CASPER_CAMPAIGN_CONFIG_SHA256:-}" ]] || exit 2
configuration="$(mktemp)"
printf '%s' "$CASPER_CAMPAIGN_CONFIG_JSON" > "$configuration"
unset CASPER_CAMPAIGN_CONFIG_JSON
export CASPER_CAMPAIGN_CONFIG="$configuration"
exec /function/casper-campaign-supervisor
