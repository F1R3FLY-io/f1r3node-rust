#!/usr/bin/env bash
set -euo pipefail
request="$1"
output="$2"
mode="$(jq -er '.request.mode // "complete"' "$request")"
iteration="$(jq -er '.request.iteration' "$request")"
value="$(jq -c '.request.expected' "$request")"
if [[ "$mode" == mismatch || ( "$mode" == failure-then-stop && "$iteration" == 1 ) ]]; then
	value='"planted-mismatch"'
fi
printf '%s\n' "$value" >"$output/sample.json"
hash="$(sha256sum "$output/sample.json" | awk '{print $1}')"
size="$(wc -c <"$output/sample.json")"
jq --argjson value "$value" --arg hash "$hash" --argjson size "$size" '
.request as $r | {observations: [($r.required_observations[0] + {
 schema_version:1, manifest_digest:$r.manifest_digest,run_id:$r.run_id,scenario_id:$r.scenario_id,
 pair_id:$r.pair_id,segment:$r.segment,iteration:$r.iteration,record_id:"record-1",
 producer:"fixture-executor",producer_sequence:"1",event_id:"event-1",presence:"observed",payload:$value,
 observation_time:{clock_id:"fixture-monotonic",monotonic_ns:"1",utc:"2026-01-01T00:00:00Z"},
 raw_reference:{path:"sample.json",bytes:$size,sha256:$hash,producer:"fixture-executor",capture_state:"complete",observation_ids:["record-1"]}
})]}' "$request" >"$output/transport.json"
filter='.'
case "$mode" in
 missing) filter='.observations=[]' ;;
 wrong-identity) filter='.observations[0].incarnation="unrelated-incarnation"' ;;
 missing-zero) filter='.observations[0] += {presence:"missing",payload:0,reason:"unavailable"}' ;;
 missing-artifact) rm "$output/sample.json" ;;
 duplicate) filter='.observations[0].raw_reference.observation_ids += ["record-2"] | .observations += [(.observations[0] + {record_id:"record-2",producer_sequence:"2"})]' ;;
 boolean-counter) filter='.observations[0].iteration=true' ;;
esac
jq "$filter" "$output/transport.json" >"$output/transport.next"
mv "$output/transport.next" "$output/transport.json"
if [[ "$mode" == fault-* ]]; then
	jq --arg mode "$mode" '.request.fault_schedule[0] | {fault_id,action,status:(if $mode=="fault-not-applied" then "requested" else "applied" end),observed_state:.required_state,prior_exit:true,ready:($mode!="fault-restart-unready"),predecessor}' "$request" >"$output/receipt.json"
	hash="$(sha256sum "$output/receipt.json" | awk '{print $1}')"
	size="$(wc -c <"$output/receipt.json")"
	jq --slurpfile receipt "$output/receipt.json" --arg hash "$hash" --argjson size "$size" '.observations += [(.observations[0] + {event_kind:"fault_acknowledgment",record_id:"receipt-1",event_id:"fault-event-1",producer_sequence:"2",payload:$receipt[0],raw_reference:{path:"receipt.json",bytes:$size,sha256:$hash,producer:"fixture-executor",capture_state:"complete",observation_ids:["receipt-1"]}})]' "$output/transport.json" >"$output/transport.next"
	mv "$output/transport.next" "$output/transport.json"
fi
if [[ "$mode" == stop-after-sample ]]; then "$SOAK_HARNESS_BIN" stop --output "$SOAK_OUTPUT_DIR"; fi
if [[ "$mode" == timeout ]]; then sleep 3; fi
if [[ "$mode" == failure-then-stop && "$iteration" == 2 ]]; then
	printf 'Synthetic resource stop.\n' >"$SOAK_OUTPUT_DIR/host-guardian-breach.txt"
fi
