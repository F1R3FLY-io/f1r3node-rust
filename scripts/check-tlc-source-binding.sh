#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/lib/tlc-run.sh"

config="$ROOT/formal/tlaplus/uptime/MC_UptimeEnvelopeDominance.cfg"
module="$ROOT/formal/tlaplus/uptime/UptimeEnvelopeDominance.tla"
source_hash="$(tlc_source_hash "$config" "$module")"
recovery_identity="$(tlc_recovery_identity "$source_hash")"

[[ "$source_hash" =~ ^[0-9a-f]{64}$ ]]
[[ "$recovery_identity" =~ ^[0-9a-f]{64}$ ]]
tlc_require_recovery_binding "$recovery_identity" -deadlock
TLC_RECOVER_IDENTITY="$recovery_identity" tlc_require_recovery_binding "$recovery_identity" -recover checkpoint
if TLC_RECOVER_IDENTITY=incorrect tlc_require_recovery_binding "$recovery_identity" -recover checkpoint 2>/dev/null; then
  echo "error: mismatched TLC recovery identity was accepted" >&2
  exit 1
fi
if env -u TLC_RECOVER_IDENTITY bash -c '
  source "$1/scripts/lib/tlc-run.sh"
  tlc_require_recovery_binding "$2" -recover checkpoint
' _ "$ROOT" "$recovery_identity" 2>/dev/null; then
  echo "error: unbound TLC checkpoint recovery was accepted" >&2
  exit 1
fi

alternate_identity="$(TLC_FP=1 tlc_recovery_identity "$source_hash")"
test "$alternate_identity" != "$recovery_identity"
tlc_require_unchanged_identity "$recovery_identity" "$recovery_identity"
if tlc_require_unchanged_identity "$recovery_identity" "$alternate_identity" 2>/dev/null; then
  echo "error: changed TLC execution identity was accepted" >&2
  exit 1
fi

echo "TLC source-binding regression passed."
