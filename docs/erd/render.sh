#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
JAR="${PLANTUML_JAR:-$ROOT/target/erd-tools/plantuml-1.2026.10.jar}"
if [[ ! -f "$JAR" ]]; then
    printf 'Set PLANTUML_JAR to PlantUML 1.2026.10. See docs/erd/README.md.\n' >&2
    exit 1
fi
printf '%s  %s\n' e9bc9e6b2eb715e8c0c929f3dcb5fc1923be13f37599b09c293c66d354e88686 "$JAR" | sha256sum --check --status
export CARGO_TARGET_DIR="$ROOT/target/erd-tools/build"
cargo run --locked --manifest-path docs/erd/Cargo.toml -- "$@"
cargo run --locked --manifest-path docs/erd/Cargo.toml -- --rank-sizes "$@"
cargo run --locked --manifest-path docs/erd/Cargo.toml --bin erd-leaf-stats -- \
    --report docs/erd/storage/leaves-observed.json docs/erd/storage/leaf-measurements.md "$@"
mapfile -t diagrams < <(find docs/erd/memory docs/erd/storage -maxdepth 1 -name '*.puml' -type f | sort)
if (( ${#diagrams[@]} == 0 )); then
    printf 'No diagrams found.\n' >&2
    exit 1
fi
if [[ " ${*} " == *" --check "* ]]; then
    java -Xmx1g -Djava.awt.headless=true -jar "$JAR" -checkonly -failfast2 "${diagrams[@]}"
else
    java -Xmx1g -Djava.awt.headless=true -jar "$JAR" -tsvg -nometadata -failfast2 -nbthread 2 "${diagrams[@]}"
fi
for diagram in "${diagrams[@]}"; do
    svg="${diagram%.puml}.svg"
    [[ -s "$svg" ]]
    grep -q '</svg>' "$svg"
    if grep -Eq 'Syntax Error|An error has occurred|java.lang.*Exception' "$svg"; then
        printf 'Render failed: %s\n' "$svg" >&2
        exit 1
    fi
done
if [[ " ${*} " == *" --check "* ]]; then
    sha256sum --check --status docs/erd/rendered.sha256
else
    sha256sum docs/erd/style.puml "${diagrams[@]}" "${diagrams[@]/%.puml/.svg}" > docs/erd/rendered.sha256
fi
cargo run --locked --manifest-path docs/erd/Cargo.toml -- --check-links
printf 'Validated %s rendered diagrams.\n' "${#diagrams[@]}"
