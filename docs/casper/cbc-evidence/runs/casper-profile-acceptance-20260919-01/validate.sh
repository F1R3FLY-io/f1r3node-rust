#!/usr/bin/env bash
set -euo pipefail
[[ $# == 1 && ! -e "$1" && ! -L "$1" ]]
P=docs/casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01
ROOT="$PWD"
mkdir -p "$1"
OUT="$(cd "$1" && pwd)"
(cd "$P"; shasum -a 256 -c artifacts.sha256; shasum -a 256 -c accepted-ledgers.sha256) >"$OUT/artifacts.txt"
for kind in source evidence; do
 mkdir "$OUT/$kind"
 gtar --no-same-owner -xzf "$P/$kind.tar.gz" -C "$OUT/$kind"
 (cd "$OUT/$kind"; shasum -a 256 -c "$ROOT/$P/$kind-files.sha256") >"$OUT/$kind.txt"
done
jq -r '(.source_digests,.claim_digests)|to_entries[]|.value+"  "+.key' "$P/report.json" | shasum -a 256 -c - >"$OUT/current-binding.txt"
for file in "$P/accepted-ledgers/"*.md; do cmp "$file" "docs/casper/cbc-evidence/$(basename "$file")"; done
jq -e '.claim_discharge=="discharged" and .phase=="pre_pr216_merge" and .tiers=={refutation:"bounded-safety-pass",construction:"not-applicable",binding:"passed"} and .node_execution==false and .policy_activation==false and .post_merge_execution=="blocked" and .d07_interpretation=="unresolved" and .waiver==null' "$P/report.json" >/dev/null
for id in 001 002 003 004; do
 target/debug/check-casper-claims --root . --output "$OUT/claim-$id.json" --strict --claim "CLAIM-CASPER-SOAK-$id" >"$OUT/claim-$id.txt" 2>&1
done
status=0
target/debug/check-casper-claims --root . --output "$OUT/bundle.json" --strict >"$OUT/bundle.txt" 2>&1 || status=$?
[[ "$status" == 4 ]]
jq -e '[.claims[]|select(.status=="pending")|.claim_id]==["CLAIM-CASPER-SOAK-005","CLAIM-CASPER-SOAK-006","CLAIM-CASPER-SOAK-007","CLAIM-CASPER-SOAK-008"]' "$OUT/bundle.json" >/dev/null
bash "$P/validate-review.sh" "$OUT/review" --historical >"$OUT/review.txt" 2>&1
E="$OUT/evidence"
for id in 5 6 7; do
 jq -e '.grade=="full" and (.gaps|length)==0' "$E/closure/TASK-017-$id-after.json" >/dev/null
 awk -v task="TASK-017-$id" '/^  - id:/ {on=($3==task)} on && /^    status:/ {if($2!="complete") exit 2; found++} END {if(found!=1) exit 2}' docs/ToDos.md
done
cmp "$E/closure/unrelated-before.json" "$E/closure/unrelated-after.json"
for profile in authority-finality publication recovery; do
 [[ "$(<"$E/$profile.exit")" == 0 ]]
 jq -e '.results|length==4 and all(.outcome=="passed" and .exit==.expected_exit)' "$E/$profile/models/report.json" >/dev/null
 expected=65; [[ "$profile" != publication ]] || expected=83; [[ "$profile" != recovery ]] || expected=55
 find "$E/$profile/fixtures" -name 'invocation-*.json' -print0 | xargs -0 jq -s --argjson expected "$expected" -e 'length==$expected and all(.actual_exit==.expected_exit and (.stdout|fromjson|.node_launch_count==0 and .soak_verdict=="non_passing"))' >"$OUT/$profile-fixtures.txt"
done
[[ "$(<"$E/shared-regressions.exit")" == 0 && "$(<"$E/refresh.exit")" == 0 && "$(<"$E/closure.exit")" == 0 ]]
while IFS=$'\t' read -r path kind expected; do
 if [[ "$kind" == symlink ]]; then [[ -L "$path" && "$(readlink "$path")" == "$expected" ]];
 else [[ ! -L "$path" && "$(shasum -a 256 "$path" | awk '{print $1}')" == "$expected" ]]; fi
done <"$E/record/compatibility-before.tsv"
shasum -a 256 -c "$E/preserved-files.sha256" >"$OUT/preserved-files.txt"
for name in authority-finality publication recovery; do [[ "$(git check-attr cbc -- ".github/workflows/casper-$name.yml")" == *': mandatory' ]]; done
jq -n --arg report "$(shasum -a 256 "$P/report.json" | awk '{print $1}')" --argjson sources "$(wc -l <"$P/source-files.sha256")" --argjson evidence "$(wc -l <"$P/evidence-files.sha256")" '{status:"accepted-profile-completion-verified",report_sha256:$report,source_files:$sources,evidence_files:$evidence,canonical_ledgers:36,strict_claims_001_004:0,strict_bundle:4,task_integrity:{"TASK-017-5":"full","TASK-017-6":"full","TASK-017-7":"full"},completion_gaps:[],scope:"bounded-pre-merge-profiles-only",node_execution:false,post_merge_execution:"blocked",d07_interpretation:"unresolved"}' >"$OUT/validation.json"
printf 'The accepted profile bindings and three task completions are verified.\n'
