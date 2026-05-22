#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JSON_OUT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --json-out)
      JSON_OUT="$2"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

json_string() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  printf '"%s"' "$value"
}

json_string_array() {
  local csv="$1"
  local first=1
  local item
  printf '['
  IFS=',' read -ra items <<< "$csv"
  for item in "${items[@]}"; do
    item="${item#"${item%%[![:space:]]*}"}"
    item="${item%"${item##*[![:space:]]}"}"
    if [[ -z "$item" ]]; then
      continue
    fi
    if [[ "$first" -eq 0 ]]; then
      printf ', '
    fi
    first=0
    json_string "$item"
  done
  printf ']'
}

surface_json() {
  local id="$1"
  local cost_surface="$2"
  local source_risk="$3"
  local file="$4"
  local symbol="$5"
  local pattern="$6"
  local reachable="$7"
  local expected_state="$8"
  local source_facets="$9"
  local cross_surface_role="${10}"
  local abs_file="$ROOT/$file"
  local match=""
  local line="0"
  local status="absent"
  local found="false"

  if [[ -f "$abs_file" ]] && match="$(rg -n -m 1 -F -- "$pattern" "$abs_file")"; then
    line="${match%%:*}"
    status="present"
    found="true"
  fi
  local source_anchor_digest
  source_anchor_digest="$(printf '%s' "$id|$file|$symbol|$pattern|$line|$status" | sha256sum | awk '{print $1}')"

  printf '    {\n'
  printf '      "id": %s,\n' "$(json_string "$id")"
  printf '      "cost_surface": %s,\n' "$(json_string "$cost_surface")"
  printf '      "source_risk": %s,\n' "$(json_string "$source_risk")"
  printf '      "source_file": %s,\n' "$(json_string "$file")"
  printf '      "source_symbol": %s,\n' "$(json_string "$symbol")"
  printf '      "match_pattern": %s,\n' "$(json_string "$pattern")"
  printf '      "source_line": %s,\n' "$line"
  printf '      "source_surface_status": %s,\n' "$(json_string "$status")"
  printf '      "found": %s,\n' "$found"
  printf '      "expected_state": %s,\n' "$(json_string "$expected_state")"
  printf '      "source_facets": %s,\n' "$(json_string_array "$source_facets")"
  printf '      "source_anchor_digest": %s,\n' "$(json_string "$source_anchor_digest")"
  printf '      "cross_surface_role": %s,\n' "$(json_string "$cross_surface_role")"
  printf '      "reachable_from_user_deploy": %s\n' "$reachable"
  printf '    }'
}

emit() {
  printf '{\n'
  printf '  "repo": "f1r3node-rust",\n'
  printf '  "schema": "cost-accounting-source-surface-v3",\n'
  printf '  "surfaces": [\n'

  local first=1
  add_surface() {
    if [[ "$first" -eq 0 ]]; then
      printf ',\n'
    fi
    first=0
    surface_json "$@"
  }

  add_surface v11_runtime_trace_slot_cap runtime_budget trace_slot_capacity rholang/src/rust/interpreter/accounting/mod.rs cost_trace_event_slots cost_trace_event_slots true present "runtime_budget,trace_commitment,capacity_bound" source
  add_surface v11_runtime_invalid_admission runtime_budget invalid_admission_before_mutation rholang/src/rust/interpreter/accounting/mod.rs validate_billable_event "fn validate_billable_event" true present "runtime_budget,admission,reject_before_mutation" source
  add_surface v11_runtime_oop_singleton runtime_budget oop_boundary_singleton rholang/src/rust/interpreter/accounting/mod.rs last_oop_event last_oop_event true present "runtime_budget,oop_boundary,trace_commitment" source
  add_surface v11_runtime_unmetered_scope runtime_budget unmetered_scope_leak rholang/src/rust/interpreter/accounting/mod.rs enter_unmetered_scope enter_unmetered_scope false present "runtime_budget,system_mode,quarantine" source
  add_surface v11_metering_pending_queue metering pending_queue_ordering rholang/src/rust/interpreter/metering.rs pending "pending: Arc<Mutex" true present "metering,queue_order,source_event_routing" bridge
  add_surface v11_metering_local_index metering local_index_determinism rholang/src/rust/interpreter/metering.rs next_local_index next_local_index true present "metering,local_index,trace_identity" bridge
  add_surface v11_parallel_futures_unordered parallel_eval completion_order_parallelism rholang/src/rust/interpreter/reduce.rs FuturesUnordered FuturesUnordered true present "parallel_eval,completion_order,max_parallelism" bridge
  add_surface v11_parallel_stable_errors parallel_eval stable_error_aggregation rholang/src/rust/interpreter/reduce.rs sort_by_key sort_by_key true present "parallel_eval,stable_error_order,deterministic_reporting" bridge
  add_surface v11_replay_digest_count casper_replay replay_auth_digest_count casper/src/rust/rholang/replay_runtime.rs replay_cost_trace replay_cost_trace true present "casper_replay,digest_count,auth_boundary" sink
  add_surface v11_replay_payload_hash casper_replay replay_payload_cache_key casper/src/rust/util/rholang/runtime_manager.rs replay_payload_hash "fn replay_payload_hash" true present "casper_replay,payload_hash,auth_boundary" sink
  add_surface v11_settlement_checked_charge settlement refund_overflow models/src/rust/casper/protocol/casper_message.rs checked_total_phlo_charge_value "fn checked_total_phlo_charge_value" true present "settlement,checked_arithmetic,overflow_rejection" sink
  add_surface v11_settlement_refund_projection settlement refund_as_fuel models/src/rust/casper/protocol/casper_message.rs refund_amount_for_token_cost "pub fn refund_amount_for_token_cost" true present "settlement,refund_projection,fuel_isolation" sink
  add_surface v11_slashing_system_deploy slashing slashing_evidence_gap casper/src/rust/util/rholang/costacc/slash_deploy.rs SlashDeploy "pub struct SlashDeploy" false present "slashing,evidence_boundary,system_effect" sink
  add_surface v11_slashing_replay_payload slashing slash_field_authentication casper/src/rust/util/rholang/runtime_manager.rs "SystemDeployData::Slash" "SystemDeployData::Slash" false present "slashing,payload_hash,auth_boundary" sink
  add_surface v11_legacy_charging_rspace_absent legacy_quarantine legacy_runtime_metering_downgrade rholang/src/rust/interpreter/storage/charging_rspace.rs ChargingRSpace ChargingRSpace true absent "legacy_quarantine,absent_surface,downgrade_guard" quarantine
  add_surface v14_slashing_epoch_authorization slashing slash_epoch_authorization casper/src/rust/slashing_authorization.rs received_slash_deploy_authorized "pub fn received_slash_deploy_authorized" false present "slashing,authorization,epoch_boundary" sink
  add_surface v14_slashing_duplicate_target slashing duplicate_target_rejection casper/src/rust/slashing_authorization.rs slash_target_key_collides "pub fn slash_target_key_collides" false present "slashing,authorization,duplicate_target" sink
  add_surface v14_slashing_parent_pre_state_authorization slashing parent_pre_state_authorization casper/src/rust/slashing_authorization.rs validate_received_slash_deploys "bonds_map" false present "slashing,authorization,parent_pre_state,bond_boundary" sink
  add_surface v14_slashing_recovered_current_evidence slashing recovered_rejected_current_evidence casper/src/rust/merging/rejected_slash.rs filter_recoverable_with_evidence "pub fn filter_recoverable_with_evidence" false present "slashing,recovered_rejected,current_evidence,epoch_boundary" sink
  add_surface v14_slashing_block_creator_current_epoch slashing recovered_slash_current_epoch_filter casper/src/rust/blocks/proposer/block_creator.rs recovered_target_activation_epoch "recovered_target_activation_epoch" false present "slashing,block_creator,current_epoch,evidence_boundary" sink
  add_surface v14_mergeable_type_domain mergeable_channels merge_type_domain rspace++/src/rspace/merger/merging_logic.rs MergeType "pub enum MergeType" true present "mergeable_channels,merge_type,type_domain" source
  add_surface v14_mergeable_bitmask_combine mergeable_channels bitmask_or_combine rspace++/src/rspace/merger/merging_logic.rs combine_mergeable_value "MergeType::BitmaskOr => ((a as u64) | (b as u64)) as i64" true present "mergeable_channels,bitmask_or,merge_combine" bridge
  add_surface v14_mergeable_bitmask_diff mergeable_channels typed_bitmask_diff_roundtrip rholang/src/rust/interpreter/merging/rholang_merging_logic.rs calculate_num_channel_diff "MergeType::BitmaskOr => ((end_val as u64) & !(*prev_val as u64)) as i64" true present "mergeable_channels,bitmask_or,diff_roundtrip" bridge
  add_surface v14_mergeable_runtime_fold mergeable_channels multi_value_fold_not_max casper/src/rust/rholang/runtime.rs fold_multi_value "MergeType::BitmaskOr => values" true present "mergeable_channels,bitmask_or,deterministic_fold" bridge
  add_surface v14_mergeable_non_numeric_fallback mergeable_channels non_numeric_mergeable_fallback rholang/src/rust/interpreter/merging/rholang_merging_logic.rs try_get_number_with_rnd "pub fn try_get_number_with_rnd" true present "mergeable_channels,non_numeric,fallback_conflict_path" bridge
  add_surface v14_mergeable_tag_propagation mergeable_channels mergeable_tag_type_propagation rholang/src/rust/interpreter/reduce.rs is_mergeable_channel "fn is_mergeable_channel(&self, chan: &Par) -> Option<MergeType>" true present "mergeable_channels,mergeable_tags,type_propagation" source
  add_surface v14_mergeable_store_type_persistence mergeable_channels merge_type_persistence rholang/src/rust/interpreter/merging/rholang_merging_logic.rs NumberChannel "pub merge_type: MergeType" true present "mergeable_channels,store_wire,type_persistence" sink
  add_surface v14_transport_tls_peer_certificates transport_tls peer_certificate_extraction comm/src/rust/transport/f1r3fly_tls_transport.rs peer_certificates "pub fn peer_certificates" false present "transport_tls,peer_identity,certificate_boundary" source
  add_surface v14_transport_tls_key_path transport_tls tls_key_material_path_config comm/src/rust/transport/tls_conf.rs key_path "pub key_path: PathBuf" false present "transport_tls,key_material,path_config" source
  add_surface v14_private_key_debug_surface crypto_key_material debug_secret_exposure crypto/src/rust/private_key.rs PrivateKey "#[derive(Debug, Clone, Eq)]" false present "crypto_key_material,secret_material,debug_boundary" source
  add_surface v14_api_preview_private_names api_ingress private_name_preview_input node/src/rust/api/deploy_grpc_service_v1.rs preview_private_names "async fn preview_private_names" false present "api_ingress,external_request,private_name_preview" source
  add_surface v14_replay_cache_event_log_bound replay_cache cache_event_log_bound casper/src/rust/util/rholang/runtime_manager.rs MAX_REPLAY_CACHE_EVENT_LOG_ENTRIES MAX_REPLAY_CACHE_EVENT_LOG_ENTRIES true present "replay_cache,event_log_bound,replay_boundary" bridge
  add_surface v14_dependency_rustsec_policy dependency_advisory accepted_rustsec_exception deny.toml RUSTSEC-2026-0098 RUSTSEC-2026-0098 false present "dependency_advisory,rustsec,accepted_exception" source

  printf '\n  ]\n'
  printf '}\n'
}

if [[ -n "$JSON_OUT" ]]; then
  mkdir -p "$(dirname "$JSON_OUT")"
  emit > "$JSON_OUT"
else
  emit
fi
