#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL="$ROOT/formal/storm/uptime/shard_reliability.prism"
PROPERTIES="$ROOT/formal/storm/uptime/shard_reliability.props"
PROFILE_DIR="$ROOT/formal/storm/uptime/profiles"
ENVELOPE="$PROFILE_DIR/current-compose-envelope.json"
LOG_DIR="$ROOT/target/verification/uptime/storm"
mkdir -p "$LOG_DIR"

for tool in storm storm-pars jq awk rg sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "error: required uptime verification tool is unavailable: $tool" >&2
    exit 1
  }
done

mapfile -t formal_authority_sources < <(
  cd "$ROOT"
  {
    rg --files formal
    rg --files scripts | rg 'check-(cost-accounted-rho|finalized-floor|fork-choice|slashing|tlc-source-binding|uptime)|scripts/lib/tlc-run.sh'
  } | rg '\.(cfg|json|mcf|mcrl2|prism|props|py|rs|sage|sh|tla|v|wl)$' | sort
)
mapfile -t implementation_sources < <(
  cd "$ROOT"
  rg --files \
    casper/src/rust \
    comm/src/rust/transport \
    docker/conf \
    docker/genesis \
    models/src/rust/casper \
    node/src/main/resources/defaults.conf \
    node/src/rust \
    rholang/src/rust/interpreter/accounting \
    rholang/src/rust/interpreter/metering.rs | sort
)
formal_authority_hash_start="$(cd "$ROOT" && sha256sum "${formal_authority_sources[@]}" | sha256sum | awk '{print $1}')"
implementation_hash_start="$(cd "$ROOT" && sha256sum "${implementation_sources[@]}" | sha256sum | awk '{print $1}')"

for source in "$MODEL" "$PROPERTIES" \
  "$ROOT/formal/storm/uptime/component_parametric.prism" \
  "$ROOT/formal/storm/uptime/component_parametric.props" \
  "$ROOT/formal/storm/uptime/soak_backtest.prism" \
  "$ROOT/formal/storm/uptime/soak_backtest.props" \
  "$ROOT/formal/storm/uptime/backtests/soak-31563121791.json" \
  "$ROOT/formal/storm/uptime/backtests/public-rss-guard.json" \
  "$PROFILE_DIR/calibrated-profile.schema.json" \
  "$PROFILE_DIR/unsafe-memory-leak.json" \
  "$ENVELOPE"; do
  test -s "$source" || {
    echo "error: missing or empty uptime verification source: $source" >&2
    exit 1
  }
done

profile_constants() {
  local profile="$1"
  jq -er '
    .schema_version == 1 and
    (.profile_id | type == "string" and length > 0) and
    (.evidence_class | IN("analytic_control", "negative_control", "engineering_envelope", "calibrated_projection")) and
    (.constants.VALIDATORS >= 1) and
    (.constants.QUORUM >= 1 and .constants.QUORUM <= .constants.VALIDATORS) and
    (.constants.QUEUE_CAP >= 1) and
    (.constants.LAG_CAP >= .constants.LAG_SLO) and
    (.constants.MEMORY_CAP >= 1) and
    (.constants.INITIAL_ELIGIBLE >= 0 and .constants.INITIAL_ELIGIBLE <= .constants.VALIDATORS) and
    (.constants.INITIAL_QUEUE >= 0 and .constants.INITIAL_QUEUE <= .constants.QUEUE_CAP) and
    (.constants.INITIAL_LAG >= 0 and .constants.INITIAL_LAG <= .constants.LAG_CAP) and
    (.constants.INITIAL_MEMORY >= 0 and .constants.INITIAL_MEMORY <= .constants.MEMORY_CAP) and
    (.constants.HORIZON_HOURS > 0)
  ' "$profile" >/dev/null
  jq -r '.constants | to_entries | map("\(.key)=\(.value|tostring)") | join(",")' "$profile"
}

validate_envelope_case() {
  local profile="$1" case_name="$2"
  jq -e --arg case_name "$case_name" --slurpfile profile "$profile" '
    .schema_version == 1 and
    .evidence_class == "engineering_envelope" and
    .envelope_id == $profile[0].envelope_id and
    $profile[0].case == $case_name and
    ((.parameters | keys) == ($profile[0].constants | keys)) and
    ([.parameters | to_entries[] | .value[$case_name] == $profile[0].constants[.key]] | all)
  ' "$ENVELOPE" >/dev/null || {
    echo "error: $case_name profile does not exactly instantiate the declared parameter envelope" >&2
    exit 1
  }
}

run_property() {
  local profile="$1" property="$2" id constants result log
  id="$(jq -er '.profile_id' "$profile")"
  constants="$(profile_constants "$profile")"
  run_constants_property "$id" "$constants" "$property"
}

