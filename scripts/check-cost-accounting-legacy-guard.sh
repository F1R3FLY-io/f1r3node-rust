#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "Checking for legacy user-path cost accounting..."

# Changed by DR-134 (DR-66 item 3): the legacy meter is deleted, and the
# remaining mentions are comments that record dev's wiring. The guard checks
# code lines only, so a `//` comment line is not a hit.
# if rg -n \
#   'ChargingRSpace|charging_rspace|CostManager::charge|[.]charge[(]' \
#   "$ROOT/rholang/src/rust/interpreter" \
#   --glob '!**/tests/**' \
#   --glob '!**/tests.rs' \
#   --glob '!accounting/cost_accounting.rs' \
#   --glob '!accounting/has_cost.rs'; then
#   echo "error: found legacy broad charging on a user execution path" >&2
#   exit 1
# fi
#
# if rg -n \
#   '\bCostManager\b' \
#   "$ROOT/rholang/src/rust/interpreter" \
#   --glob '!**/tests/**' \
#   --glob '!**/tests.rs' \
#   --glob '!accounting/cost_accounting.rs' \
#   --glob '!accounting/has_cost.rs'; then
#   echo "error: found legacy CostManager usage outside compatibility modules" >&2
#   exit 1
# fi
code_hits() {
  rg -n "$1" \
    "$ROOT/rholang/src/rust/interpreter" \
    --glob '!**/tests/**' \
    --glob '!**/tests.rs' \
    --glob '!accounting/cost_accounting.rs' \
    --glob '!accounting/has_cost.rs' |
    rg -v '^[^:]+:[0-9]+:[[:space:]]*//' || true
}

hits="$(code_hits 'ChargingRSpace|charging_rspace|CostManager::charge|[.]charge[(]')"
if [[ -n "$hits" ]]; then
  printf '%s\n' "$hits"
  echo "error: found legacy broad charging on a user execution path" >&2
  exit 1
fi

hits="$(code_hits '\bCostManager\b')"
if [[ -n "$hits" ]]; then
  printf '%s\n' "$hits"
  echo "error: found legacy CostManager usage outside compatibility modules" >&2
  exit 1
fi

echo "Legacy cost-accounting guard passed."
