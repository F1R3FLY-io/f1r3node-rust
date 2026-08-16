#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="$REPO_ROOT/target/llvm-cov"
SUMMARY="$OUT_DIR/cost-accounted-rho-branch-summary.tsv"
COVERAGE_JOBS="${COVERAGE_JOBS:-8}"
RUST_HOST="$(rustc -vV | awk '/^host:/ { print $2 }')"
LLVM_TOOLS="$(rustc --print sysroot)/lib/rustlib/$RUST_HOST/bin"
LLVM_COV="$LLVM_TOOLS/llvm-cov"
LLVM_PROFDATA="$LLVM_TOOLS/llvm-profdata"

command -v cargo >/dev/null
test -x "$LLVM_COV"
test -x "$LLVM_PROFDATA"
cargo llvm-cov --version >/dev/null
mkdir -p "$OUT_DIR"

packages=(crypto models rholang casper rspace_plus_plus)
declare -A profiles
declare -A object_lists

for package in "${packages[@]}"; do
    package_target="$REPO_ROOT/target/llvm-cov-isolated/$package"
    CARGO_TARGET_DIR="$package_target" cargo llvm-cov clean --workspace
    CARGO_TARGET_DIR="$package_target" cargo llvm-cov nextest \
        --branch \
        --package "$package" \
        --no-fail-fast \
        --release \
        --no-report \
        --jobs "$COVERAGE_JOBS" \
        -- \
        --skip schnorr_secp256k1_experimental

    mapfile -t raw_profiles < <(
        find "$package_target/llvm-cov-target" -maxdepth 1 -type f -name '*.profraw' -print
    )
    if [[ "${#raw_profiles[@]}" -eq 0 ]]; then
        printf 'missing raw coverage profiles for %s\n' "$package" >&2
        exit 1
    fi
    profile="$package_target/llvm-cov-target/$package.profdata"
    "$LLVM_PROFDATA" merge -sparse "${raw_profiles[@]}" -o "$profile"

    object_list="$package_target/coverage-objects.txt"
    find "$package_target/llvm-cov-target/release/deps" \
        -maxdepth 1 -type f -perm -u+x ! -name '*.so' -print | sort >"$object_list"
    if [[ ! -s "$object_list" ]]; then
        printf 'missing coverage objects for %s\n' "$package" >&2
        exit 1
    fi
    profiles[$package]="$profile"
    object_lists[$package]="$object_list"
done

critical_files=(
    crypto:crypto/src/rust/signatures/signed.rs
    models:models/src/rust/casper/protocol/casper_message.rs
    rholang:rholang/src/rust/interpreter/accounting/mod.rs
    rholang:rholang/src/rust/interpreter/accounting/authority.rs
    rholang:rholang/src/rust/interpreter/accounting/delta_sigma.rs
    rholang:rholang/src/rust/interpreter/accounting/resource_logic.rs
    casper:casper/src/rust/util/rholang/acceptance.rs
    casper:casper/src/rust/util/rholang/supply.rs
    casper:casper/src/rust/util/rholang/runtime_manager.rs
    casper:casper/src/rust/merging/deploy_chain_index.rs
    rspace_plus_plus:rspace++/src/rspace/merger/state_change_merger.rs
)

printf 'package\tfile\tbranches\tcovered\tpercent\tmissed_lines\n' >"$SUMMARY"
printf 'Cost-accounting branch-outcome coverage:\n'
for entry in "${critical_files[@]}"; do
    package="${entry%%:*}"
    file="${entry#*:}"
    absolute_file="$REPO_ROOT/$file"
    lcov="$OUT_DIR/$package-$(basename "$file").lcov"
    : >"$lcov"
    exported_objects=0
    unsupported_objects=0
    while IFS= read -r object; do
        if bash -c '
            "$1" export \
                -format=lcov \
                -instr-profile="$2" \
                "$3" \
                --sources "$4"
        ' _ "$LLVM_COV" "${profiles[$package]}" "$object" "$absolute_file" \
            >>"$lcov" 2>/dev/null; then
            exported_objects=$((exported_objects + 1))
        else
            unsupported_objects=$((unsupported_objects + 1))
        fi
    done <"${object_lists[$package]}"
    if [[ "$exported_objects" -eq 0 ]]; then
        printf 'no coverage-compatible objects for %s (%s unsupported)\n' \
            "$file" "$unsupported_objects" >&2
        exit 1
    fi

    row="$(awk -F, '
        /^BRDA:/ {
            split($1, prefix, ":")
            key = prefix[2] SUBSEP $2 SUBSEP $3
            seen[key] = 1
            if ($4 != "-" && ($4 + 0) > 0) {
                hit[key] = 1
            }
        }
        END {
            for (key in seen) {
                total++
                split(key, parts, SUBSEP)
                if (hit[key]) {
                    covered++
                } else {
                    missed++
                }
            }
            percent = total == 0 ? 0 : (100 * covered / total)
            printf "%d\t%d\t%.2f%%\t%d\n", total, covered, percent, missed
        }
    ' "$lcov")"
    IFS=$'\t' read -r count covered percent missed <<<"$row"
    missed_lines="$(awk -F, '
        /^BRDA:/ {
            split($1, prefix, ":")
            key = prefix[2] SUBSEP $2 SUBSEP $3
            seen[key] = 1
            if ($4 != "-" && ($4 + 0) > 0) {
                hit[key] = 1
            }
        }
        END {
            for (key in seen) {
                if (!hit[key]) {
                    split(key, parts, SUBSEP)
                    print parts[1]
                }
            }
        }
    ' "$lcov" | sort -n -u | paste -sd, -)"
    if [[ "$count" -eq 0 ]]; then
        printf 'missing branch coverage record: %s\n' "$file" >&2
        exit 1
    fi
    printf '  %-65s %6s / %-6s %8s (%s missed)\n' \
        "$file" "$covered" "$count" "$percent" "$missed"
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$package" "$file" "$count" "$covered" "$percent" "$missed_lines" >>"$SUMMARY"
done

printf 'Branch-outcome summary: %s\n' "$SUMMARY"
