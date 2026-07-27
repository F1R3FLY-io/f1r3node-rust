#!/usr/bin/env bash
# ===========================================================================
# stack_depth_probe.sh — bisect the minimum thread stack a traversal needs.
#
# For each traversal T and each nesting depth N, binary-searches the smallest
# `stack_size` S for which `rholang/tests/stack_depth_probe.rs` survives, then
# reports the least-squares fit S(N) = a + b*N.  A materially non-zero b proves
# T is Theta(depth) in NATIVE STACK; b ~= 0 proves it is heap-bounded.
#
# Usage:
#   scripts/stack_depth_probe.sh <probe-binary> "<what ...>" "<depth ...>"
#
# Example:
#   cargo test -p rholang --test stack_depth_probe --no-run
#   scripts/stack_depth_probe.sh target/debug/deps/stack_depth_probe-XXXX \
#       "subst sort clone drop" "10 20 40 80"
#
# NOTE: `ulimit -c 0` is set below.  Without it every faulting run writes a
# ~300 MB core file and the bisection takes ~30 s per probe point instead of
# ~0.3 s.
# ===========================================================================
set -u

BIN="${1:?probe binary}"
WHATS="${2:-subst}"
DEPTHS="${3:-10 20 40 80}"

ulimit -c 0

RESOLUTION=${RESOLUTION:-4096}   # bisect to page granularity
CEILING=${CEILING:-1073741824}   # 1 GiB hard cap

# Run one probe point.  0 = survived, 1 = overflowed/failed.
#
# Failure-mode discipline: a probe can fail for two very different reasons —
# (a) it overflowed its stack (what we are measuring), or (b) it panicked for a
# reason unrelated to stack (a wrong proto tag, a library recursion LIMIT that
# returns Err, an unmatched pattern).  Conflating them silently produces
# nonsense fits, so `try` records the mode and `classify` reports it.
LAST_MODE=""
try() {
  local what="$1" depth="$2" stack="$3" err
  err=$(PROBE_WHAT="$what" PROBE_DEPTH="$depth" PROBE_STACK="$stack" \
          "$BIN" --ignored --exact probe 2>&1 >/dev/null)
  local rc=$?
  if [ $rc -eq 0 ]; then LAST_MODE="ok"; return 0; fi
  case "$err" in
    *"has overflowed its stack"*) LAST_MODE="overflow" ;;
    *panicked*)                   LAST_MODE="panic: $(printf '%s' "$err" | grep -m1 -o "panicked at.*" | cut -c1-120)" ;;
    *)                            LAST_MODE="rc=$rc" ;;
  esac
  return 1
}

for what in $WHATS; do
  echo "### $what"
  pts=""
  for depth in $DEPTHS; do
    # 1. exponential search for a stack that survives
    hi=$RESOLUTION
    while [ "$hi" -le "$CEILING" ]; do
      if try "$what" "$depth" "$hi"; then break; fi
      hi=$((hi * 2))
    done
    if [ "$hi" -gt "$CEILING" ]; then
      echo "  depth=$depth  NO-FIT (> $CEILING) last mode: $LAST_MODE"
      continue
    fi
    lo=$((hi / 2))
    # 2. binary search in (lo, hi]
    while [ $((hi - lo)) -gt "$RESOLUTION" ]; do
      mid=$(((lo + hi) / 2))
      if try "$what" "$depth" "$mid"; then hi=$mid; else lo=$mid; fi
    done
    echo "  depth=$depth  min_stack=$hi bytes ($((hi / 1024)) KiB)"
    pts="$pts $depth:$hi"
  done
  # 3. least-squares fit S(N) = a + b*N
  echo "$pts" | awk '{
    n=0; sx=0; sy=0; sxx=0; sxy=0;
    for (i=1;i<=NF;i++) { split($i,p,":"); x=p[1]; y=p[2];
      n++; sx+=x; sy+=y; sxx+=x*x; sxy+=x*y }
    if (n>1) {
      b=(n*sxy-sx*sy)/(n*sxx-sx*sx); a=(sy-b*sx)/n;
      printf "  FIT  S(N) = %.0f + %.0f*N   (%.2f KiB/level, intercept %.1f KiB)\n",
             a, b, b/1024, a/1024
    }
  }'
done
