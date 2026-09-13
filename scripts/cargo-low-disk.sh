#!/usr/bin/env bash
set -euo pipefail

if [[ ${1:-} == --help ]]; then
    printf '%s\n' 'Usage: bash scripts/cargo-low-disk.sh build|check|test [CARGO_ARGUMENTS]' 'This local profile uses an external cache, line-table debug information, and no incremental compilation.'
    exit 0
fi
case ${1:-} in
    build|check|test) ;;
    *) printf '%s\n' 'Expected build, check, or test.' >&2; exit 2 ;;
esac
for variable in CI GITHUB_ACTIONS GITLAB_CI SOAK_OUTPUT_DIR LLVM_PROFILE_FILE; do
    if [[ -n ${!variable:-} && ${!variable} != false && ${!variable} != 0 ]]; then
        printf '%s\n' 'The low-disk profile is restricted to local development.' >&2
        exit 2
    fi
done
for argument in "$@"; do
    case "$argument" in
        --release|--profile|--profile=*|--target-dir|--target-dir=*|--config|--config=*)
            printf '%s\n' 'The low-disk profile does not accept profile, cache, or configuration overrides.' >&2
            exit 2 ;;
    esac
done
export CARGO_TARGET_DIR="${CBC_CARGO_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/f1r3node/low-disk}"
export CARGO_PROFILE_DEV_DEBUG=line-tables-only
export CARGO_PROFILE_TEST_DEBUG=line-tables-only
export CARGO_INCREMENTAL=0
python3 -I - "$CARGO_TARGET_DIR" "${CBC_CACHE_LIMIT_GIB:-64}" "${CBC_MIN_FREE_GIB:-10}" <<'PY'
import os
from pathlib import Path
import shutil
import subprocess
import sys

try:
    raw = Path(sys.argv[1])
    if not raw.is_absolute():
        raise ValueError("The build cache path must be absolute.")
    cache = raw.resolve()
    if cache == Path.cwd() or Path.cwd() in cache.parents:
        raise ValueError("The build cache must be outside the working directory.")
    limit, floor = [int(value) for value in sys.argv[2:]]
    if not all(0 < value <= 1048576 for value in (limit, floor)):
        raise ValueError("The cache limits must be positive GiB values no greater than 1048576.")
    parent = cache
    while not parent.exists():
        parent = parent.parent
    if shutil.disk_usage(parent).free < floor * 1024**3:
        raise ValueError("The build cache has insufficient free space.")
    if cache.exists():
        result = subprocess.run(["du", "-sk", str(cache)], check=True, capture_output=True, text=True, timeout=30)
        if int(result.stdout.split()[0]) * 1024 >= limit * 1024**3:
            raise ValueError("The build cache reached its admission limit.")
    cache.mkdir(parents=True, exist_ok=True, mode=0o700)
except (OSError, ValueError, subprocess.SubprocessError) as error:
    print(f"FAIL: {error}", file=sys.stderr)
    sys.exit(2)
PY
exec cargo "$@"
