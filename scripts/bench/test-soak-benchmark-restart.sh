#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOAK_DISK_TEST_SCENARIO=restart-benchmark \
    bash "$ROOT/scripts/bench/test-soak-disk-admission.sh" "$@"