run_constants_property() {
  local id="$1" constants="$2" property="$3" result log
  result="$LOG_DIR/$id-$property.json"
  log="$LOG_DIR/$id-$property.log"
  storm \
    --prism "$MODEL" \
    --prop "$PROPERTIES" "$property" \
    --constants "$constants" \
    --sound \
    --precision 1e-10 \
    --prismcompat \
    --exportresult "$result" >"$log" 2>&1
  jq -er '.[0].v | numbers' "$result"
}

run_backtest_property() {
  local property="$1" result log
  result="$LOG_DIR/backtest-$property.json"
  log="$LOG_DIR/backtest-$property.log"
  storm \
    --prism "$ROOT/formal/storm/uptime/soak_backtest.prism" \
    --prop "$ROOT/formal/storm/uptime/soak_backtest.props" "$property" \
    --constants "ITERATIONS=19,OBSERVED_FAILURES=18,FAILURE_PROBABILITY=10/11" \
    --sound \
    --precision 1e-10 \
    --prismcompat \
    --exportresult "$result" >"$log" 2>&1
  jq -er '.[0].v | numbers' "$result"
}

analytic_profile="$PROFILE_DIR/ci-analytic.json"
analytic_actual="$(run_property "$analytic_profile" month_quorum_loss)"
analytic_expected="$(awk 'BEGIN { printf "%.15f", 1 - exp(-1) }')"
awk -v actual="$analytic_actual" -v expected="$analytic_expected" 'BEGIN {
  delta = actual - expected;
  if (delta < 0) delta = -delta;
  exit !(delta <= 1e-10)
}' || {
  echo "error: analytic reliability control diverged: actual=$analytic_actual expected=$analytic_expected" >&2
  exit 1
}

recovery_profile="$PROFILE_DIR/recovery-quorum-loss.json"
recovery_probability="$(run_property "$recovery_profile" eventual_recovery)"
recovery_hours="$(run_property "$recovery_profile" expected_recovery_hours)"
awk -v probability="$recovery_probability" -v hours="$recovery_hours" 'BEGIN {
  exit !(probability == 1 && hours > 0 && hours < 0.1)
}' || {
  echo "error: recoverable quorum-loss control did not recover within its declared bound" >&2
  exit 1
}

no_repair_probability="$(run_property "$PROFILE_DIR/unsafe-no-repair.json" eventual_recovery)"
awk -v probability="$no_repair_probability" 'BEGIN { exit !(probability == 0) }' || {
  echo "error: no-repair negative control unexpectedly recovered" >&2
  exit 1
}

overload_probability="$(run_property "$PROFILE_DIR/unsafe-overload.json" month_queue_cap)"
awk -v probability="$overload_probability" 'BEGIN { exit !(probability > 0.99) }' || {
  echo "error: overload negative control did not reach its queue cap" >&2
  exit 1
}

memory_leak_probability="$(run_property "$PROFILE_DIR/unsafe-memory-leak.json" month_memory_cap)"
awk -v probability="$memory_leak_probability" 'BEGIN { exit !(probability > 0.99) }' || {
  echo "error: unreclaimed-memory negative control did not reach its resident-memory cap" >&2
  exit 1
}

backtest_manifest="$ROOT/formal/storm/uptime/backtests/soak-31563121791.json"
backtest_node_revision="$(jq -er '.node_revision' "$backtest_manifest")"
backtest_control_revision="$(jq -er '.control_revision' "$backtest_manifest")"
git cat-file -e "$backtest_node_revision^{commit}"
git cat-file -e "$backtest_control_revision^{commit}"
backtest_bond_count="$(git show "${backtest_node_revision}:docker/genesis/bonds.txt" | awk 'NF == 2 { count++ } END { print count + 0 }')"
backtest_ftt="$(git show "${backtest_node_revision}:docker/conf/default.conf" | awk '/fault-tolerance-threshold/ { print $3; exit }')"
backtest_workflow="$(git show "${backtest_control_revision}:.github/workflows/merge-recovery-soak.yml")"
test "$backtest_bond_count" = "3" && test "$backtest_ftt" = "0.1" || {
  echo "error: historical topology reconstruction diverged from its manifest" >&2
  exit 1
}
grep -Fq 'SOAK_RSS_CEILING_MB: "45056"' <<<"$backtest_workflow"
grep -Fq 'SOAK_HOST_FREE_FLOOR_MB: "8192"' <<<"$backtest_workflow"
grep -Fq 'SYSTEM_INTEGRATION_REF: 755301662e4f86ffb55f51077b88f32212a1f3ed' <<<"$backtest_workflow"
backtest_probability="$(run_backtest_property observed_or_worse_probability)"
backtest_expected_failures="$(run_backtest_property expected_failures)"
awk -v probability="$backtest_probability" -v expected="$backtest_expected_failures" 'BEGIN {
  exit !(probability >= 0.05 && expected >= 17 && expected <= 18)
}' || {
  echo "error: held-out soak behavior falls outside the declared aggregate backtest" >&2
  exit 1
}

