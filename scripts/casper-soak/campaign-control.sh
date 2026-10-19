#!/usr/bin/env bash
set -euo pipefail
umask 077
[[ $# == 5 ]] || { printf 'Expected a source root, request, configuration, command, and evidence directory.\n' >&2; exit 2; }
root="$(realpath -e -- "$1")"
request="$(realpath -e -- "$2")"
configuration="$(realpath -e -- "$3")"
action="$4"
out="$5"
[[ "$action" == approval || "$action" == dispatch ]] || exit 2
[[ ! -e "$out" && ! -L "$out" ]] || exit 2
mkdir -m 700 -- "$out"
out="$(realpath -e -- "$out")"
status=non_passing
trap 'code=$?; jq -n --arg status "$status" --argjson code "$code" \
  '\''{schema_version:1,scope:"campaign-controller-wrapper",status:$status,exit_code:$code,campaign_result:"pending",claim_discharge:"pending"}'\'' > "$out/wrapper.json"' EXIT
[[ "${GITHUB_EVENT_NAME:-}" == workflow_dispatch && "${GITHUB_RUN_ATTEMPT:-}" == 1 ]] || exit 2
[[ "${GITHUB_REPOSITORY:-}" == F1R3FLY-io/f1r3node-rust ]] || exit 2
[[ "${GITHUB_RUN_ID:-}" =~ ^[1-9][0-9]{0,19}$ ]] || exit 2
[[ "${CAMPAIGN_CONTROL_REVISION:-}" == "$(git -C "$root" rev-parse HEAD)" ]] || exit 2
[[ "${INPUT_TARGET_REF:-dev}" == dev && "${INPUT_RETRY_ATTEMPT:-0}" == 0 ]] || exit 2
[[ -z "${INPUT_SCHEDULED_SLOT:-}${INPUT_WINDOW_END:-}${INPUT_SERIES:-}${INPUT_RESTART_OF_RUN_ID:-}${INPUT_CANDIDATE_TAG:-}" ]] || exit 2
for value in "${INPUT_PREFLIGHT_ONLY:-false}" "${INPUT_SKIP_PREFLIGHT:-false}" "${INPUT_CANARY:-false}" "${INPUT_INJECT_PROTECTION_BREACH:-false}"; do
  [[ "$value" == false ]] || exit 2
done
binary="$root/target/debug/casper-campaign-control"
[[ -x "$binary" ]] || { printf 'Build the campaign controller before dispatch.\n' >&2; exit 2; }
bash "$root/scripts/casper-soak/campaign.sh" plan "$root" "$request" > "$out/plan.json"
case "$(jq -er .stage "$out/plan.json")" in
  preflight) selection=campaign-preflight ;;
  baseline) selection=campaign-baseline-24h ;;
  stability) selection=campaign-stability-60h ;;
  *) exit 2 ;;
esac
[[ "${CAMPAIGN_SELECTION:-}" == "$selection" ]] || exit 2
bash "$root/scripts/casper-soak/campaign.sh" verify-priors "$out/plan.json" "$out"
"$binary" "$action" --root "$root" --config "$configuration" --request "$request" \
  --plan "$out/plan.json" --run "$GITHUB_RUN_ID" --evidence "$out/control" > "$out/receipt.json"
status=passed
if [[ "$action" == approval && -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  {
    printf 'Approve this exact campaign binding in the casper-campaign environment:\n\n'
    cat "$out/receipt.json"
  } >> "$GITHUB_STEP_SUMMARY"
fi
