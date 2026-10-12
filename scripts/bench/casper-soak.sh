#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN="${SOAK_HARNESS_BIN:-$ROOT/target/debug/casper-soak}"
if [[ ! -x "$BIN" ]]; then
	printf 'Build the harness with cargo build -p casper-soak, or set SOAK_HARNESS_BIN.\n' >&2
	exit 2
fi
exec "$BIN" "$@" --root "$ROOT"
