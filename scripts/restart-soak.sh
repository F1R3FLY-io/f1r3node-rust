#!/usr/bin/env bash
# Restart (or manually redeploy) a merge-recovery soak within a bounded
# window, from an operator machine.
#
# This wraps the workflow's restart mode: it dispatches merge-recovery-soak.yml
# with window_end_epoch + series + retry_attempt=1, so the run soaks only up
# to the given window end, is stamped "restarted" in the published data, and
# is excluded from baseline duty — a short manual run must never become the
# baseline a full-window run is judged against. It also cannot spill into the
# next scheduled slot: the run ends at the window end you choose, and the
# next cron slot proceeds normally.
#
#   scripts/restart-soak.sh --last-failed
#       Find the most recent FAILED scheduled soak run, recompute the window
#       its cron slot defined (Fri 19:30 Pacific + 60h = weekend, Mon-Thu
#       19:30 Pacific + 22h = daily), and restart for whatever remains of it.
#
#   scripts/restart-soak.sh --series daily --until-next-slot
#       Start now and end 30 minutes before the next 19:30 Pacific launch —
#       the shape for a same-day validation run that completes, publishes to
#       the dashboard, and stays out of tonight's way.
#
#   scripts/restart-soak.sh --series daily --hours 4
#   scripts/restart-soak.sh --series weekend --window-end 1785594600
#
# Options:
#   --last-failed          derive series/target/window from the latest failed
#                          scheduled run (mutually exclusive with the rest)
#   --series X             daily | weekend
#   --target-ref R         branch/SHA to soak (default: dev for daily,
#                          master for weekend — matching the schedule gate)
#   --window-end EPOCH     absolute end instant (epoch seconds)
#   --until-next-slot      end 30 min before the next 19:30 Pacific slot
#   --hours N              end N hours from now
#   --dry-run              print the dispatch command without running it
#   --yes                  skip the confirmation prompt
#
# Needs: gh (authenticated with actions:write on this repo), python3.
# The dispatched run appears in Actions as workflow_dispatch with
# retry_attempt=1; the workflow's own automatic retry will not chain off it.
set -euo pipefail

# The ambient GITHUB_TOKEN in agent/CI shells is a restricted PAT that lacks
# the scopes gh needs here; drop it so gh falls back to GH_TOKEN or its own
# keychain auth.
unset GITHUB_TOKEN || true

REPO_SLUG="F1R3FLY-io/f1r3node-rust"
WORKFLOW="merge-recovery-soak.yml"
FLOOR_SECONDS=7200
NEXT_SLOT_MARGIN=1800

usage() { sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'; }

command -v gh >/dev/null || { echo "gh CLI not found" >&2; exit 2; }
command -v python3 >/dev/null || { echo "python3 not found (needed for Pacific-time window maths)" >&2; exit 2; }
command -v jq >/dev/null || { echo "jq not found" >&2; exit 2; }

py() { python3 - "$@"; }

# Prints "<slot_epoch> <series>" for the 19:30 Pacific slot at or before the
# given epoch — the slot that launched a scheduled run created at that time.
slot_before() {
  py "$1" <<'PY'
import sys
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo
tz = ZoneInfo("America/Los_Angeles")
t = datetime.fromtimestamp(int(sys.argv[1]), tz)
slot = t.replace(hour=19, minute=30, second=0, microsecond=0)
if slot > t:
    slot -= timedelta(days=1)
series = "weekend" if slot.weekday() == 4 else "daily"
print(int(slot.timestamp()), series)
PY
}

next_slot_epoch() {
  py <<'PY'
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo
tz = ZoneInfo("America/Los_Angeles")
now = datetime.now(tz)
slot = now.replace(hour=19, minute=30, second=0, microsecond=0)
if slot <= now:
    slot += timedelta(days=1)
print(int(slot.timestamp()))
PY
}

pacific() {
  py "$1" <<'PY'
import sys
from datetime import datetime
from zoneinfo import ZoneInfo
print(datetime.fromtimestamp(int(sys.argv[1]), ZoneInfo("America/Los_Angeles")).strftime("%a %Y-%m-%d %H:%M %Z"))
PY
}

LAST_FAILED=false
SERIES=""
TARGET_REF=""
WINDOW_END=""
UNTIL_NEXT_SLOT=false
HOURS=""
DRY_RUN=false
ASSUME_YES=false

while [ $# -gt 0 ]; do
  case "$1" in
    --last-failed) LAST_FAILED=true ;;
    --series) SERIES="${2:?--series needs a value}"; shift ;;
    --target-ref) TARGET_REF="${2:?--target-ref needs a value}"; shift ;;
    --window-end) WINDOW_END="${2:?--window-end needs a value}"; shift ;;
    --until-next-slot) UNTIL_NEXT_SLOT=true ;;
    --hours) HOURS="${2:?--hours needs a value}"; shift ;;
    --dry-run) DRY_RUN=true ;;
    --yes) ASSUME_YES=true ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

NOW="$(date +%s)"

if [ "$LAST_FAILED" = "true" ]; then
  if [ -n "$SERIES$TARGET_REF$WINDOW_END$HOURS" ] || [ "$UNTIL_NEXT_SLOT" = "true" ]; then
    echo "--last-failed derives everything itself; do not combine it with other selectors" >&2
    exit 2
  fi
  # Only scheduled runs: their window is defined by the cron slot, which can
  # be recomputed from the run's creation time. A dispatched run's window
  # lives in its inputs, which the API does not expose.
  failed="$(gh run list --repo "$REPO_SLUG" --workflow "$WORKFLOW" \
      --status failure --limit 30 \
      --json databaseId,createdAt,event,url \
    | jq -r '[.[] | select(.event == "schedule")][0] // empty
             | "\(.databaseId)\t\(.createdAt)\t\(.url)"')"
  [ -n "$failed" ] || { echo "no failed scheduled soak run found" >&2; exit 1; }
  run_id="${failed%%$'\t'*}"
  rest="${failed#*$'\t'}"
  created_at="${rest%%$'\t'*}"
  run_url="${rest#*$'\t'}"
  created_epoch="$(py "$created_at" <<'PY'
