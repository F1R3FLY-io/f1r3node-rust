#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT"
runtime="$(bash scripts/ci/install-cbc-runtime.sh --check)"
if [ "${1:-}" = record ]; then
    shift
    exec python3 "$runtime/lib/cbc-acceptance.py" record "$@"
fi
exec bash "$runtime/cbc.sh" "$@"
