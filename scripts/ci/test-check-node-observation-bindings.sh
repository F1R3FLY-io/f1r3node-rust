#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DRIVER="$ROOT/scripts/ci/check-node-observation-bindings.sh"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cat > "$WORK/fixture.rs" <<'RS'
extern "C" fn initialize() {}
#[used]
#[unsafe(link_section = ".preinit_array")]
static BEFORE: extern "C" fn() = initialize;
#[used]
#[unsafe(link_section = ".init_array")]
static INITIALIZE: extern "C" fn() = initialize;
#[used]
#[unsafe(link_section = ".fini_array")]
static FINALIZE: extern "C" fn() = initialize;
fn main() {}
RS
rustc --edition 2021 -g "$WORK/fixture.rs" -o "$WORK/raw"
header() {
    readelf --file-header "$1" | awk -F: -v key="$2" '$1 ~ key { sub(/^[[:space:]]+/, "", $2); split($2, fields, " "); print fields[1] }'
}
test "$(header "$WORK/raw" 'Class')" = ELF64
section_header() {
    local file=$1 section=$2 index start size
    index=$(readelf --wide --sections "$file" | awk -v name="$section" '
        /^[[:space:]]*\[[[:space:]]*[0-9]+\]/ {
            sub(/^[[:space:]]*\[[[:space:]]*/, "")
            sub(/\]/, "")
            if ($2 == name) print $1
        }')
    test -n "$index"
    start=$(header "$file" 'Start of section headers')
    size=$(header "$file" 'Size of section headers')
    printf '%s\n' "$((start + index * size))"
}
zero_entry_size() {
    local offset
    offset=$(section_header "$1" "$2")
    printf '\0\0\0\0\0\0\0\0' | dd of="$1" bs=1 seek="$((offset + 56))" conv=notrunc status=none
}
flip_byte() {
    local value octal
    value=$(od -An -tu1 -j "$2" -N 1 "$1")
    printf -v octal '\\%03o' "$((value ^ 1))"
    printf '%b' "$octal" | dd of="$1" bs=1 seek="$2" conv=notrunc status=none
}
for section in .preinit_array .init_array .fini_array; do
    zero_entry_size "$WORK/raw" "$section"
done
objcopy --strip-debug "$WORK/raw" "$WORK/derived"
for file in raw derived; do
    readelf --wide --sections "$WORK/$file" | awk '/^[[:space:]]*\[[[:space:]]*[0-9]+\]/ { sub(/^[[:space:]]*\[[[:space:]]*[0-9]+\][[:space:]]+/, ""); if ($7 ~ /A/) print }' > "$WORK/$file-original.txt"
done
if cmp -s "$WORK/raw-original.txt" "$WORK/derived-original.txt"; then
    printf 'The fixture did not reproduce the original entry-size mismatch.\n' >&2
    exit 1
fi
bash "$DRIVER" --check-strip "$WORK/raw" "$WORK/derived" "$WORK/accepted"
"$WORK/raw"
"$WORK/derived"
count=0
reject() {
    local label=$1 result
    set +e
    bash "$DRIVER" --check-strip "$WORK/raw" "$WORK/mutant" "$WORK/$label" > "$WORK/rejection.log" 2>&1
    result=$?
    set -e
    if [[ "$result" != 1 ]]; then
        printf 'Expected exit 1 for %s, observed %s\n' "$label" "$result" >&2
        cat "$WORK/rejection.log" >&2
        exit 1
    fi
    count=$((count + 1))
}
for section in .text .preinit_array .init_array .fini_array; do
    cp "$WORK/derived" "$WORK/mutant"
    offset=$(readelf --wide --sections "$WORK/mutant" | awk -v name="$section" '/^[[:space:]]*\[[[:space:]]*[0-9]+\]/ { sub(/^[[:space:]]*\[[[:space:]]*[0-9]+\][[:space:]]+/, ""); if ($1 == name) print $4 }')
    test -n "$offset"
    flip_byte "$WORK/mutant" "$((16#$offset))"
    reject "bytes${section}"
done
for field in 'flags 8' 'size 32' 'alignment 48' 'entry-size 56'; do
    read -r label offset <<< "$field"
    cp "$WORK/derived" "$WORK/mutant"
    base=$(section_header "$WORK/mutant" .init_array)
    flip_byte "$WORK/mutant" "$((base + offset))"
    reject "$label"
done
cp "$WORK/derived" "$WORK/mutant"
base=$(section_header "$WORK/mutant" .text)
flip_byte "$WORK/mutant" "$((base + 56))"
reject non-array-entry-size
cp "$WORK/derived" "$WORK/mutant"
base=$(header "$WORK/mutant" 'Start of program headers')
flip_byte "$WORK/mutant" "$((base + 4))"
reject program-flags
cp "$WORK/derived" "$WORK/mutant"
flip_byte "$WORK/mutant" 24
reject entry-point
cp "$WORK/derived" "$WORK/mutant"
base=$(section_header "$WORK/mutant" .bss)
flip_byte "$WORK/mutant" "$((base + 32))"
reject nobits-size
printf 'Strip equivalence: entry-size normalization passed; %s executable mutations rejected.\n' "$count"