rss_guard_manifest="$ROOT/formal/storm/uptime/backtests/public-rss-guard.json"
jq -e '
  .schema_version == 1 and
  .evidence_class == "historical_backtest" and
  (.sources | length == 2) and
  (.records | length == 9) and
  ([.records[] |
    (.run_id | numbers) and
    (.target_revision | type == "string" and length == 40) and
    (.rss_peak_mb > 0) and
    (.rss_ceiling_mb > 0) and
    (.observed_protection_breach | type == "boolean") and
    (.direct_rss_guard_crossing | type == "boolean")
  ] | all)
' "$rss_guard_manifest" >/dev/null || {
  echo "error: public RSS-guard backtest manifest is malformed" >&2
  exit 1
}
while IFS= read -r record; do
  revision="$(jq -r '.target_revision' <<<"$record")"
  expected_ceiling="$(jq -r '.rss_ceiling_mb' <<<"$record")"
  peak="$(jq -r '.rss_peak_mb' <<<"$record")"
  expected_crossing="$(jq -r '.direct_rss_guard_crossing' <<<"$record")"
  observed_breach="$(jq -r '.observed_protection_breach' <<<"$record")"
  git cat-file -e "$revision^{commit}"
  revision_workflow="$(git show "$revision:.github/workflows/merge-recovery-soak.yml")"
  reconstructed_ceiling="$(awk -F'"' '/SOAK_RSS_CEILING_MB:/ { print $2; exit }' <<<"$revision_workflow")"
  test "$reconstructed_ceiling" = "$expected_ceiling" || {
    echo "error: reconstructed RSS ceiling $reconstructed_ceiling differs from $expected_ceiling at $revision" >&2
    exit 1
  }
  actual_crossing=false
  (( peak >= expected_ceiling )) && actual_crossing=true
  test "$actual_crossing" = "$expected_crossing" || {
    echo "error: RSS crossing classification differs for revision $revision" >&2
    exit 1
  }
  if [[ "$actual_crossing" == true && "$observed_breach" != true ]]; then
    echo "error: historical RSS guard crossing did not coincide with a protection breach" >&2
    exit 1
  fi
done < <(jq -c '.records[]' "$rss_guard_manifest")
rss_guard_crossings="$(jq '[.records[] | select(.direct_rss_guard_crossing)] | length' "$rss_guard_manifest")"
rss_guard_crossing_breaches="$(jq '[.records[] | select(.direct_rss_guard_crossing and .observed_protection_breach)] | length' "$rss_guard_manifest")"
rss_guard_below_cap_breaches="$(jq '[.records[] | select((.direct_rss_guard_crossing | not) and .observed_protection_breach)] | length' "$rss_guard_manifest")"
test "$rss_guard_crossings" = "$rss_guard_crossing_breaches" || {
  echo "error: historical RSS-guard backtest has a false-negative direct crossing" >&2
  exit 1
}

pessimistic_profile="$PROFILE_DIR/month-pessimistic-engineering.json"
nominal_profile="$PROFILE_DIR/month-engineering-bound.json"
optimistic_profile="$PROFILE_DIR/month-optimistic-engineering.json"

validate_envelope_case "$pessimistic_profile" worst
validate_envelope_case "$nominal_profile" expected
validate_envelope_case "$optimistic_profile" best

pessimistic_lifetime="$(run_property "$pessimistic_profile" mean_time_to_service_loss)"
nominal_lifetime="$(run_property "$nominal_profile" mean_time_to_service_loss)"
optimistic_lifetime="$(run_property "$optimistic_profile" mean_time_to_service_loss)"
pessimistic_survival="$(run_property "$pessimistic_profile" month_survival)"
nominal_survival="$(run_property "$nominal_profile" month_survival)"
optimistic_survival="$(run_property "$optimistic_profile" month_survival)"
pessimistic_down_hours="$(run_property "$pessimistic_profile" month_down_hours)"
nominal_down_hours="$(run_property "$nominal_profile" month_down_hours)"
pessimistic_resource_exhaustion="$(run_property "$pessimistic_profile" month_resource_exhaustion)"
nominal_resource_exhaustion="$(run_property "$nominal_profile" month_resource_exhaustion)"
optimistic_down_hours="$(run_property "$optimistic_profile" month_down_hours)"
optimistic_resource_exhaustion="$(run_property "$optimistic_profile" month_resource_exhaustion)"
pessimistic_memory_exhaustion="$(run_property "$pessimistic_profile" month_memory_cap)"
nominal_memory_exhaustion="$(run_property "$nominal_profile" month_memory_cap)"
optimistic_memory_exhaustion="$(run_property "$optimistic_profile" month_memory_cap)"
pessimistic_quorum_loss="$(run_property "$pessimistic_profile" month_quorum_loss)"
nominal_quorum_loss="$(run_property "$nominal_profile" month_quorum_loss)"
optimistic_quorum_loss="$(run_property "$optimistic_profile" month_quorum_loss)"
pessimistic_common_failure="$(run_property "$pessimistic_profile" month_common_failure)"
nominal_common_failure="$(run_property "$nominal_profile" month_common_failure)"
optimistic_common_failure="$(run_property "$optimistic_profile" month_common_failure)"
pessimistic_storage_failure="$(run_property "$pessimistic_profile" month_storage_unavailable)"
nominal_storage_failure="$(run_property "$nominal_profile" month_storage_unavailable)"
optimistic_storage_failure="$(run_property "$optimistic_profile" month_storage_unavailable)"

