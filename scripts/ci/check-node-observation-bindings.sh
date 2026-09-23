#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if (($# != 1)); then
    printf 'Usage: %s NEW_OUTPUT_DIRECTORY\n' "$0" >&2
    exit 2
fi
mkdir -m 700 -- "$1"
OUTPUT="$(cd "$1" && pwd)"
record_exit() {
    local result=$?
    printf '%s\n' "$result" > "$OUTPUT/result.exit"
}
trap record_exit EXIT
cd "$ROOT"
sha256sum Cargo.lock rust-toolchain.toml .cargo/config.toml node/Cargo.toml \
    node/src/rust/soak_observer.rs node/tests/soak_observer.rs \
    scripts/ci/check-node-observation-bindings.sh \
    docs/claims/casper-node-observation.md docs/claims/casper-node-authority-snapshot.md \
    formal/tlaplus/node_observation/*.tla formal/tlaplus/node_observation/*.cfg \
    formal/tlaplus/node_observation/*.json formal/tlaplus/node_observation/README.md \
    formal/rocq/node_observation/_CoqProject formal/rocq/node_observation/README.md \
    formal/rocq/node_observation/theories/*.v > "$OUTPUT/inputs.sha256"
rustc -vV > "$OUTPUT/compiler.txt"
cargo test --locked -p node --lib --test soak_observer --no-run --message-format=json \
    > "$OUTPUT/build.jsonl" 2> "$OUTPUT/build.log"
jq -r \
    'select(.reason=="compiler-artifact" and .profile.test==true and .executable!=null) | .executable' \
    "$OUTPUT/build.jsonl" | sort -u > "$OUTPUT/executables.txt"
mapfile -t executables < "$OUTPUT/executables.txt"
test "${#executables[@]}" = 2
allocated() {
    readelf --wide --sections "$1" | awk '/^[[:space:]]*\[[[:space:]]*[0-9]+\]/ { sub(/^[[:space:]]*\[[[:space:]]*[0-9]+\][[:space:]]+/, ""); if ($7 ~ /A/) print }'
}
section_hashes() {
    local file=$1 table=$2 name kind rest digest
    while read -r name kind rest; do
        if [[ "$kind" == NOBITS ]]; then
            printf 'NOBITS %s\n' "$name"
        else
            digest=$(readelf --hex-dump="$name" "$file" | sha256sum)
            printf '%s %s\n' "${digest%% *}" "$name"
        fi
    done < "$table"
}
for raw in "${executables[@]}"; do
    name=$(basename "$raw")
    sha256sum "$raw" >> "$OUTPUT/raw-executables.sha256"
    executable="$raw"
    if [[ "$name" == soak_observer-* ]]; then
        executable="$OUTPUT/soak-observer-debug-stripped"
        objcopy --strip-debug "$raw" "$executable"
        for kind in raw derived; do
            file="$raw"
            [[ "$kind" != derived ]] || file="$executable"
            allocated "$file" > "$OUTPUT/$kind-allocated.txt"
            readelf --wide --segments "$file" > "$OUTPUT/$kind-segments.txt"
            section_hashes "$file" "$OUTPUT/$kind-allocated.txt" > "$OUTPUT/$kind-sections.sha256"
        done
        for kind in allocated.txt segments.txt sections.sha256; do
            cmp "$OUTPUT/raw-$kind" "$OUTPUT/derived-$kind"
        done
        test "$(stat -c %s "$executable")" -le 536870912
        "$executable" --list > "$OUTPUT/$name.list"
        grep -Fx 'session_sequences_match_an_independent_event_oracle: test' "$OUTPUT/$name.list"
    else
        "$executable" --list > "$OUTPUT/$name.list"
        grep -Fx 'rust::soak_observer::linux::tests::repeated_entropy_cannot_replay_a_request: test' "$OUTPUT/$name.list"
        grep -Fx 'rust::soak_observer::linux::tests::exhausted_sequence_refuses_a_handshake: test' "$OUTPUT/$name.list"
    fi
    sha256sum "$executable" >> "$OUTPUT/executed.sha256"
    timeout --signal=TERM --kill-after=10 300 "$executable" --test-threads=2 > "$OUTPUT/$name.log" 2>&1
done
sha256sum -c "$OUTPUT/inputs.sha256" > "$OUTPUT/inputs-after.log"
sha256sum -c "$OUTPUT/raw-executables.sha256" > "$OUTPUT/raw-after.log"
sha256sum -c "$OUTPUT/executed.sha256" > "$OUTPUT/executed-after.log"
printf 'Node observation binding tests passed. Claim acceptance remains separate.\n'
