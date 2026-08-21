#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="$REPO_ROOT/target/llvm-cov"
SUMMARY="$OUT_DIR/cost-accounted-rho-branch-summary.tsv"
COVERAGE_SCRATCH="$OUT_DIR/scratch"
COVERAGE_JOBS="${COVERAGE_JOBS:-2}"
COVERAGE_REUSE_PROFILES="${COVERAGE_REUSE_PROFILES:-0}"
RUST_HOST="$(rustc -vV | awk '/^host:/ { print $2 }')"
LLVM_TOOLS="$(rustc --print sysroot)/lib/rustlib/$RUST_HOST/bin"
LLVM_COV="$LLVM_TOOLS/llvm-cov"
LLVM_PROFDATA="$LLVM_TOOLS/llvm-profdata"
LLVM_READOBJ="$LLVM_TOOLS/llvm-readobj"

command -v cargo >/dev/null
cargo llvm-cov --version >/dev/null
test -x "$LLVM_COV"
test -x "$LLVM_PROFDATA"
test -x "$LLVM_READOBJ"
if [[ "$COVERAGE_REUSE_PROFILES" != 0 && "$COVERAGE_REUSE_PROFILES" != 1 ]]; then
    printf 'COVERAGE_REUSE_PROFILES must be 0 or 1\n' >&2
    exit 1
fi
mkdir -p "$OUT_DIR"
rm -rf "$COVERAGE_SCRATCH"
mkdir -p "$COVERAGE_SCRATCH"
export TMPDIR="$COVERAGE_SCRATCH"
ulimit -c 0

