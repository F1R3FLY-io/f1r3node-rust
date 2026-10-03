#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
out="${1:?A new evidence directory is required.}"
[[ ! -e "$out" && ! -L "$out" ]] || exit 2
mkdir -m 700 -- "$out"
out="$(realpath -e -- "$out")"
status=failed
trap 'code=$?; jq -n --arg status "$status" --argjson code "$code" \
  '\''{schema_version:1,status:$status,exit_code:$code,evidence_kind:"controlled_provider_fixture",
    node_launches:0,cloud_launches:0,claim_discharge:"pending"}'\'' > "$out/report.json"' EXIT
cargo test --locked -p casper-soak --test campaign_control --bin casper-campaign-supervisor \
  > "$out/rust.log" 2>&1
bash -n scripts/casper-soak/campaign-control.sh scripts/casper-soak/campaign-host.sh \
  scripts/casper-soak/campaign-bootstrap.sh scripts/casper-soak/campaign-host-guard.sh \
  scripts/casper-soak/campaign-finish.sh scripts/casper-soak/campaign-job.sh \
  scripts/casper-soak/supervisor/start.sh
if bash scripts/casper-soak/campaign-control.sh > "$out/missing-input.log" 2>&1; then exit 1; fi
if bash scripts/casper-soak/campaign-host.sh > "$out/missing-host.log" 2>&1; then exit 1; fi
if bash scripts/casper-soak/campaign-job.sh > "$out/missing-job.log" 2>&1; then exit 1; fi
status=passed
printf 'Campaign control fixtures passed.\n'
