#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == TASK-017-4 ]] || { printf 'Only TASK-017-4 is supported.\n' >&2; exit 2; }
shift
MODE=check
HELPER="${SA_TASK_COMPLETE_HELPER:-}"
while [[ $# -gt 0 ]]; do
 case "$1" in
  --helper) HELPER="$2"; shift 2 ;;
  --complete) MODE=complete; shift ;;
  --check) MODE=check; shift ;;
  *) printf 'Unsupported completion option.\n' >&2; exit 2 ;;
 esac
done
[[ -f "$HELPER" && ! -L "$HELPER" ]] || { printf 'A regular shared completion helper is required.\n' >&2; exit 2; }
EXPECTED=924a1cde6ae61d15a72dda276687b0d63d9df0dd75d7b7f1aba4e8300f502d2e
[[ "$(shasum -a 256 "$HELPER" | awk '{print $1}')" == "$EXPECTED" ]] || { printf 'The shared completion helper needs compatibility review.\n' >&2; exit 2; }
DIRECTORY="$(cd "$(dirname "$HELPER")" && pwd)"
TEMP="$(mktemp -d)"
trap 'rm -rf "$TEMP"' EXIT
ln -s "$DIRECTORY/lib" "$TEMP/lib"
awk 'BEGIN { n=0 } $0 == "main \"$@\"" { n++; next } { print } END { if (n != 1) exit 2 }' "$HELPER" >"$TEMP/helper.sh"
source "$TEMP/helper.sh"
ROOT="$(git rev-parse --show-toplevel)"
report="$(validate_task TASK-017-4)"
claim_status=0
"${SOAK_CLAIM_CHECKER_BIN:-$ROOT/target/debug/check-casper-claims}" --root "$ROOT" --claim CLAIM-CASPER-SOAK-001 --strict --output "$TEMP/claim-check.json" || claim_status=$?
if [[ "$claim_status" != 0 ]]; then
 report="$(jq '.grade="partial" | .gaps=((.gaps+["undischarged_cbc_claim"])|unique) | .details += [{code:"undischarged_cbc_claim",message:"The source-bound harness claim audit did not pass.",location:"docs/claims/casper-soak-harness.md"}]' <<<"$report")"
 printf '%s\n' "$report"
 exit "$claim_status"
fi
if [[ "$MODE" == complete ]]; then
 mark_task_complete TASK-017-4 "" true false false
else
 printf '%s\n' "$report"
 if ! jq -e '.grade=="full" and (.gaps|length)==0' <<<"$report" >/dev/null; then exit 3; fi
fi
