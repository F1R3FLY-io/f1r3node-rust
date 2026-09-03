#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

uptime_inputs_hash() {
  local -a sources=()
  mapfile -t sources < <(
    cd "$ROOT"
    {
      find docs/casper/theory/uptime -type f -print
      find formal/storm/uptime formal/tlaplus/uptime formal/mcrl2/uptime formal/wolfram/uptime -type f -print
      printf '%s\n' \
        scripts/check-tlc-source-binding.sh \
        scripts/lib/tlc-run.sh \
        scripts/check-uptime-ALL.sh \
        scripts/check-uptime-storm.sh \
        scripts/check-uptime-tla.sh \
        scripts/check-uptime-mcrl2.sh \
        scripts/check-uptime-documentation.sh \
        scripts/check-uptime-wolfram.sh \
        scripts/render-uptime-engineering-report.sh
    } | sort -u
  )
  (cd "$ROOT" && sha256sum "${sources[@]}" | sha256sum | awk '{print $1}')
}

uptime_inputs_before="$(uptime_inputs_hash)"

"$ROOT/scripts/check-tlc-source-binding.sh"
"$ROOT/scripts/check-uptime-storm.sh"
"$ROOT/scripts/check-uptime-tla.sh"
"$ROOT/scripts/check-uptime-mcrl2.sh"
"$ROOT/scripts/check-uptime-documentation.sh"

if [[ "${RUN_WOLFRAM:-0}" == "1" ]]; then
  "$ROOT/scripts/check-uptime-wolfram.sh"
else
  echo "Wolfram uptime optimization skipped; set RUN_WOLFRAM=1 to opt in."
fi

uptime_inputs_after="$(uptime_inputs_hash)"
if [[ "$uptime_inputs_before" != "$uptime_inputs_after" ]]; then
  echo "error: uptime formal inputs changed during the aggregate verification run" >&2
  exit 75
fi

echo "Uptime formal verification pipeline passed."
