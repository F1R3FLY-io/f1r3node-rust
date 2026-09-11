#!/usr/bin/env bash
set -euo pipefail
trap '' TERM
printf '%s\n' "$$" >"$1.pid"
for n in $(seq 1 600); do printf '%s\n' "$n" >>"$1.writes"; sleep 0.1; done
