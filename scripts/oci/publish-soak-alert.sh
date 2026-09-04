#!/usr/bin/env bash
# Publish the weekly soak verdict as a plain-text ONS email (EPOCH-010
# TASK-010-5). Runs on the OCI soak runner with instance-principal auth —
# no GitHub secrets involved. ONS is plain-text only, so the mail carries
# the four metrics, week-over-week deltas, the verdict, and URLs to the
# dashboard for anything deeper.
#
# Environment:
#   REPORT_DIR (required)     directory with verdict.json + weekly-summary.json
#   ONS_TOPIC_OCID            target topic; unset => print notice and exit 0
#   DASHBOARD_URL             trend dashboard (default: repo Pages site)
#   RUN_URL                   optional link to the Actions run

set -euo pipefail

REPORT_DIR="${REPORT_DIR:?REPORT_DIR is required}"
ONS_TOPIC_OCID="${ONS_TOPIC_OCID:-}"
DASHBOARD_URL="${DASHBOARD_URL:-https://f1r3fly-io.github.io/f1r3node-rust/}"
RUN_URL="${RUN_URL:-}"

if [ -z "$ONS_TOPIC_OCID" ]; then
  echo "SOAK_ONS_TOPIC_OCID is not configured; skipping ONS alert" >&2
  exit 0
fi
command -v oci >/dev/null || { echo "oci CLI not found" >&2; exit 2; }
command -v jq >/dev/null || { echo "jq not found" >&2; exit 2; }
[ -s "$REPORT_DIR/verdict.json" ] || { echo "missing $REPORT_DIR/verdict.json" >&2; exit 2; }

VERDICT="$(jq -r '.verdict' "$REPORT_DIR/verdict.json")"
DATE="$(jq -r '.run.date' "$REPORT_DIR/verdict.json")"
TITLE="F1R3FLY weekend soak: $(printf '%s' "$VERDICT" | tr '[:lower:]' '[:upper:]') (${DATE%%T*})"

BODY="$(jq -r \
  --arg dashboard "$DASHBOARD_URL" \
  --arg run_url "$RUN_URL" \
  --slurpfile week "$REPORT_DIR/weekly-summary.json" \
'
  def fmt: if . == null then "n/a" else tostring end;
  ($week[0].passive // {}) as $p
  | ($week[0].active // {}) as $a
  | [
      "Weekend soak verdict: \(.verdict | ascii_upcase)",
      "Target: \(.run.target_ref) @ \(.run.target_sha[0:12])",
      "Baseline: \(.baseline_run.date // "none (bootstrap run)")",
      "",
      (if (.failures | length) > 0 then
        ("REGRESSIONS:", (.failures[] | "  - " + .), "") else empty end),
      (if (.warnings | length) > 0 then
        ("Warnings:", (.warnings[] | "  - " + .), "") else empty end),
      "Metrics (this week; deltas vs baseline are in the lines above):",
      "  failure rate:      \($p.failure_rate | fmt)",
      "  throughput:        \($p.iterations_per_hour | fmt) iterations/hour",
      "  peak node RSS:     \($p.rss_peak_mb | fmt) MB",
      "  finalization p95:  \($p.finalization_p95_ms | fmt) ms",
      "  bench-segment p95: \($a.p95_ms | fmt) ms",
      "",
      "Trends and full detail: " + $dashboard,
      (if $run_url == "" then empty else "Run: " + $run_url end),
      "",
      "Manage this subscription via the unsubscribe link in this email."
    ]
  | flatten | join("\n")
' "$REPORT_DIR/verdict.json")"

# Advisory docs link sweep (weekend runs): append a warning line when the
# sweep artifact reports broken external links. The sweep never gates the
# soak; absence of the file means the sweep did not run or was not fetched.
if [ -n "${LINK_SWEEP_JSON:-}" ] && [ -s "$LINK_SWEEP_JSON" ]; then
  LINK_BROKEN="$(jq -r '.broken // 0' "$LINK_SWEEP_JSON" 2>/dev/null || echo 0)"
  if [ "$LINK_BROKEN" -gt 0 ] 2>/dev/null; then
    BODY="$BODY

WARNING: the advisory docs link sweep found $LINK_BROKEN broken external link(s).
This warning does not gate the soak or the release. Details: the
docs-link-sweep artifact on the workflow run."
  else
    BODY="$BODY

Docs external link sweep: OK (advisory)."
  fi
fi

oci ons message publish \
  --auth instance_principal \
  --topic-id "$ONS_TOPIC_OCID" \
  --title "$TITLE" \
  --body "$BODY"
echo "published ONS alert: $TITLE" >&2