import sys
from datetime import datetime
print(int(datetime.fromisoformat(sys.argv[1].replace("Z", "+00:00")).timestamp()))
PY
)"
  read -r slot_epoch SERIES <<<"$(slot_before "$created_epoch")"
  if [ "$SERIES" = "weekend" ]; then
    WINDOW_END=$((slot_epoch + 216000))
    TARGET_REF="master"
  else
    WINDOW_END=$((slot_epoch + 79200))
    TARGET_REF="dev"
  fi
  echo "latest failed scheduled run: $run_id ($run_url)"
  echo "  slot:   $(pacific "$slot_epoch") -> $SERIES"
else
  case "$SERIES" in
    daily|weekend) ;;
    "") echo "--series is required (daily | weekend) unless using --last-failed" >&2; exit 2 ;;
    *) echo "--series must be daily or weekend" >&2; exit 2 ;;
  esac
  if [ -z "$TARGET_REF" ]; then
    TARGET_REF="dev"
    [ "$SERIES" = "weekend" ] && TARGET_REF="master"
  fi
  ends=0
  [ -n "$WINDOW_END" ] && ends=$((ends + 1))
  [ -n "$HOURS" ] && ends=$((ends + 1))
  [ "$UNTIL_NEXT_SLOT" = "true" ] && ends=$((ends + 1))
  if [ "$ends" -ne 1 ]; then
    echo "pick exactly one of --window-end, --hours, --until-next-slot" >&2
    exit 2
  fi
  if [ "$UNTIL_NEXT_SLOT" = "true" ]; then
    WINDOW_END=$(( $(next_slot_epoch) - NEXT_SLOT_MARGIN ))
  elif [ -n "$HOURS" ]; then
    [[ "$HOURS" =~ ^[1-9][0-9]*$ ]] || { echo "--hours must be a positive integer" >&2; exit 2; }
    WINDOW_END=$((NOW + HOURS * 3600))
  fi
  [[ "$WINDOW_END" =~ ^[1-9][0-9]*$ ]] || { echo "--window-end must be epoch seconds" >&2; exit 2; }
fi

case "$SERIES" in
  daily) WINDOW_NOMINAL=79200 ;;
  weekend) WINDOW_NOMINAL=216000 ;;
esac

REMAINING=$((WINDOW_END - NOW))

# The gate refuses a remainder longer than the series' nominal window — a
# restart may only ever shorten what it inherits. Mirror that here so a bad
# invocation fails on this machine with an explanation, instead of dispatching
# a run that no-ops in CI a minute later.
#
# --until-next-slot is capped rather than rejected: its intent is "end before
# the next scheduled launch", and invoked in the evening the gap to tomorrow's
# 19:30 exceeds a 22h daily (e.g. 23h at 20:00 PT). Capping preserves the
# intent and still ends before the slot. An explicit --hours/--window-end is
# rejected instead, because the operator named a span we cannot honour.
if [ "$REMAINING" -gt "$WINDOW_NOMINAL" ]; then
  if [ "$UNTIL_NEXT_SLOT" = "true" ]; then
    echo "note: $((REMAINING / 3600))h to the next slot exceeds the ${SERIES} window ($((WINDOW_NOMINAL / 3600))h); capping"
    WINDOW_END=$((NOW + WINDOW_NOMINAL))
    REMAINING="$WINDOW_NOMINAL"
  else
    echo "window is ${REMAINING}s, beyond the ${WINDOW_NOMINAL}s ${SERIES} window; the gate would refuse it. A restart can only shorten the window it inherits." >&2
    exit 1
  fi
fi

if [ "$REMAINING" -lt "$FLOOR_SECONDS" ]; then
  echo "only ${REMAINING}s remain before $(pacific "$WINDOW_END") — under the 2h floor; the gate would refuse, so not dispatching" >&2
  exit 1
fi

DURATION="daily-24h"
[ "$SERIES" = "weekend" ] && DURATION="weekend-60h"

echo "restart plan:"
echo "  series:      $SERIES"
echo "  target ref:  $TARGET_REF"
echo "  window ends: $(pacific "$WINDOW_END") (~$((REMAINING / 3600))h from now)"
echo "  marked:      restarted (retry_attempt=1; excluded from baselines)"

CMD=(gh workflow run "$WORKFLOW" --repo "$REPO_SLUG"
  -f "target_ref=$TARGET_REF"
  -f "duration=$DURATION"
  -f "window_end_epoch=$WINDOW_END"
  -f "series=$SERIES"
  -f "retry_attempt=1")

if [ "$DRY_RUN" = "true" ]; then
  printf 'dry run — would execute:\n  %q' "${CMD[0]}"
  printf ' %q' "${CMD[@]:1}"
  printf '\n'
  exit 0
fi

if [ "$ASSUME_YES" != "true" ]; then
  printf 'dispatch this run? [y/N] '
  read -r answer
  case "$answer" in y|Y|yes|YES) ;; *) echo "aborted"; exit 1 ;; esac
fi

"${CMD[@]}"
echo "dispatched. Watch it with:"
echo "  gh run list --repo $REPO_SLUG --workflow $WORKFLOW --limit 3"
