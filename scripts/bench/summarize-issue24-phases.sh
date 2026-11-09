#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat >&2 <<'EOF'
Usage:
  summarize-issue24-phases.sh records [--provider docker|subprocess] [--session N] LOG...
  summarize-issue24-phases.sh compare [--phase NAME] [--slow-inclusion-p95 SECONDS] RECORDS...

records  Read test_load output (an iteration pytest.log or a soak job log) and
         write one JSON line for each phase and validator. Without --provider,
         a test that writes a subprocess genesis is subprocess for all of its
         sessions, and other tests are docker. A test ends at its PASSED,
         FAILED, or ERROR line, or when a new pytest session starts. --session
         sets the number of the first session (default 1), so the records of
         several iterations stay apart.
compare  Read records and print the slow-phase share for each provider and the
         mean metrics of the fast, slow, and failing phases. A phase is failing
         when it has unfinalized deploys, and slow when its inclusion p95 is
         above the threshold (default 9 s). The default phase is high.
EOF
	exit 2
}

records() {
	local provider="" first=1
	while [ "$#" -gt 0 ]; do
		case "$1" in
		--provider)
			provider="${2:?--provider needs a value}"
			case "$provider" in docker | subprocess) ;; *) usage ;; esac
			shift 2
			;;
		--session)
			first="${2:?--session needs a value}"
			case "$first" in "" | *[!0-9]*) usage ;; esac
			shift 2
			;;
		--) shift; break ;;
		-*) usage ;;
		*) break ;;
		esac
	done
	[ "$#" -gt 0 ] || usage
	local log
	for log in "$@"; do
		[ -r "$log" ] || { echo "cannot read $log" >&2; exit 2; }
	done
	awk -v forced="$provider" -v first="$first" '
	function esc(s) { gsub(/\\/, "\\\\", s); gsub(/"/, "\\\"", s); gsub(/\t/, "\\t", s); return s }
	function reset_session() { phase = ""; validator = ""; subprocess = 0; genesis = 0; split("", seen) }
	function start_session() { session++; if (genesis) subprocess = 1; genesis = 0 }
	BEGIN { session = first - 1; genesis = 0; subprocess = 0; reset_session() }
	{
		line = $0
		stamp = ""
		if (match(line, /^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9:.]+Z /)) {
			stamp = substr(line, 1, RLENGTH - 1)
			line = substr(line, RLENGTH + 1)
		}
		if (index(line, "Subprocess genesis written") > 0) genesis = 1
		if (line ~ /^  Phase [a-z_]+: inclusion p50=/) {
			name = line; sub(/^  Phase /, "", name); sub(/:.*/, "", name)
			if (phase == "" || (name in seen)) { split("", seen); start_session() }
			seen[name] = 1
			phase = name
			validator = ""
			head = line
			p_stamp = stamp
			next
		}
		if (phase != "" && match(line, /Node internals \([^)]*\)/)) {
			validator = substr(line, RSTART + 16, RLENGTH - 17)
			next
		}
		if (phase != "" && validator != "" && line ~ /^ISSUE24_METRICS \{/) {
			json = substr(line, 17)
			n = split(head, f, /[ =#(),>-]+/)
			printf "{\"session\":%d,\"provider\":\"%s\",\"phase\":\"%s\",\"validator\":\"%s\",\"time\":\"%s\",\"line\":\"%s\",\"metrics\":%s}\n", \
				session, (forced != "" ? forced : (subprocess ? "subprocess" : "docker")), esc(phase), esc(validator), esc(p_stamp), esc(head), json
			validator = ""
			next
		}
		if (line ~ /(PASSED|FAILED|ERROR) +\[ *[0-9]+%\]/ || line ~ /^=+ test session starts =+$/) {
			reset_session()
		}
	}' < <(cat -- "$@") | jq -cR '
		fromjson? |
		(.line | capture("inclusion p50=(?<ip50>[0-9.]+)s p95=(?<ip95>[0-9.]+)s, finalization p50=(?<fp50>[0-9.]+)s p95=(?<fp95>[0-9.]+)s, LFB #(?<ls>[0-9]+)->#(?<le>[0-9]+) \\((?<rate>[0-9.]+) blk/min\\), unfinalized=(?<un>[0-9]+)")) as $p |
		{
			session, provider, phase, validator, time,
			inclusion_p50: ($p.ip50 | tonumber), inclusion_p95: ($p.ip95 | tonumber),
			finalization_p50: ($p.fp50 | tonumber), finalization_p95: ($p.fp95 | tonumber),
			lfb_start: ($p.ls | tonumber), lfb_end: ($p.le | tonumber),
			lfb_rate: ($p.rate | tonumber), unfinalized: ($p.un | tonumber),
			histograms: (.metrics.histograms // {} | with_entries(.value |= .mean)),
			samples: (.metrics.histograms // {} | with_entries(.value |= .samples)),
			counters: (.metrics.counters // {} | with_entries(.value |= .delta))
		}'
}

compare() {
	local phase=high threshold=9
	while [ "$#" -gt 0 ]; do
		case "$1" in
		--phase) phase="${2:?--phase needs a value}"; shift 2 ;;
		--slow-inclusion-p95) threshold="${2:?--slow-inclusion-p95 needs a value}"; shift 2 ;;
		--) shift; break ;;
		-*) usage ;;
		*) break ;;
		esac
	done
	[[ "$threshold" =~ ^[0-9]+([.][0-9]+)?$ ]] || { echo "--slow-inclusion-p95 must be a number of seconds" >&2; exit 2; }
	[ "$#" -gt 0 ] || usage
	jq -rs --arg phase "$phase" --argjson threshold "$threshold" '
		def group: if .unfinalized > 0 then "failing" elif .inclusion_p95 > $threshold then "slow" else "fast" end;
		def mean: map(select(. != null)) | if length == 0 then "-" else (add / length * 1000 | round / 1000 | tostring) end;
		def rows: [
			["Blocks created", "samples", "proposer_create_time"],
			["Heartbeat proposals", "counters", "heartbeat_proposals"],
			["Empty blocks built", "counters", "block_creator_empty_block_built"],
			["Deploy selection age (s)", "histograms", "deploy_selection_age_time"],
			["Proposer create time (s)", "histograms", "proposer_create_time"],
			["Replay of user deploys, each block (s)", "histograms", "block_replay_phase_user_deploys_time"],
			["Floor distance (blocks)", "histograms", "compute_parents_post_state_floor_distance"],
			["Finalizer run time (s)", "histograms", "finalizer_run_time"],
			["Finalizer timeouts", "counters", "finalizer_run_timeouts"],
			["Deploy pool lock wait (s)", "histograms", "deploy_pool_lock_wait_time"],
			["Runtime scheduler delay (s)", "histograms", "runtime_scheduler_delay_time"],
			["Backlog admission cap", "counters", "block_creator_deploy_admission_cap_source_backlog"],
			["Ordinary deploys deferred", "counters", "block_creator_ordinary_deploys_deferred"]
		];
		map(select(.phase == $phase)) as $all |
		if ($all | length) == 0 then "No \($phase) phases in the records." | halt_error(1) else . end |
		($all | unique_by([.session, .provider, .time, .lfb_start, .lfb_end, .inclusion_p95, .unfinalized])) as $phases |
		["fast", "slow", "failing"] as $groups |
		"## Phase \($phase): slow share (inclusion p95 above \($threshold) s)",
		"",
		"| Provider | Phases | Fast | Slow | Failing |",
		"|---|---|---|---|---|",
		($phases | group_by(.provider)[] |
			"| \(.[0].provider) | \(length) | \(map(select(group == "fast")) | length) | \(map(select(group == "slow")) | length) | \(map(select(group == "failing")) | length) |"),
		"",
		"## Phase \($phase): mean for each validator and phase",
		"",
		"| Metric | " + ($groups | map(. as $g | "\($g) (\($all | map(select(group == $g)) | length))") | join(" | ")) + " |",
		"|---|---|---|---|",
		(rows[] as [$label, $kind, $name] |
			"| \($label) | " + ($groups | map(. as $g | $all | map(select(group == $g) | .[$kind][$name]) | mean) | join(" | ")) + " |")
	' "$@"
}

[ "$#" -gt 0 ] || usage
command="$1"
shift
case "$command" in
records) records "$@" ;;
compare) compare "$@" ;;
*) usage ;;
esac