cleanup() {
    rm -f "$OUT_DIR"/.coverage-export-*.lcov "$OUT_DIR"/.coverage-export-*.stderr
    rm -rf "$COVERAGE_SCRATCH"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

critical_files=(
    crypto:crypto/src/rust/signatures/signed.rs
    models:models/src/rust/casper/protocol/casper_message.rs
    rholang:rholang/src/rust/interpreter/accounting/mod.rs
    rholang:rholang/src/rust/interpreter/accounting/authority.rs
    rholang:rholang/src/rust/interpreter/accounting/byte_accounting.rs
    rholang:rholang/src/rust/interpreter/accounting/delta_sigma.rs
    rholang:rholang/src/rust/interpreter/accounting/resource_logic.rs
    rholang:rholang/src/rust/interpreter/interpreter.rs
    rholang:rholang/src/rust/interpreter/metering.rs
    rholang:rholang/src/rust/interpreter/reduce.rs
    rholang:rholang/src/rust/interpreter/rho_runtime.rs
    casper:casper/src/rust/casper.rs
    casper:casper/src/rust/engine/engine.rs
    casper:casper/src/rust/engine/lfs_block_requester.rs
    casper:casper/src/rust/engine/multi_parent_casper/validation_dispatcher.rs
    casper:casper/src/rust/merging/conflict_set_merger.rs
    casper:casper/src/rust/rholang/replay_runtime.rs
    casper:casper/src/rust/rholang/runtime.rs
    casper:casper/src/rust/util/construct_deploy.rs
    casper:casper/src/rust/util/mergeable_channels_gc.rs
    casper:casper/src/rust/util/rholang/acceptance.rs
    casper:casper/src/rust/util/rholang/interpreter_util.rs
    casper:casper/src/rust/util/rholang/supply.rs
    casper:casper/src/rust/util/rholang/runtime_manager.rs
    casper:casper/src/rust/merging/deploy_chain_index.rs
)

stable_coverage_files=(
    casper:casper/src/rust/engine/initializing.rs
    casper:casper/src/rust/engine/running.rs
    rspace_plus_plus:rspace++/src/rspace/replay_rspace.rs
    rspace_plus_plus:rspace++/src/rspace/reporting_rspace.rs
    rspace_plus_plus:rspace++/src/rspace/rspace.rs
    rspace_plus_plus:rspace++/src/rspace/merger/state_change_merger.rs
)

compile_only_files=(
    rspace_plus_plus:rspace++/src/rspace/rspace_interface.rs
)

all_coverage_files=("${critical_files[@]}" "${stable_coverage_files[@]}" "${compile_only_files[@]}")
for entry in "${all_coverage_files[@]}"; do
    file="${entry#*:}"
    absolute_file="$REPO_ROOT/$file"
    if [[ ! -f "$absolute_file" ]]; then
        printf 'missing consensus-critical coverage source: %s\n' "$file" >&2
        exit 1
    fi
    output_name="${file//\//_}"
    : >"$OUT_DIR/${entry%%:*}-$output_name.lcov"
done

packages=(crypto models rholang casper rspace_plus_plus)
export_sequence=0
for package in "${packages[@]}"; do
    package_target="$REPO_ROOT/target/llvm-cov-isolated/$package"
    coverage_target="$package_target/llvm-cov-target"
    if [[ "$COVERAGE_REUSE_PROFILES" == 0 ]]; then
        CARGO_TARGET_DIR="$package_target" cargo llvm-cov clean --workspace
        CARGO_TARGET_DIR="$package_target" cargo llvm-cov nextest \
            --branch \
            --package "$package" \
            --no-fail-fast \
            --release \
            --no-report \
            --jobs "$COVERAGE_JOBS" \
            --status-level fail \
            --final-status-level fail \
            -- \
            --skip schnorr_secp256k1_experimental
    fi

    mapfile -d '' -t raw_profiles < <(
        find "$coverage_target" -maxdepth 1 -type f -name '*.profraw' -print0
    )
    if [[ "${#raw_profiles[@]}" -eq 0 ]]; then
        printf 'missing raw coverage profiles for %s\n' "$package" >&2
        exit 1
    fi

    object_map="$package_target/coverage-build-ids.tsv"
    : >"$object_map"
    while IFS= read -r -d '' object; do
        build_id="$("$LLVM_READOBJ" --notes "$object" 2>/dev/null \
            | awk '/Build ID:/ { print $3; exit }')"
        if [[ -n "$build_id" ]]; then
            printf '%s\t%s\n' "$build_id" "$object" >>"$object_map"
        fi
    done < <(
        find "$coverage_target/release/deps" -maxdepth 1 -type f \
            \( -perm -u+x -o -name '*.so' \) -print0
    )

    profile_signatures=()
    for raw_profile in "${raw_profiles[@]}"; do
        profile_name="${raw_profile##*/}"
        module_and_pool="${profile_name##*-}"
        module_signature="${module_and_pool%_*}"
        if [[ ! "$module_signature" =~ ^[0-9]+$ ]]; then
            printf 'unrecognized %%m profile filename for %s: %s\n' \
                "$package" "$profile_name" >&2
            exit 1
        fi
        profile_signatures+=("$module_signature")
    done
    mapfile -t unique_signatures < <(printf '%s\n' "${profile_signatures[@]}" | sort -u)
    profile_dir="$coverage_target/module-profiles"
    mkdir -p "$profile_dir"
    matched_objects=0
    unmatched_profiles=0
    failed_exports=0
    package_entries=()
    package_sources=()
    for entry in "${critical_files[@]}"; do
        if [[ "${entry%%:*}" == "$package" ]]; then
            package_entries+=("$entry")
            package_sources+=("$REPO_ROOT/${entry#*:}")
        fi
    done
    for module_signature in "${unique_signatures[@]}"; do
        module_profiles=()
        representative=""
        for index in "${!raw_profiles[@]}"; do
            if [[ "${profile_signatures[$index]}" == "$module_signature" ]]; then
                module_profiles+=("${raw_profiles[$index]}")
                representative="${raw_profiles[$index]}"
            fi
        done
        mapfile -t binary_ids < <(
            "$LLVM_PROFDATA" show --binary-ids "$representative" \
                | awk '/^[0-9a-f]{40}$/ { print }'
        )
        if [[ "${#binary_ids[@]}" -ne 1 ]]; then
            printf 'profile module %s for %s has %s binary IDs\n' \
                "$module_signature" "$package" "${#binary_ids[@]}" >&2
            exit 1
        fi
        object="$(awk -F '\t' -v id="${binary_ids[0]}" \
            '$1 == id { print $2; exit }' "$object_map")"
        if [[ -z "$object" ]]; then
            unmatched_profiles=$((unmatched_profiles + 1))
            continue
        fi

        profile="$profile_dir/$module_signature.profdata"
        "$LLVM_PROFDATA" merge "${module_profiles[@]}" -o "$profile"
        matched_objects=$((matched_objects + 1))
        export_sequence=$((export_sequence + 1))
        candidate="$OUT_DIR/.coverage-export-$export_sequence.lcov"
        diagnostics="$OUT_DIR/.coverage-export-$export_sequence.stderr"
        if perl -e '
            system @ARGV;
            exit 127 if $? == -1;
            exit 128 + ($? & 127) if $? & 127;
            exit $? >> 8;
        ' timeout 300s "$LLVM_COV" export \
                -format=lcov \
                -skip-functions \
                -instr-profile="$profile" \
                "$object" \
                --sources "${package_sources[@]}" \
                >"$candidate" 2>"$diagnostics"; then
            for entry in "${package_entries[@]}"; do
                file="${entry#*:}"
                absolute_file="$REPO_ROOT/$file"
                output_name="${file//\//_}"
                lcov="$OUT_DIR/$package-$output_name.lcov"
                awk -v source="SF:$absolute_file" '
                    /^SF:/ { keep = ($0 == source) }
                    keep { print }
                ' "$candidate" >>"$lcov"
            done
        else
            failed_exports=$((failed_exports + 1))
        fi
        rm -f "$candidate" "$diagnostics"
    done
    if [[ "$matched_objects" -eq 0 ]]; then
        printf 'no build-ID-matched coverage objects for %s\n' "$package" >&2
        exit 1
    fi
    printf 'Coverage object matching for %s: %s matched, %s transient build profiles ignored, %s LLVM export failures contained\n' \
        "$package" "$matched_objects" "$unmatched_profiles" "$failed_exports"
done

stable_target="$REPO_ROOT/target/llvm-cov-isolated/casper-engine-stable"
if [[ "$COVERAGE_REUSE_PROFILES" == 0 ]]; then
    CARGO_TARGET_DIR="$stable_target" cargo llvm-cov clean --workspace
    CARGO_TARGET_DIR="$stable_target" cargo llvm-cov nextest \
        --package casper \
        --no-fail-fast \
        --release \
        --no-report \
        --jobs "$COVERAGE_JOBS" \
        --status-level fail \
        --final-status-level fail \
        --test mod \
        -- engine::
fi
casper_stable_report="$OUT_DIR/casper-engine-stable.lcov"
CARGO_TARGET_DIR="$stable_target" cargo llvm-cov report \
    --package casper \
    --release \
    --lcov \
    --output-path "$casper_stable_report"

rspace_stable_target="$REPO_ROOT/target/llvm-cov-isolated/rspace-engine-stable"
if [[ "$COVERAGE_REUSE_PROFILES" == 0 ]]; then
    CARGO_TARGET_DIR="$rspace_stable_target" cargo llvm-cov clean --workspace
    CARGO_TARGET_DIR="$rspace_stable_target" cargo llvm-cov nextest \
        --package rspace_plus_plus \
        --no-fail-fast \
        --release \
        --no-report \
        --jobs "$COVERAGE_JOBS" \
        --status-level fail \
        --final-status-level fail
fi
rspace_stable_report="$OUT_DIR/rspace-engine-stable.lcov"
CARGO_TARGET_DIR="$rspace_stable_target" cargo llvm-cov report \
    --package rspace_plus_plus \
    --release \
    --lcov \
    --output-path "$rspace_stable_report"

declare -A stable_reports=(
    [casper]="$casper_stable_report"
    [rspace_plus_plus]="$rspace_stable_report"
)
for entry in "${stable_coverage_files[@]}"; do
    package="${entry%%:*}"
    file="${entry#*:}"
    absolute_file="$REPO_ROOT/$file"
    output_name="${file//\//_}"
    lcov="$OUT_DIR/$package-$output_name.lcov"
    if ! awk -v source="SF:$absolute_file" '
        /^SF:/ { keep = ($0 == source); if (keep) found = 1 }
        keep { print }
        END { if (!found) exit 1 }
    ' "${stable_reports[$package]}" >"$lcov"; then
        printf 'no exact stable coverage record for %s\n' "$file" >&2
        exit 1
    fi
done

printf 'package\tfile\tbranches\tcovered_branches\tbranch_percent\tlines\tcovered_lines\tline_percent\tmissed_branch_lines\tmissed_code_lines\n' >"$SUMMARY"
printf 'Cost-accounting source coverage:\n'
for entry in "${all_coverage_files[@]}"; do
    package="${entry%%:*}"
    file="${entry#*:}"
    absolute_file="$REPO_ROOT/$file"
    if [[ ! -f "$absolute_file" ]]; then
        printf 'missing consensus-critical coverage source: %s\n' "$file" >&2
        exit 1
    fi
    output_name="${file//\//_}"
    lcov="$OUT_DIR/$package-$output_name.lcov"
    compile_only=0
    for compile_only_entry in "${compile_only_files[@]}"; do
        if [[ "$entry" == "$compile_only_entry" ]]; then
            compile_only=1
            break
        fi
    done
    if [[ "$compile_only" == 1 ]]; then
        printf '  %-65s branches %6s / %-6s %8s; lines %6s / %-6s %8s\n' \
            "$file" 0 0 n/a 0 0 n/a
        printf '%s\t%s\t0\t0\tn/a\t0\t0\tn/a\tdeclaration-only\tdeclaration-only\n' \
            "$package" "$file" >>"$SUMMARY"
        continue
    fi
    if ! grep -Fqx "SF:$absolute_file" "$lcov"; then
        printf 'no exact coverage record for %s\n' "$file" >&2
        exit 1
    fi

    row="$(awk -F, '
        /^BRDA:/ {
            split($1, prefix, ":")
            key = prefix[2] SUBSEP $2 SUBSEP $3
            seen[key] = 1
            if ($4 != "-" && ($4 + 0) > 0) {
                branch_hit[key] = 1
            }
        }
        /^DA:/ {
            split($1, prefix, ":")
            line = prefix[2]
            line_seen[line] = 1
            if (($2 + 0) > 0) {
                line_hit[line] = 1
            }
        }
        END {
            for (key in seen) {
                branch_total++
                if (branch_hit[key]) branch_covered++
            }
            for (line in line_seen) {
                line_total++
                if (line_hit[line]) line_covered++
            }
            branch_percent = branch_total == 0 ? "n/a" : sprintf("%.2f%%", 100 * branch_covered / branch_total)
            line_percent = line_total == 0 ? "n/a" : sprintf("%.2f%%", 100 * line_covered / line_total)
            printf "%d\t%d\t%s\t%d\t%d\t%s\n", branch_total, branch_covered, branch_percent, line_total, line_covered, line_percent
        }
    ' "$lcov")"
    IFS=$'\t' read -r branch_count branch_covered branch_percent line_count line_covered line_percent <<<"$row"
    missed_branch_lines="$(awk -F, '
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
    missed_code_lines="$(awk -F, '
        /^DA:/ {
            split($1, prefix, ":")
            line = prefix[2]
            seen[line] = 1
            if (($2 + 0) > 0) hit[line] = 1
        }
        END {
            for (line in seen) if (!hit[line]) print line
        }
    ' "$lcov" | sort -n -u | paste -sd, -)"
    if [[ "$line_count" -eq 0 ]]; then
        printf 'missing line coverage record: %s\n' "$file" >&2
        exit 1
    fi
    printf '  %-65s branches %6s / %-6s %8s; lines %6s / %-6s %8s\n' \
        "$file" "$branch_covered" "$branch_count" "$branch_percent" \
        "$line_covered" "$line_count" "$line_percent"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$package" "$file" "$branch_count" "$branch_covered" "$branch_percent" \
        "$line_count" "$line_covered" "$line_percent" "$missed_branch_lines" \
        "$missed_code_lines" >>"$SUMMARY"
done

printf 'Source coverage summary: %s\n' "$SUMMARY"
