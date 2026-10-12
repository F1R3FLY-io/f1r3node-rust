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
    scripts/casper-soak/src/authority_p2p.rs scripts/casper-soak/src/authority_process.rs
    scripts/casper-soak/src/authority_incarnation.rs scripts/casper-soak/src/authority_execution.rs
    scripts/casper-soak/src/profiles/authority_finality.rs scripts/casper-soak/tests/authority_finality.rs
    scripts/casper-soak/src/bin/casper-authority-finality.rs
    docs/claims/casper-soak-authority-finality.md formal/tlaplus/casper_soak/profiles/authority_finality/README.md
    scripts/casper-soak/src/bin/casper-authority-p2p.rs scripts/casper-soak/src/bin/casper-authority-process.rs
    scripts/casper-soak/tests/authority_p2p.rs scripts/casper-soak/tests/authority_process.rs
    scripts/casper-soak/src/authority_observer.rs scripts/casper-soak/src/authority_mapping.rs
    scripts/casper-soak/src/bin/casper-authority-observe.rs scripts/casper-soak/src/lib.rs
    scripts/casper-soak/src/manifest.rs scripts/casper-soak/Cargo.toml Cargo.toml Cargo.lock
    .github/workflows/casper-authority-adapter.yml docs/claims/casper-authority-live-executor.md
    docs/casper/design/authority-live-executor.md docs/casper/design/authority-provider-adaptation.md
    rust-toolchain.toml .cargo/config.toml)
git -c safe.directory="$root" ls-files comm models crypto shared 'rspace++' graphz > "$out/dependency-files.txt"
while IFS= read -r path; do files+=("$path"); done < "$out/dependency-files.txt"
sha256sum "${files[@]}" > "$out/source-before.sha256"
export CARGO_PROFILE_TEST_OPT_LEVEL="${CARGO_PROFILE_TEST_OPT_LEVEL:-1}"
export SOAK_AUTHORITY_LIVE_EVIDENCE="$out/fixtures"
cargo test --locked -p casper-soak --features p2p --test authority_live --test authority_p2p --test authority_process --test authority_finality > "$out/tests.txt" 2>&1
cargo clippy --locked -p casper-soak --features p2p --bin casper-authority-live --bin casper-authority-p2p --bin casper-authority-process --bin casper-authority-finality --test authority_live --test authority_p2p --test authority_process --test authority_finality -- -D warnings > "$out/clippy.txt" 2>&1
sha256sum "${files[@]}" > "$out/source-after.sha256"
cmp "$out/source-before.sha256" "$out/source-after.sha256"
