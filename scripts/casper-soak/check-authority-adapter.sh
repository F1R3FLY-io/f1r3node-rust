#!/usr/bin/env bash
set -euo pipefail
umask 077
[[ $# == 1 && "$(uname -s)" == Linux ]] || { printf 'Usage on Linux: %s OUTPUT\n' "$0" >&2; exit 2; }
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
out="$1"
[[ ! -e "$out" && ! -L "$out" ]] || exit 2
mkdir -m 700 "$out"
out="$(cd "$out" && pwd)"
finish() {
    code=$?
    trap - EXIT
    printf '{"schema_version":1,"status":"%s","exit_code":%s,"scope":"controlled-authority-adapter","qualification":"pending","soak_verdict":"non_passing","node_launches":0,"cloud_launches":0}\n' \
        "$(if [[ "$code" == 0 ]]; then printf passed; else printf failed; fi)" "$code" > "$out/report.json"
    exit "$code"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
files=(scripts/casper-soak/src/authority_mapping.rs scripts/casper-soak/src/authority_observer.rs
    scripts/casper-soak/src/bin/casper-authority-observe.rs scripts/casper-soak/process_faults.py
    scripts/casper-soak/tests/authority_mapping.rs scripts/casper-soak/tests/authority_observer.rs
    scripts/casper-soak/tests/test_process_faults.py scripts/casper-soak/check-authority-adapter.sh
    scripts/casper-soak/src/lib.rs scripts/casper-soak/src/manifest.rs scripts/casper-soak/Cargo.toml
    .github/workflows/casper-authority-adapter.yml docs/claims/casper-authority-adapter.md
    docs/claims/casper-authority-observer-client.md Cargo.lock rust-toolchain.toml .cargo/config.toml)
sha256sum "${files[@]}" > "$out/source-before.sha256"
export CARGO_PROFILE_TEST_OPT_LEVEL="${CARGO_PROFILE_TEST_OPT_LEVEL:-1}"
export PYTHONDONTWRITEBYTECODE=1
cargo test --locked -p casper-soak --test authority_mapping --test authority_observer > "$out/rust-tests.txt" 2>&1
python3 -m unittest discover -s scripts/casper-soak/tests -p test_process_faults.py -v > "$out/process-tests.txt" 2>&1
cargo clippy --locked -p casper-soak --bin casper-authority-observe --test authority_mapping --test authority_observer -- -D warnings > "$out/clippy.txt" 2>&1
sha256sum "${files[@]}" > "$out/source-after.sha256"
cmp "$out/source-before.sha256" "$out/source-after.sha256"
