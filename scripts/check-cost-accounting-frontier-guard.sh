#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REDUCE_RS="$ROOT_DIR/rholang/src/rust/interpreter/reduce.rs"

ZERO_CAPABLE_COSTS='hex_to_bytes_cost|bytes_to_hex_cost|diff_cost|union_cost|byte_array_append_cost|list_append_cost|string_append_cost|interpolate_cost|to_byte_array_cost|size_method_cost|slice_cost|take_cost|to_list_cost'

if rg -U -n "reserve_primitive\s*\([^;]*(${ZERO_CAPABLE_COSTS})" "$REDUCE_RS"; then
    echo "zero-capable primitive work must use reserve_incremental_primitive" >&2
    exit 1
fi

if rg -U -n "reserve_substitution\s*\([^;]*Cost::create\s*\(\s*0" \
    "$ROOT_DIR/rholang/src/rust/interpreter"; then
    echo "standalone substitution billing must not emit zero-weight billable events" >&2
    exit 1
fi

if rg -U -n "reserve_source_step\s*\([^;]*Cost::create\s*\(\s*0" \
    "$ROOT_DIR/rholang/src/rust/interpreter"; then
    echo "source-step billing must not emit zero-weight billable events" >&2
    exit 1
fi

echo "cost-accounting frontier guard passed"
