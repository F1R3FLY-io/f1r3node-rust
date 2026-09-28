#!/usr/bin/env bash
set -euo pipefail
P=docs/casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01
B=docs/casper/cbc-evidence/runs/casper-profile-binding-review-20260919-01
OUT=target/casper-profile-acceptance/record
[[ ! -e "$P/report.json" && ! -e "$OUT" ]]
mkdir -p "$OUT" "$P/accepted-ledgers"
(cd "$B"; shasum -a 256 -c artifacts.sha256) >"$OUT/review-integrity.txt"
shasum -a 256 -c "$B/ledgers-files.sha256" >"$OUT/old-ledgers.txt"
cp target/casper-profile-review-validation-02/validation.json "$P/review-validation.json"
: >"$OUT/source.jsonl"
: >"$OUT/claims.jsonl"
: >"$OUT/compatibility-before.tsv"
for file in "$B/candidate-ledgers/"*.md; do
 data="$(awk '/^```json$/ {on=1;next} on && /^```$/ {exit} on {print}' "$file")"
 path="$(jq -r '.artifact.path' <<<"$data")"
 sha="$(shasum -a 256 "$path" | awk '{print $1}')"
 jq -e --arg sha "$sha" '.artifact.sha256==$sha and .status=="pending"' <<<"$data" >/dev/null
 jq -n --arg path "$path" --arg sha "$sha" '{($path):$sha}' >>"$OUT/source.jsonl"
 compat="docs/cbc-evidence/$(basename "$file")"
 if [[ -L "$compat" ]]; then
  printf '%s\tsymlink\t%s\n' "$compat" "$(readlink "$compat")" >>"$OUT/compatibility-before.tsv"
 else
  [[ -f "$compat" ]]
  printf '%s\tregular\t%s\n' "$compat" "$(shasum -a 256 "$compat" | awk '{print $1}')" >>"$OUT/compatibility-before.tsv"
 fi
done
for name in authority-finality publication recovery; do
 path="docs/claims/casper-soak-$name.md"
 grep -x 'status: discharged' "$path" >/dev/null
 grep -x 'binding: passed' "$path" >/dev/null
 sha="$(shasum -a 256 "$path" | awk '{print $1}')"
 jq -n --arg path "$path" --arg sha "$sha" '{($path):$sha}' >>"$OUT/claims.jsonl"
 [[ "$(git check-attr cbc -- ".github/workflows/casper-$name.yml")" == *': mandatory' ]]
done
jq -s add "$OUT/source.jsonl" >"$OUT/source.json"
jq -s add "$OUT/claims.jsonl" >"$OUT/claims.json"
[[ "$(jq length "$OUT/source.json")" == 36 ]]
jq -n --slurpfile sources "$OUT/source.json" --slurpfile specs "$OUT/claims.json" --arg review "$B/report.json" --arg digest "$(shasum -a 256 "$B/report.json" | awk '{print $1}')" --arg head "$(git rev-parse HEAD)" --arg time "$(date -u +%FT%TZ)" '{schema_version:1,status:"accepted-bounded-profile-bindings",claim_ids:["CLAIM-CASPER-SOAK-002","CLAIM-CASPER-SOAK-003","CLAIM-CASPER-SOAK-004"],claim_discharge:"discharged",phase:"pre_pr216_merge",scope:"bounded-controlled-transcript-profiles",approval:{confirmation:"yes - I authorized acceptance",scope:"claims-002-003-004-and-their-three-workflow-tags"},review:{ref:$review,sha256:$digest},source_digests:$sources[0],claim_digests:$specs[0],tiers:{refutation:"bounded-safety-pass",construction:"not-applicable",binding:"passed"},observed_head:$head,verified_at:$time,workflow_tags:"ratified-and-applied",lifecycle_claim_001:"pending-separate-source-acceptance",d07_interpretation:"unresolved",live_adapters:"unqualified",post_merge_execution:"blocked",node_execution:false,policy_activation:false,soak:"pending",waiver:null}' >"$P/report.json"
report_hash="$(shasum -a 256 "$P/report.json" | awk '{print $1}')"
: >"$OUT/ledgers.patch.txt"
for file in "$B/candidate-ledgers/"*.md; do
 data="$(awk '/^```json$/ {on=1;next} on && /^```$/ {exit} on {print}' "$file")"
 name="$(basename "$file")"
 path="docs/casper/cbc-evidence/$name"
 target="$P/accepted-ledgers/$name"
 spec="$(jq -r '.claim' <<<"$data")"
 spec_sha="$(shasum -a 256 "$spec" | awk '{print $1}')"
 {
  printf '# CbC Evidence: %s\n\nThe user accepted this bounded pre-merge profile binding. Node correctness and live execution remain outside this discharge.\n\n```json\n' "$(jq -r '.artifact.path' <<<"$data")"
  jq --arg spec "$spec_sha" --arg ref "$P/report.json" --arg digest "$report_hash" --arg commit "$(git rev-parse HEAD)" --arg time "$(jq -r '.verified_at' "$P/report.json")" '.claim_digests[.claim]=$spec | .artifact.commit=$commit | .status="discharged" | .tiers.binding="passed" | .phase_status.pre_pr216_merge="discharged" | .evidence.ref=$ref | .evidence.sha256=$digest | .verified_at=$time' <<<"$data"
  printf '```\n'
 } >"$target"
 status=0
 diff -u --label "a/$path" --label "b/$path" "$path" "$target" >>"$OUT/ledgers.patch.txt" || status=$?
 [[ "$status" == 1 ]]
done
shasum -a 256 -c "$B/ledgers-files.sha256" >"$OUT/old-ledgers-recheck.txt"
git apply --check "$OUT/ledgers.patch.txt"
git apply "$OUT/ledgers.patch.txt"
for file in "$P/accepted-ledgers/"*.md; do cmp "$file" "docs/casper/cbc-evidence/$(basename "$file")"; done
while IFS=$'\t' read -r path kind expected; do
 if [[ "$kind" == symlink ]]; then [[ -L "$path" && "$(readlink "$path")" == "$expected" ]];
 else [[ ! -L "$path" && "$(shasum -a 256 "$path" | awk '{print $1}')" == "$expected" ]]; fi
done <"$OUT/compatibility-before.tsv"
for id in 002 003 004; do
 target/debug/check-casper-claims --root . --output "$OUT/claim-$id.json" --strict --claim "CLAIM-CASPER-SOAK-$id" >"$OUT/claim-$id.txt" 2>&1
 printf '0\n' >"$OUT/claim-$id.exit"
done
for id in 001 bundle; do
 args=(); [[ "$id" == bundle ]] || args=(--claim CLAIM-CASPER-SOAK-001)
 status=0
 target/debug/check-casper-claims --root . --output "$OUT/claim-$id.json" --strict "${args[@]}" >"$OUT/claim-$id.txt" 2>&1 || status=$?
 printf '%s\n' "$status" >"$OUT/claim-$id.exit"
 [[ "$status" == 4 ]]
done
printf 'Three profile claims passed strict source-bound audits. No tracker or index changed.\n'
