#!/usr/bin/env bash
set -euo pipefail
ROOT="${1:-.}"
MODE="${2:-full}"
[[ "$MODE" == full || "$MODE" == structure ]] || exit 2
cd "$ROOT"
PLAN=docs/plans/casper-ratified-soak-2026-09-16.md
CONTRACT=docs/casper/design/soak-interface-contract.md
MODEL=formal/tlaplus/casper_soak/verification-plan.jsonc
MATRIX=docs/casper/design/soak-candidate-matrix.jsonc
for number in $(seq -w 1 12); do
 [[ "$(grep -c "^| D-$number |" "$PLAN")" == 1 ]]
done
jq -e '.scope=="harness-and-profiles-only" and .node_role=="system-under-test" and .construction=="not-applicable" and .proposed_bounds.candidates==2 and .proposed_bounds.segments==2 and .proposed_bounds.iterations_total==4 and .proposed_bounds.children==1 and (.negative_controls|length)==10 and ([.negative_controls[].fixture]|unique|length)==10 and ([.negative_controls[].property]|unique|length)==10 and all(.negative_controls[];.expected_exit==12) and .positive_control.expected_exit==0 and (.profile_verification.claims|length)==7 and .registration_policy.pr_budget_seconds==900 and .registration_policy.casper_per_configuration_seconds==120 and .registration_policy.casper_kill_grace_seconds==60' "$MODEL" >/dev/null
while IFS= read -r fixture; do grep -Fq "| $fixture |" "$CONTRACT"; done < <(jq -r '.negative_controls[].fixture' "$MODEL")
for profile in authority-finality publication recovery merge-accounting slashing version-phlo carrier-index; do
 claim="docs/claims/casper-soak-$profile.md"
 [[ "$(grep -Ec '^\| [A-Za-z]+ \| [A-Za-z]+ \| [a-z_]+ \|' "$claim")" == 3 ]]
 grep -q '^construction: not-applicable$' "$claim"
 grep -q '^post_merge_tasks: \[TASK-018-' "$claim"
 grep -Fq "casper-soak-$profile.md" "$CONTRACT"
done
for suffix in _complete _capability_missing _generation; do grep -Fq "$suffix" "$CONTRACT"; done
for claim in docs/claims/casper-soak-*.md; do
 while IFS= read -r path; do
  case "$path" in scripts/*|formal/tlaplus/casper_soak/*|.github/workflows/*) ;; *) exit 2 ;; esac
 done < <(awk '/^artifacts:$/ {active=1;next} active && /^  - / {sub(/^  - /, "");print;next} active {exit}' "$claim")
done
jq -e '.matrix_status=="not-dispatchable" and .external_harness_revision=="b3d14b27e3c6276b1eb4ab9ccef04e02b0c4e283" and (.candidates|length)==2 and all(.candidates[];.admission=="blocked" and .workload_configuration_digest==null and (.node_revision|test("^[0-9a-f]{40}$")) and (.node_binary_digest|test("^sha256:[0-9a-f]{64}$")) and (.image_digest|test("^sha256:[0-9a-f]{64}$"))) and all(.optional_references[];.selected==false) and .post_merge_gate.actual_pr216_merge_revision==null and .post_merge_gate.accepted_handoff==false' "$MATRIX" >/dev/null
printf 'Document structure: 12 decisions, 8 claims, 10 lifecycle controls, 21 profile negative controls, 21 positive/generation obligations.\n'
[[ "$MODE" == full ]] || exit 0
TEMP="$(mktemp -d)"
trap 'rm -rf "$TEMP"' EXIT
for package in casper-prerequisite-application-20260917-01 casper-stack-integration-20260917-01; do
 directory="docs/casper/cbc-evidence/runs/$package"
 if jq -e '.artifacts|type=="array"' "$directory/report.json" >/dev/null; then
  jq -r '.artifacts[] | .retained_sha256+"  "+.path' "$directory/report.json" >"$TEMP/hashes"
  (cd "$directory" && shasum -a 256 -c "$TEMP/hashes") >"$TEMP/hash-results"
  count="$(wc -l <"$TEMP/hashes" | tr -d ' ')"
 else
  while IFS= read -r key; do
   jq -j --arg key "$key" '.records[$key].content' "$directory/report.json" >"$TEMP/record"
   [[ "$(shasum -a 256 "$TEMP/record" | awk '{print $1}')" == "$(jq -r --arg key "$key" '.records[$key].retained_sha256' "$directory/report.json")" ]]
  done < <(jq -r '.records|keys[]' "$directory/report.json")
  count="$(jq '.records|length' "$directory/report.json")"
 fi
 printf '%s: %s retained records verified.\n' "$package" "$count"
done
previous=''
while IFS= read -r revision; do
 [[ -z "$previous" ]] || git merge-base --is-ancestor "$previous" "$revision"
 git merge-base --is-ancestor "$revision" HEAD
 previous="$revision"
done < <(jq -r '.prerequisite_ancestry_chain[]' docs/casper/cbc-evidence/runs/casper-stack-integration-20260917-01/report.json)
while IFS= read -r candidate; do
 path="$(jq -r .artifact_identity_record <<<"$candidate")"
 jq -e --argjson candidate "$candidate" '.source_revision==$candidate.node_revision and .platform==$candidate.platform and .image_digest==$candidate.image_digest and .image_config_digest==$candidate.image_config_digest and ("sha256:"+.node_binary.sha256)==$candidate.node_binary_digest and .node_executed==false and .artifact_attestation_verified==false' "$path" >/dev/null
done < <(jq -c '.candidates[]' "$MATRIX")
manifest="$(jq -r .source_manifest "$MATRIX")"
[[ "$(shasum -a 256 "$manifest" | awk '{print $1}')" == "$(jq -r .source_manifest_sha256 "$MATRIX")" ]]
target/debug/check-casper-claims --root . --claim CLAIM-CASPER-SOAK-001 --strict --output "$TEMP/claim.json"
status=0
target/debug/check-casper-claims --root . --strict --output "$TEMP/bundle.json" || status=$?
[[ "$status" == 4 ]]
jq -e '.claims|length==8' "$TEMP/bundle.json" >/dev/null
jq -e '[.claims[]|select(.status=="pending")]|length==7' "$TEMP/bundle.json" >/dev/null
printf 'Historical evidence integrity, stack ancestry, initial candidate identities, and current claim boundaries passed. No model, process fixture, or node ran.\n'
