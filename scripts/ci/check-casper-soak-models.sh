#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
args=()
while [[ $# -gt 0 ]]; do
	case "$1" in
		--output-dir) args+=(--output "${2:?Missing output directory}"); shift 2 ;;
		--java) JAVA="${2:?Missing Java executable}"; shift 2 ;;
		*) args+=("$1"); shift ;;
	esac
done
exec bash "$ROOT/scripts/bench/casper-soak.sh" models --java "${JAVA:-java}" "${args[@]}"
