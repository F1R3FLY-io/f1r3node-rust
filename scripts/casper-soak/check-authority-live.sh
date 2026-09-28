#!/usr/bin/env bash
set -euo pipefail
umask 077
[[ $# == 1 && "$(uname -s)" == Linux ]] || { printf 'Usage on Linux: %s OUTPUT\n' "$0" >&2; exit 2; }
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
out="$1"
[[ ! -e "$out" && ! -L "$out" ]] || exit 2
mkdir -p "$(dirname "$out")"
mkdir -m 700 "$out"
out="$(cd "$out" && pwd)"
finish() {
    code=$?
    trap - EXIT
    printf '{"schema_version":1,"status":"%s","exit_code":%s,"scope":"controlled-live-executor","qualification":"pending","soak_verdict":"non_passing","blockchain_nodes":0,"cloud_dispatches":0}\n' \
        "$(if [[ "$code" == 0 ]]; then printf passed; else printf failed; fi)" "$code" > "$out/report.json"
    exit "$code"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
files=(scripts/casper-soak/src/authority_live.rs scripts/casper-soak/src/bin/casper-authority-live.rs
    scripts/casper-soak/tests/authority_live.rs scripts/casper-soak/check-authority-live.sh
    scripts/casper-soak/src/authority_observer.rs scripts/casper-soak/src/authority_mapping.rs
    scripts/casper-soak/src/bin/casper-authority-observe.rs scripts/casper-soak/src/lib.rs
    scripts/casper-soak/src/manifest.rs scripts/casper-soak/Cargo.toml Cargo.lock
    .github/workflows/casper-authority-adapter.yml docs/claims/casper-authority-live-executor.md
    docs/casper/design/authority-live-executor.md
    rust-toolchain.toml .cargo/config.toml)
sha256sum "${files[@]}" > "$out/source-before.sha256"
export CARGO_PROFILE_TEST_OPT_LEVEL="${CARGO_PROFILE_TEST_OPT_LEVEL:-1}"
export SOAK_AUTHORITY_LIVE_EVIDENCE="$out/fixtures"
cargo test --locked -p casper-soak --test authority_live > "$out/tests.txt" 2>&1
cargo clippy --locked -p casper-soak --bin casper-authority-live --test authority_live -- -D warnings > "$out/clippy.txt" 2>&1
sha256sum "${files[@]}" > "$out/source-after.sha256"
cmp "$out/source-before.sha256" "$out/source-after.sha256"