pessimistic_first_quorum="$(run_property "$pessimistic_profile" first_loss_quorum)"
pessimistic_first_common="$(run_property "$pessimistic_profile" first_loss_common)"
pessimistic_first_storage="$(run_property "$pessimistic_profile" first_loss_storage)"
pessimistic_first_queue="$(run_property "$pessimistic_profile" first_loss_queue)"
pessimistic_first_lag="$(run_property "$pessimistic_profile" first_loss_lag)"
pessimistic_first_memory="$(run_property "$pessimistic_profile" first_loss_memory)"
nominal_first_quorum="$(run_property "$nominal_profile" first_loss_quorum)"
nominal_first_common="$(run_property "$nominal_profile" first_loss_common)"
nominal_first_storage="$(run_property "$nominal_profile" first_loss_storage)"
nominal_first_queue="$(run_property "$nominal_profile" first_loss_queue)"
nominal_first_lag="$(run_property "$nominal_profile" first_loss_lag)"
nominal_first_memory="$(run_property "$nominal_profile" first_loss_memory)"
optimistic_first_quorum="$(run_property "$optimistic_profile" first_loss_quorum)"
optimistic_first_common="$(run_property "$optimistic_profile" first_loss_common)"
optimistic_first_storage="$(run_property "$optimistic_profile" first_loss_storage)"
optimistic_first_queue="$(run_property "$optimistic_profile" first_loss_queue)"
optimistic_first_lag="$(run_property "$optimistic_profile" first_loss_lag)"
optimistic_first_memory="$(run_property "$optimistic_profile" first_loss_memory)"

validate_first_loss_partition() {
  local case_name="$1" survival="$2" quorum="$3" common="$4" storage="$5" queue="$6" lag="$7" memory="$8"
  awk \
    -v case_name="$case_name" \
    -v survival="$survival" \
    -v quorum="$quorum" \
    -v common="$common" \
    -v storage="$storage" \
    -v queue="$queue" \
    -v lag="$lag" \
    -v memory="$memory" \
    'BEGIN {
      total = survival + quorum + common + storage + queue + lag + memory;
      residual = total - 1;
      if (residual < 0) residual = -residual;
      valid = survival >= 0 && survival <= 1 &&
        quorum >= 0 && quorum <= 1 && common >= 0 && common <= 1 &&
        storage >= 0 && storage <= 1 && queue >= 0 && queue <= 1 &&
        lag >= 0 && lag <= 1 && memory >= 0 && memory <= 1;
      if (!valid || residual > 1e-8) {
        printf "error: %s first-loss partition invalid: total=%.15g residual=%.15g\n", case_name, total, residual > "/dev/stderr";
        exit 1;
      }
    }'
}

validate_first_loss_partition worst "$pessimistic_survival" "$pessimistic_first_quorum" "$pessimistic_first_common" "$pessimistic_first_storage" "$pessimistic_first_queue" "$pessimistic_first_lag" "$pessimistic_first_memory"
validate_first_loss_partition expected "$nominal_survival" "$nominal_first_quorum" "$nominal_first_common" "$nominal_first_storage" "$nominal_first_queue" "$nominal_first_lag" "$nominal_first_memory"
validate_first_loss_partition best "$optimistic_survival" "$optimistic_first_quorum" "$optimistic_first_common" "$optimistic_first_storage" "$optimistic_first_queue" "$optimistic_first_lag" "$optimistic_first_memory"

