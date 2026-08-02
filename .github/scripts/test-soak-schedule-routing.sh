#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

ruby -ryaml -e '
  doc = YAML.load_file(ARGV[0])
  step = doc.dig("jobs", "schedule_gate", "steps").find { |item| item["id"] == "resolve" }
  abort "schedule gate not found" unless step && step["run"].is_a?(String)
  File.write(ARGV[1], step["run"])
' "$ROOT/.github/workflows/merge-recovery-soak.yml" "$TMP/gate.sh"

mkdir -p "$TMP/bin"
cat >"$TMP/bin/date" <<'SH'
#!/usr/bin/env bash
args="$*"
case "$args" in
  "-u +%s") printf '%s\n' "$FAKE_NOW_EPOCH" ;;
  *" +%H%M") printf '%s\n' "${FAKE_PACIFIC_HM:-1930}" ;;
  *" +%u") printf '%s\n' "$FAKE_PACIFIC_WEEKDAY" ;;
  *" +%FT%TZ") printf '%s\n' '2026-01-01T00:00:00Z' ;;
  *" +%Y-%m-%dT%H:%M:%SZ") printf '%s\n' '2026-01-01T00:00:00Z' ;;
  *" +%F") printf '%s\n' '2026-01-01' ;;
  *" +%s")
    case "$args" in
      *":30:00"*) printf '%s\n' "$FAKE_SLOT_EPOCH" ;;
      *) printf '0\n' ;;
    esac
    ;;
  *) printf 'unsupported fake date invocation: %s\n' "$args" >&2; exit 1 ;;
esac
SH
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *"/commits?"*) printf '1\n' ;;
  *) printf '%s\n' '{"workflow_runs":[]}' ;;
esac
SH
chmod +x "$TMP/bin/date" "$TMP/bin/gh"

run_gate() {
	local mode="$1" weekday="$2" slot="$3" output
	output="$TMP/output-$mode-$weekday"
	: >"$output"
	PATH="$TMP/bin:$PATH" \
		FAKE_NOW_EPOCH="$((slot + 60))" \
		FAKE_SLOT_EPOCH="$slot" \
		FAKE_PACIFIC_WEEKDAY="$weekday" \
		EVENT_NAME="$([ "$mode" = oci ] && printf workflow_dispatch || printf schedule)" \
		EVENT_SCHEDULE="30 2 * * *" \
		INPUT_SCHEDULED_SLOT="$([ "$mode" = oci ] && printf '%s' "$slot")" \
		INPUT_DURATION=daily-24h \
		INPUT_TARGET_REF=dev \
		INPUT_WINDOW_END="" \
		INPUT_SERIES="" \
		INPUT_RETRY_ATTEMPT=0 \
		INPUT_CANARY=false \
		INPUT_INJECT_PROTECTION_BREACH=false \
		GH_TOKEN=test \
		GITHUB_REPOSITORY=F1R3FLY-io/f1r3node-rust \
		GITHUB_RUN_ID=999 \
		GITHUB_OUTPUT="$output" \
		GITHUB_STEP_SUMMARY="$TMP/summary" \
		bash -euo pipefail "$TMP/gate.sh" >/dev/null
	printf '%s\n' "$output"
}

for mode in oci cron; do
	friday="$(run_gate "$mode" 5 1785551400)"
	grep -qx 'target_ref=master' "$friday"
	grep -qx 'duration_seconds=216000' "$friday"
	grep -qx 'kind=weekend' "$friday"
	grep -qx 'run_benchmarks=true' "$friday"

	monday="$(run_gate "$mode" 1 1785810600)"
	grep -qx 'target_ref=dev' "$monday"
	grep -qx 'duration_seconds=79200' "$monday"
	grep -qx 'kind=daily' "$monday"
	grep -qx 'run_benchmarks=false' "$monday"
done

if PATH="$TMP/bin:$PATH" \
	FAKE_NOW_EPOCH=1785551460 \
	FAKE_SLOT_EPOCH=1785551400 \
	FAKE_PACIFIC_WEEKDAY=5 \
	EVENT_NAME=workflow_dispatch \
	EVENT_SCHEDULE="" \
	INPUT_SCHEDULED_SLOT=1785551400 \
	INPUT_DURATION=daily-24h \
	INPUT_TARGET_REF=dev \
	INPUT_WINDOW_END="" \
	INPUT_SERIES="" \
	INPUT_RETRY_ATTEMPT=0 \
	INPUT_CANARY=true \
	INPUT_INJECT_PROTECTION_BREACH=false \
	GH_TOKEN=test \
	GITHUB_REPOSITORY=F1R3FLY-io/f1r3node-rust \
	GITHUB_RUN_ID=999 \
	GITHUB_OUTPUT="$TMP/conflict-output" \
	GITHUB_STEP_SUMMARY="$TMP/summary" \
	bash -euo pipefail "$TMP/gate.sh" >/dev/null 2>&1; then
	echo 'scheduled slot accepted conflicting canary controls' >&2
	exit 1
fi

printf 'soak schedule routing tests passed\n'
