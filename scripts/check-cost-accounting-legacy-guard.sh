#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "Checking for legacy user-path cost accounting..."

if rg -n \
  'ChargingRSpace|charging_rspace|CostManager::charge|[.]charge[(]' \
  "$ROOT/rholang/src/rust/interpreter" \
  "$ROOT/casper/src/rust" \
  --glob '!accounting/cost_accounting.rs' \
  --glob '!accounting/has_cost.rs'; then
  echo "error: found legacy broad charging on a user execution path" >&2
  exit 1
fi

if rg -n \
  '\bCostManager\b' \
  "$ROOT/rholang/src/rust/interpreter" \
  "$ROOT/casper/src/rust" \
  --glob '!accounting/cost_accounting.rs' \
  --glob '!accounting/has_cost.rs'; then
  echo "error: found legacy CostManager usage outside compatibility modules" >&2
  exit 1
fi

if rg -n \
  'clear_event_log[(]' \
  "$ROOT/casper/src/rust"; then
  echo "error: Casper must not clear diagnostic logs during replay/finalization; RuntimeBudget finalization reads the completed consensus trace and deploy reset clears retained trace state" >&2
  exit 1
fi

echo "Legacy cost-accounting guard passed."
