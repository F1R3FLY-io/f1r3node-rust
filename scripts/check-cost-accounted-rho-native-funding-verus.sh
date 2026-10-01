#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENTRY_DIR="formal/verus/cost_accounting"
NATIVE_DIR="rholang/src/rust/interpreter/accounting/monetary_allocation"
WORK_ROOT="$REPO_ROOT/target/verification/cost-accounted-rho/native-funding-verus"

for required in verus jq timeout rg; do
  command -v "$required" >/dev/null || { echo "Required tool is unavailable: $required" >&2; exit 1; }
done
mkdir -p "$WORK_ROOT"
RUN_ROOT="$(mktemp -d "$WORK_ROOT/run.XXXXXXXX")"
trap 'rm -rf -- "$RUN_ROOT"' EXIT

verify_source() {
  local source="$1" name="$2"
  timeout 300 verus --crate-name "$crate" --no-cheating --output-json \
    "$source" > "$WORK_ROOT/$name.json" 2> "$WORK_ROOT/$name.stderr"
}

check_report() {
  local name="$1" verified="$2" errors="$3"
  jq -e --argjson verified "$verified" --argjson errors "$errors" --arg function "$function" '
    .["verification-results"] as $result |
    $result.verified == $verified and
    $result.errors == $errors and
    $result["encountered-vir-error"] == false and
    $result["is-verifying-entire-crate"] == true and
    (.["func-details"] | has($function))
  ' "$WORK_ROOT/$name.json" >/dev/null
}

sha256sum "$REPO_ROOT/$NATIVE_DIR/funding_arithmetic.rs" "$REPO_ROOT/$ENTRY_DIR/funding_arithmetic.rs" \
  "$REPO_ROOT/$NATIVE_DIR/funding_graph.rs" "$REPO_ROOT/$ENTRY_DIR/funding_graph.rs" \
  "$REPO_ROOT/$NATIVE_DIR/funding_augmentation.rs" "$REPO_ROOT/$ENTRY_DIR/funding_augmentation.rs" \
  "$REPO_ROOT/$NATIVE_DIR/funding_discovery.rs" "$REPO_ROOT/$ENTRY_DIR/funding_discovery.rs" \
  "$REPO_ROOT/$NATIVE_DIR/funding_adjacency.rs" "$REPO_ROOT/$ENTRY_DIR/funding_adjacency.rs" > "$WORK_ROOT/sources.sha256"
mkdir -p "$RUN_ROOT/$ENTRY_DIR" "$RUN_ROOT/$NATIVE_DIR"
for kernel in arithmetic graph augmentation discovery adjacency; do
  crate="funding_$kernel"
  entry="$ENTRY_DIR/$crate.rs"
  native="$NATIVE_DIR/$crate.rs"
  if [ "$kernel" = arithmetic ]; then
    function="$crate::$crate::checked_residual_transfer"
    mutations=(missing_forward_debit missing_reverse_credit false_rejection)
    verified_count=1
  elif [ "$kernel" = graph ]; then
    function="$crate::$crate::add_edge"
    mutations=(incorrect_reverse_endpoint nonzero_initial_reverse missing_reverse_head cyclic_forward_adjacency)
    verified_count=7
  elif [ "$kernel" = augmentation ]; then
    function="$crate::$crate::augment_edge"
    mutations=(premature_capacity_mutation omitted_pair_credit wrong_predecessor)
    verified_count=11
  elif [ "$kernel" = discovery ]; then
    function="$crate::$crate::discover_parent"
    mutations=(overwrite_existing_parent enqueue_zero_capacity omit_queue_push)
    verified_count=10
  else
    function="$crate::$crate::advance_adjacency"
    mutations=(unadvanced_adjacency returned_successor sentinel_as_edge)
    verified_count=8
  fi
  verify_source "$REPO_ROOT/$entry" "$kernel-native"
  check_report "$kernel-native" "$verified_count" 0
  jq -e '.["verification-results"]["encountered-error"] == false' "$WORK_ROOT/$kernel-native.json" >/dev/null
  cp "$REPO_ROOT/$entry" "$RUN_ROOT/$entry"
  for dependency in arithmetic graph augmentation discovery adjacency; do
    cp "$REPO_ROOT/$NATIVE_DIR/funding_$dependency.rs" "$RUN_ROOT/$NATIVE_DIR/funding_$dependency.rs"
  done
  for mutation in "${mutations[@]}"; do
    case "$mutation" in
      missing_forward_debit) expression='s/forward\.checked_sub(amount)?/forward/' ;;
      missing_reverse_credit) expression='s/reverse\.checked_add(amount)?/reverse/' ;;
      false_rejection) expression='s/    Some((forward\.checked_sub(amount)?, reverse\.checked_add(amount)?))/    None/' ;;
      incorrect_reverse_endpoint) expression='s/to: from,/to: to,/' ;;
      nonzero_initial_reverse) expression='s/capacity: 0,/capacity: 1,/' ;;
      missing_reverse_head) expression='/    heads\[to\] = forward + 1;/d' ;;
      cyclic_forward_adjacency) expression='s/next: heads\[from\],/next: forward,/' ;;
      premature_capacity_mutation) expression='/    let (forward, reverse) =/i\    edges[index].capacity = 0;' ;;
      omitted_pair_credit) expression='/    edges\[index \^ 1\].capacity = reverse;/d' ;;
      wrong_predecessor) expression='s/Some(edges\[index \^ 1\].to)/Some(edges[index].to)/' ;;
      overwrite_existing_parent) expression='s/if edge.capacity != 0 \&\& parents\[edge.to\] == usize::MAX {/if edge.capacity != 0 {/' ;;
      enqueue_zero_capacity) expression='s/if edge.capacity != 0 \&\& parents\[edge.to\] == usize::MAX {/if parents[edge.to] == usize::MAX {/' ;;
      omit_queue_push) expression='/        queue.push(edge.to);/d' ;;
      unadvanced_adjacency) expression='/    \*cursor = edges\[current\].next;/d' ;;
      returned_successor) expression='s/    Some(current)/    Some(*cursor)/' ;;
      sentinel_as_edge) expression='s/        return None;/        return Some(0);/' ;;
    esac
    sed "$expression" "$REPO_ROOT/$native" > "$RUN_ROOT/$native"
    if cmp -s "$REPO_ROOT/$native" "$RUN_ROOT/$native"; then
      echo "Mutation did not change the native body: $mutation" >&2
      exit 1
    fi
    if verify_source "$RUN_ROOT/$entry" "$mutation"; then
      echo "Verifier accepted an incorrect native body: $mutation" >&2
      exit 1
    else
      verification_status=$?
      if [ "$verification_status" -ne 1 ]; then
        echo "Verifier failed outside the expected proof check: $mutation (status $verification_status)" >&2
        exit 1
      fi
    fi
    check_report "$mutation" "$((verified_count - 1))" 1
    if [ "$mutation" = overwrite_existing_parent ]; then
      rg -q 'precondition not satisfied' "$WORK_ROOT/$mutation.stderr"
      rg -q -F '!queue.contains(fresh)' "$WORK_ROOT/$mutation.stderr"
      rg -q -F 'queue.no_duplicates()' "$WORK_ROOT/$mutation.stderr"
    else
      rg -q 'postcondition not satisfied' "$WORK_ROOT/$mutation.stderr"
    fi
  done
done
sha256sum --check --status "$WORK_ROOT/sources.sha256"
echo "Native funding arithmetic, graph construction, augmentation, discovery, and adjacency verified. All sixteen incorrect bodies were rejected by contract verification."