sensitivity_jsonl="$LOG_DIR/parameter-sensitivity.jsonl"
sensitivity_json="$LOG_DIR/parameter-sensitivity.json"
: >"$sensitivity_jsonl"
while IFS= read -r parameter; do
  adverse_constants="$(jq -r \
    --arg parameter "$parameter" \
    --slurpfile envelope "$ENVELOPE" \
    '.constants | .[$parameter] = $envelope[0].parameters[$parameter].worst |
      to_entries | map("\(.key)=\(.value|tostring)") | join(",")' \
    "$nominal_profile")"
  favorable_constants="$(jq -r \
    --arg parameter "$parameter" \
    --slurpfile envelope "$ENVELOPE" \
    '.constants | .[$parameter] = $envelope[0].parameters[$parameter].best |
      to_entries | map("\(.key)=\(.value|tostring)") | join(",")' \
    "$nominal_profile")"
  adverse_lifetime="$(run_constants_property "sensitivity-$parameter-adverse" "$adverse_constants" mean_time_to_service_loss)"
  favorable_lifetime="$(run_constants_property "sensitivity-$parameter-favorable" "$favorable_constants" mean_time_to_service_loss)"
  adverse_survival="$(run_constants_property "sensitivity-$parameter-adverse" "$adverse_constants" month_survival)"
  favorable_survival="$(run_constants_property "sensitivity-$parameter-favorable" "$favorable_constants" month_survival)"
  awk \
    -v parameter="$parameter" \
    -v adverse_lifetime="$adverse_lifetime" \
    -v central_lifetime="$nominal_lifetime" \
    -v favorable_lifetime="$favorable_lifetime" \
    -v adverse_survival="$adverse_survival" \
    -v central_survival="$nominal_survival" \
    -v favorable_survival="$favorable_survival" \
    'BEGIN {
      lifetime_tolerance = central_lifetime * 1e-8 + 1e-9;
      probability_tolerance = 1e-8;
      if (adverse_lifetime > central_lifetime + lifetime_tolerance ||
          central_lifetime > favorable_lifetime + lifetime_tolerance ||
          adverse_survival > central_survival + probability_tolerance ||
          central_survival > favorable_survival + probability_tolerance) {
        printf "error: %s one-at-a-time sensitivity violates envelope direction\n", parameter > "/dev/stderr";
        exit 1;
      }
    }'
  jq -cn \
    --arg parameter "$parameter" \
    --arg unit "$(jq -r --arg parameter "$parameter" '.parameters[$parameter].unit' "$ENVELOPE")" \
    --arg source "$(jq -r --arg parameter "$parameter" '.parameters[$parameter].source' "$ENVELOPE")" \
    --argjson adverse_value "$(jq -c --arg parameter "$parameter" '.parameters[$parameter].worst' "$ENVELOPE")" \
    --argjson central_value "$(jq -c --arg parameter "$parameter" '.parameters[$parameter].expected' "$ENVELOPE")" \
    --argjson favorable_value "$(jq -c --arg parameter "$parameter" '.parameters[$parameter].best' "$ENVELOPE")" \
    --argjson adverse_lifetime "$adverse_lifetime" \
    --argjson central_lifetime "$nominal_lifetime" \
    --argjson favorable_lifetime "$favorable_lifetime" \
    --argjson adverse_survival "$adverse_survival" \
    --argjson central_survival "$nominal_survival" \
    --argjson favorable_survival "$favorable_survival" \
    'def absolute: if . < 0 then -. else . end;
    (($central_lifetime - $adverse_lifetime) / $central_lifetime) as $adverse_lifetime_fraction |
    (($favorable_lifetime - $central_lifetime) / $central_lifetime) as $favorable_lifetime_fraction |
    {
      parameter: $parameter,
      unit: $unit,
      source: $source,
      values: {adverse: $adverse_value, central: $central_value, favorable: $favorable_value},
      expected_lifetime_hours: {adverse: $adverse_lifetime, central: $central_lifetime, favorable: $favorable_lifetime},
      month_survival_probability: {adverse: $adverse_survival, central: $central_survival, favorable: $favorable_survival},
      lifetime_fractional_change: {adverse: $adverse_lifetime_fraction, favorable: $favorable_lifetime_fraction},
      survival_probability_point_change: {
        adverse: ($central_survival - $adverse_survival),
        favorable: ($favorable_survival - $central_survival)
      },
      ranking_score: ([$adverse_lifetime_fraction | absolute, $favorable_lifetime_fraction | absolute] | max)
    }' >>"$sensitivity_jsonl"
