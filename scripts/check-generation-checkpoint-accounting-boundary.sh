#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$repo_root/rspace++/Cargo.toml"
production_root="$repo_root/rspace++/src"
publisher="$production_root/rspace/persistence/durable/publication.rs"

metadata="$(cargo metadata --format-version 1 --no-deps --manifest-path "$manifest")"
reverse_accounting_dependencies="$({
  printf '%s' "$metadata" | jq -r '
    [.packages[]
      | select(.name == "rspace_plus_plus")
      | .dependencies[]
      | select(.kind == null)
      | select(.name == "rholang" or .name == "casper")]
    | length
  '
})"
if [[ "$reverse_accounting_dependencies" != "0" ]]; then
  echo "generation-checkpoint-accounting: rspace_plus_plus gained a production rholang/casper dependency" >&2
  exit 1
fi

forbidden_pattern='RuntimeBudget|BillableKind|reserve_comm|cost_trace_digest|storage_charge'
if rg -n --glob '*.rs' "$forbidden_pattern" "$production_root"; then
  echo "generation-checkpoint-accounting: the physical persistence layer gained a semantic-accounting capability" >&2
  exit 1
fi

publisher_occurrences="$(rg -n 'publish_generation_checkpoint_v1' "$production_root" --glob '*.rs')"
if [[ -z "$publisher_occurrences" ]]; then
  echo "generation-checkpoint-accounting: checkpoint publisher entry point disappeared" >&2
  exit 1
fi
while IFS=: read -r source_path _; do
  if [[ "$source_path" != "$publisher" ]]; then
    echo "generation-checkpoint-accounting: checkpoint publication escaped its dormant physical boundary" >&2
    exit 1
  fi
done <<<"$publisher_occurrences"

echo "generation-checkpoint-accounting: dormant physical publisher has no semantic-accounting capability"
