#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C

allocated() {
    local file=$1 width
    width=$(readelf --file-header "$file" | awk '$1 == "Class:" { print $2 }')
    case "$width" in
        ELF32) width=04 ;;
        ELF64) width=08 ;;
        *) printf 'Unsupported ELF class: %s\n' "$width" >&2; return 1 ;;
    esac
    readelf --wide --sections "$file" | awk -v width="$width" '
        /^[[:space:]]*\[[[:space:]]*[0-9]+\]/ {
            sub(/^[[:space:]]*\[[[:space:]]*[0-9]+\][[:space:]]+/, "")
            if ($7 !~ /A/) next
            if ($2 ~ /^(PREINIT_ARRAY|INIT_ARRAY|FINI_ARRAY)$/) {
                if ($6 != "00" && $6 != width) exit 1
                $6 = width
            }
            $1 = $1
            print
            count++
        }
        END { if (!count) exit 1 }
    '
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
check_strip_equivalence() {
    local raw=$1 executable=$2 kind file
    for kind in raw derived; do
        file="$raw"
        [[ "$kind" != derived ]] || file="$executable"
        readelf --wide --sections "$file" > "$OUTPUT/$kind-section-headers.txt"
        allocated "$file" > "$OUTPUT/$kind-allocated.txt"
        readelf --wide --segments "$file" > "$OUTPUT/$kind-segments.txt"
        section_hashes "$file" "$OUTPUT/$kind-allocated.txt" > "$OUTPUT/$kind-sections.sha256"
    done
    for kind in allocated.txt segments.txt sections.sha256; do
        cmp "$OUTPUT/raw-$kind" "$OUTPUT/derived-$kind"
    done
}
if [[ ${1:-} == --check-strip ]]; then
    if (($# != 4)); then
        printf 'Usage: %s --check-strip RAW DERIVED NEW_OUTPUT_DIRECTORY\n' "$0" >&2
        exit 2
    fi
    mkdir -m 700 -- "$4"
    OUTPUT="$(cd "$4" && pwd)"
    check_strip_equivalence "$2" "$3"
    exit 0
fi

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
find node shared block-storage casper comm crypto models rspace++ rholang rho-pure-eval graphz \
    -type f \( -name '*.rs' -o -name '*.proto' -o -name Cargo.toml \) -print0 |
    sort -z | xargs -0 sha256sum > "$OUTPUT/inputs.sha256"
sha256sum Cargo.toml Cargo.lock rust-toolchain.toml .cargo/config.toml \
    scripts/ci/check-node-observation-bindings.sh scripts/ci/check-node-canonical-wire.sh \
    scripts/ci/test-check-node-observation-bindings.sh \
    docs/claims/casper-node-observation.md docs/claims/casper-node-authority-snapshot.md \
    formal/tlaplus/node_observation/*.tla formal/tlaplus/node_observation/*.cfg \
    formal/tlaplus/node_observation/*.json formal/tlaplus/node_observation/README.md \
    formal/rocq/node_observation/_CoqProject formal/rocq/node_observation/README.md \
    formal/rocq/node_observation/theories/*.v \
    formal/rocq/node_observation/b11/_CoqProject formal/rocq/node_observation/b11/README.md \
    formal/rocq/node_observation/b11/theories/*.v >> "$OUTPUT/inputs.sha256"
rustc -vV > "$OUTPUT/compiler.txt"
build() {
    local package=$1
    shift
    cargo test --locked -p "$package" "$@" --no-run --message-format=json \
        > "$OUTPUT/$package-build.jsonl" 2> "$OUTPUT/$package-build.log"
    jq -r --arg package "$package" \
        'select(.reason=="compiler-artifact" and .profile.test==true and .executable!=null) | [$package, .target.name, .executable] | @tsv' \
        "$OUTPUT/$package-build.jsonl" >> "$OUTPUT/executables.tsv"
}
build node --lib --test soak_observer
build shared --test soak_snapshot
build block-storage --lib --test soak_snapshot
test "$(wc -l < "$OUTPUT/executables.tsv")" -eq 5
while IFS=$'\t' read -r package target raw; do
    args=(--test-threads=2)
    name=$(basename "$raw")
    sha256sum "$raw" >> "$OUTPUT/raw-executables.sha256"
    executable="$raw"
    if [[ "$name" == soak_observer-* ]]; then
        executable="$OUTPUT/soak-observer-debug-stripped"
        objcopy --strip-debug "$raw" "$executable"
        check_strip_equivalence "$raw" "$executable"
        test "$(stat -c %s "$executable")" -le 536870912
        "$executable" --list > "$OUTPUT/$name.list"
        grep -Fx 'session_sequences_match_an_independent_event_oracle: test' "$OUTPUT/$name.list"
    elif [[ "$package" == node ]]; then
        "$executable" --list > "$OUTPUT/$name.list"
        grep -Fx 'rust::soak_observer::linux::tests::repeated_entropy_cannot_replay_a_request: test' "$OUTPUT/$name.list"
        grep -Fx 'rust::soak_observer::linux::tests::exhausted_sequence_refuses_a_handshake: test' "$OUTPUT/$name.list"
    else
        "$executable" --list > "$OUTPUT/$name.list"
        if [[ "$target" == block_storage ]]; then
            grep -Fx 'rust::dag::soak_snapshot::tests::generation_change_after_validation_rejects_capture: test' "$OUTPUT/$name.list"
            grep -Fx 'rust::dag::soak_snapshot::tests::all_capture_guards_use_the_supplied_deadline: test' "$OUTPUT/$name.list"
            args+=(rust::dag::soak_snapshot::)
        elif [[ "$package" == block-storage ]]; then
            grep -Fx 'capture_outcomes_match_the_bounded_capture_oracle: test' "$OUTPUT/$name.list"
        fi
    fi
    sha256sum "$executable" >> "$OUTPUT/executed.sha256"
    timeout --signal=TERM --kill-after=10 300 "$executable" "${args[@]}" > "$OUTPUT/$name.log" 2>&1
done < "$OUTPUT/executables.tsv"
jq -r '.claims[].properties[].tests[].name' formal/tlaplus/node_observation/bindings.json |
    sort -u > "$OUTPUT/required-tests.txt"
while IFS= read -r required; do
    grep -hFx "test $required ... ok" "$OUTPUT"/*.log > /dev/null
done < "$OUTPUT/required-tests.txt"
bash scripts/ci/check-node-canonical-wire.sh export "$OUTPUT/canonical-wire"
sha256sum -c "$OUTPUT/inputs.sha256" > "$OUTPUT/inputs-after.log"
sha256sum -c "$OUTPUT/raw-executables.sha256" > "$OUTPUT/raw-after.log"
sha256sum -c "$OUTPUT/executed.sha256" > "$OUTPUT/executed-after.log"
printf 'Node observation binding tests passed. Claim acceptance remains separate.\n'
