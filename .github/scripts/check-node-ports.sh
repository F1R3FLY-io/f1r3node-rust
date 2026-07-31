#!/usr/bin/env bash
# Verify a host port range is free before starting anything that binds it.
#
# Usage: check-node-ports.sh <first-port> <last-port>
#
# CI binds 14400-14455 (see docker/ci-ports.*.yml), which is BELOW the kernel
# ephemeral floor of 32768, so nothing can be auto-assigned into it and no
# reservation is needed. This is a precondition check, not a drain wait.
#
# It used to reserve and wait on 40400-40455, inside the ephemeral range, and
# that is the whole reason the heavy pipeline had to be re-run so often. The
# Actions agent opens its control connections to GitHub before the job exists
# and can source them from that range:
#
#   ESTAB 10.1.0.54:40418 -> 20.85.130.105:443  (("Runner.Listener",pid=2060))
#   ESTAB 10.1.0.32:40422 -> 140.82.112.24:443  (("hosted-compute-",pid=2034))
#
# Those are ESTAB for the life of the job. They never drain, so the old 90s
# wait could not have helped -- it only delayed the failure. A re-run passed
# because a fresh runner's agent picked different source ports, which is
# exactly why this read as an intermittent flake for months.
#
# TIME_WAIT is still worth waiting out: it genuinely clears in ~60s, and a
# previous job on the same runner can leave some behind. So the two cases are
# separated -- wait for what drains, fail immediately and name the holder for
# what does not.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <first-port> <last-port>" >&2
  exit 2
fi
first="$1"
last="$2"
for p in "$first" "$last"; do
  case "$p" in
    ''|*[!0-9]*)
      echo "::error::port arguments must be numeric; got '$first' '$last'" >&2
      exit 2
      ;;
  esac
done
if [ "$first" -gt "$last" ]; then
  echo "::error::first port ($first) is above last port ($last)" >&2
  exit 2
fi
# Refuse a range the kernel can auto-assign into. The whole point of the move
# was to get below the ephemeral floor; a caller that passes an overlapping
# range has reintroduced the original bug, and silently checking it would let
# that pass as a green step.
floor="$(sysctl -n net.ipv4.ip_local_port_range 2>/dev/null | awk '{print $1}')"
case "$floor" in
  ''|*[!0-9]*) floor=32768 ;;
esac
if [ "$last" -ge "$floor" ]; then
  echo "::error::range ${first}-${last} reaches into the ephemeral range (starts at ${floor}); the Actions agent can already hold ports there before this job exists"
  exit 1
fi

range="sport >= :${first} and sport <= :${last}"
# 13 samples at t=0,5,...,60 -- the last sample IS the decision, so the loop
# never sleeps past a check it does not then act on.
for i in $(seq 1 13); do
  snapshot="$(sudo ss -Htanp "$range" 2>/dev/null || true)"
  busy="$(printf '%s' "$snapshot" | grep -c . || true)"
  [ "$busy" -eq 0 ] && exit 0
  stuck="$(printf '%s\n' "$snapshot" | grep -v '^TIME-WAIT' || true)"
  if [ -n "$stuck" ]; then
    echo "FAIL: port(s) in ${first}-${last} held by a socket that will not drain:"
    printf '%s\n' "$stuck"
    echo "Not TIME_WAIT, so waiting cannot clear these. If the holder is the"
    echo "Actions agent, the CI range now overlaps the ephemeral range again --"
    echo "check net.ipv4.ip_local_port_range on this runner."
    exit 1
  fi
  [ "$i" -eq 13 ] && break
  echo "  [${i}] ${busy} TIME_WAIT socket(s) in ${first}-${last}; waiting for drain..."
  sleep 5
done
echo "FAIL: ${busy} TIME_WAIT socket(s) still holding ${first}-${last} after 60s:"
printf '%s\n' "$snapshot"
exit 1
