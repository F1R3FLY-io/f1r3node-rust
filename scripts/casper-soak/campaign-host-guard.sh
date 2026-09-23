#!/usr/bin/env bash
set -euo pipefail
umask 077
[[ $# == 3 ]] || exit 2
reservation="$1"
root="$(realpath -e -- "$2")"
out="$(realpath -e -- "$3")"
[[ "$reservation" =~ ^[a-f0-9]{64}$ && -d "$out" && ! -L "$out" ]] || exit 2
slice="casper-$reservation.slice"
while :; do
  reason=""
  available="$(awk '/^MemAvailable:/{print int($2/1024)}' /proc/meminfo)" || reason=memory_observation_failed
  free="$(df -Pm -- "$root" | awk 'NR==2{print $4}')" || reason=disk_observation_failed
  [[ "$available" =~ ^[0-9]+$ && "$free" =~ ^[0-9]+$ ]] || reason=resource_observation_invalid
  if [[ -z "$reason" ]]; then
    if (( available < 8192 )); then reason=host_memory_floor; fi
    if (( free < 4096 )); then reason=disk_floor; fi
  fi
  ceiling="$(systemctl --user show "$slice" --property MemoryMax --value)" || reason=memory_ceiling_unknown
  [[ "$ceiling" == 47244640256 ]] || reason=memory_ceiling_changed
  if [[ -n "$reason" ]]; then
    (
      set -o noclobber
      jq -n --arg reason "$reason" --arg reservation "$reservation" \
        '{schema_version:1,reservation_id:$reservation,reason:$reason,host_protection:"failed",termination_confirmed:false}' \
        > "$out/guard-failure.json"
    ) || true
    systemctl --user kill --kill-who=all --signal=KILL "$slice" || true
    exit 2
  fi
  sleep 2
done