done < <(jq -r '
  .parameters | to_entries[] |
  select(.value.provenance == "assumption_hole") |
  select(.value.worst != .value.expected or .value.expected != .value.best) |
  .key
' "$ENVELOPE")
jq -s 'sort_by(-.ranking_score, .parameter)' "$sensitivity_jsonl" >"$sensitivity_json"
test "$(jq 'length' "$sensitivity_json")" -gt 0 || {
  echo "error: uptime sensitivity analysis produced no parameters" >&2
  exit 1
}

awk \
  -v minimum="$pessimistic_lifetime" \
  -v average="$nominal_lifetime" \
  -v maximum="$optimistic_lifetime" \
  'BEGIN { exit !(minimum > 0 && minimum <= average && average <= maximum) }' || {
  echo "error: projected expected-lifetime envelope is invalid" >&2
  exit 1
}

awk \
  -v minimum="$pessimistic_survival" \
  -v average="$nominal_survival" \
  -v maximum="$optimistic_survival" \
  'BEGIN { exit !(minimum >= 0 && minimum <= average && average <= maximum && maximum <= 1) }' || {
  echo "error: projected 30-day survival envelope is invalid" >&2
  exit 1
}

parametric_log="$LOG_DIR/component-parametric.log"
storm-pars \
  --prism "$ROOT/formal/storm/uptime/component_parametric.prism" \
  --prop "$ROOT/formal/storm/uptime/component_parametric.props" \
  --parametric \
  --parametric:mode solutionfunction \
  --prismcompat >"$parametric_log" 2>&1
grep -Eq 'Result.*\(1\).*/.*\(lambda\)' "$parametric_log" || {
  echo "error: Storm-pars did not derive the exact mean-time-to-failure function" >&2
  exit 1
}
grep -Eq 'Result.*\(mu\).*(mu\+lambda|lambda\+mu)' "$parametric_log" || {
  echo "error: Storm-pars did not derive the exact steady-state availability function" >&2
  exit 1
}

model_hash="$(sha256sum \
  "$MODEL" "$PROPERTIES" \
  "$ROOT/formal/storm/uptime/component_parametric.prism" \
  "$ROOT/formal/storm/uptime/component_parametric.props" \
  "$ROOT/formal/storm/uptime/soak_backtest.prism" \
  "$ROOT/formal/storm/uptime/soak_backtest.props" | sha256sum | awk '{print $1}')"
formal_authority_hash="$(cd "$ROOT" && sha256sum "${formal_authority_sources[@]}" | sha256sum | awk '{print $1}')"
profile_hash="$(sha256sum "$ENVELOPE" "$pessimistic_profile" "$nominal_profile" "$optimistic_profile" | sha256sum | awk '{print $1}')"
implementation_hash="$(cd "$ROOT" && sha256sum "${implementation_sources[@]}" | sha256sum | awk '{print $1}')"
if [[ "$formal_authority_hash" != "$formal_authority_hash_start" ||
      "$implementation_hash" != "$implementation_hash_start" ]]; then
  echo "error: implementation or formal-authority inputs changed during uptime verification" >&2
  exit 75
fi
git_commit="$(git -C "$ROOT" rev-parse HEAD)"
if git -C "$ROOT" diff --quiet -- "${implementation_sources[@]}" &&
  [[ -z "$(git -C "$ROOT" ls-files --others --exclude-standard -- "${implementation_sources[@]}")" ]]; then
  implementation_dirty=false
else
  implementation_dirty=true
fi
if git -C "$ROOT" diff --quiet -- "${formal_authority_sources[@]}" &&
  [[ -z "$(git -C "$ROOT" ls-files --others --exclude-standard -- "${formal_authority_sources[@]}")" ]]; then
  formal_authority_dirty=false
else
  formal_authority_dirty=true
fi
generated_at="$(date -u +%FT%TZ)"

jq -n \
  --arg model_hash "$model_hash" \
  --arg formal_authority_hash "$formal_authority_hash" \
  --arg profile_hash "$profile_hash" \
  --arg implementation_hash "$implementation_hash" \
  --arg git_commit "$git_commit" \
  --argjson implementation_dirty "$implementation_dirty" \
  --argjson formal_authority_dirty "$formal_authority_dirty" \
  --arg generated_at "$generated_at" \
  --slurpfile envelope "$ENVELOPE" \
  --slurpfile worst_profile "$pessimistic_profile" \
  --slurpfile expected_profile "$nominal_profile" \
  --slurpfile best_profile "$optimistic_profile" \
  --slurpfile backtest_manifest "$backtest_manifest" \
  --slurpfile rss_guard_manifest "$rss_guard_manifest" \
  --slurpfile sensitivity "$sensitivity_json" \
  --argjson worst_lifetime "$pessimistic_lifetime" \
  --argjson expected_lifetime "$nominal_lifetime" \
  --argjson best_lifetime "$optimistic_lifetime" \
  --argjson worst_survival "$pessimistic_survival" \
  --argjson expected_survival "$nominal_survival" \
  --argjson best_survival "$optimistic_survival" \
  --argjson worst_down_hours "$pessimistic_down_hours" \
  --argjson expected_down_hours "$nominal_down_hours" \
  --argjson best_down_hours "$optimistic_down_hours" \
  --argjson worst_resource "$pessimistic_resource_exhaustion" \
  --argjson expected_resource "$nominal_resource_exhaustion" \
  --argjson best_resource "$optimistic_resource_exhaustion" \
  --argjson worst_memory "$pessimistic_memory_exhaustion" \
  --argjson expected_memory "$nominal_memory_exhaustion" \
  --argjson best_memory "$optimistic_memory_exhaustion" \
  --argjson worst_quorum "$pessimistic_quorum_loss" \
  --argjson expected_quorum "$nominal_quorum_loss" \
  --argjson best_quorum "$optimistic_quorum_loss" \
  --argjson worst_common "$pessimistic_common_failure" \
  --argjson expected_common "$nominal_common_failure" \
  --argjson best_common "$optimistic_common_failure" \
  --argjson worst_storage "$pessimistic_storage_failure" \
  --argjson expected_storage "$nominal_storage_failure" \
  --argjson best_storage "$optimistic_storage_failure" \
  --argjson worst_first_quorum "$pessimistic_first_quorum" \
  --argjson worst_first_common "$pessimistic_first_common" \
  --argjson worst_first_storage "$pessimistic_first_storage" \
  --argjson worst_first_queue "$pessimistic_first_queue" \
  --argjson worst_first_lag "$pessimistic_first_lag" \
  --argjson worst_first_memory "$pessimistic_first_memory" \
  --argjson expected_first_quorum "$nominal_first_quorum" \
  --argjson expected_first_common "$nominal_first_common" \
  --argjson expected_first_storage "$nominal_first_storage" \
  --argjson expected_first_queue "$nominal_first_queue" \
  --argjson expected_first_lag "$nominal_first_lag" \
  --argjson expected_first_memory "$nominal_first_memory" \
  --argjson best_first_quorum "$optimistic_first_quorum" \
  --argjson best_first_common "$optimistic_first_common" \
  --argjson best_first_storage "$optimistic_first_storage" \
  --argjson best_first_queue "$optimistic_first_queue" \
  --argjson best_first_lag "$optimistic_first_lag" \
  --argjson best_first_memory "$optimistic_first_memory" \
  --argjson backtest_probability "$backtest_probability" \
  --argjson backtest_expected_failures "$backtest_expected_failures" \
  --argjson rss_guard_crossings "$rss_guard_crossings" \
  --argjson rss_guard_crossing_breaches "$rss_guard_crossing_breaches" \
  --argjson rss_guard_below_cap_breaches "$rss_guard_below_cap_breaches" \
  --argjson analytic_probability "$analytic_actual" \
  --argjson recovery_probability "$recovery_probability" \
  --argjson recovery_hours "$recovery_hours" \
  --argjson no_repair_probability "$no_repair_probability" \
  --argjson overload_probability "$overload_probability" \
  --argjson memory_leak_probability "$memory_leak_probability" \
  'def absolute: if . < 0 then -. else . end;
  def first_cause($probability; $loss_probability): {
    probability: $probability,
    conditional_share: (if $loss_probability > 0 then $probability / $loss_probability else 0 end)
  };
  def first_loss($survival; $quorum; $common; $storage; $queue; $lag; $memory):
    (1 - $survival) as $loss_probability |
    ($quorum + $common + $storage + $queue + $lag + $memory) as $partition_sum |
    {
      within_horizon_probability: $loss_probability,
      partition_sum: $partition_sum,
      partition_residual: (($loss_probability - $partition_sum) | absolute),
      causes: {
        quorum: first_cause($quorum; $loss_probability),
        common: first_cause($common; $loss_probability),
        storage: first_cause($storage; $loss_probability),
        queue: first_cause($queue; $loss_probability),
        lag: first_cause($lag; $loss_probability),
        memory: first_cause($memory; $loss_probability)
      }
    };
  def scenario($name; $profile; $lifetime; $survival; $down; $resource; $memory; $quorum; $common; $storage; $first_quorum; $first_common; $first_storage; $first_queue; $first_lag; $first_memory): {
    case: $name,
    interpretation: $envelope[0].interpretation[$name],
    parameters: $profile[0].constants,
    expected_lifetime_hours: $lifetime,
    month_uninterrupted_survival_probability: $survival,
    expected_month_down_hours: $down,
    month_resource_exhaustion_probability: $resource,
    month_memory_cap_probability: $memory,
    month_quorum_loss_probability: $quorum,
    month_common_failure_probability: $common,
    month_storage_unavailability_probability: $storage,
    first_service_loss: first_loss($survival; $first_quorum; $first_common; $first_storage; $first_queue; $first_lag; $first_memory)
  };
  {
    schema_version: 3,
    report_kind: "current_implementation_engineering_envelope",
    evidence_class: "engineering_envelope",
    reportable_with_assumptions: true,
    release_certified: false,
    certification: "not release-certified; current-branch continuous-shard calibration is unavailable",
    horizon_hours: 720,
    generated_at: $generated_at,
    provenance: {
      git_commit: $git_commit,
      implementation_dirty: $implementation_dirty,
      formal_authority_dirty: $formal_authority_dirty,
      implementation_hash: $implementation_hash,
      model_hash: $model_hash,
      formal_authority_hash: $formal_authority_hash,
      profile_hash: $profile_hash
    },
    assumptions: $envelope[0],
    scenarios: [
      scenario("worst"; $worst_profile; $worst_lifetime; $worst_survival; $worst_down_hours; $worst_resource; $worst_memory; $worst_quorum; $worst_common; $worst_storage; $worst_first_quorum; $worst_first_common; $worst_first_storage; $worst_first_queue; $worst_first_lag; $worst_first_memory),
      scenario("expected"; $expected_profile; $expected_lifetime; $expected_survival; $expected_down_hours; $expected_resource; $expected_memory; $expected_quorum; $expected_common; $expected_storage; $expected_first_quorum; $expected_first_common; $expected_first_storage; $expected_first_queue; $expected_first_lag; $expected_first_memory),
      scenario("best"; $best_profile; $best_lifetime; $best_survival; $best_down_hours; $best_resource; $best_memory; $best_quorum; $best_common; $best_storage; $best_first_quorum; $best_first_common; $best_first_storage; $best_first_queue; $best_first_lag; $best_first_memory)
    ],
    parameter_sensitivity: $sensitivity[0],
    historical_backtest: {
      manifest: $backtest_manifest[0],
      expected_failures: $backtest_expected_failures,
      observed_or_worse_probability: $backtest_probability,
      structural_reconstruction_passed: true,
      continuous_uptime_rate_calibrated: false
    },
    rss_guard_backtest: {
      manifest: $rss_guard_manifest[0],
      direct_crossings: $rss_guard_crossings,
      crossing_breaches: $rss_guard_crossing_breaches,
      below_cap_breaches: $rss_guard_below_cap_breaches,
      guard_classification_reproduced: ($rss_guard_crossings == $rss_guard_crossing_breaches),
      continuous_memory_rates_calibrated: false
    },
    controls: {
      analytic_probability: $analytic_probability,
      recoverable_probability: $recovery_probability,
      recoverable_expected_hours: $recovery_hours,
      no_repair_probability: $no_repair_probability,
      overload_probability: $overload_probability,
      memory_leak_probability: $memory_leak_probability
    }
  }' >"$LOG_DIR/engineering-envelope.json"

if [[ "${UPTIME_RELEASE_CERTIFY:-0}" == "1" ]]; then
  calibration="${UPTIME_CALIBRATION_PROFILE:-}"
  test -n "$calibration" && test -f "$calibration" || {
    echo "error: release uptime projection requires UPTIME_CALIBRATION_PROFILE" >&2
    exit 1
  }
  test "$implementation_dirty" = false && test "$formal_authority_dirty" = false || {
    echo "error: release uptime projection cannot bind to dirty implementation or formal authorities" >&2
    exit 1
  }
  jq -e \
    --arg commit "$git_commit" \
    --arg implementation_hash "$implementation_hash" \
    --arg model_hash "$model_hash" \
    --arg formal_authority_hash "$formal_authority_hash" '
    .evidence_class == "calibrated_projection" and
    .calibration.status == "calibrated" and
    .calibration.source_commit == $commit and
    .calibration.implementation_hash == $implementation_hash and
    .calibration.model_hash == $model_hash and
    .calibration.formal_authority_hash == $formal_authority_hash and
    (.calibration.valid_until | type == "string" and length > 0) and
    (.calibration.continuous_shard_hours >= 60) and
    (.calibration.telemetry_complete == true) and
    (.calibration.right_censoring_recorded == true) and
    (.calibration.confidence_level >= 0.95) and
    ((.parameters | keys) == (.constants | keys)) and
    ([.parameters | to_entries[] |
      (.value.unit | type == "string" and length > 0) and
      (.value.source | type == "string" and length > 0) and
      (.value.estimator | type == "string" and length > 0) and
      (.value.lower != null) and (.value.estimate != null) and (.value.upper != null)
    ] | all)
  ' "$calibration" >/dev/null || {
    echo "error: release uptime calibration is stale, incomplete, or not bound to this implementation and model" >&2
    exit 1
  }
  valid_until="$(jq -r '.calibration.valid_until' "$calibration")"
  [[ "$valid_until" < "$(date -u +%F)" ]] && {
    echo "error: release uptime calibration expired on $valid_until" >&2
    exit 1
  }
  calibrated_survival="$(run_property "$calibration" month_survival)"
  calibrated_lifetime="$(run_property "$calibration" mean_time_to_service_loss)"
  required_survival="${UPTIME_MONTH_SURVIVAL_MIN:?release uptime projection requires UPTIME_MONTH_SURVIVAL_MIN}"
  awk -v actual="$calibrated_survival" -v required="$required_survival" 'BEGIN { exit !(actual >= required) }' || {
    echo "error: calibrated 30-day survival $calibrated_survival is below $required_survival" >&2
    exit 1
  }
  jq -n \
    --arg git_commit "$git_commit" \
    --arg implementation_hash "$implementation_hash" \
    --arg model_hash "$model_hash" \
    --arg formal_authority_hash "$formal_authority_hash" \
    --argjson survival "$calibrated_survival" \
    --argjson lifetime "$calibrated_lifetime" \
    --argjson required_survival "$required_survival" \
    --slurpfile calibration "$calibration" \
    '{schema_version: 1, report_kind: "calibrated_uptime_projection", release_certified: true,
      provenance: {git_commit: $git_commit, implementation_hash: $implementation_hash, model_hash: $model_hash,
        formal_authority_hash: $formal_authority_hash},
      parameters: $calibration[0], expected_lifetime_hours: $lifetime,
      month_uninterrupted_survival_probability: $survival, required_month_survival_probability: $required_survival}' \
    >"$LOG_DIR/calibrated-projection.json"
fi

"$ROOT/scripts/render-uptime-engineering-report.sh" "$LOG_DIR/engineering-envelope.json" "$ROOT/target/verification/uptime/engineering-envelope.md"

echo "Storm uptime verification passed."
echo "Engineering envelope: $LOG_DIR/engineering-envelope.json"
